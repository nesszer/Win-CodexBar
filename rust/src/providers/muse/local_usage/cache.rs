//! Disk cache for the Muse local token history scan.

use super::*;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct FileStamp {
    pub(crate) identity: String,
    pub(crate) length: u64,
    pub(crate) modified_ms: u128,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct CachedFile {
    pub(crate) stamp: FileStamp,
    pub(crate) events: Vec<Event>,
    pub(crate) complete: bool,
    pub(crate) digest: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub(crate) struct Cache {
    pub(crate) version: u32,
    pub(crate) sessions_root: String,
    pub(crate) since_day: String,
    pub(crate) until_day: String,
    pub(crate) timezone: String,
    pub(crate) files: BTreeMap<String, CachedFile>,
}
pub(crate) fn file_stamp(path: &Path) -> Option<FileStamp> {
    let metadata = fs::metadata(path).ok()?;
    Some(FileStamp {
        identity: platform_file_identity(path, &metadata)?,
        length: metadata.len(),
        modified_ms: metadata
            .modified()
            .ok()?
            .duration_since(UNIX_EPOCH)
            .ok()?
            .as_millis(),
    })
}
pub(crate) fn platform_file_identity(path: &Path, _metadata: &fs::Metadata) -> Option<String> {
    use std::os::windows::io::AsRawHandle;

    use windows::Win32::Foundation::HANDLE;
    use windows::Win32::Storage::FileSystem::{
        BY_HANDLE_FILE_INFORMATION, GetFileInformationByHandle,
    };

    let file = File::open(path).ok()?;
    let mut info = BY_HANDLE_FILE_INFORMATION::default();
    // SAFETY: The file handle is open for the duration of this call and the
    // output structure is valid for writes.
    let ok = unsafe { GetFileInformationByHandle(HANDLE(file.as_raw_handle()), &mut info) };
    if ok.is_err() {
        return None;
    }
    let file_index = ((info.nFileIndexHigh as u64) << 32) | info.nFileIndexLow as u64;
    Some(format!("{}:{file_index}", info.dwVolumeSerialNumber))
}
pub(crate) fn digest_bytes(
    path: &Path,
    stamp: &FileStamp,
    state: &mut ScanState,
) -> Option<String> {
    if !state.charge_file(stamp.length) {
        return None;
    }
    let mut file = File::open(path).ok()?;
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    let mut bytes_read = 0_u64;
    loop {
        let read = file.read(&mut buffer).ok()?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
        bytes_read = bytes_read.checked_add(u64::try_from(read).ok()?)?;
        if state.check() {
            return None;
        }
    }
    if bytes_read != stamp.length || file_stamp(path).as_ref() != Some(stamp) {
        return None;
    }
    Some(format!("{:x}", hasher.finalize()))
}
pub(crate) fn cache_path(root: &Path) -> PathBuf {
    root.join("cost-usage").join(CACHE_FILE)
}
pub(crate) fn load_cache(root: &Path, sessions: &Path, since: &str, until: &str) -> Cache {
    let path = cache_path(root);
    let Ok(metadata) = fs::metadata(&path) else {
        return Cache::default();
    };
    if metadata.len() > MAX_CACHE_BYTES {
        return Cache::default();
    }
    let Ok(bytes) = fs::read(&path) else {
        return Cache::default();
    };
    let Ok(cache) = serde_json::from_slice::<Cache>(&bytes) else {
        return Cache::default();
    };
    if cache.version == CACHE_VERSION
        && cache.sessions_root == sessions.to_string_lossy()
        && cache.since_day == since
        && cache.until_day == until
        && cache.timezone == Local::now().offset().to_string()
    {
        cache
    } else {
        Cache::default()
    }
}
pub(crate) fn save_cache(root: &Path, sessions: &Path, since: &str, until: &str, mut cache: Cache) {
    cache.version = CACHE_VERSION;
    cache.sessions_root = sessions.to_string_lossy().into_owned();
    cache.since_day = since.to_string();
    cache.until_day = until.to_string();
    cache.timezone = Local::now().offset().to_string();
    let Ok(bytes) = serde_json::to_vec(&cache) else {
        return;
    };
    if bytes.len() as u64 > MAX_CACHE_BYTES {
        return;
    }
    let path = cache_path(root);
    let Some(parent) = path.parent() else { return };
    if fs::create_dir_all(parent).is_err() {
        return;
    }
    if let Err(error) = crate::atomic_file::write_atomic(&path, &bytes) {
        tracing::debug!(?error, "failed to write Muse usage cache");
    }
}
