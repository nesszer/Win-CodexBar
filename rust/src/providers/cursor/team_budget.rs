//! Team-member budget enrichment for the Cursor dashboard APIs.
//!
//! Owns the dashboard client, the pagination policy, and the member-selection
//! rule. Team spend is optional enrichment: every failure or ambiguity
//! collapses to "no budget" so the usage-summary result survives.

use super::api::{CursorApi, UsageSummary, UserInfo};
use crate::core::ProviderError;
use crate::providers::cookie_values;
use serde::Deserialize;
use serde::de::DeserializeOwned;
use std::collections::HashSet;
use std::time::{Duration, Instant};

const TEAM_LOOKUP_TIMEOUT: Duration = Duration::from_secs(10);
const TEAM_REQUEST_TIMEOUT: Duration = Duration::from_secs(5);
const TEAM_PAGE_SIZE: usize = 50;
const MAX_TEAM_PAGES: u32 = 20;

/// Verified spend and limit for one team member, in USD.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct CursorMemberBudget {
    pub(super) used_usd: f64,
    pub(super) limit_usd: f64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CursorTeamSpend {
    team_member_spend: Vec<CursorTeamMember>,
    total_pages: Option<u32>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CursorTeamMember {
    email: Option<String>,
    overall_spend_cents: Option<f64>,
    effective_per_user_limit_dollars: Option<f64>,
    monthly_limit_dollars: Option<f64>,
}

impl CursorTeamMember {
    fn budget(&self) -> Option<CursorMemberBudget> {
        let used_usd = self.overall_spend_cents? / 100.0;
        let limit_usd = self
            .effective_per_user_limit_dollars
            .or(self.monthly_limit_dollars)?;
        (used_usd.is_finite() && used_usd >= 0.0 && limit_usd.is_finite() && limit_usd > 0.0)
            .then_some(CursorMemberBudget {
                used_usd,
                limit_usd,
            })
    }
}

/// Outcome of matching one verified email against the member pages.
#[derive(Debug, Clone, Copy, PartialEq)]
enum MemberSelection {
    Budget(CursorMemberBudget),
    /// More than one matching member, or a match without a usable budget,
    /// must not select an arbitrary record.
    Ambiguous,
    NotFound,
}

impl MemberSelection {
    fn into_budget(self) -> Option<CursorMemberBudget> {
        match self {
            MemberSelection::Budget(budget) => Some(budget),
            MemberSelection::Ambiguous | MemberSelection::NotFound => None,
        }
    }
}

/// Accumulates matching members across the flattened page stream, so a
/// duplicate in any page is caught by the same single mechanism.
struct MemberBudgetSelector {
    email: String,
    selection: MemberSelection,
}

impl MemberBudgetSelector {
    fn new(email: &str) -> Self {
        Self {
            email: email.to_ascii_lowercase(),
            selection: MemberSelection::NotFound,
        }
    }

    fn feed(&mut self, page: &[CursorTeamMember]) {
        if self.selection == MemberSelection::Ambiguous {
            return;
        }
        for member in page {
            let matches = member
                .email
                .as_deref()
                .map(str::trim)
                .is_some_and(|member_email| member_email.eq_ignore_ascii_case(&self.email));
            if !matches {
                continue;
            }
            self.selection = match (self.selection, member.budget()) {
                (MemberSelection::NotFound, Some(budget)) => MemberSelection::Budget(budget),
                _ => MemberSelection::Ambiguous,
            };
            if self.selection == MemberSelection::Ambiguous {
                return;
            }
        }
    }
}

/// Validate one spend page against the pagination policy and return the
/// declared page count, or `None` when the page violates it.
fn page_invariant(expected_pages: Option<u32>, page: u32, spend: &CursorTeamSpend) -> Option<u32> {
    let total_pages = spend
        .total_pages
        .filter(|pages| (1..=MAX_TEAM_PAGES).contains(pages))?;
    if expected_pages.is_some_and(|expected| expected != total_pages)
        || spend.team_member_spend.is_empty()
        || spend.team_member_spend.len() > TEAM_PAGE_SIZE
        || (page != total_pages && spend.team_member_spend.len() != TEAM_PAGE_SIZE)
    {
        return None;
    }
    Some(total_pages)
}

/// Pure dashboard payload: the list of team ids for the signed-in user.
#[derive(Debug, Deserialize)]
struct CursorTeams {
    teams: Vec<CursorTeam>,
}

#[derive(Debug, Deserialize)]
struct CursorTeam {
    id: i64,
}

/// Team-id selection policy: a single `portal-selected-team-id`/`team_id`
/// cookie value that exists in the team list, otherwise the only team when
/// exactly one remains.
fn selected_team_id(teams: &CursorTeams, cookie_header: &str) -> Option<i64> {
    let ids: HashSet<i64> = teams
        .teams
        .iter()
        .map(|team| team.id)
        .filter(|id| *id > 0)
        .collect();
    if ids.len() != teams.teams.len() {
        return None;
    }

    for name in ["portal-selected-team-id", "team_id"] {
        let values = cookie_values(cookie_header, name);
        if !values.is_empty() {
            let [value] = values.as_slice() else {
                return None;
            };
            let id = value.parse::<i64>().ok().filter(|id| ids.contains(id))?;
            return Some(id);
        }
    }
    (ids.len() == 1).then(|| ids.into_iter().next()).flatten()
}

impl CursorApi {
    /// Optional team-member enrichment for verified team members.
    ///
    /// A failed or ambiguous lookup must leave the already-valid
    /// usage-summary result available, so every failure collapses to `None`.
    pub(super) async fn resolve_team_budget(
        &self,
        summary: &UsageSummary,
        user_info: Option<&UserInfo>,
        cookie_header: &str,
    ) -> Option<CursorMemberBudget> {
        if !summary.is_team_plan() {
            return None;
        }
        let email = user_info.and_then(UserInfo::verified_email)?;
        self.fetch_team_budget(cookie_header, email)
            .await
            .ok()
            .flatten()
    }

    async fn fetch_team_budget(
        &self,
        cookie_header: &str,
        email: &str,
    ) -> Result<Option<CursorMemberBudget>, ProviderError> {
        let deadline = Instant::now() + TEAM_LOOKUP_TIMEOUT;
        let teams: CursorTeams = self
            .fetch_team_dashboard("teams", serde_json::json!({}), cookie_header, deadline)
            .await?;
        let Some(team_id) = selected_team_id(&teams, cookie_header) else {
            return Ok(None);
        };

        let mut expected_pages = None;
        let mut selector = MemberBudgetSelector::new(email);
        for page in 1..=MAX_TEAM_PAGES {
            let spend: CursorTeamSpend = self
                .fetch_team_dashboard(
                    "get-team-spend",
                    serde_json::json!({
                        "teamId": team_id,
                        "page": page,
                        "pageSize": TEAM_PAGE_SIZE,
                        "sortBy": "name",
                        "sortDirection": "asc"
                    }),
                    cookie_header,
                    deadline,
                )
                .await?;
            let Some(total_pages) = page_invariant(expected_pages, page, &spend) else {
                return Ok(None);
            };
            expected_pages = Some(total_pages);

            selector.feed(&spend.team_member_spend);
            if selector.selection == MemberSelection::Ambiguous {
                return Ok(None);
            }
            if page == total_pages {
                return Ok(selector.selection.into_budget());
            }
        }
        Ok(None)
    }

    async fn fetch_team_dashboard<Response: DeserializeOwned>(
        &self,
        endpoint: &str,
        body: serde_json::Value,
        cookie_header: &str,
        deadline: Instant,
    ) -> Result<Response, ProviderError> {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(ProviderError::Timeout);
        }
        let response = self
            .client()
            .post(format!("{BASE_URL}/api/dashboard/{endpoint}"))
            .header("Cookie", cookie_header)
            .header("Accept", "application/json")
            .header("Content-Type", "application/json")
            .header("Origin", BASE_URL)
            .header("Referer", format!("{BASE_URL}/dashboard"))
            .json(&body)
            .timeout(remaining.min(TEAM_REQUEST_TIMEOUT))
            .send()
            .await?;
        if response.status() == 401 || response.status() == 403 {
            return Err(ProviderError::AuthRequired);
        }
        if !response.status().is_success() {
            return Err(ProviderError::Other(format!(
                "Cursor team API returned {}",
                response.status()
            )));
        }
        response
            .json()
            .await
            .map_err(|error| ProviderError::Parse(error.to_string()))
    }
}

