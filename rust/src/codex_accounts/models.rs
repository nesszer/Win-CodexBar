//! Domain model for Codex accounts and their usage snapshots.
//!
//! Field names intentionally mirror CodexControl's `windows/.../models.py` (MIT)
//! so stored data interops with that project.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub use super::extra_usage::CodexExtraUsageCost;

/// `parse_from_rfc3339` requires an offset; append `Z` only when none is present.
pub fn parse_datetime(value: &str) -> Option<DateTime<Utc>> {
    let text = value.trim();
    if text.is_empty() {
        return None;
    }
    // `parse_from_rfc3339` requires an offset; append `Z` only when none is present.
    let text_dt = text.trim();
    let has_offset = text_dt.ends_with(['Z', 'z']) || contains_offset(text_dt);
    let normalized = if has_offset {
        String::from(text_dt)
    } else {
        format!("{text_dt}Z")
    };
    DateTime::parse_from_rfc3339(&normalized)
        .ok()
        .map(|dt| dt.with_timezone(&Utc))
}

/// Does the string carry an explicit `+HH:MM` / `-HH:MM` UTC offset (not `Z`)?
fn contains_offset(text: &str) -> bool {
    let Some(time_start) = text.find('T') else {
        return false;
    };
    let rest = &text[time_start + 1..];
    let Some(sign) = rest.rfind(['+', '-']) else {
        return false;
    };
    let tail = &rest[sign + 1..];
    let mut chars = tail.chars();
    let digits = chars.next().is_some_and(|c| c.is_ascii_digit())
        && chars.next().is_some_and(|c| c.is_ascii_digit());
    digits && tail.contains(':')
}

/// Format a UTC instant the same way CodexControl does (`...Z`).
pub fn format_datetime(value: Option<DateTime<Utc>>) -> Option<String> {
    value.map(|dt| dt.to_rfc3339_opts(chrono::SecondsFormat::AutoSi, true))
}

pub fn utc_now() -> DateTime<Utc> {
    Utc::now()
}

fn normalize_identifier(value: Option<&str>) -> Option<String> {
    value
        .map(|v| v.trim().to_lowercase())
        .filter(|v| !v.is_empty())
}

/// Where an account's `CODEX_HOME` lives.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CodexAccountSource {
    /// The environment's `~/.codex` (the identity the Codex CLI/Desktop uses).
    Ambient,
    /// An app-owned home directory under `managed-homes/`.
    ManagedByApp,
}

impl CodexAccountSource {
    pub fn display_name(self) -> &'static str {
        match self {
            CodexAccountSource::Ambient => "System",
            CodexAccountSource::ManagedByApp => "Managed",
        }
    }

    /// Whether the app owns (and may delete) this account's files.
    pub fn owns_files(self) -> bool {
        matches!(self, CodexAccountSource::ManagedByApp)
    }

    pub fn from_raw(value: &str) -> Option<Self> {
        match value {
            "ambient" => Some(CodexAccountSource::Ambient),
            "managedByApp" | "importedCodexBar" => Some(CodexAccountSource::ManagedByApp),
            _ => None,
        }
    }
}

/// A stored Codex account.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexAccount {
    pub id: Uuid,
    pub nickname: Option<String>,
    pub email_hint: Option<String>,
    pub auth_subject: Option<String>,
    /// Legacy persisted workspace selection. New records should prefer
    /// `workspace_account_id`, but this remains a valid selected-workspace
    /// fallback for v0.56.3 accounts.
    pub provider_account_id: Option<String>,
    /// App-owned remote workspace selection. This deliberately is not copied
    /// into the Codex auth file, whose account id may name another default.
    #[serde(default)]
    pub workspace_account_id: Option<String>,
    pub codex_home_path: PathBuf,
    pub source: CodexAccountSource,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub last_authenticated_at: Option<DateTime<Utc>>,
}

