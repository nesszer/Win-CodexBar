use std::path::PathBuf;

use crate::core::ProviderError;

pub(super) fn locate_agy_binary() -> Result<Option<PathBuf>, ProviderError> {
    resolve_agy_binary(
        std::env::var_os("ANTIGRAVITY_CLI_PATH").map(PathBuf::from),
        || {
            agy_binary_candidates(
                which::which("agy").ok(),
                std::env::var_os("LOCALAPPDATA").map(PathBuf::from),
                dirs::home_dir(),
            )
        },
    )
}

fn validate_agy_binary_override(
    explicit: Option<PathBuf>,
) -> Result<Option<PathBuf>, ProviderError> {
    let Some(path) = explicit else {
        return Ok(None);
    };
    if path.is_file() {
        Ok(Some(path))
    } else {
        Err(ProviderError::NotInstalled(format!(
            "ANTIGRAVITY_CLI_PATH is set but does not point to a usable agy file: {}. Fix or unset the variable; automatic CLI discovery is disabled while it is set.",
            path.display()
        )))
    }
}

fn resolve_agy_binary<F>(
    explicit: Option<PathBuf>,
    discover: F,
) -> Result<Option<PathBuf>, ProviderError>
where
    F: FnOnce() -> Vec<PathBuf>,
{
    if let Some(path) = validate_agy_binary_override(explicit)? {
        return Ok(Some(path));
    }
    Ok(discover().into_iter().find(|path| path.is_file()))
}

fn agy_binary_candidates(
    path_lookup: Option<PathBuf>,
    local_app_data: Option<PathBuf>,
    home: Option<PathBuf>,
) -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    if let Some(path) = path_lookup {
        candidates.push(path);
    }
    if let Some(root) = local_app_data {
        candidates.push(root.join("agy").join("bin").join("agy.exe"));
    }
    if let Some(root) = home {
        candidates.push(root.join(".local").join("bin").join(if cfg!(windows) {
            "agy.exe"
        } else {
            "agy"
        }));
    }
    candidates
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn candidates_prefer_path_then_known_installs() {
        let path_lookup = PathBuf::from(r"C:\path\agy.exe");
        let local_app_data = PathBuf::from(r"C:\Users\test\AppData\Local");
        let home = PathBuf::from(r"C:\Users\test");

        let candidates = agy_binary_candidates(
            Some(path_lookup.clone()),
            Some(local_app_data.clone()),
            Some(home.clone()),
        );

        assert_eq!(candidates[0], path_lookup);
        assert_eq!(
            candidates[1],
            local_app_data.join("agy").join("bin").join("agy.exe")
        );
        assert_eq!(
            candidates[2],
            home.join(".local")
                .join("bin")
                .join(if cfg!(windows) { "agy.exe" } else { "agy" })
        );
    }

    #[test]
    fn usable_override_is_selected_without_discovery() {
        let temp = tempfile::tempdir().expect("temporary directory");
        let override_path = temp.path().join("configured-agy.exe");
        std::fs::write(&override_path, b"test executable placeholder").expect("write fixture");

        let resolved = resolve_agy_binary(Some(override_path.clone()), || {
            panic!("a configured override must not trigger automatic discovery")
        })
        .expect("usable override should resolve");

        assert_eq!(resolved, Some(override_path));
    }

    #[test]
    fn unusable_override_fails_without_automatic_discovery() {
        let temp = tempfile::tempdir().expect("temporary directory");
        let missing_override = temp.path().join("missing-agy.exe");

        let error = resolve_agy_binary(Some(missing_override), || {
            panic!("an invalid configured override must block automatic discovery")
        })
        .expect_err("invalid override should fail closed");

        let message = error.to_string();
        assert!(message.contains("ANTIGRAVITY_CLI_PATH is set"));
        assert!(message.contains("automatic CLI discovery is disabled"));
    }

    #[test]
    fn unset_override_preserves_automatic_discovery() {
        let temp = tempfile::tempdir().expect("temporary directory");
        let discovered_path = temp.path().join("discovered-agy.exe");
        std::fs::write(&discovered_path, b"test executable placeholder").expect("write fixture");

        let resolved = resolve_agy_binary(None, || {
            vec![
                temp.path().join("missing-first.exe"),
                discovered_path.clone(),
            ]
        })
        .expect("automatic discovery should resolve");

        assert_eq!(resolved, Some(discovered_path));
    }
}
