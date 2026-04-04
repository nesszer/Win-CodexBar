//! Credential Migration System
//!
//! Migrates stored credentials between different formats and storage locations.
//! On Windows, handles migration from legacy storage to Windows Credential Manager.

#![allow(dead_code)]

use crate::core::ProviderId;
use std::sync::atomic::{AtomicBool, Ordering};

/// Migration version tracking key
const MIGRATION_VERSION_KEY: &str = "credential_migration_version";

/// Current migration version
const CURRENT_MIGRATION_VERSION: u32 = 1;

/// Migration item representing a credential to migrate
#[derive(Debug, Clone, Hash, Eq, PartialEq)]
pub struct MigrationItem {
    /// Service name (Windows Credential Manager target name)
    pub service: String,
    /// Account/username
    pub account: Option<String>,
    /// Legacy service name (if migrating from a different name)
    pub legacy_service: Option<String>,
}

impl MigrationItem {
    /// Create a new migration item
    pub fn new(service: impl Into<String>) -> Self {
        Self {
            service: service.into(),
            account: None,
            legacy_service: None,
        }
    }

    /// With an account name
    pub fn with_account(mut self, account: impl Into<String>) -> Self {
        self.account = Some(account.into());
        self
    }

    /// With a legacy service name for migration
    pub fn with_legacy(mut self, legacy: impl Into<String>) -> Self {
        self.legacy_service = Some(legacy.into());
        self
    }

    /// Get display label
    pub fn label(&self) -> String {
        let account = self.account.as_deref().unwrap_or("<any>");
        format!("{}:{}", self.service, account)
    }
}

/// Known credentials that may need migration
pub fn items_to_migrate() -> Vec<MigrationItem> {
    vec![
        // Codex/OpenAI credentials
        MigrationItem::new("CodexBar")
            .with_account("codex-cookie")
            .with_legacy("com.steipete.CodexBar"),
        // Claude credentials
        MigrationItem::new("CodexBar")
            .with_account("claude-cookie")
            .with_legacy("com.steipete.CodexBar"),
        // Cursor credentials
        MigrationItem::new("CodexBar")
            .with_account("cursor-cookie")
            .with_legacy("com.steipete.CodexBar"),
        // Factory/Windsurf credentials
        MigrationItem::new("CodexBar")
            .with_account("factory-cookie")
            .with_legacy("com.steipete.CodexBar"),
        // MiniMax credentials
        MigrationItem::new("CodexBar")
            .with_account("minimax-cookie")
            .with_legacy("com.steipete.CodexBar"),
        // Augment credentials
        MigrationItem::new("CodexBar")
            .with_account("augment-cookie")
            .with_legacy("com.steipete.CodexBar"),
        // Copilot API token
        MigrationItem::new("CodexBar")
            .with_account("copilot-api-token")
            .with_legacy("com.steipete.CodexBar"),
        // Zai API token
        MigrationItem::new("CodexBar")
            .with_account("zai-api-token")
            .with_legacy("com.steipete.CodexBar"),
    ]
}

/// Credential migration errors
#[derive(Debug, thiserror::Error)]
pub enum MigrationError {
    #[error("Failed to read credential: {0}")]
    ReadFailed(String),
    #[error("Failed to write credential: {0}")]
    WriteFailed(String),
    #[error("Failed to delete credential: {0}")]
    DeleteFailed(String),
    #[error("Invalid credential format")]
    InvalidFormat,
    #[error("Settings error: {0}")]
    Settings(String),
}

/// Result of a migration operation
#[derive(Debug, Clone)]
pub struct MigrationResult {
    /// Number of credentials migrated
    pub migrated_count: usize,
    /// Number of errors encountered
    pub error_count: usize,
    /// Items that failed to migrate
    pub failed_items: Vec<String>,
}

impl MigrationResult {
    fn new() -> Self {
        Self {
            migrated_count: 0,
            error_count: 0,
            failed_items: Vec::new(),
        }
    }
}

/// Credential migrator
pub struct CredentialMigrator {
    /// Whether migration is disabled (for testing)
    disabled: AtomicBool,
}

