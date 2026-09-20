use chrono::{DateTime, TimeZone, Utc};
use reqwest::header::HeaderMap;

use crate::core::ProviderError;

mod reset_coupons;

pub(super) use reset_coupons::parse_grpc_web_reset_coupons;

#[derive(Debug, Clone, Copy)]
pub(super) struct GrokBillingSnapshot {
    pub(super) used_percent: Option<f64>,
    pub(super) used_percent_is_wire_published: bool,
    pub(super) used_percent_is_implicit_zero: bool,
    pub(super) resets_at: Option<DateTime<Utc>>,
    pub(super) window_minutes: Option<u32>,
}

pub(super) fn validate_grpc_headers(headers: &HeaderMap) -> Result<(), ProviderError> {
    if let Some(status) = headers
        .get("grpc-status")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<u16>().ok())
        && status != 0
    {
        return map_grpc_status(status, "Grok RPC");
    }
    Ok(())
}

pub(super) fn parse_grpc_web_response(data: &[u8]) -> Result<GrokBillingSnapshot, ProviderError> {
    parse_grpc_web_response_at(data, Utc::now())
}

/// Map a gRPC status code onto the provider error policy shared by the
/// billing and reset-credit endpoints.
pub(super) fn map_grpc_status(status: u16, context: &str) -> Result<(), ProviderError> {
    if status != 0 {
        if status == 16 {
            return Err(ProviderError::AuthRequired);
        }
        return Err(ProviderError::Other(format!(
            "{context} failed with status {status}"
        )));
    }
    Ok(())
}

/// Decode a length-prefixed field body: `read_varint -> try_from ->
/// checked_add -> bounds-check` in one place.
pub(super) fn read_length_field(
    data: &[u8],
    index: usize,
    what: &str,
) -> Result<(usize, usize), ProviderError> {
    let (len, start) = read_varint(data, index)
        .ok_or_else(|| ProviderError::Parse(format!("Grok {what} is malformed")))?;
    let len = usize::try_from(len)
        .map_err(|_| ProviderError::Parse(format!("Grok {what} is too large")))?;
    let end = start
        .checked_add(len)
        .ok_or_else(|| ProviderError::Parse(format!("Grok {what} length overflowed")))?;
    if end > data.len() {
        return Err(ProviderError::Parse(format!("Grok {what} is truncated")));
    }
    Ok((start, end))
}

/// Decode a varint Unix-seconds timestamp with the shared epoch bounding.
pub(super) fn unix_seconds_timestamp(seconds: u64) -> Option<DateTime<Utc>> {
    // Varint timestamps are Unix seconds inside the range checked below.
    #[allow(
        clippy::cast_possible_wrap,
        reason = "varint timestamps are bounded to the Unix-seconds range checked below"
    )]
    let seconds = seconds as i64;
    (1_700_000_000..=2_100_000_000)
        .contains(&seconds)
        .then(|| Utc.timestamp_opt(seconds, 0).single())
        .flatten()
}

/// One parameterized gRPC-web frame walker. `on_malformed` decides the
/// malformed-frame policy: billing swallows malformed frames, the optional
/// reset lookup fails closed.
///
/// Yields `(flags, payload)` for every frame, data and trailer alike; callers
/// split on the trailer flag.
pub(super) fn grpc_web_frames(
    data: &[u8],
    on_malformed: fn(&str) -> Option<ProviderError>,
) -> Result<Vec<(u8, &[u8])>, ProviderError> {
    let mut frames = Vec::new();
    let mut index = 0;
    while index < data.len() {
        if index + 5 > data.len() {
            return on_malformed("truncated").map_or(Ok(frames), Err);
        }
        let flags = data[index];
        let len = ((data[index + 1] as usize) << 24)
            | ((data[index + 2] as usize) << 16)
            | ((data[index + 3] as usize) << 8)
            | (data[index + 4] as usize);
        let start = index + 5;
        let Some(end) = start.checked_add(len) else {
            return on_malformed("frame is too large").map_or(Ok(frames), Err);
        };
        if end > data.len() {
            return on_malformed("truncated").map_or(Ok(frames), Err);
        }
        frames.push((flags, &data[start..end]));
        index = end;
    }
    Ok(frames)
}

