use std::io::Write as _;
use std::path::{Path, PathBuf};

/// Atomic file write: temp sibling + fsync + rename. On failure, truncate the
/// owned temp sibling instead of deleting it so callers never need destructive cleanup.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> anyhow::Result<()> {
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    if !parent.is_dir() {
        anyhow::bail!(
            "output directory does not exist: {} (it is not created)",
            parent.display()
        );
    }
    let mut temp_name = path.as_os_str().to_os_string();
    temp_name.push(format!(".tmp-{}", std::process::id()));
    let temp = PathBuf::from(temp_name);
    let result = (|| -> anyhow::Result<()> {
        let mut file = std::fs::File::create(&temp)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        std::fs::rename(&temp, path)?;
        Ok(())
    })();
    if result.is_err()
        && let Ok(file) = std::fs::OpenOptions::new().write(true).open(&temp)
    {
        let _truncated = file.set_len(0);
    }
    result
}
