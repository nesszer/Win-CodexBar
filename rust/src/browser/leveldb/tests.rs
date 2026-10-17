//! Tests for the LevelDB reader. Fixtures are built in-memory from the documented on-disk
//! formats (log block/record framing, table block/footer layout, Snappy elements).

#![allow(
    clippy::cast_possible_truncation,
    reason = "fixture builders encode small in-memory sizes into fixed-width fields"
)]

use super::local_storage::{
    LocalStorageEntry, decode_origin_entries, local_storage_dir, read_local_storage_entries,
};
use super::snappy::{self, SnappyError};
use super::table::{TableError, read_table};
use super::{Entry, LevelDbError, Record, log, read_current_value_strict, read_entries};

// ---------------------------------------------------------------------------------------------
// Fixture builders
// ---------------------------------------------------------------------------------------------

fn varint(mut value: u64) -> Vec<u8> {
    let mut out = Vec::new();
    while value >= 0x80 {
        out.push((value as u8 & 0x7f) | 0x80);
        value >>= 7;
    }
    out.push(value as u8);
    out
}

/// A valid Snappy stream made only of literal elements (no back-references).
fn snappy_literal(data: &[u8]) -> Vec<u8> {
    let mut out = varint(data.len() as u64);
    for chunk in data.chunks(60) {
        out.push(((chunk.len() - 1) as u8) << 2);
        out.extend_from_slice(chunk);
    }
    out
}

enum Op<'a> {
    Put(&'a [u8], &'a [u8]),
    Delete(&'a [u8]),
}

fn write_batch(sequence: u64, ops: &[Op]) -> Vec<u8> {
    let mut out = sequence.to_le_bytes().to_vec();
    out.extend_from_slice(&(ops.len() as u32).to_le_bytes());
    for op in ops {
        match op {
            Op::Put(key, value) => {
                out.push(1);
                out.extend(varint(key.len() as u64));
                out.extend_from_slice(key);
                out.extend(varint(value.len() as u64));
                out.extend_from_slice(value);
            }
            Op::Delete(key) => {
                out.push(0);
                out.extend(varint(key.len() as u64));
                out.extend_from_slice(key);
            }
        }
    }
    out
}

/// Frame batches as a LevelDB log: 32 KiB blocks, fragmenting records that cross a block edge.
fn write_log(batches: &[Vec<u8>]) -> Vec<u8> {
    const BLOCK: usize = 32 * 1024;
    let mut out = Vec::new();
    for batch in batches {
        let mut remaining = batch.as_slice();
        let mut first = true;
        loop {
            let left_in_block = BLOCK - out.len() % BLOCK;
            if left_in_block < 7 {
                out.extend(std::iter::repeat_n(0u8, left_in_block));
                continue;
            }
            let chunk_len = remaining.len().min(left_in_block - 7);
            let last = chunk_len == remaining.len();
            let kind: u8 = match (first, last) {
                (true, true) => 1,
                (true, false) => 2,
                (false, false) => 3,
                (false, true) => 4,
            };
            out.extend_from_slice(&[0, 0, 0, 0]); // checksum (not verified by the reader)
            out.extend_from_slice(&(chunk_len as u16).to_le_bytes());
            out.push(kind);
            out.extend_from_slice(&remaining[..chunk_len]);
            remaining = &remaining[chunk_len..];
            first = false;
            if last {
                break;
            }
        }
    }
    out
}

fn internal_key(user_key: &[u8], sequence: u64, kind: u8) -> Vec<u8> {
    let mut key = user_key.to_vec();
    key.extend_from_slice(&((sequence << 8) | u64::from(kind)).to_le_bytes());
    key
}