fn skip_field(data: &[u8], index: usize, wire: u64) -> Option<usize> {
    match wire {
        0 => read_varint(data, index).map(|(_, next)| next),
        1 => index.checked_add(8).filter(|end| *end <= data.len()),
        2 => {
            let (len, start) = read_varint(data, index)?;
            let len = usize::try_from(len).ok()?;
            start.checked_add(len).filter(|end| *end <= data.len())
        }
        5 => index.checked_add(4).filter(|end| *end <= data.len()),
        _ => None,
    }
}

fn parse_grpc_web_response_at(
    data: &[u8],
    now: DateTime<Utc>,
) -> Result<GrokBillingSnapshot, ProviderError> {
    let mut payloads = grpc_web_data_frames(data);
    if payloads.is_empty() && looks_like_protobuf_payload(data) {
        payloads.push(data.to_vec());
    }
    if payloads.is_empty() {
        return Err(ProviderError::Parse(
            "Grok web billing returned no payload".to_string(),
        ));
    }
    let mut scan = ProtoScan::default();
    for payload in &payloads {
        scan.scan_message(payload, &mut Vec::new(), 0);
    }
    let percent_fields: Vec<&Fixed32Field> = scan
        .fixed32
        .iter()
        .filter(|field| field.path.last() == Some(&1))
        .collect();
    let mut valid_percent_fields: Vec<&Fixed32Field> = percent_fields
        .iter()
        .copied()
        .filter(|field| field.value.is_finite() && (0.0..=100.0).contains(&field.value))
        .collect();
    valid_percent_fields.sort_by(|a, b| {
        a.path
            .len()
            .cmp(&b.path.len())
            .then_with(|| a.order.cmp(&b.order))
    });
    let conflicting_percent = percent_fields.len() != valid_percent_fields.len()
        || valid_percent_fields.first().is_some_and(|first| {
            valid_percent_fields
                .iter()
                .any(|field| field.value != first.value)
        });
    let parsed_percent = if scan.is_complete && !conflicting_percent {
        valid_percent_fields.first().map(|field| field.value as f64)
    } else {
        None
    };

    let reset_fields: Vec<(&VarintField, DateTime<Utc>)> = scan
        .varints
        .iter()
        .filter_map(|field| varint_timestamp(field).map(|dt| (field, dt)))
        .collect();
    let future_resets: Vec<DateTime<Utc>> = reset_fields
        .iter()
        .filter(|(_, dt)| *dt > now)
        .map(|(_, dt)| *dt)
        .collect();
    let resets_at = reset_fields
        .iter()
        .filter(|(field, dt)| field.path.as_slice() == [1, 5, 1] && *dt > now)
        .map(|(_, dt)| *dt)
        .min()
        .or_else(|| future_resets.into_iter().min());

    let has_usage_period = scan.varints.iter().any(|field| {
        field.path.starts_with(&[1, 6])
            || (field.path.as_slice() == [1, 8, 1] && (field.value == 1 || field.value == 2))
    });
    let no_usage_yet = parsed_percent.is_none()
        && scan.fixed32.is_empty()
        && resets_at.is_some()
        && has_usage_period;
    let window_minutes = current_period_window_minutes(&scan, now);
    let has_active_current_period = window_minutes.is_some();
    let used_percent_is_implicit_zero =
        no_usage_yet && payloads.len() == 1 && scan.is_complete && has_active_current_period;
    let used_percent = parsed_percent.or_else(|| used_percent_is_implicit_zero.then_some(0.0));
    Ok(GrokBillingSnapshot {
        used_percent,
        used_percent_is_wire_published: parsed_percent.is_some(),
        used_percent_is_implicit_zero,
        resets_at,
        window_minutes,
    })
}

fn looks_like_protobuf_payload(data: &[u8]) -> bool {
    let Some(&first) = data.first() else {
        return false;
    };
    let field_number = first >> 3;
    let wire_type = first & 0x07;
    field_number > 0 && matches!(wire_type, 0 | 1 | 2 | 5)
}

fn varint_timestamp(field: &VarintField) -> Option<DateTime<Utc>> {
    unix_seconds_timestamp(field.value)
}

