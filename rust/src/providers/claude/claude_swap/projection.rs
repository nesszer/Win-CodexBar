//! Display-only projection of parsed cswap rows.
//!
//! Projection is where external identity and status become the provider-neutral
//! snapshot the UI and CLI consume. Identity is the source-issued numeric slot
//! (`claude-swap:<slot>`), never email or credential-derived values, and
//! personal information collapses to `Account N` ordinals when hidden.

use serde::Serialize;

use super::{
    ClaudeSwapAccountList, ClaudeSwapAccountRow, ClaudeSwapUsageStatus, ClaudeSwapUsageWindow,
};

/// Bridge-facing external account row. Identity is the source-issued numeric
/// slot (`claude-swap:<slot>`), never email or credential-derived values.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClaudeSwapAccount {
    pub id: String,
    pub slot: u32,
    pub label: String,
    pub email: Option<String>,
    pub organization: Option<String>,
    pub alias: Option<String>,
    pub is_active: bool,
    pub can_activate: bool,
    pub status: String,
    pub error: Option<String>,
    pub five_hour: Option<ClaudeSwapUsageWindowDto>,
    pub seven_day: Option<ClaudeSwapUsageWindowDto>,
    pub scoped: Vec<ClaudeSwapScopedWindowDto>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClaudeSwapUsageWindowDto {
    pub used_percent: f64,
    pub resets_at: Option<chrono::DateTime<chrono::Utc>>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClaudeSwapScopedWindowDto {
    pub name: String,
    pub used_percent: f64,
    pub resets_at: Option<chrono::DateTime<chrono::Utc>>,
}

fn normalized_email(email: &str) -> String {
    email.trim().to_lowercase()
}

fn collision_labels(rows: &[&ClaudeSwapAccountRow]) -> Vec<String> {
    let mut counts: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    for email in rows.iter().map(|row| normalized_email(&row.email)) {
        if !email.is_empty() {
            *counts.entry(email).or_default() += 1;
        }
    }
    let duplicates = counts
        .into_iter()
        .filter(|(_, count)| *count > 1)
        .map(|(email, _)| email)
        .collect::<std::collections::HashSet<_>>();

    let labels = rows
        .iter()
        .map(|row| candidate_label(row, &duplicates))
        .collect::<Vec<_>>();
    let mut label_counts: std::collections::HashMap<String, usize> =
        std::collections::HashMap::new();
    for (row, label) in rows.iter().zip(labels.iter()) {
        if row.alias.is_none() && duplicates.contains(&normalized_email(&row.email)) {
            *label_counts.entry(label.to_lowercase()).or_default() += 1;
        }
    }
    rows.iter()
        .zip(labels)
        .map(|(row, label)| {
            let colliding = row.alias.is_none()
                && duplicates.contains(&normalized_email(&row.email))
                && label_counts
                    .get(&label.to_lowercase())
                    .is_some_and(|count| *count > 1);
            if colliding {
                format!("{label} · Account {}", row.number)
            } else {
                label
            }
        })
        .collect()
}

fn candidate_label(
    row: &ClaudeSwapAccountRow,
    duplicates: &std::collections::HashSet<String>,
) -> String {
    if let Some(alias) = &row.alias {
        return alias.clone();
    }
    if row.email.is_empty() {
        return format!("Account {}", row.number);
    }
    if !duplicates.contains(&normalized_email(&row.email)) {
        return row.email.clone();
    }
    if !row.organization_name.is_empty() {
        return format!("{} · {}", row.email, row.organization_name);
    }
    format!("{} · Account {}", row.email, row.number)
}

fn error_text_for(row: &ClaudeSwapAccountRow) -> Option<String> {
    let has_windows = row.five_hour.is_some() || row.seven_day.is_some() || !row.scoped.is_empty();
    match row.usage_status {
        ClaudeSwapUsageStatus::Ok => {
            if has_windows {
                None
            } else {
                Some("No usage windows reported.".to_string())
            }
        }
        ClaudeSwapUsageStatus::TokenExpired => {
            Some("Token expired. Switch to this account in claude-swap to refresh it.".to_string())
        }
        ClaudeSwapUsageStatus::ReloginRequired => {
            Some("Re-login required. Re-authenticate this account in claude-swap.".to_string())
        }
        ClaudeSwapUsageStatus::ApiKey => {
            Some("API-key account; subscription usage is unavailable.".to_string())
        }
        ClaudeSwapUsageStatus::KeychainUnavailable => {
            Some("claude-swap could not read the active account's Keychain entry.".to_string())
        }
        ClaudeSwapUsageStatus::NoCredentials => {
            Some("No stored credentials for this account slot.".to_string())
        }
        ClaudeSwapUsageStatus::Unavailable => {
            Some("Polling deferred until a limit resets.".to_string())
        }
        ClaudeSwapUsageStatus::Unknown => Some("Unrecognized claude-swap status.".to_string()),
    }
}

/// Project parsed rows into the provider-neutral account snapshot consumed by
/// the settings UI. When personal information is hidden, labels collapse to
/// stable `Account N` ordinals and identity is omitted from the bridge payload.
pub fn project_accounts(
    list: &ClaudeSwapAccountList,
    hide_personal_info: bool,
) -> Vec<ClaudeSwapAccount> {
    let mut ordered = list.accounts.iter().collect::<Vec<_>>();
    ordered.sort_by(|lhs, rhs| {
        if lhs.is_active != rhs.is_active {
            return rhs.is_active.cmp(&lhs.is_active);
        }
        lhs.number.cmp(&rhs.number)
    });
    let labels = collision_labels(&ordered);

    ordered
        .into_iter()
        .zip(labels)
        .map(|(row, label)| {
            let display_label = if hide_personal_info {
                format!("Account {}", row.number)
            } else {
                label
            };
            let to_window = |window: &Option<ClaudeSwapUsageWindow>| {
                window.as_ref().map(|window| ClaudeSwapUsageWindowDto {
                    used_percent: window.used_percent,
                    resets_at: window.resets_at,
                })
            };
            ClaudeSwapAccount {
                id: format!("claude-swap:{}", row.number),
                slot: row.number,
                label: display_label,
                email: if hide_personal_info || row.email.is_empty() {
                    None
                } else {
                    Some(row.email.clone())
                },
                organization: if hide_personal_info || row.organization_name.is_empty() {
                    None
                } else {
                    Some(row.organization_name.clone())
                },
                alias: if hide_personal_info {
                    None
                } else {
                    row.alias.clone()
                },
                is_active: row.is_active,
                can_activate: !row.is_active && row.usage_status.can_activate(),
                status: row.usage_status.as_label().to_string(),
                error: error_text_for(row),
                five_hour: to_window(&row.five_hour),
                seven_day: to_window(&row.seven_day),
                scoped: row
                    .scoped
                    .iter()
                    .map(|window| ClaudeSwapScopedWindowDto {
                        name: window.name.clone(),
                        used_percent: window.used_percent,
                        resets_at: window.resets_at,
                    })
                    .collect(),
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::super::parse_account_list;
    use super::*;
    use serde_json::json;

    fn list_fixture() -> ClaudeSwapAccountList {
        let raw = json!({
            "schemaVersion": 1,
            "activeAccountNumber": 2,
            "accounts": [
                {
                    "number": 1,
                    "email": "same@example.com",
                    "organizationName": "Work",
                    "active": false,
                    "usageStatus": "ok",
                    "usage": { "fiveHour": { "pct": 10.0 } }
                },
                {
                    "number": 2,
                    "email": "same@example.com",
                    "organizationName": "Personal",
                    "active": true,
                    "usageStatus": "ok",
                    "usage": { "fiveHour": { "pct": 81.0 } }
                },
                {
                    "number": 3,
                    "email": "expired@example.com",
                    "organizationName": "",
                    "alias": "Backup",
                    "active": false,
                    "usageStatus": "token_expired"
                }
            ]
        });
        parse_account_list(&raw.to_string()).unwrap()
    }

    #[test]
    fn same_email_accounts_get_distinct_stable_ids_and_labels() {
        let projected = project_accounts(&list_fixture(), false);
        // Active row sorts first.
        assert_eq!(projected[0].id, "claude-swap:2");
        let work = projected.iter().find(|a| a.slot == 1).unwrap();
        let personal = projected.iter().find(|a| a.slot == 2).unwrap();
        assert_ne!(work.id, personal.id);
        assert_eq!(work.label, "same@example.com · Work");
        assert_eq!(personal.label, "same@example.com · Personal");
        // Alias wins over email and expired slots are not actionable.
        let backup = projected.iter().find(|a| a.slot == 3).unwrap();
        assert_eq!(backup.label, "Backup");
        assert!(!backup.can_activate);
        assert_eq!(personal.status, "ok");
        assert!(projected.iter().find(|a| a.slot == 1).unwrap().can_activate);
    }

    #[test]
    fn hiding_personal_info_collapses_to_ordinals() {
        let projected = project_accounts(&list_fixture(), true);
        for account in &projected {
            assert_eq!(account.label, format!("Account {}", account.slot));
            assert!(account.email.is_none());
            assert!(account.organization.is_none());
            assert!(account.alias.is_none());
        }
    }

    #[test]
    fn unknown_status_is_neither_echoed_nor_actionable() {
        let raw = json!({
            "schemaVersion": 1,
            "activeAccountNumber": null,
            "accounts": [{
                "number": 1,
                "email": "x@example.com",
                "active": false,
                "usageStatus": "super_secret_token\u{1b}]0;leak\u{07}",
                "usage": { "fiveHour": { "pct": 1.0 } }
            }]
        });
        let parsed = parse_account_list(&raw.to_string()).unwrap();
        let projected = project_accounts(&parsed, false);
        let account = &projected[0];
        assert_eq!(account.status, "unknown");
        assert!(!account.can_activate);
        let error = account.error.as_deref().unwrap();
        assert!(!error.contains("super_secret_token"));
        assert!(!error.contains('\u{1b}'));
    }
}