/// Encode a block with prefix compression and a restart every `restart_interval` entries.
fn write_block(entries: &[(Vec<u8>, Vec<u8>)], restart_interval: usize) -> Vec<u8> {
    let mut out = Vec::new();
    let mut restarts = vec![0u32];
    let mut previous: &[u8] = &[];
    for (index, (key, value)) in entries.iter().enumerate() {
        let shared = if index % restart_interval == 0 {
            if index > 0 {
                restarts.push(out.len() as u32);
            }
            0
        } else {
            previous.iter().zip(key).take_while(|(a, b)| a == b).count()
        };
        out.extend(varint(shared as u64));
        out.extend(varint((key.len() - shared) as u64));
        out.extend(varint(value.len() as u64));
        out.extend_from_slice(&key[shared..]);
        out.extend_from_slice(value);
        previous = key;
    }
    for restart in &restarts {
        out.extend_from_slice(&restart.to_le_bytes());
    }
    out.extend_from_slice(&(restarts.len() as u32).to_le_bytes());
    out
}

#[derive(Clone, Copy, PartialEq)]
enum Compression {
    None,
    Snappy,
}

/// A table record: user key, sequence, kind (1 = value, 0 = delete), value.
type TableRecord<'a> = (&'a [u8], u64, u8, &'a [u8]);

/// Build a table with one data block per slice of `blocks`.
fn write_table(blocks: &[(&[TableRecord], Compression)]) -> Vec<u8> {
    let mut file = Vec::new();
    let mut index_entries = Vec::new();
    for (records, compression) in blocks {
        let entries: Vec<_> = records
            .iter()
            .map(|(key, sequence, kind, value)| {
                (internal_key(key, *sequence, *kind), value.to_vec())
            })
            .collect();
        let block = write_block(&entries, 2);
        let (contents, tag) = match compression {
            Compression::None => (block, 0u8),
            Compression::Snappy => (snappy_literal(&block), 1u8),
        };
        let mut handle = varint(file.len() as u64);
        handle.extend(varint(contents.len() as u64));
        file.extend_from_slice(&contents);
        file.push(tag);
        file.extend_from_slice(&[0, 0, 0, 0]);
        index_entries.push((entries.last().unwrap().0.clone(), handle));
    }

    let index = write_block(&index_entries, 1);
    let mut index_handle = varint(file.len() as u64);
    index_handle.extend(varint(index.len() as u64));
    file.extend_from_slice(&index);
    file.extend_from_slice(&[0, 0, 0, 0, 0]);

    let mut footer = varint(0); // metaindex handle: offset 0, size 0 (never read)
    footer.extend(varint(0));
    footer.extend_from_slice(&index_handle);
    footer.resize(40, 0);
    footer.extend_from_slice(&0xdb47_7524_8b80_fb57u64.to_le_bytes());
    file.extend_from_slice(&footer);
    file
}

type Collected = Vec<(Vec<u8>, u64, Option<Vec<u8>>)>;

fn collect_log(data: &[u8]) -> Collected {
    let mut records = Vec::new();
    log::read_log(data, &mut |r: Record| {
        records.push((r.key, r.sequence, r.value));
    });
    records
}

fn collect_table(data: &[u8]) -> Result<Collected, TableError> {
    let mut records = Vec::new();
    read_table(data, &mut |r: Record| {
        records.push((r.key, r.sequence, r.value));
    })?;
    Ok(records)
}

// ---------------------------------------------------------------------------------------------
// Snappy
// ---------------------------------------------------------------------------------------------

#[test]
fn snappy_decodes_literal() {
    let mut stream = vec![5, 4 << 2];
    stream.extend_from_slice(b"hello");
    assert_eq!(snappy::decompress(&stream, 1024).unwrap(), b"hello");
}

#[test]
fn snappy_decodes_long_literal_with_extra_length_byte() {
    let data: Vec<u8> = (0..100u8).collect();
    let mut stream = varint(100);
    stream.extend_from_slice(&[60 << 2, 99]);
    stream.extend_from_slice(&data);
    assert_eq!(snappy::decompress(&stream, 1024).unwrap(), data);
}

#[test]
fn snappy_decodes_overlapping_copy_with_one_byte_offset() {
    // "abc" literal, then copy of length 9 at offset 3 (copy-1 tag: len-4 in bits 2..5).
    let stream = [12, 2 << 2, b'a', b'b', b'c', 0b01 | (5 << 2), 3];
    assert_eq!(snappy::decompress(&stream, 1024).unwrap(), b"abcabcabcabc");
}

