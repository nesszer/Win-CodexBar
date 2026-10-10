//! Loading, migrating and saving `settings.json`.

use super::*;

impl Settings {
    /// Get the settings file path (`CODEXBAR_CONFIG` when the CLI applied it).
    pub fn settings_path() -> Option<PathBuf> {
        config_path::settings_file_for(settings_file_override(), crate::logging::config_root())
    }

    /// Load settings from disk
    pub fn load() -> Self {
        let path = Self::settings_path();
        #[allow(
            unused_mut,
            reason = "mutability is needed for conditional initialization paths that the compiler cannot prove"
        )]
        let mut settings = Self::load_from_path(path.as_deref());

        // Sync autostart toggle with actual registry state and repair stale commands from older builds.
        // A CLI run on a `CODEXBAR_CONFIG` file skips these desktop integrations, so it touches
        // neither the Run key nor the default config root.
        #[cfg(target_os = "windows")]
        if settings_file_override().is_none() {
            settings.start_at_login = Self::sync_start_at_login_registry();
            settings.apply_promote_tray_default_migration();
        }

        // One-shot migration: materialize the persisted hidden usage-item list
        // from the pre-0.62 per-provider visibility flags. After this the list
        // is the sole source of truth and the flags stay untouched.
        settings.migrate_legacy_usage_item_flags();

        settings
    }

    pub(super) fn load_from_path(path: Option<&Path>) -> Self {
        match path {
            Some(path) if path.exists() => match crate::secure_file::read_string(path) {
                Ok(content) => {
                    serde_json::from_str(content.trim_start_matches('\u{feff}')).unwrap_or_default()
                }
                Err(_) => Self::default(),
            },
            _ => Self::default(),
        }
    }

    /// Materialize `hidden_usage_item_ids` from the pre-0.62 per-provider
    /// visibility flags where the list was never persisted. Idempotent: a
    /// provider with an explicit list is left alone.
    pub(super) fn migrate_legacy_usage_item_flags(&mut self) {
        if self
            .provider_config(ProviderId::Codex)
            .and_then(|config| config.hidden_usage_item_ids.as_ref())
            .is_none()
            && !self.spark_usage_visible(ProviderId::Codex)
        {
            self.toggle_hidden_items(ProviderId::Codex, &CODEX_SPARK_USAGE_ITEM_IDS, false);
        }
        if self
            .provider_config(ProviderId::Claude)
            .and_then(|config| config.hidden_usage_item_ids.as_ref())
            .is_none()
            && !self.claude_daily_routines_usage_visible
        {
            self.toggle_hidden_items(
                ProviderId::Claude,
                &[CLAUDE_DAILY_ROUTINES_USAGE_ITEM_ID],
                false,
            );
        }
    }

    /// Marker written after the one-shot "pin tray by default" migration (issue #237).
    pub(super) fn promote_tray_default_marker_path() -> Option<PathBuf> {
        crate::logging::config_root().map(|p| p.join(".tray-pin-default-v1"))
    }

    /// Old builds defaulted `promote_tray_icon` to false and persisted that on any
    /// settings save. Flip those installs to the new default once; later opt-outs
    /// are preserved because the marker file remains.
    pub(super) fn should_migrate_promote_tray_default(
        promote_tray_icon: bool,
        already_migrated: bool,
    ) -> bool {
        !already_migrated && !promote_tray_icon
    }

    pub(super) fn apply_promote_tray_default_migration(&mut self) {
        let Some(marker) = Self::promote_tray_default_marker_path() else {
            return;
        };
        let already_migrated = marker.exists();
        if Self::should_migrate_promote_tray_default(self.promote_tray_icon, already_migrated) {
            self.promote_tray_icon = true;
            if let Err(error) = self.save() {
                tracing::warn!("Failed to persist promote_tray_icon default migration: {error}");
            }
        }
        if !already_migrated && let Some(parent) = marker.parent() {
            // Best-effort marker dir creation; the write below reports failure.
            let _created_dir = std::fs::create_dir_all(parent);
            if let Err(error) = std::fs::write(&marker, b"1") {
                tracing::warn!("Failed to write promote_tray_icon migration marker: {error}");
            }
        }
    }

    /// Save settings to disk
    pub fn save(&self) -> anyhow::Result<()> {
        let path = Self::settings_path()
            .ok_or_else(|| anyhow::anyhow!("Could not determine settings path"))?;

        self.save_to_path(&path)
    }

    pub(super) fn save_to_path(&self, path: &Path) -> anyhow::Result<()> {
        // Ensure parent directory exists
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let json = serde_json::to_string_pretty(self)?;
        crate::secure_file::write_string(path, &json)?;

        Ok(())
    }
}