impl CodexAccount {
    #[allow(
        clippy::too_many_arguments,
        reason = "constructor accepts all account fields for complete initialization"
    )]
    pub fn new(
        id: Uuid,
        nickname: Option<String>,
        email_hint: Option<String>,
        auth_subject: Option<String>,
        provider_account_id: Option<String>,
        codex_home_path: PathBuf,
        source: CodexAccountSource,
        created_at: DateTime<Utc>,
        updated_at: DateTime<Utc>,
        last_authenticated_at: Option<DateTime<Utc>>,
    ) -> Self {
        Self {
            id,
            nickname,
            email_hint,
            auth_subject,
            provider_account_id,
            workspace_account_id: None,
            codex_home_path,
            source,
            created_at,
            updated_at,
            last_authenticated_at,
        }
    }

    pub fn display_name(&self) -> String {
        self.display_label_base()
    }

    /// Return the user-facing account label without falling back to
    /// credentials, provider identifiers, or filesystem paths.
    fn display_label_base(&self) -> String {
        let nickname = self
            .nickname
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty());
        let email = self
            .email_hint
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_lowercase);

        match (email, nickname) {
            (Some(email), Some(nickname)) => format!("{email} — {nickname}"),
            (Some(email), None) => email,
            (None, Some(nickname)) => nickname.to_string(),
            (None, None) => "Workspace".to_string(),
        }
    }

    pub fn normalized_email_hint(&self) -> Option<String> {
        normalize_identifier(self.email_hint.as_deref())
    }

    pub fn normalized_auth_subject(&self) -> Option<String> {
        normalize_identifier(self.auth_subject.as_deref())
    }

    pub fn normalized_provider_account_id(&self) -> Option<String> {
        normalize_identifier(self.provider_account_id.as_deref())
    }

    pub fn normalized_workspace_account_id(&self) -> Option<String> {
        normalize_identifier(self.workspace_account_id.as_deref())
    }

    /// The remote workspace owned by the app for this account.
    ///
    /// `provider_account_id` was the selected workspace field in the local
    /// v0.56.3 store. Keep it as the compatibility fallback, while an explicit
    /// selection always wins over the auth file's default account id.
    pub fn effective_workspace_account_id(&self) -> Option<String> {
        self.normalized_workspace_account_id()
            .or_else(|| self.normalized_provider_account_id())
    }

    /// Whether the app-selected workspace differs from the auth file default.
    /// A missing side is not a proven mismatch, matching the upstream guard.
    pub fn selected_workspace_differs_from_auth_default(
        &self,
        auth_default_account_id: Option<&str>,
    ) -> bool {
        match (
            self.effective_workspace_account_id(),
            normalize_identifier(auth_default_account_id),
        ) {
            (Some(selected), Some(default_id)) => selected != default_id,
            _ => false,
        }
    }

    pub fn standardized_home_path(&self) -> String {
        std::path::absolute(&self.codex_home_path)
            .unwrap_or_else(|_| self.codex_home_path.clone())
            .to_string_lossy()
            .to_lowercase()
    }

    fn display_identity(&self) -> String {
        self.effective_workspace_account_id()
            .unwrap_or_else(|| self.id.to_string().to_lowercase())
    }

    fn source_priority(&self) -> u8 {
        if self.source.owns_files() { 2 } else { 1 }
    }

    fn recency_date(&self) -> DateTime<Utc> {
        self.last_authenticated_at.unwrap_or(self.updated_at)
    }

    /// Whether two accounts refer to the same identity.
    pub fn matches(&self, other: &CodexAccount) -> bool {
        if self.standardized_home_path() == other.standardized_home_path() {
            return true;
        }
        if let (Some(a), Some(b)) = (
            self.effective_workspace_account_id(),
            other.effective_workspace_account_id(),
        ) && a == b
        {
            return true;
        }
        if self.effective_workspace_account_id().is_some()
            || other.effective_workspace_account_id().is_some()
        {
            return false;
        }
        if let (Some(a), Some(b)) = (
            self.normalized_auth_subject(),
            other.normalized_auth_subject(),
        ) && a == b
        {
            return true;
        }
        if let (Some(a), Some(b)) = (self.normalized_email_hint(), other.normalized_email_hint())
            && a == b
        {
            return true;
        }
        false
    }

    /// Merge a fresher discovery into this account, preferring managed/recency.
    pub fn merge_from(&mut self, other: &CodexAccount) {
        if self
            .nickname
            .as_deref()
            .map(str::trim)
            .is_none_or(|s| s.is_empty())
        {
            self.nickname = other.nickname.clone();
        }

        let prefer_other = other.source_priority() > self.source_priority()
            || (other.source_priority() == self.source_priority()
                && other.recency_date() >= self.recency_date());

        let pick = |mine: &mut Option<String>, value: Option<&String>| {
            let newer = prefer_other && value.is_some_and(|v| !v.trim().is_empty());
            if newer || mine.is_none() {
                *mine = value.cloned();
            }
        };
        pick(&mut self.email_hint, other.email_hint.as_ref());
        pick(&mut self.auth_subject, other.auth_subject.as_ref());
        if self.workspace_account_id.is_none() {
            if other.workspace_account_id.is_some() {
                self.workspace_account_id = other.workspace_account_id.clone();
            } else if self.provider_account_id.is_none()
                || self.normalized_provider_account_id() == other.normalized_provider_account_id()
            {
                pick(
                    &mut self.provider_account_id,
                    other.provider_account_id.as_ref(),
                );
            }
        } else {
            // The explicit app-owned selection is authoritative. The legacy
            // provider field may still refresh as auth metadata, but must never
            // replace the selected workspace above.
            pick(
                &mut self.provider_account_id,
                other.provider_account_id.as_ref(),
            );
        }

        if prefer_other {
            self.source = other.source;
            self.codex_home_path = other.codex_home_path.clone();
        }

        self.updated_at = self.updated_at.max(other.updated_at);
        self.last_authenticated_at = match (self.last_authenticated_at, other.last_authenticated_at)
        {
            (Some(a), Some(b)) => Some(a.max(b)),
            (a, b) => a.or(b),
        };
    }
}