#[test]
fn snappy_decodes_copy_with_two_byte_offset() {
    let mut stream = vec![15, 9 << 2];
    stream.extend_from_slice(b"0123456789");
    stream.extend_from_slice(&[0b10 | (4 << 2), 10, 0]);
    assert_eq!(
        snappy::decompress(&stream, 1024).unwrap(),
        b"012345678901234"
    );
}

#[test]
fn snappy_decodes_copy_with_four_byte_offset() {
    let mut stream = vec![14, 9 << 2];
    stream.extend_from_slice(b"0123456789");
    stream.extend_from_slice(&[0b11 | (3 << 2), 10, 0, 0, 0]);
    assert_eq!(
        snappy::decompress(&stream, 1024).unwrap(),
        b"01234567890123"
    );
}

#[test]
fn snappy_rejects_malformed_streams() {
    assert_eq!(snappy::decompress(&[], 16), Err(SnappyError::BadPreamble));
    // Declared length larger than the limit is refused before allocating.
    assert_eq!(
        snappy::decompress(&varint(u64::from(u32::MAX)), 1024),
        Err(SnappyError::TooLarge(u32::MAX as usize, 1024))
    );
    // Literal runs past the end of input.
    assert_eq!(
        snappy::decompress(&[5, 4 << 2, b'h'], 16),
        Err(SnappyError::Truncated)
    );
    // Copy before any output exists.
    assert_eq!(
        snappy::decompress(&[4, 0b01, 1], 16),
        Err(SnappyError::BadOffset)
    );
    // Zero offset is invalid.
    assert_eq!(
        snappy::decompress(&[5, 0, b'a', 0b01, 0], 16),
        Err(SnappyError::BadOffset)
    );
    // Output shorter than the preamble promised.
    assert_eq!(
        snappy::decompress(&[5, 0, b'a'], 16),
        Err(SnappyError::LengthMismatch)
    );
    // Output longer than the preamble promised.
    assert_eq!(
        snappy::decompress(&[1, 1 << 2, b'a', b'b'], 16),
        Err(SnappyError::LengthMismatch)
    );
}

// ---------------------------------------------------------------------------------------------
// Write-ahead log
// ---------------------------------------------------------------------------------------------

#[test]
fn log_yields_puts_and_deletes_with_incrementing_sequences() {
    let data = write_log(&[write_batch(
        10,
        &[Op::Put(b"a", b"1"), Op::Delete(b"b"), Op::Put(b"c", b"3")],
    )]);
    assert_eq!(
        collect_log(&data),
        vec![
            (b"a".to_vec(), 10, Some(b"1".to_vec())),
            (b"b".to_vec(), 11, None),
            (b"c".to_vec(), 12, Some(b"3".to_vec())),
        ]
    );
}

#[test]
fn log_reassembles_records_that_span_blocks() {
    let big = vec![0xabu8; 80 * 1024];
    let data = write_log(&[
        write_batch(1, &[Op::Put(b"small", b"x")]),
        write_batch(2, &[Op::Put(b"big", &big)]),
        write_batch(3, &[Op::Put(b"after", b"y")]),
    ]);
    assert!(data.len() > 64 * 1024);
    let records = collect_log(&data);
    assert_eq!(records.len(), 3);
    assert_eq!(records[1].2.as_deref(), Some(big.as_slice()));
    assert_eq!(records[2].0, b"after");
}

#[test]
fn log_skips_block_trailer_padding() {
    // Leave fewer than 7 bytes at the end of the first block so the writer pads it.
    let filler = vec![1u8; 32 * 1024 - 7 - 12 - 1 - 2 - 1 - 1 - 4];
    let data = write_log(&[
        write_batch(1, &[Op::Put(b"k", &filler)]),
        write_batch(2, &[Op::Put(b"z", b"after-padding")]),
    ]);
    let records = collect_log(&data);
    assert_eq!(records.len(), 2);
    assert_eq!(records[1].2.as_deref(), Some(b"after-padding".as_slice()));
}

#[test]
fn log_keeps_records_before_a_truncated_tail() {
    let mut data = write_log(&[
        write_batch(1, &[Op::Put(b"kept", b"1")]),
        write_batch(2, &[Op::Put(b"lost", b"2")]),
    ]);
    data.truncate(data.len() - 3);
    let records = collect_log(&data);
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].0, b"kept");
}

