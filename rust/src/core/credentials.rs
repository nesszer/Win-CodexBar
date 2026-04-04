//! Credential storage abstraction

use thiserror::Error;

/// Errors that can occur with credential operations
#[derive(Debug, Error)]
pub enum CredentialError {
    #[error("Credential not found")]
    NotFound,

    #[error("Access denied")]
    AccessDenied,

    #[error("Storage error: {0}")]
    Storage(String),

    #[error("Invalid credential format")]
    InvalidFormat,
}

/// Trait for credential storage backends
pub trait CredentialStore: Send + Sync {
    /// Get a credential by key
    fn get(&self, service: &str, key: &str) -> Result<String, CredentialError>;

    /// Set a credential
    fn set(&self, service: &str, key: &str, value: &str) -> Result<(), CredentialError>;

    /// Delete a credential
    fn delete(&self, service: &str, key: &str) -> Result<(), CredentialError>;
}

/// Windows Credential Manager implementation
#[cfg(windows)]
pub struct WindowsCredentialStore;

#[cfg(windows)]
impl WindowsCredentialStore {
    pub fn new() -> Self {
        Self
    }
}

#[cfg(windows)]
impl Default for WindowsCredentialStore {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(windows)]
impl CredentialStore for WindowsCredentialStore {
    fn get(&self, service: &str, key: &str) -> Result<String, CredentialError> {
        let entry = keyring::Entry::new(service, key)
            .map_err(|e| CredentialError::Storage(e.to_string()))?;
        entry.get_password().map_err(|e| match e {
            keyring::Error::NoEntry => CredentialError::NotFound,
            keyring::Error::Ambiguous(_) => CredentialError::Storage("Ambiguous entry".to_string()),
            _ => CredentialError::Storage(e.to_string()),
        })
    }

    fn set(&self, service: &str, key: &str, value: &str) -> Result<(), CredentialError> {
        let entry = keyring::Entry::new(service, key)
            .map_err(|e| CredentialError::Storage(e.to_string()))?;
        entry
            .set_password(value)
            .map_err(|e| CredentialError::Storage(e.to_string()))
    }

    fn delete(&self, service: &str, key: &str) -> Result<(), CredentialError> {
        let entry = keyring::Entry::new(service, key)
            .map_err(|e| CredentialError::Storage(e.to_string()))?;
        entry.delete_credential().map_err(|e| match e {
            keyring::Error::NoEntry => CredentialError::NotFound,
            _ => CredentialError::Storage(e.to_string()),
        })
    }
}

#[cfg(not(windows))]
pub struct WindowsCredentialStore;

#[cfg(not(windows))]
impl WindowsCredentialStore {
    pub fn new() -> Self {
        Self
    }
}

#[cfg(not(windows))]
impl Default for WindowsCredentialStore {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(not(windows))]
impl CredentialStore for WindowsCredentialStore {
    fn get(&self, service: &str, key: &str) -> Result<String, CredentialError> {
        let entry = keyring::Entry::new(service, key)
            .map_err(|e| CredentialError::Storage(e.to_string()))?;
        entry.get_password().map_err(|e| match e {
            keyring::Error::NoEntry => CredentialError::NotFound,
            keyring::Error::Ambiguous(_) => CredentialError::Storage("Ambiguous entry".to_string()),
            _ => CredentialError::Storage(e.to_string()),
        })
    }

    fn set(&self, service: &str, key: &str, value: &str) -> Result<(), CredentialError> {
        let entry = keyring::Entry::new(service, key)
            .map_err(|e| CredentialError::Storage(e.to_string()))?;
        entry
            .set_password(value)
            .map_err(|e| CredentialError::Storage(e.to_string()))
    }

    fn delete(&self, service: &str, key: &str) -> Result<(), CredentialError> {
        let entry = keyring::Entry::new(service, key)
            .map_err(|e| CredentialError::Storage(e.to_string()))?;
        entry.delete_credential().map_err(|e| match e {
            keyring::Error::NoEntry => CredentialError::NotFound,
            _ => CredentialError::Storage(e.to_string()),
        })
    }
}
/// OAuth credentials structure
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct OAuthCredentials {
    pub access_token: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub refresh_token: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<chrono::DateTime<chrono::Utc>>,
    #[serde(default)]
    pub scopes: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rate_limit_tier: Option<String>,
}

impl OAuthCredentials {
    /// Check if the token is expired
    pub fn is_expired(&self) -> bool {
        if let Some(expires_at) = self.expires_at {
            // Consider expired if within 5 minutes of expiry
            expires_at <= chrono::Utc::now() + chrono::Duration::minutes(5)
        } else {
            false
        }
    }

    /// Check if the credentials have a specific scope
    pub fn has_scope(&self, scope: &str) -> bool {
        self.scopes.iter().any(|s| s == scope)
    }
}