/// Build the stable labels used by account-facing surfaces.
///
/// A provider account id is a workspace identity, but it must never be shown
/// directly. Only accounts whose privacy-safe display labels collide receive
/// an opaque suffix. The stored account UUID is folded into the hashed
/// identity when two entries claim the same provider identity, keeping
/// separate local profiles distinguishable without exposing a home path or
/// provider id.
pub fn display_names_by_id(accounts: &[CodexAccount]) -> HashMap<Uuid, String> {
    let mut groups: HashMap<String, Vec<usize>> = HashMap::new();
    for (index, account) in accounts.iter().enumerate() {
        groups
            .entry(account.display_label_base().to_lowercase())
            .or_default()
            .push(index);
    }

    let mut labels = HashMap::with_capacity(accounts.len());
    for indexes in groups.into_values() {
        if indexes.len() == 1 {
            let index = indexes[0];
            labels.insert(accounts[index].id, accounts[index].display_label_base());
            continue;
        }

        let mut identity_counts: HashMap<String, usize> = HashMap::new();
        for &index in &indexes {
            *identity_counts
                .entry(accounts[index].display_identity())
                .or_default() += 1;
        }

        for &index in &indexes {
            let account = &accounts[index];
            let identity = account.display_identity();
            let identity = if identity_counts.get(&identity) == Some(&1) {
                identity
            } else {
                format!("{identity}\0{}", account.id)
            };
            let suffix = crate::core::sha256_hex(identity.as_bytes());
            labels.insert(
                account.id,
                format!("{} · {}", account.display_label_base(), &suffix[..8]),
            );
        }
    }
    labels
}

/// Identity of a previously-removed account, kept to avoid re-adding it.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemovedAccountIdentity {
    pub id: Uuid,
    pub email_hint: Option<String>,
    pub auth_subject: Option<String>,
    pub provider_account_id: Option<String>,
    #[serde(default)]
    pub workspace_account_id: Option<String>,
    pub codex_home_path: PathBuf,
    pub source: CodexAccountSource,
    pub removed_at: DateTime<Utc>,
}