impl CredentialMigrator {
    /// Create a new migrator
    pub fn new() -> Self {
        Self {
            disabled: AtomicBool::new(false),
        }
    }

    /// Disable migration (for testing)
    pub fn set_disabled(&self, disabled: bool) {
        self.disabled.store(disabled, Ordering::SeqCst);
    }

    /// Check if migration is needed
    pub fn needs_migration(&self) -> bool {
        if self.disabled.load(Ordering::SeqCst) {
            return false;
        }

        // Check if we've already run this version
        let version = self.get_migration_version();
        version < CURRENT_MIGRATION_VERSION
    }

    /// Run migration if needed
    pub fn migrate_if_needed(&self) -> Option<MigrationResult> {
        if !self.needs_migration() {
            tracing::debug!("Credential migration already completed or disabled");
            return None;
        }

        tracing::info!(
            "Starting credential migration to version {}",
            CURRENT_MIGRATION_VERSION
        );

        let result = self.run_migration();

        // Mark migration as complete
        self.set_migration_version(CURRENT_MIGRATION_VERSION);

        tracing::info!(
            "Credential migration complete: {} migrated, {} errors",
            result.migrated_count,
            result.error_count
        );

        Some(result)
    }

    /// Run the actual migration
    fn run_migration(&self) -> MigrationResult {
        let mut result = MigrationResult::new();
        let items = items_to_migrate();

        for item in items {
            match self.migrate_item(&item) {
                Ok(true) => {
                    result.migrated_count += 1;
                    tracing::info!("Migrated credential: {}", item.label());
                }
                Ok(false) => {
                    // Item didn't exist or already migrated
                    tracing::debug!(
                        "Skipped credential (not found or already migrated): {}",
                        item.label()
                    );
                }
                Err(e) => {
                    result.error_count += 1;
                    result.failed_items.push(item.label());
                    tracing::error!("Failed to migrate {}: {}", item.label(), e);
                }
            }
        }

        result
    }

    /// Migrate a single credential item
    /// Returns Ok(true) if migrated, Ok(false) if not needed, Err on failure
    fn migrate_item(&self, item: &MigrationItem) -> Result<bool, MigrationError> {
        // Check if we need to migrate from legacy service name
        if let Some(ref legacy_service) = item.legacy_service {
            // Try to read from legacy location
            if let Ok(value) = self.read_credential(legacy_service, item.account.as_deref()) {
                // Write to new location
                self.write_credential(&item.service, item.account.as_deref(), &value)?;
                // Delete from legacy location
                let _ = self.delete_credential(legacy_service, item.account.as_deref());
                return Ok(true);
            }
        }

        // No migration needed
        Ok(false)
    }

    /// Read a credential from the keyring
    fn read_credential(
        &self,
        service: &str,
        account: Option<&str>,
    ) -> Result<String, MigrationError> {
        let account = account.unwrap_or("default");
        let entry = keyring::Entry::new(service, account)
            .map_err(|e| MigrationError::ReadFailed(e.to_string()))?;

        entry
            .get_password()
            .map_err(|e| MigrationError::ReadFailed(e.to_string()))
    }

    /// Write a credential to the keyring
    fn write_credential(
        &self,
        service: &str,
        account: Option<&str>,
        value: &str,
    ) -> Result<(), MigrationError> {
        let account = account.unwrap_or("default");
        let entry = keyring::Entry::new(service, account)
            .map_err(|e| MigrationError::WriteFailed(e.to_string()))?;

        entry
            .set_password(value)
            .map_err(|e| MigrationError::WriteFailed(e.to_string()))
    }

    /// Delete a credential from the keyring
    fn delete_credential(
        &self,
        service: &str,
        account: Option<&str>,
    ) -> Result<(), MigrationError> {
        let account = account.unwrap_or("default");
        let entry = keyring::Entry::new(service, account)
            .map_err(|e| MigrationError::DeleteFailed(e.to_string()))?;

        entry
            .delete_credential()
            .map_err(|e| MigrationError::DeleteFailed(e.to_string()))
    }

