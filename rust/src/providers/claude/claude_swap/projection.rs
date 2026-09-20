//! Display-only projection of parsed cswap rows.
//!
//! Projection is where external identity and status become the provider-neutral
//! snapshot the UI and CLI consume. Identity is the source-issued numeric slot
//! (`claude-swap:<slot>`), never email or credential-derived values, and
//! personal information collapses to `Account N` ordinals when hidden.

use serde::Serialize;

use super::{
    ClaudeSwapAccountList, ClaudeSwapAccountRow, ClaudeSwapHistoricalUsage, ClaudeSwapScopedWindow,
    ClaudeSwapSpendWindow, ClaudeSwapUsageMeasurement, ClaudeSwapUsageStatus,
    ClaudeSwapUsageWindow,
};

pub const HISTORICAL_USAGE_PROVENANCE: &str = "source_reported_last_good";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ClaudeSwapAccountAction {
    Switch,
    Reauthenticate,
}

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
    pub action: Option<ClaudeSwapAccountAction>,
    pub is_disabled: bool,
    pub status: String,
    pub error: Option<String>,
    pub five_hour: Option<ClaudeSwapUsageWindowDto>,
    pub seven_day: Option<ClaudeSwapUsageWindowDto>,
    pub scoped: Vec<ClaudeSwapScopedWindowDto>,
    pub spend: Option<ClaudeSwapSpendWindowDto>,
    pub historical_usage: Option<ClaudeSwapHistoricalUsageDto>,
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

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClaudeSwapSpendWindowDto {
    pub used: f64,
    pub limit: f64,
    pub used_percent: f64,
    pub currency_code: Option<String>,
    pub resets_at: Option<chrono::DateTime<chrono::Utc>>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClaudeSwapHistoricalUsageDto {
    pub five_hour: Option<ClaudeSwapUsageWindowDto>,
    pub seven_day: Option<ClaudeSwapUsageWindowDto>,
    pub scoped: Vec<ClaudeSwapScopedWindowDto>,
    pub spend: Option<ClaudeSwapSpendWindowDto>,
    pub fetched_at: chrono::DateTime<chrono::Utc>,
    pub provenance: &'static str,
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
    match row.usage_status {
        ClaudeSwapUsageStatus::Ok => {
            if !row.usage.is_empty() {
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
        ClaudeSwapUsageStatus::ForeignCredential => Some(
            "claude-swap reports the live credential belongs to a different account. Re-authenticate this account in claude-swap to restore its saved login."
                .to_string(),
        ),
        ClaudeSwapUsageStatus::Unavailable => {
            Some("Usage unavailable.".to_string())
        }
        ClaudeSwapUsageStatus::Unknown => Some("Unrecognized claude-swap status.".to_string()),
    }
}

fn to_window(window: &Option<ClaudeSwapUsageWindow>) -> Option<ClaudeSwapUsageWindowDto> {
    window.as_ref().map(|window| ClaudeSwapUsageWindowDto {
        used_percent: window.used_percent,
        resets_at: window.resets_at,
    })
}

fn to_scoped_window(window: &ClaudeSwapScopedWindow) -> ClaudeSwapScopedWindowDto {
    ClaudeSwapScopedWindowDto {
        name: window.name.clone(),
        used_percent: window.used_percent,
        resets_at: window.resets_at,
    }
}

fn to_spend(spend: &Option<ClaudeSwapSpendWindow>) -> Option<ClaudeSwapSpendWindowDto> {
    spend.as_ref().map(|spend| ClaudeSwapSpendWindowDto {
        used: spend.used,
        limit: spend.limit,
        used_percent: spend.used_percent,
        currency_code: spend.currency_code.clone(),
        resets_at: spend.resets_at,
    })
}

fn to_measurement(
    measurement: &ClaudeSwapUsageMeasurement,
    fetched_at: chrono::DateTime<chrono::Utc>,
) -> ClaudeSwapHistoricalUsageDto {
    ClaudeSwapHistoricalUsageDto {
        five_hour: to_window(&measurement.five_hour),
        seven_day: to_window(&measurement.seven_day),
        scoped: measurement.scoped.iter().map(to_scoped_window).collect(),
        spend: to_spend(&measurement.spend),
        fetched_at,
        provenance: HISTORICAL_USAGE_PROVENANCE,
    }
}

fn to_historical_usage(
    historical: &Option<ClaudeSwapHistoricalUsage>,
) -> Option<ClaudeSwapHistoricalUsageDto> {
    historical
        .as_ref()
        .map(|historical| to_measurement(&historical.measurement, historical.fetched_at))
}

pub fn action_for_account(row: &ClaudeSwapAccountRow) -> Option<ClaudeSwapAccountAction> {
    if row.is_active {
        (row.usage_status == ClaudeSwapUsageStatus::ForeignCredential)
            .then_some(ClaudeSwapAccountAction::Reauthenticate)
    } else if row.usage_status.can_switch_to() {
        Some(ClaudeSwapAccountAction::Switch)
    } else {
        None
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
            let action = action_for_account(row);
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
                action,
                is_disabled: row.is_disabled,
                status: row.usage_status.as_label().to_string(),
                error: error_text_for(row),
                five_hour: to_window(&row.usage.five_hour),
                seven_day: to_window(&row.usage.seven_day),
                scoped: row.usage.scoped.iter().map(to_scoped_window).collect(),
                spend: to_spend(&row.usage.spend),
                historical_usage: to_historical_usage(&row.historical_usage),
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
        assert!(backup.action.is_none());
        assert_eq!(personal.status, "ok");
        assert!(
            projected
                .iter()
                .find(|a| a.slot == 1)
                .unwrap()
                .action
                .is_some()
        );
    }

    #[test]
    fn hiding_personal_info_collapses_to_ordinals() {
        let projected = project_accounts(&list_fixture(), true);
        let ids = projected
            .iter()
            .map(|account| account.id.as_str())
            .collect::<std::collections::HashSet<_>>();
        assert_eq!(ids.len(), projected.len());
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
        assert!(account.action.is_none());
        let error = account.error.as_deref().unwrap();
        assert!(!error.contains("super_secret_token"));
        assert!(!error.contains('\u{1b}'));
    }

    #[test]
    fn foreign_credentials_expose_explicit_reauthentication_action() {
        let raw = json!({
            "schemaVersion": 1,
            "activeAccountNumber": 1,
            "accounts": [{
                "number": 1,
                "email": "x@example.com",
                "active": true,
                "usageStatus": "foreign_credential"
            }]
        });
        let parsed = parse_account_list(&raw.to_string()).unwrap();
        let account = &project_accounts(&parsed, false)[0];
        assert_eq!(
            account.action,
            Some(ClaudeSwapAccountAction::Reauthenticate)
        );
        assert!(
            account
                .error
                .as_deref()
                .unwrap()
                .contains("different account")
        );
    }

    #[test]
    fn historical_usage_is_typed_and_marked_as_source_reported() {
        let raw = json!({
            "schemaVersion": 1,
            "activeAccountNumber": null,
            "accounts": [{
                "number": 1,
                "email": "x@example.com",
                "active": false,
                "usageStatus": "token_expired",
                "disabled": true,
                "lastGoodUsage": {
                    "fiveHour": { "pct": 42.0 },
                    "spend": { "used": 2.0, "limit": 20.0, "pct": 10.0, "currency": "USD" }
                },
                "lastGoodFetchedAt": "2026-09-12T00:45:00Z"
            }]
        });
        let parsed = parse_account_list(&raw.to_string()).unwrap();
        let account = &project_accounts(&parsed, false)[0];
        assert!(account.is_disabled);
        assert_eq!(
            account.historical_usage.as_ref().unwrap().provenance,
            HISTORICAL_USAGE_PROVENANCE
        );
        assert_eq!(
            account
                .historical_usage
                .as_ref()
                .unwrap()
                .five_hour
                .as_ref()
                .unwrap()
                .used_percent,
            42.0
        );
    }

    #[test]
    fn spend_only_ok_usage_is_not_reported_as_empty() {
        let raw = json!({
            "schemaVersion": 1,
            "activeAccountNumber": null,
            "accounts": [{
                "number": 1,
                "email": "spend@example.com",
                "active": false,
                "usageStatus": "ok",
                "usage": {
                    "spend": { "used": 2.0, "limit": 20.0, "pct": 10.0 }
                }
            }]
        });
        let parsed = parse_account_list(&raw.to_string()).unwrap();
        let account = &project_accounts(&parsed, false)[0];
        assert!(account.spend.is_some());
        assert!(account.error.is_none());
    }
}