fn current_period_window_minutes(scan: &ProtoScan, now: DateTime<Utc>) -> Option<u32> {
    let period_type = unique_varint_at_path(scan, &[1, 8, 1])?;
    if period_type != 1 && period_type != 2 {
        return None;
    }
    let timestamp_at = |path: &[u64]| {
        unique_varint_at_path(scan, path).and_then(|value| {
            varint_timestamp(&VarintField {
                path: path.to_vec(),
                value,
            })
        })
    };
    let start = timestamp_at(&[1, 8, 2, 1])?;
    let end = timestamp_at(&[1, 8, 3, 1])?;
    if start > now || end <= now || end <= start {
        return None;
    }
    u32::try_from((end - start).num_minutes())
        .ok()
        .filter(|minutes| *minutes > 0)
}

fn unique_varint_at_path(scan: &ProtoScan, path: &[u64]) -> Option<u64> {
    let mut values = scan
        .varints
        .iter()
        .filter(|field| field.path.as_slice() == path)
        .map(|field| field.value);
    let first = values.next()?;
    values.all(|value| value == first).then_some(first)
}

fn grpc_web_data_frames(data: &[u8]) -> Vec<Vec<u8>> {
    grpc_web_frames(data, |_| None)
        .unwrap_or_default()
        .into_iter()
        .filter(|(flags, _)| flags & 0x80 == 0)
        .map(|(_, payload)| payload.to_vec())
        .collect()
}

struct ProtoScan {
    fixed32: Vec<Fixed32Field>,
    varints: Vec<VarintField>,
    order: usize,
    is_complete: bool,
}

impl Default for ProtoScan {
    fn default() -> Self {
        Self {
            fixed32: Vec::new(),
            varints: Vec::new(),
            order: 0,
            is_complete: true,
        }
    }
}

struct Fixed32Field {
    path: Vec<u64>,
    value: f32,
    order: usize,
}

struct VarintField {
    path: Vec<u64>,
    value: u64,
}

impl ProtoScan {
    fn scan_message(&mut self, data: &[u8], path: &mut Vec<u64>, depth: usize) {
        if depth > 8 {
            self.is_complete = false;
            return;
        }
        let mut i = 0;
        while i < data.len() {
            let field_start = i;
            let Some((field, wire, next)) = read_key(data, i) else {
                self.is_complete = false;
                i = field_start.saturating_add(1);
                continue;
            };
            i = next;
            path.push(field);
            let Some(next) = self.scan_field(data, i, path, depth, wire) else {
                self.is_complete = false;
                path.pop();
                i = field_start.saturating_add(1);
                continue;
            };
            i = next;
            path.pop();
        }
    }

    fn scan_field(
        &mut self,
        data: &[u8],
        i: usize,
        path: &mut Vec<u64>,
        depth: usize,
        wire: u64,
    ) -> Option<usize> {
        if (path.as_slice() == [1, 1] && wire != 5) || (is_known_billing_message(path) && wire != 2)
        {
            return None;
        }
        match wire {
            0 => self.scan_varint(data, i, path),
            2 => self.scan_length_delimited(data, i, path, depth),
            5 => self.scan_fixed32(data, i, path),
            1 => i.checked_add(8).filter(|end| *end <= data.len()),
            _ => None,
        }
    }

    fn scan_varint(&mut self, data: &[u8], i: usize, path: &[u64]) -> Option<usize> {
        let (value, next) = read_varint(data, i)?;
        self.varints.push(VarintField {
            path: path.to_vec(),
            value,
        });
        Some(next)
    }

    fn scan_length_delimited(
        &mut self,
        data: &[u8],
        i: usize,
        path: &mut Vec<u64>,
        depth: usize,
    ) -> Option<usize> {
        let (len, next) = read_varint(data, i)?;
        let start = next;
        let len_usize = usize::try_from(len).ok()?;
        let end = start.checked_add(len_usize)?;
        if end > data.len() {
            return None;
        }
        if depth < 4 && is_known_billing_message(path) {
            self.scan_message(&data[start..end], path, depth + 1);
        }
        Some(end)
    }