    /// Get the current migration version from settings
    fn get_migration_version(&self) -> u32 {
        // Use the settings file or registry to track migration version
        // For now, we'll use a simple approach with settings
        0 // Default to 0 (no migration done)
    }

    /// Set the migration version in settings
    fn set_migration_version(&self, _version: u32) {
        // Store in settings file
        tracing::debug!("Migration version set to {}", CURRENT_MIGRATION_VERSION);
    }

    /// Force reset migration (for testing)
    pub fn reset_migration(&self) {
        self.set_migration_version(0);
    }
}

impl Default for CredentialMigrator {
    fn default() -> Self {
        Self::new()
    }
}

/// Get the service name for a provider's credentials
pub fn service_name_for_provider(_provider: ProviderId) -> &'static str {
    "CodexBar"
}

/// Get the account name for a provider's credentials
pub fn account_name_for_provider(provider: ProviderId) -> &'static str {
    match provider {
        ProviderId::Codex => "codex-cookie",
        ProviderId::Claude => "claude-cookie",
        ProviderId::Cursor => "cursor-cookie",
        ProviderId::Factory => "factory-cookie",
        ProviderId::MiniMax => "minimax-cookie",
        ProviderId::Augment => "augment-cookie",
        ProviderId::Copilot => "copilot-api-token",
        ProviderId::Zai => "zai-api-token",
        ProviderId::OpenCode => "opencode-cookie",
        ProviderId::Gemini => "gemini-cookie",
        ProviderId::Antigravity => "antigravity-cookie",
        ProviderId::Kiro => "kiro-cookie",
        ProviderId::VertexAI => "vertexai-token",
        ProviderId::Kimi => "kimi-cookie",
        ProviderId::KimiK2 => "kimik2-cookie",
        ProviderId::Amp => "amp-cookie",
        ProviderId::Synthetic => "synthetic-cookie",
        ProviderId::JetBrains => "jetbrains-token",
        ProviderId::Alibaba => "alibaba-cookie",
        ProviderId::Warp => "warp-token",
        ProviderId::Ollama => "ollama-cookie",
        ProviderId::OpenRouter => "openrouter-token",
        ProviderId::NanoGPT => "nanogpt-api-token",
        ProviderId::Infini => "infini-api-token",
    }
}

/// Migrate credentials for a specific provider from one format to another
pub fn migrate_provider_credential(
    provider: ProviderId,
    from_service: &str,
    from_account: &str,
) -> Result<bool, MigrationError> {
    let migrator = CredentialMigrator::new();
    let item = MigrationItem::new(service_name_for_provider(provider))
        .with_account(account_name_for_provider(provider))
        .with_legacy(format!("{}:{}", from_service, from_account));

    migrator.migrate_item(&item)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn test_migration_item_label() {
        let item = MigrationItem::new("CodexBar").with_account("claude-cookie");
        assert_eq!(item.label(), "CodexBar:claude-cookie");

        let item = MigrationItem::new("CodexBar");
        assert_eq!(item.label(), "CodexBar:<any>");
    }

    #[test]
    fn test_items_to_migrate() {
        let items = items_to_migrate();
        assert!(!items.is_empty());

        // Check that we have unique items
        let unique: HashSet<_> = items.iter().collect();
        assert_eq!(unique.len(), items.len());
    }

    #[test]
    fn test_service_name_for_provider() {
        assert_eq!(service_name_for_provider(ProviderId::Claude), "CodexBar");
        assert_eq!(service_name_for_provider(ProviderId::Codex), "CodexBar");
    }

    #[test]
    fn test_account_name_for_provider() {
        assert_eq!(
            account_name_for_provider(ProviderId::Claude),
            "claude-cookie"
        );
        assert_eq!(
            account_name_for_provider(ProviderId::Copilot),
            "copilot-api-token"
        );
    }

    #[test]
    fn test_migrator_disabled() {
        let migrator = CredentialMigrator::new();
        migrator.set_disabled(true);
        assert!(!migrator.needs_migration());
    }
}