const BASE_URL: &str = "https://cursor.com";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn member_budget_preserves_true_zero_and_rejects_invalid_limits() {
        let zero: CursorTeamMember = serde_json::from_str(
            r#"{"overallSpendCents":0,"effectivePerUserLimitDollars":150,"monthlyLimitDollars":200}"#,
        )
        .unwrap();
        assert_eq!(
            zero.budget(),
            Some(CursorMemberBudget {
                used_usd: 0.0,
                limit_usd: 150.0
            })
        );
        let monthly_fallback: CursorTeamMember =
            serde_json::from_str(r#"{"overallSpendCents":1312,"monthlyLimitDollars":150}"#)
                .unwrap();
        assert_eq!(
            monthly_fallback.budget(),
            Some(CursorMemberBudget {
                used_usd: 13.12,
                limit_usd: 150.0
            })
        );

        for json in [
            r#"{"overallSpendCents":1312,"effectivePerUserLimitDollars":0,"monthlyLimitDollars":150}"#,
            r#"{"overallSpendCents":-1,"monthlyLimitDollars":150}"#,
            r#"{"overallSpendCents":1312,"monthlyLimitDollars":-1}"#,
        ] {
            let member: CursorTeamMember = serde_json::from_str(json).unwrap();
            assert!(
                member.budget().is_none(),
                "invalid budget must fail closed: {json}"
            );
        }
    }

    #[test]
    fn member_selection_requires_exactly_one_valid_match() {
        let missing = MemberBudgetSelector::new("missing@example.com");
        assert_eq!(missing.selection, MemberSelection::NotFound);

        let mut single = MemberBudgetSelector::new("member@example.com");
        single.feed(&[CursorTeamMember {
            email: Some("Member@Example.com".into()),
            overall_spend_cents: Some(1312.0),
            effective_per_user_limit_dollars: Some(150.0),
            monthly_limit_dollars: None,
        }]);
        assert_eq!(
            single.selection,
            MemberSelection::Budget(CursorMemberBudget {
                used_usd: 13.12,
                limit_usd: 150.0
            })
        );

        let duplicate = CursorTeamMember {
            email: Some("member@example.com".into()),
            overall_spend_cents: Some(1312.0),
            effective_per_user_limit_dollars: Some(150.0),
            monthly_limit_dollars: None,
        };
        let mut page_duplicate = MemberBudgetSelector::new("member@example.com");
        page_duplicate.feed(&[duplicate.clone(), duplicate.clone()]);
        assert_eq!(page_duplicate.selection, MemberSelection::Ambiguous);

        let mut cross_page = MemberBudgetSelector::new("member@example.com");
        cross_page.feed(std::slice::from_ref(&duplicate));
        cross_page.feed(std::slice::from_ref(&duplicate));
        assert_eq!(cross_page.selection, MemberSelection::Ambiguous);

        let mut invalid_budget = MemberBudgetSelector::new("member@example.com");
        invalid_budget.feed(&[CursorTeamMember {
            email: Some("member@example.com".into()),
            overall_spend_cents: Some(-1.0),
            effective_per_user_limit_dollars: Some(150.0),
            monthly_limit_dollars: None,
        }]);
        assert_eq!(invalid_budget.selection, MemberSelection::Ambiguous);
    }

    #[test]
    fn team_selection_requires_one_valid_identity() {
        let teams: CursorTeams =
            serde_json::from_str(r#"{"teams":[{"id":11},{"id":22}]}"#).unwrap();
        assert_eq!(
            selected_team_id(&teams, "auth=fixture; portal-selected-team-id=22"),
            Some(22)
        );
        assert_eq!(selected_team_id(&teams, "auth=fixture"), None);
        assert_eq!(
            selected_team_id(
                &teams,
                "auth=fixture; portal-selected-team-id=99; team_id=11"
            ),
            None
        );
        assert_eq!(
            selected_team_id(&teams, "auth=fixture; team_id=11; team_id=22"),
            None
        );

        let single: CursorTeams = serde_json::from_str(r#"{"teams":[{"id":22}]}"#).unwrap();
        assert_eq!(selected_team_id(&single, "auth=fixture"), Some(22));
    }

    #[test]
    fn page_invariant_rejects_mismatched_empty_and_undense_pages() {
        fn spend(members: usize, total_pages: Option<u32>) -> CursorTeamSpend {
            CursorTeamSpend {
                team_member_spend: vec![
                    CursorTeamMember {
                        email: None,
                        overall_spend_cents: None,
                        effective_per_user_limit_dollars: None,
                        monthly_limit_dollars: None,
                    };
                    members
                ],
                total_pages,
            }
        }

        let full = spend(TEAM_PAGE_SIZE, Some(2));
        assert_eq!(page_invariant(None, 1, &full), Some(2));
        assert_eq!(page_invariant(Some(2), 1, &full), Some(2));

        let short_final = spend(5, Some(1));
        assert_eq!(page_invariant(None, 1, &short_final), Some(1));

        assert_eq!(page_invariant(Some(2), 1, &short_final), None);
        assert_eq!(page_invariant(None, 1, &spend(0, Some(1))), None);
        assert_eq!(
            page_invariant(None, 1, &spend(TEAM_PAGE_SIZE + 1, Some(1))),
            None
        );
        assert_eq!(
            page_invariant(None, 1, &spend(TEAM_PAGE_SIZE, Some(0))),
            None
        );
        assert_eq!(
            page_invariant(None, 1, &spend(TEAM_PAGE_SIZE, Some(3))),
            Some(3)
        );
    }
}
