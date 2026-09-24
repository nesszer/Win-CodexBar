//! Small test-only helpers that avoid adding runtime or dev dependencies.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

const TEMP_PREFIX: &str = "win-codexbar-test-";

pub struct TempDir {
    path: PathBuf,
}

impl TempDir {
    pub fn new() -> Self {
        let root = fs::canonicalize(std::env::temp_dir())
            .expect("system temporary directory must be available");
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock must be after Unix epoch")
            .as_nanos();
        for attempt in 0..32 {
            let path = root.join(format!(
                "{TEMP_PREFIX}{}-{nonce}-{attempt}",
                std::process::id()
            ));
            match fs::create_dir(&path) {
                Ok(()) => return Self { path },
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(error) => panic!("cannot create test directory {}: {error}", path.display()),
            }
        }
        panic!(
            "could not allocate a unique test directory under {}",
            root.display()
        );
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let Ok(temp_root) = fs::canonicalize(std::env::temp_dir()) else {
            return;
        };
        let Ok(canonical) = fs::canonicalize(&self.path) else {
            return;
        };
        if canonical.parent() != Some(temp_root.as_path())
            || !canonical
                .file_name()
                .is_some_and(|name| name.to_string_lossy().starts_with(TEMP_PREFIX))
        {
            return;
        }
        let Ok(metadata) = fs::symlink_metadata(&self.path) else {
            return;
        };
        if metadata.file_type().is_symlink() || is_reparse_point(&metadata) {
            return;
        }
        let _ = fs::remove_dir_all(canonical);
    }
}

#[cfg(windows)]
fn is_reparse_point(metadata: &fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    metadata.file_attributes() & 0x400 != 0
}

#[cfg(not(windows))]
fn is_reparse_point(_metadata: &fs::Metadata) -> bool {
    false
}
