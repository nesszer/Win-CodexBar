//! UTF-16 command-line and environment-block encoding for `CreateProcessW`.

use super::*;

/// NUL-terminated `CreateProcessW` command line: `program` and `args`, each
/// quoted with the MSDN rules. Also used by `host::console_launch`.
pub(crate) fn build_command_line(
    program: &Path,
    args: &[OsString],
) -> ManagedProcessResult<Vec<u16>> {
    let mut cmdline = Vec::new();
    append_quoted(program.as_os_str(), &mut cmdline)?;
    for arg in args {
        cmdline.push(b' ' as u16);
        append_quoted(arg, &mut cmdline)?;
    }
    cmdline.push(0);
    Ok(cmdline)
}

pub(super) fn build_environment_block(
    overrides: &[(OsString, OsString)],
) -> ManagedProcessResult<Vec<u16>> {
    let mut values: Vec<(OsString, OsString)> = std::env::vars_os().collect();
    for (key, value) in overrides {
        values.retain(|(existing, _)| {
            !existing
                .to_string_lossy()
                .eq_ignore_ascii_case(&key.to_string_lossy())
        });
        values.push((key.clone(), value.clone()));
    }
    // CreateProcessW expects Unicode environment blocks sorted case-insensitively.
    values.sort_by_cached_key(|(key, _)| key.to_string_lossy().to_uppercase());
    let mut block = Vec::new();
    for (key, value) in values {
        let mut entry = key;
        entry.push("=");
        entry.push(value);
        block.extend(encode_wide(&entry)?);
        block.push(0);
    }
    block.push(0);
    Ok(block)
}

fn encode_wide(value: &OsStr) -> ManagedProcessResult<Vec<u16>> {
    let wide: Vec<u16> = value.encode_wide().collect();
    if wide.contains(&0) {
        return Err(ManagedProcessError::new(
            "Process argument contains an embedded NUL".to_string(),
        ));
    }
    Ok(wide)
}

pub(super) fn encode_wide_nul(value: &OsStr) -> ManagedProcessResult<Vec<u16>> {
    let mut wide = encode_wide(value)?;
    wide.push(0);
    Ok(wide)
}

/// Quote one argument using the MSDN command-line rules.
fn append_quoted(value: &OsStr, output: &mut Vec<u16>) -> ManagedProcessResult<()> {
    let value = encode_wide(value)?;
    let needs_quotes = value.is_empty()
        || value
            .iter()
            .any(|code| matches!(*code, 0x20 | 0x09 | 0x0a | 0x0b | 0x22));
    if !needs_quotes {
        output.extend(value);
        return Ok(());
    }

    output.push(b'"' as u16);
    let mut backslashes = 0_usize;
    for code in value {
        if code == b'\\' as u16 {
            backslashes += 1;
            continue;
        }
        let trailing = if code == b'"' as u16 {
            backslashes * 2 + 1
        } else {
            backslashes
        };
        output.extend(std::iter::repeat_n(b'\\' as u16, trailing));
        output.push(code);
        backslashes = 0;
    }
    output.extend(std::iter::repeat_n(b'\\' as u16, backslashes * 2));
    output.push(b'"' as u16);
    Ok(())
}