impl RemovedAccountIdentity {
    pub fn from_account(account: &CodexAccount) -> Self {
        Self {
            id: Uuid::new_v4(),
            email_hint: account.email_hint.clone(),
            auth_subject: account.auth_subject.clone(),
            provider_account_id: account.provider_account_id.clone(),
            workspace_account_id: account.workspace_account_id.clone(),
            codex_home_path: account.codex_home_path.clone(),
            source: account.source,
            removed_at: utc_now(),
        }
    }

    pub fn matches(&self, account: &CodexAccount) -> bool {
        if self.standardized_home_path() == account.standardized_home_path() {
            return true;
        }
        if let (Some(a), Some(b)) = (
            self.effective_workspace_account_id(),
            account.effective_workspace_account_id(),
        ) && a == b
        {
            return true;
        }
        if self.effective_workspace_account_id().is_some()
            || account.effective_workspace_account_id().is_some()
        {
            return false;
        }
        if let (Some(a), Some(b)) = (
            normalize_identifier(self.auth_subject.as_deref()),
            account.normalized_auth_subject(),
        ) && a == b
        {
            return true;
        }
        if let (Some(a), Some(b)) = (
            normalize_identifier(self.email_hint.as_deref()),
            account.normalized_email_hint(),
        ) && a == b
        {
            return true;
        }
        false
    }

    fn standardized_home_path(&self) -> String {
        std::path::absolute(&self.codex_home_path)
            .unwrap_or_else(|_| self.codex_home_path.clone())
            .to_string_lossy()
            .to_lowercase()
    }

    fn effective_workspace_account_id(&self) -> Option<String> {
        normalize_identifier(self.workspace_account_id.as_deref())
            .or_else(|| normalize_identifier(self.provider_account_id.as_deref()))
    }
}

/// A single quota window (session or weekly).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageWindowSnapshot {
    pub used_percent: f64,
    pub reset_at: Option<DateTime<Utc>>,
    pub limit_window_seconds: i64,
}

impl UsageWindowSnapshot {
    pub fn new(
        used_percent: f64,
        reset_at: Option<DateTime<Utc>>,
        limit_window_seconds: i64,
    ) -> Self {
        Self {
            used_percent,
            reset_at,
            limit_window_seconds,
        }
    }

    pub fn remaining_percent(&self) -> f64 {
        100.0_f64.max(self.used_percent) - self.used_percent
    }

    pub fn role(&self) -> WindowRole {
        use crate::core::RateWindowCadence;
        match RateWindowCadence::from_seconds(self.limit_window_seconds) {
            RateWindowCadence::Session => WindowRole::Session,
            RateWindowCadence::Weekly => WindowRole::Weekly,
            RateWindowCadence::Monthly => WindowRole::Monthly,
            RateWindowCadence::Unknown => WindowRole::Unknown,
        }
    }
}

/// Normalized role of a window based on its duration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowRole {
    Session,
    Weekly,
    Monthly,
    Unknown,
}

/// Codex credits balance.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreditsBalanceSnapshot {
    pub has_credits: bool,
    pub unlimited: bool,
    pub balance: Option<f64>,
}

impl CreditsBalanceSnapshot {
    pub fn display_value(&self) -> String {
        if self.unlimited {
            return "Unlimited".to_string();
        }
        if let Some(balance) = self.balance {
            return format!("{balance:.2}")
                .trim_end_matches('0')
                .trim_end_matches('.')
                .to_string();
        }
        if self.has_credits {
            return "Available".to_string();
        }
        "None".to_string()
    }
}

/// A fetched snapshot for one Codex account.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountUsageSnapshot {
    pub email: Option<String>,
    pub provider_account_id: Option<String>,
    pub plan: Option<String>,
    pub allowed: Option<bool>,
    pub limit_reached: Option<bool>,
    pub primary_window: Option<UsageWindowSnapshot>,
    pub secondary_window: Option<UsageWindowSnapshot>,
    pub credits: Option<CreditsBalanceSnapshot>,
    /// Account-scoped extra-usage cost persisted with the account lane.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cost: Option<crate::core::CostSnapshot>,
    /// Subscription dates observed from the same account-scoped OpenAI
    /// dashboard/API request as this quota snapshot.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subscription: Option<crate::core::SubscriptionMetadata>,
    pub updated_at: DateTime<Utc>,
}

