//! Display-label policy for Codex account surfaces.
//!
//! Labels never fall back to credentials, provider identifiers, or filesystem
//! paths. Privacy mode applies to the ambient/System account only; managed
//! accounts retain their existing display names so account actions and
//! identity siloing continue to address the same account.

use std::collections::HashMap;

use uuid::Uuid;

use super::models::{CodexAccount, CodexAccountSource};

impl CodexAccount {
    /// The user-facing account label.
    pub fn display_name(&self) -> String {
        self.display_label_base()
    }

    /// The label used by a user-facing account surface.
    pub fn privacy_safe_display_name(
        &self,
        hide_personal_info: bool,
        ordinal: usize,
        generic_label: &str,
    ) -> String {
        if hide_personal_info && self.source == CodexAccountSource::Ambient {
            let label = generic_label.trim();
            let label = if label.is_empty() { "Account" } else { label };
            return format!("{label} {ordinal}");
        }
        self.display_name()
    }

    /// The user-facing account label without falling back to credentials,
    /// provider identifiers, or filesystem paths.
    pub(crate) fn display_label_base(&self) -> String {
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

    /// The identity a label disambiguates against: the effective workspace id
    /// when present, otherwise the account UUID in lowercase.
    pub(crate) fn display_identity(&self) -> String {
        self.effective_workspace_account_id()
            .unwrap_or_else(|| self.id.to_string().to_lowercase())
    }
}

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

/// Assign stable, opaque ordinals for account surfaces that hide personal data.
///
/// The account UUID is never displayed. Sorting it here keeps tray and React
/// consumers consistent even when discovery returns accounts in a different
/// order.
pub fn ordinals_by_id(accounts: &[CodexAccount]) -> HashMap<Uuid, usize> {
    let mut ordered: Vec<(String, usize, Uuid)> = accounts
        .iter()
        .enumerate()
        .map(|(index, account)| (account.id.to_string(), index, account.id))
        .collect();
    ordered.sort_by(|left, right| left.0.cmp(&right.0).then_with(|| left.1.cmp(&right.1)));

    ordered
        .into_iter()
        .enumerate()
        .map(|(index, (_, _, id))| (id, index + 1))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codex_accounts::models::utc_now;
    use std::path::PathBuf;

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
    fn privacy_safe_display_name_hides_system_identity_but_preserves_managed_labels() {
        let mut system =
            display_account("11111111-1111-1111-1111-111111111111", "system-workspace");
        system.source = CodexAccountSource::Ambient;
        system.nickname = Some("Private System Name".to_string());
        let mut managed =
            display_account("22222222-2222-2222-2222-222222222222", "managed-workspace");
        managed.nickname = Some("Work".to_string());

        let hidden_system = system.privacy_safe_display_name(true, 1, "Account");
        let hidden_managed = managed.privacy_safe_display_name(true, 2, "Account");
        assert_eq!(hidden_system, "Account 1");
        assert!(!hidden_system.contains("@"));
        assert!(!hidden_system.contains("Private System Name"));
        assert_eq!(hidden_managed, "user@example.com — Work");

        assert_eq!(
            system.privacy_safe_display_name(false, 1, "Account"),
            "user@example.com — Private System Name"
        );
        assert_eq!(
            managed.privacy_safe_display_name(false, 2, "Account"),
            "user@example.com — Work"
        );
    }

    #[test]
    fn ordinals_are_stable_when_account_discovery_order_changes() {
        let first = display_account("22222222-2222-2222-2222-222222222222", "workspace-alpha");
        let second = display_account("11111111-1111-1111-1111-111111111111", "workspace-beta");

        let forward = ordinals_by_id(&[first.clone(), second.clone()]);
        let reversed = ordinals_by_id(&[second.clone(), first.clone()]);

        assert_eq!(forward, reversed);
        assert_eq!(forward[&second.id], 1);
        assert_eq!(forward[&first.id], 2);
    }
}
