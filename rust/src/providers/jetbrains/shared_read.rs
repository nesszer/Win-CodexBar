//! Shared-read file access for JetBrains IDE files.
//!
//! A running IDE keeps `idea.log` open while it writes. On Windows a reader
//! can only open such a file when its share mode covers the writer's access,
//! so every JetBrains file read goes through [`open_shared_read`], which
//! accepts concurrent read, write, and delete access. Callers still bound how
//! many bytes they read.

use std::fs::{File, OpenOptions};
use std::path::Path;

#[cfg(windows)]
const FILE_SHARE_READ: u32 = 0x0000_0001;
#[cfg(windows)]
const FILE_SHARE_WRITE: u32 = 0x0000_0002;
#[cfg(windows)]
const FILE_SHARE_DELETE: u32 = 0x0000_0004;

/// Open `path` for reading without blocking the IDE's own writes or rotation.
pub(super) fn open_shared_read(path: &Path) -> std::io::Result<File> {
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE);
    }
    options.open(path)
}