impl AccountUsageSnapshot {
    pub fn is_quota_blocked(&self) -> bool {
        self.limit_reached == Some(true) || self.allowed == Some(false)
    }

    pub fn has_quota_windows(&self) -> bool {
        self.primary_window.is_some() || self.secondary_window.is_some()
    }

    pub fn has_usable_quota_now(&self) -> bool {
        if self.is_quota_blocked() {
            return false;
        }
        let values = [self.primary_window.as_ref(), self.secondary_window.as_ref()]
            .into_iter()
            .flatten()
            .map(|w| w.remaining_percent());
        let mut values = values.peekable();
        values.peek().is_some() && values.any(|v| v > 0.001)
    }

    pub fn lowest_remaining_percent(&self) -> f64 {
        if self.is_quota_blocked() {
            return 0.0;
        }
        [self.secondary_window.as_ref(), self.primary_window.as_ref()]
            .into_iter()
            .flatten()
            .map(|w| w.remaining_percent())
            .fold(f64::MAX, f64::min)
    }

    pub fn next_reset_at(&self) -> Option<DateTime<Utc>> {
        [self.primary_window.as_ref(), self.secondary_window.as_ref()]
            .into_iter()
            .flatten()
            .filter_map(|w| w.reset_at)
            .min()
    }
}

/// Sort weight used to order accounts by practical usefulness.
pub fn account_sort_priority(snapshot: &AccountUsageSnapshot) -> u8 {
    if snapshot.has_usable_quota_now() {
        0
    } else if snapshot.next_reset_at().is_some() {
        1
    } else {
        2
    }
}