#[test]
fn log_ignores_garbage_without_panicking() {
    assert!(collect_log(&[0xff; 100]).is_empty());
    assert!(collect_log(&[]).is_empty());
    // Middle fragment with no preceding first fragment.
    let orphan = [0, 0, 0, 0, 1, 0, 3, 9];
    assert!(collect_log(&orphan).is_empty());
}

// ---------------------------------------------------------------------------------------------
// Tables
// ---------------------------------------------------------------------------------------------

#[test]
fn table_reads_prefix_compressed_multi_block_data() {
    let first: &[TableRecord] = &[
        (b"app:alpha", 5, 1, b"one"),
        (b"app:alpha2", 6, 1, b"two"),
        (b"app:beta", 7, 1, b"three"),
    ];
    let second: &[TableRecord] = &[(b"zeta", 8, 1, b"four"), (b"zeta-gone", 9, 0, b"")];
    let data = write_table(&[(first, Compression::None), (second, Compression::None)]);

    let records = collect_table(&data).unwrap();
    assert_eq!(records.len(), 5);
    assert_eq!(
        records[1],
        (b"app:alpha2".to_vec(), 6, Some(b"two".to_vec()))
    );
    assert_eq!(records[4], (b"zeta-gone".to_vec(), 9, None));
}

#[test]
fn table_reads_snappy_compressed_blocks() {
    let long_value = vec![b'v'; 500];
    let records: &[TableRecord] = &[(b"k1", 1, 1, &long_value), (b"k2", 2, 1, b"short")];
    let data = write_table(&[(records, Compression::Snappy)]);

    let decoded = collect_table(&data).unwrap();
    assert_eq!(decoded.len(), 2);
    assert_eq!(decoded[0].2.as_deref(), Some(long_value.as_slice()));
}

#[test]
fn table_with_bad_magic_or_length_is_rejected() {
    let records: &[TableRecord] = &[(b"k", 1, 1, b"v")];
    let mut data = write_table(&[(records, Compression::None)]);
    assert!(matches!(
        collect_table(&data[..20]),
        Err(TableError::BadFooter)
    ));
    let last = data.len() - 1;
    data[last] ^= 0xff;
    assert!(matches!(collect_table(&data), Err(TableError::BadFooter)));
}

#[test]
fn table_skips_an_undecodable_block_but_keeps_the_rest() {
    let good: &[TableRecord] = &[(b"good", 1, 1, b"ok")];
    let bad: &[TableRecord] = &[(b"bad", 2, 1, b"nope")];
    let mut data = write_table(&[(bad, Compression::Snappy), (good, Compression::None)]);
    // Corrupt the first block's compression tag to an unsupported type (2).
    let bad_block_len = snappy_literal(&write_block(
        &[(internal_key(b"bad", 2, 1), b"nope".to_vec())],
        2,
    ))
    .len();
    data[bad_block_len] = 2;

    let mut records = Vec::new();
    let skipped = read_table(&data, &mut |r: Record| records.push(r.key)).unwrap();
    assert_eq!(skipped, 1);
    assert_eq!(records, vec![b"good".to_vec()]);
}

#[test]
fn table_with_out_of_range_handle_does_not_panic() {
    let records: &[TableRecord] = &[(b"k", 1, 1, b"v")];
    let mut data = write_table(&[(records, Compression::None)]);
    // Point the index handle far past the end of the file.
    let footer = data.len() - 48;
    data[footer..footer + 40].fill(0);
    data[footer + 2..footer + 6].copy_from_slice(&[0xff, 0xff, 0xff, 0x0f]);
    assert!(collect_table(&data).is_err());
}

// ---------------------------------------------------------------------------------------------
// Directory reader
// ---------------------------------------------------------------------------------------------