    fn scan_fixed32(&mut self, data: &[u8], i: usize, path: &[u64]) -> Option<usize> {
        let end = i.checked_add(4)?;
        if end > data.len() {
            return None;
        }
        let bytes = [data[i], data[i + 1], data[i + 2], data[i + 3]];
        self.fixed32.push(Fixed32Field {
            path: path.to_vec(),
            value: f32::from_le_bytes(bytes),
            order: self.order,
        });
        self.order += 1;
        Some(end)
    }
}

fn read_key(data: &[u8], i: usize) -> Option<(u64, u64, usize)> {
    let (key, next) = read_varint(data, i)?;
    let field = key >> 3;
    (field > 0 && field <= 536_870_911).then_some((field, key & 0x07, next))
}

fn read_varint(data: &[u8], mut i: usize) -> Option<(u64, usize)> {
    let mut value = 0u64;
    let mut shift = 0;
    while i < data.len() && shift < 64 {
        let b = data[i];
        i += 1;
        if shift == 63 && b > 1 {
            return None;
        }
        value |= u64::from(b & 0x7f) << shift;
        if b & 0x80 == 0 {
            return Some((value, i));
        }
        shift += 7;
    }
    None
}

fn is_known_billing_message(path: &[u64]) -> bool {
    // Only these descriptor-declared messages are recursively decoded. Other
    // length-delimited fields may be opaque bytes and must not affect billing.
    matches!(
        path,
        [1] | [1, 2]
            | [1, 3]
            | [1, 4]
            | [1, 5]
            | [1, 6]
            | [1, 7]
            | [1, 8]
            | [1, 12]
            | [1, 6, 1]
            | [1, 6, 2]
            | [1, 6, 3]
            | [1, 8, 2]
            | [1, 8, 3]
            | [1, 6, 3, 2]
            | [1, 6, 3, 3]
    )
}

#[cfg(test)]
mod tests {
    use super::super::{primary_label_for_cycle_minutes, result_from_billing};
    use super::*;

    #[test]
    fn splits_grpc_web_data_frames() {
        let data = [0, 0, 0, 0, 2, 1, 2, 0x80, 0, 0, 0, 1, b'x'];
        assert_eq!(grpc_web_data_frames(&data), vec![vec![1, 2]]);
    }

    #[test]
    fn current_period_paths_define_the_full_cycle() {
        let now = Utc.timestamp_opt(1_800_000_000, 0).single().unwrap();
        let start = now - chrono::Duration::days(25);
        let end = now + chrono::Duration::days(6);
        let scan = ProtoScan {
            fixed32: Vec::new(),
            varints: vec![
                VarintField {
                    path: vec![1, 8, 1],
                    value: 1,
                },
                VarintField {
                    path: vec![1, 8, 2, 1],
                    value: u64::try_from(start.timestamp()).unwrap(),
                },
                VarintField {
                    path: vec![1, 8, 3, 1],
                    value: u64::try_from(end.timestamp()).unwrap(),
                },
            ],
            order: 0,
            is_complete: true,
        };

        assert_eq!(
            current_period_window_minutes(&scan, now),
            Some(31 * 24 * 60)
        );
        assert_eq!(
            primary_label_for_cycle_minutes(current_period_window_minutes(&scan, now).unwrap()),
            Some("Monthly")
        );
    }

    #[test]
    fn captured_active_period_with_omitted_usage_is_zero() {
        let frame = hex_bytes(
            "00000000440a4212001a00220b0887a8c6d40610f0d7dd142a0b08879debd40610f0d7dd14".to_owned()
                + "421c0802120b0887a8c6d40610f0d7dd141a0b08879debd40610f0d7dd14580162006801"
                + "800000000f677270632d7374617475733a300d0a",
        );
        let parsed = parse_grpc_web_response_at(&frame, fixed_time(1_788_000_000)).unwrap();

        assert_eq!(parsed.used_percent, Some(0.0));
        assert!(parsed.used_percent_is_implicit_zero);
        assert!(parsed.resets_at.is_some());
        assert!(parsed.window_minutes.is_some());
    }