fn _path_is_trailing(path: &Path) -> bool {
    path.as_os_str()
        .to_string_lossy()
        .ends_with(std::path::MAIN_SEPARATOR)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn display_account(id: &str, provider_account_id: &str) -> CodexAccount {
        CodexAccount::new(
            Uuid::parse_str(id).unwrap(),
            None,
            Some("user@example.com".to_string()),
            None,
            Some(provider_account_id.to_string()),
            PathBuf::from(format!("C:/private/{provider_account_id}")),
            CodexAccountSource::ManagedByApp,
            utc_now(),
            utc_now(),
            None,
        )
    }

    fn account(
        id: &str,
        home: &str,
        source: CodexAccountSource,
        provider_id: Option<&str>,
    ) -> CodexAccount {
        CodexAccount::new(
            Uuid::parse_str(id).unwrap(),
            None,
            None,
            None,
            provider_id.map(str::to_string),
            PathBuf::from(home),
            source,
            utc_now(),
            utc_now(),
            None,
        )
    }

    #[test]
    fn matches_by_home_path() {
        let a = account(
            "11111111-1111-1111-1111-111111111111",
            "/x/a",
            CodexAccountSource::ManagedByApp,
            None,
        );
        let b = account(
            "22222222-2222-2222-2222-222222222222",
            "/x/a",
            CodexAccountSource::ManagedByApp,
            None,
        );
        assert!(a.matches(&b));
    }

    #[test]
    fn matches_by_provider_account_id() {
        let a = account(
            "11111111-1111-1111-1111-111111111111",
            "/x/a",
            CodexAccountSource::ManagedByApp,
            Some("acct-1"),
        );
        let b = account(
            "22222222-2222-2222-2222-222222222222",
            "/y/b",
            CodexAccountSource::ManagedByApp,
            Some("ACCT-1"),
        );
        assert!(a.matches(&b));
    }

    #[test]
    fn disambiguates_different_provider_ids() {
        let a = account(
            "11111111-1111-1111-1111-111111111111",
            "/x/a",
            CodexAccountSource::ManagedByApp,
            Some("acct-1"),
        );
        let b = account(
            "22222222-2222-2222-2222-222222222222",
            "/y/b",
            CodexAccountSource::ManagedByApp,
            Some("acct-2"),
        );
        assert!(!a.matches(&b));
    }

    #[test]
    fn explicit_workspace_beats_auth_default_and_survives_discovery_merge() {
        let mut selected = account(
            "11111111-1111-1111-1111-111111111111",
            "/managed/selected",
            CodexAccountSource::ManagedByApp,
            Some("auth-default-a"),
        );
        selected.workspace_account_id = Some("selected-workspace-b".to_string());
        let discovered = account(
            "22222222-2222-2222-2222-222222222222",
            "/managed/selected",
            CodexAccountSource::ManagedByApp,
            Some("auth-default-a"),
        );

        assert_eq!(
            selected.effective_workspace_account_id().as_deref(),
            Some("selected-workspace-b")
        );
        assert!(selected.selected_workspace_differs_from_auth_default(Some("auth-default-a")));
        selected.merge_from(&discovered);
        assert_eq!(
            selected.effective_workspace_account_id().as_deref(),
            Some("selected-workspace-b")
        );
    }

    #[test]
    fn legacy_provider_account_id_is_selected_workspace_fallback() {
        let account = account(
            "11111111-1111-1111-1111-111111111111",
            "/managed/selected",
            CodexAccountSource::ManagedByApp,
            Some("Selected-Workspace-B"),
        );
        assert_eq!(
            account.effective_workspace_account_id().as_deref(),
            Some("selected-workspace-b")
        );
    }

    #[test]
    fn source_displays_and_ownership() {
        assert_eq!(CodexAccountSource::Ambient.display_name(), "System");
        assert_eq!(CodexAccountSource::ManagedByApp.display_name(), "Managed");
        assert!(CodexAccountSource::ManagedByApp.owns_files());
        assert!(!CodexAccountSource::Ambient.owns_files());
    }

    #[test]
    fn display_names_disambiguate_same_email_without_exposing_workspace_identity() {
        let first = display_account("11111111-1111-1111-1111-111111111111", "workspace-alpha");
        let second = display_account("22222222-2222-2222-2222-222222222222", "workspace-beta");

        let labels = display_names_by_id(&[first.clone(), second.clone()]);
        let first_label = labels.get(&first.id).unwrap();
        let second_label = labels.get(&second.id).unwrap();
        assert_ne!(first_label, second_label);
        for label in [first_label, second_label] {
            assert!(label.starts_with("user@example.com · "));
            assert_eq!(label.rsplit_once(' ').unwrap().1.len(), 8);
            assert!(!label.contains("workspace-"));
            assert!(!label.contains("C:/private"));
            assert!(!label.contains("auth0|"));
        }

        let reordered = display_names_by_id(&[second, first.clone()]);
        assert_eq!(reordered.get(&first.id), Some(first_label));

        let mut relaunched_first = first.clone();
        relaunched_first.codex_home_path = PathBuf::from("C:/different-managed-home");
        let mut relaunched_second = first.clone();
        relaunched_second.id = Uuid::parse_str("66666666-6666-6666-6666-666666666666").unwrap();
        relaunched_second.provider_account_id = Some("workspace-beta".to_string());
        let relaunched = display_names_by_id(&[relaunched_first, relaunched_second]);
        assert_eq!(relaunched.get(&first.id), Some(first_label));
    }

    #[test]
    fn display_names_keep_duplicate_workspace_profiles_distinct() {
        let first = display_account("33333333-3333-3333-3333-333333333333", "shared-workspace");
        let second = display_account("44444444-4444-4444-4444-444444444444", "shared-workspace");

        let labels = display_names_by_id(&[first.clone(), second.clone()]);
        assert_ne!(labels.get(&first.id), labels.get(&second.id));
        assert!(labels[&first.id].starts_with("user@example.com · "));
        assert!(labels[&second.id].starts_with("user@example.com · "));
        let reordered = display_names_by_id(&[second, first.clone()]);
        assert_eq!(reordered.get(&first.id), labels.get(&first.id));
    }

    #[test]
    fn display_names_use_generic_base_when_identity_fields_are_missing() {
        let mut account =
            display_account("55555555-5555-5555-5555-555555555555", "secret-workspace");
        account.email_hint = None;
        account.auth_subject = Some("auth0|secret-subject".to_string());
        account.nickname = None;

        let labels = display_names_by_id(&[account]);
        let label = labels.values().next().unwrap();
        assert!(label.starts_with("Workspace"));
        assert!(!label.contains("secret-workspace"));
        assert!(!label.contains("secret-subject"));
        assert!(!label.contains("C:/private"));
    }

    #[test]
    fn window_role_classification() {
        assert_eq!(
            UsageWindowSnapshot::new(0.0, None, 18_000).role(),
            WindowRole::Session
        );
        assert_eq!(
            UsageWindowSnapshot::new(0.0, None, 604_800).role(),
            WindowRole::Weekly
        );
        assert_eq!(
            UsageWindowSnapshot::new(0.0, None, 1234).role(),
            WindowRole::Unknown
        );
        assert_eq!(
            UsageWindowSnapshot::new(0.0, None, 2_592_000).role(),
            WindowRole::Monthly
        );
    }

    #[test]
    fn blocked_account_has_no_usable_quota() {
        let snapshot = AccountUsageSnapshot {
            email: None,
            provider_account_id: None,
            plan: None,
            allowed: Some(false),
            limit_reached: None,
            primary_window: Some(UsageWindowSnapshot::new(10.0, None, 18_000)),
            secondary_window: None,
            credits: None,
            cost: None,
            subscription: None,
            updated_at: utc_now(),
        };
        assert!(snapshot.is_quota_blocked());
        assert!(!snapshot.has_usable_quota_now());
        assert_eq!(snapshot.lowest_remaining_percent(), 0.0);
    }

    #[test]
    fn parse_datetime_accepts_z_and_offset() {
        assert!(parse_datetime("2026-01-01T00:00:00Z").is_some());
        assert!(parse_datetime("2026-01-01T00:00:00+00:00").is_some());
        assert!(parse_datetime("").is_none());
    }

    #[test]
    fn credits_display_value() {
        assert_eq!(
            CreditsBalanceSnapshot {
                has_credits: true,
                unlimited: true,
                balance: None
            }
            .display_value(),
            "Unlimited"
        );
        assert_eq!(
            CreditsBalanceSnapshot {
                has_credits: true,
                unlimited: false,
                balance: Some(12.50)
            }
            .display_value(),
            "12.5"
        );
        assert_eq!(
            CreditsBalanceSnapshot {
                has_credits: true,
                unlimited: false,
                balance: None
            }
            .display_value(),
            "Available"
        );
        assert_eq!(
            CreditsBalanceSnapshot {
                has_credits: false,
                unlimited: false,
                balance: None
            }
            .display_value(),
            "None"
        );
    }

    #[test]
    fn merge_prefers_managed_and_recency() {
        let mut managed = account(
            "11111111-1111-1111-1111-111111111111",
            "/x/managed",
            CodexAccountSource::ManagedByApp,
            None,
        );
        managed.nickname = Some("My acct".to_string());
        let ambient = account(
            "22222222-2222-2222-2222-222222222222",
            "~/.codex-like/ambient",
            CodexAccountSource::Ambient,
            None,
        );
        managed.merge_from(&ambient);
        assert_eq!(managed.source, CodexAccountSource::ManagedByApp);
        assert_eq!(managed.display_name(), "My acct");
    }

    #[test]
    fn display_name_does_not_fall_back_to_home_path() {
        let acct = account(
            "11111111-1111-1111-1111-111111111111",
            "/x/my-home-dir",
            CodexAccountSource::ManagedByApp,
            None,
        );
        assert_eq!(acct.display_name(), "Workspace");
        let _ = _path_is_trailing(std::path::Path::new("/x/"));
    }
}