#[test]
fn read_entries_resolves_newest_sequence_across_log_and_table() {
    let dir = tempfile::tempdir().unwrap();
    let table_records: &[TableRecord] = &[
        (b"keep", 1, 1, b"table-keep"),
        (b"stale", 2, 1, b"table-old"),
        (b"removed", 3, 1, b"table-removed"),
    ];
    std::fs::write(
        dir.path().join("000005.ldb"),
        write_table(&[(table_records, Compression::None)]),
    )
    .unwrap();
    std::fs::write(
        dir.path().join("000006.log"),
        write_log(&[write_batch(
            10,
            &[
                Op::Put(b"stale", b"log-new"),
                Op::Delete(b"removed"),
                Op::Put(b"fresh", b"log-fresh"),
            ],
        )]),
    )
    .unwrap();
    // Bookkeeping files and unrelated data are ignored.
    std::fs::write(dir.path().join("LOCK"), b"").unwrap();
    std::fs::write(dir.path().join("CURRENT"), b"MANIFEST-000001\n").unwrap();
    std::fs::write(dir.path().join("notes.txt"), b"not a database").unwrap();

    let entries = read_entries(dir.path()).unwrap();
    let pairs: Vec<(&[u8], &[u8])> = entries
        .iter()
        .map(|e| (e.key.as_slice(), e.value.as_slice()))
        .collect();
    assert_eq!(
        pairs,
        vec![
            (b"fresh".as_slice(), b"log-fresh".as_slice()),
            (b"keep".as_slice(), b"table-keep".as_slice()),
            (b"stale".as_slice(), b"log-new".as_slice()),
        ]
    );
}

#[test]
fn read_entries_lets_a_newer_table_delete_beat_an_older_log_put() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("000001.log"),
        write_log(&[write_batch(1, &[Op::Put(b"gone", b"old")])]),
    )
    .unwrap();
    let deletes: &[TableRecord] = &[(b"gone", 2, 0, b"")];
    std::fs::write(
        dir.path().join("000002.ldb"),
        write_table(&[(deletes, Compression::None)]),
    )
    .unwrap();
    assert!(read_entries(dir.path()).unwrap().is_empty());
}

#[test]
fn read_entries_skips_corrupt_files_and_keeps_good_ones() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("000001.ldb"), b"definitely not a table").unwrap();
    std::fs::write(
        dir.path().join("000002.log"),
        write_log(&[write_batch(1, &[Op::Put(b"ok", b"1")])]),
    )
    .unwrap();
    assert_eq!(
        read_entries(dir.path()).unwrap(),
        vec![Entry {
            key: b"ok".to_vec(),
            value: b"1".to_vec()
        }]
    );
}

#[test]
fn read_entries_reports_a_missing_directory() {
    let dir = tempfile::tempdir().unwrap();
    assert!(read_entries(&dir.path().join("absent")).is_err());
}

fn strict_dir(files: &[(&str, Vec<u8>)]) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("CURRENT"), b"MANIFEST-000001\n").unwrap();
    for (name, data) in files {
        std::fs::write(dir.path().join(name), data).unwrap();
    }
    dir
}

#[test]
fn strict_read_returns_the_newest_value_of_the_exact_key() {
    let table: &[TableRecord] = &[(b"token", 1, 1, b"old"), (b"token-other", 2, 1, b"x")];
    let dir = strict_dir(&[
        ("000002.ldb", write_table(&[(table, Compression::None)])),
        (
            "000003.log",
            write_log(&[write_batch(5, &[Op::Put(b"token", b"new")])]),
        ),
    ]);
    assert_eq!(
        read_current_value_strict(dir.path(), b"token").unwrap(),
        Some(b"new".to_vec())
    );
    assert_eq!(
        read_current_value_strict(dir.path(), b"absent").unwrap(),
        None
    );
}

#[test]
fn strict_read_honors_a_newer_delete() {
    let dir = strict_dir(&[(
        "000003.log",
        write_log(&[
            write_batch(1, &[Op::Put(b"token", b"old")]),
            write_batch(2, &[Op::Delete(b"token")]),
        ]),
    )]);
    assert_eq!(
        read_current_value_strict(dir.path(), b"token").unwrap(),
        None
    );
}