    #[test]
    fn complete_monthly_and_weekly_periods_support_implicit_zero() {
        for period_type in [1, 2] {
            let parsed = parse_grpc_web_response_at(
                &payload(period_type, Some(1_787_000_000), true, &[]),
                fixed_time(1_788_000_000),
            )
            .unwrap();

            assert_eq!(parsed.used_percent, Some(0.0));
            assert!(parsed.used_percent_is_implicit_zero);
            let result = result_from_billing(parsed, "grok-web", None, None, None);
            assert_eq!(result.usage.primary.used_percent, 0.0);
        }
    }

    #[test]
    fn malformed_frames_cannot_turn_unknown_usage_into_zero() {
        let malformed = [
            vec![0x00],
            fixed64_field(&[0x01]),
            vec![0x02, 0x00],
            fixed64_field(&[0x81, 0x80, 0x80, 0x80, 0x10]),
            fixed64_field(&[0x89, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x02]),
            vec![0x0d, 0x00],
            vec![0x08, 0x01],
            vec![0x72, 0x04, 0x08],
            vec![0x70, 0x80],
        ];

        for suffix in malformed {
            let parsed = parse_grpc_web_response_at(
                &payload(2, Some(1_787_000_000), true, &suffix),
                fixed_time(1_788_000_000),
            )
            .unwrap();
            assert_eq!(parsed.used_percent, None);
            assert!(!parsed.used_percent_is_implicit_zero);
        }
    }

    #[test]
    fn valid_unknown_fields_preserve_implicit_zero() {
        let unknown_fields = [fixed64_field(&[0xf9, 0xff, 0xff, 0xff, 0x0f]), {
            let mut field = vec![0x70];
            field.extend(varint(u64::MAX));
            field
        }];

        for suffix in unknown_fields {
            let parsed = parse_grpc_web_response_at(
                &payload(2, Some(1_787_000_000), true, &suffix),
                fixed_time(1_788_000_000),
            )
            .unwrap();
            assert_eq!(parsed.used_percent, Some(0.0));
            assert!(parsed.used_percent_is_implicit_zero);
        }
    }

    #[test]
    fn opaque_unknown_fields_cannot_invalidate_or_invent_billing_values() {
        let opaque_payloads = [
            fixed64_field(&[0x01]),
            vec![0x0d, 0x00, 0x00, 0x14, 0x42],
            {
                let mut field = vec![0x08];
                field.extend(varint(1_788_500_000));
                field
            },
        ];

        for at_root in [false, true] {
            for bytes in &opaque_payloads {
                let opaque_field = length_field(14, bytes);
                let mut raw = payload(
                    2,
                    Some(1_787_000_000),
                    true,
                    if at_root { &[] } else { &opaque_field },
                );
                if at_root {
                    raw.extend(opaque_field);
                }
                let parsed = parse_grpc_web_response_at(&raw, fixed_time(1_788_000_000)).unwrap();

                assert_eq!(parsed.used_percent, Some(0.0));
                assert!(parsed.used_percent_is_implicit_zero);
                assert_eq!(parsed.resets_at, Some(fixed_time(1_789_000_000)));
            }
        }
    }

    #[test]
    fn malformed_known_messages_still_prevent_implicit_zero() {
        let paths = [
            vec![1],
            vec![1, 2],
            vec![1, 3],
            vec![1, 4],
            vec![1, 5],
            vec![1, 6],
            vec![1, 7],
            vec![1, 8],
            vec![1, 12],
            vec![1, 6, 1],
            vec![1, 6, 2],
            vec![1, 6, 3],
            vec![1, 8, 2],
            vec![1, 8, 3],
            vec![1, 6, 3, 2],
            vec![1, 6, 3, 3],
        ];

        for path in paths {
            let mut raw = payload(2, Some(1_787_000_000), true, &[]);
            raw.extend(message(&path, &fixed64_field(&[0x01])));
            let parsed = parse_grpc_web_response_at(&raw, fixed_time(1_788_000_000)).unwrap();

            assert_eq!(parsed.used_percent, None, "path {path:?}");
            assert!(!parsed.used_percent_is_implicit_zero, "path {path:?}");
        }
    }

    #[test]
    fn historical_period_timestamps_remain_readable_without_current_usage() {
        let mut timestamp = vec![0x08];
        timestamp.extend(varint(1_789_000_000));
        let parsed = parse_grpc_web_response_at(
            &message(&[1, 6, 3, 3], &timestamp),
            fixed_time(1_788_000_000),
        )
        .unwrap();

        assert_eq!(parsed.resets_at, Some(fixed_time(1_789_000_000)));
        assert_eq!(parsed.used_percent, None);
        assert!(!parsed.used_percent_is_implicit_zero);
    }