#[test]
fn strict_read_fails_instead_of_skipping_a_file() {
    let good = write_log(&[write_batch(1, &[Op::Put(b"token", b"old")])]);
    let mut torn = write_log(&[write_batch(2, &[Op::Delete(b"token")])]);
    torn.truncate(torn.len() - 1);
    let cases = [
        vec![
            ("000003.log", good.clone()),
            ("000004.ldb", b"not a table".to_vec()),
        ],
        vec![("000003.log", good.clone()), ("000004.log", torn)],
    ];
    for files in cases {
        let dir = strict_dir(&files);
        assert!(matches!(
            read_current_value_strict(dir.path(), b"token"),
            Err(LevelDbError::Incomplete)
        ));
        // The best-effort reader still returns the stale value from the readable file.
        assert_eq!(read_entries(dir.path()).unwrap().len(), 1);
    }
}

#[test]
fn strict_read_requires_current() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("000003.log"),
        write_log(&[write_batch(1, &[Op::Put(b"token", b"v")])]),
    )
    .unwrap();
    assert!(matches!(
        read_current_value_strict(dir.path(), b"token"),
        Err(LevelDbError::Incomplete)
    ));
}

// ---------------------------------------------------------------------------------------------
// Chromium Local Storage layer
// ---------------------------------------------------------------------------------------------

fn latin1(text: &str) -> Vec<u8> {
    let mut out = vec![1u8];
    out.extend(text.chars().map(|c| c as u8));
    out
}

fn utf16(text: &str) -> Vec<u8> {
    let mut out = vec![0u8];
    out.extend(text.encode_utf16().flat_map(u16::to_le_bytes));
    out
}

fn storage_key(origin: &str, encoded_key: &[u8]) -> Vec<u8> {
    let mut key = format!("_{origin}").into_bytes();
    key.push(0);
    key.extend_from_slice(encoded_key);
    key
}

#[test]
fn local_storage_decodes_latin1_and_utf16_items_for_one_origin() {
    let entries = vec![
        Entry {
            key: b"META:https://www.kimi.ai".to_vec(),
            value: b"\x08\x01".to_vec(),
        },
        Entry {
            key: storage_key("https://www.kimi.ai", &latin1("access_token")),
            value: latin1("eyJ.body.sig"),
        },
        Entry {
            key: storage_key("https://www.kimi.ai", &utf16("name")),
            value: utf16("Zoë \u{1F600}"),
        },
        Entry {
            key: storage_key("https://www.kimi.com", &latin1("access_token")),
            value: latin1("other-origin"),
        },
        Entry {
            key: storage_key("https://www.kimi.ai", &latin1("bad_format")),
            value: vec![7, b'x'],
        },
    ];

    let decoded = decode_origin_entries(&entries, "https://www.kimi.ai/");
    assert_eq!(
        decoded,
        vec![
            LocalStorageEntry {
                key: "access_token".into(),
                value: "eyJ.body.sig".into()
            },
            LocalStorageEntry {
                key: "name".into(),
                value: "Zoë \u{1F600}".into()
            },
        ]
    );
}

#[test]
fn local_storage_origin_match_is_exact_not_a_prefix() {
    let entries = vec![Entry {
        key: storage_key("https://www.kimi.ai.evil.example", &latin1("access_token")),
        value: latin1("nope"),
    }];
    assert!(decode_origin_entries(&entries, "https://www.kimi.ai").is_empty());
}

#[test]
fn local_storage_rejects_odd_length_utf16_values() {
    let entries = vec![Entry {
        key: storage_key("https://a.example", &latin1("k")),
        value: vec![0, b'x'],
    }];
    assert!(decode_origin_entries(&entries, "https://a.example").is_empty());
}

#[test]
fn read_local_storage_entries_reads_a_profile_directory() {
    let profile = tempfile::tempdir().unwrap();
    let dir = local_storage_dir(profile.path());
    std::fs::create_dir_all(&dir).unwrap();

    let key = storage_key("https://www.kimi.ai", &latin1("access_token"));
    let value = latin1("a.b.c");
    std::fs::write(
        dir.join("000003.log"),
        write_log(&[write_batch(4, &[Op::Put(&key, &value)])]),
    )
    .unwrap();

    assert_eq!(
        read_local_storage_entries(&dir, "https://www.kimi.ai").unwrap(),
        vec![LocalStorageEntry {
            key: "access_token".into(),
            value: "a.b.c".into()
        }]
    );
}