    #[test]
    fn conflicting_percent_tags_remain_unknown() {
        let mut suffix = fixed32_field(20.0);
        suffix.extend(fixed32_field(30.0));
        let parsed = parse_grpc_web_response_at(
            &payload(2, Some(1_787_000_000), true, &suffix),
            fixed_time(1_788_000_000),
        )
        .unwrap();

        assert_eq!(parsed.used_percent, None);
        assert!(!parsed.used_percent_is_implicit_zero);
    }

    #[test]
    fn malformed_payload_does_not_publish_a_partial_percent() {
        let mut suffix = fixed32_field(23.5);
        suffix.extend([0x70, 0x80]);
        let parsed = parse_grpc_web_response_at(
            &payload(2, Some(1_787_000_000), true, &suffix),
            fixed_time(1_788_000_000),
        )
        .unwrap();

        assert_eq!(parsed.used_percent, None);
        assert!(!parsed.used_percent_is_wire_published);
        assert!(!parsed.used_percent_is_implicit_zero);
    }

    #[test]
    fn explicit_percent_remains_unchanged() {
        let parsed = parse_grpc_web_response_at(
            &payload(2, Some(1_787_000_000), true, &fixed32_field(23.5)),
            fixed_time(1_788_000_000),
        )
        .unwrap();

        assert_eq!(parsed.used_percent, Some(23.5));
        assert!(parsed.used_percent_is_wire_published);
        assert!(!parsed.used_percent_is_implicit_zero);
    }

    #[test]
    fn incomplete_or_non_current_periods_remain_unknown() {
        for period_type in [0, 3] {
            let parsed = parse_grpc_web_response_at(
                &payload(period_type, Some(1_787_000_000), true, &[]),
                fixed_time(1_788_000_000),
            )
            .unwrap();
            assert_eq!(parsed.used_percent, None);
            assert!(!parsed.used_percent_is_implicit_zero);
        }

        for (start, include_start) in [(Some(1_800_000_000), true), (Some(1_787_000_000), false)] {
            let parsed = parse_grpc_web_response_at(
                &payload(2, start, include_start, &[]),
                fixed_time(1_788_000_000),
            )
            .unwrap();
            assert_eq!(parsed.used_percent, None);
            assert!(!parsed.used_percent_is_implicit_zero);
        }
    }

    #[test]
    fn reset_coupons_filter_expired_records_and_sort_by_expiry() {
        let now = fixed_time(1_800_000_000);
        let mut payload = Vec::new();
        payload.extend(reset_coupon_record("later", 1_900_000_000));
        payload.extend(reset_coupon_record("expired", 1_700_000_000));
        payload.extend(reset_coupon_record("earlier", 1_850_000_000));
        payload.extend(reset_coupon_record("", 1_950_000_000));

        let coupons = parse_grpc_web_reset_coupons(&payload, now).unwrap();

        assert_eq!(
            coupons
                .iter()
                .map(|coupon| coupon.token_id.as_str())
                .collect::<Vec<_>>(),
            ["earlier", "later"]
        );
        assert!(coupons.iter().all(|coupon| coupon.expires_at > now));
    }

    #[test]
    fn reset_coupon_empty_payload_is_valid_and_malformed_payload_is_atomic() {
        assert!(
            parse_grpc_web_reset_coupons(&[0, 0, 0, 0, 0], fixed_time(1_800_000_000))
                .unwrap()
                .is_empty()
        );

        let mut malformed = reset_coupon_record("valid", 1_900_000_000);
        malformed.extend([0x52, 0x05, b'a']);
        assert!(parse_grpc_web_reset_coupons(&malformed, fixed_time(1_800_000_000)).is_err());
    }

    #[test]
    fn reset_coupon_truncated_timestamp_is_rejected() {
        let mut record = length_field(10, b"valid");
        record.extend([0xf2, 0x01, 0x01, 0x08]);
        let payload = length_field(10, &record);

        assert!(parse_grpc_web_reset_coupons(&payload, fixed_time(1_800_000_000)).is_err());
    }

    #[test]
    fn reset_coupon_nonzero_grpc_web_trailer_is_rejected() {
        let payload = reset_coupon_record("valid", 1_900_000_000);
        let mut framed = grpc_web_frame(0, &payload);
        framed.extend(grpc_web_frame(0x80, b"grpc-status: 13\r\n"));

        assert!(matches!(
            parse_grpc_web_reset_coupons(&framed, fixed_time(1_800_000_000)),
            Err(ProviderError::Other(message)) if message.contains("status 13")
        ));
    }

    #[test]
    fn reset_coupon_zero_grpc_web_trailer_is_accepted() {
        let payload = reset_coupon_record("valid", 1_900_000_000);
        let mut framed = grpc_web_frame(0, &payload);
        framed.extend(grpc_web_frame(0x80, b"grpc-status: 0\r\n"));

        let coupons = parse_grpc_web_reset_coupons(&framed, fixed_time(1_800_000_000)).unwrap();
        assert_eq!(coupons.len(), 1);
        assert_eq!(coupons[0].token_id, "valid");
    }

    fn reset_coupon_record(token_id: &str, expires_at: u64) -> Vec<u8> {
        let timestamp = {
            let mut bytes = vec![0x08];
            bytes.extend(varint(expires_at));
            bytes
        };
        let mut record = length_field(10, token_id.as_bytes());
        record.extend(length_field(30, &timestamp));
        length_field(10, &record)
    }

    fn fixed_time(seconds: i64) -> DateTime<Utc> {
        Utc.timestamp_opt(seconds, 0).single().unwrap()
    }

    fn payload(period_type: u8, start: Option<u64>, include_start: bool, suffix: &[u8]) -> Vec<u8> {
        let mut period = vec![0x08, period_type];
        if include_start {
            let start = start.expect("start timestamp required");
            let mut timestamp = vec![0x08];
            timestamp.extend(varint(start));
            period.extend(length_field(2, &timestamp));
        }
        let mut end_timestamp = vec![0x08];
        end_timestamp.extend(varint(1_789_000_000));
        period.extend(length_field(3, &end_timestamp));

        let mut config = length_field(8, &period);
        config.extend(suffix);
        length_field(1, &config)
    }

    fn message(path: &[u64], contents: &[u8]) -> Vec<u8> {
        path.iter().rev().fold(contents.to_vec(), |payload, field| {
            length_field(*field, &payload)
        })
    }

    fn length_field(field: u64, contents: &[u8]) -> Vec<u8> {
        let mut encoded = varint((field << 3) | 2);
        encoded.extend(varint(contents.len() as u64));
        encoded.extend(contents);
        encoded
    }

    fn grpc_web_frame(flags: u8, payload: &[u8]) -> Vec<u8> {
        let mut frame = vec![flags];
        frame.extend(
            u32::try_from(payload.len())
                .expect("test gRPC-web payload length fits u32")
                .to_be_bytes(),
        );
        frame.extend(payload);
        frame
    }

    fn fixed32_field(value: f32) -> Vec<u8> {
        let mut encoded = vec![0x0d];
        encoded.extend(value.to_le_bytes());
        encoded
    }

    fn fixed64_field(tag: &[u8]) -> Vec<u8> {
        let mut encoded = tag.to_vec();
        encoded.extend([0; 8]);
        encoded
    }

    fn varint(mut value: u64) -> Vec<u8> {
        let mut encoded = Vec::new();
        while value >= 0x80 {
            encoded.push(u8::try_from(value & 0x7f).expect("masked varint byte fits u8") | 0x80);
            value >>= 7;
        }
        encoded.push(u8::try_from(value).expect("terminal varint byte fits u8"));
        encoded
    }

    fn hex_bytes(hex: String) -> Vec<u8> {
        let (pairs, remainder) = hex.as_bytes().as_chunks::<2>();
        assert!(remainder.is_empty(), "hex fixture length must be even");
        pairs
            .iter()
            .map(|chunk| {
                let text = std::str::from_utf8(chunk).unwrap();
                u8::from_str_radix(text, 16).unwrap()
            })
            .collect()
    }
}
