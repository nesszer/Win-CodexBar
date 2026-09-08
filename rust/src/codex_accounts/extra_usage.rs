use chrono::{DateTime, Utc};

use super::models::{AccountUsageSnapshot, CreditsBalanceSnapshot};

/// Reconciles Codex's purchased-credit observation with a separately fetched
/// monthly extra-usage cap.
pub struct CodexExtraUsageCost;

impl CodexExtraUsageCost {
    pub const CURRENCY_CODE: &'static str = "Credits";

    pub fn from_credits(
        credits: Option<&CreditsBalanceSnapshot>,
        observed_at: DateTime<Utc>,
        account_id: Option<&str>,
        attached: Option<&crate::core::CostSnapshot>,
    ) -> Option<crate::core::CostSnapshot> {
        let Some(credits) = credits else {
            return attached.cloned();
        };
        let balance = Self::purchased_extra_credits_balance(credits, attached);
        // `Some(credits)` is itself the successful credits observation; this
        // preserves a confirmed zero while keeping legacy persisted rows valid.
        let balance_updated_at = Some(observed_at);
        let mut live = crate::core::CostSnapshot::new(0.0, Self::CURRENCY_CODE, "Extra usage")
            .replacing_balance(balance, balance_updated_at);
        if let Some(account_id) = account_id.filter(|id| !id.trim().is_empty()) {
            live = live.with_account_id(account_id);
        }
        crate::core::CostSnapshot::reconcile(Some(&live), attached)
    }

    /// Purchased credits are reported as the provider's remaining balance. A
    /// matching monthly remainder means no purchased credits, but a successful
    /// zero observation is still retained by `balance_updated_at`.
    pub fn purchased_extra_credits_balance(
        credits: &CreditsBalanceSnapshot,
        attached: Option<&crate::core::CostSnapshot>,
    ) -> Option<f64> {
        let balance = credits.balance?;
        if balance <= 0.0 {
            return Some(0.0);
        }
        if let Some(monthly_remaining) = attached
            .filter(|cost| cost.currency_code.eq_ignore_ascii_case(Self::CURRENCY_CODE))
            .and_then(crate::core::CostSnapshot::remaining)
            && (balance - monthly_remaining).abs() <= 0.000_1
        {
            return Some(0.0);
        }
        Some(balance)
    }
}

impl AccountUsageSnapshot {
    /// Build the account-scoped credit cost while preserving a separately
    /// fetched monthly cap, if one is available.
    pub fn extra_usage_cost(
        &self,
        attached: Option<&crate::core::CostSnapshot>,
    ) -> Option<crate::core::CostSnapshot> {
        let attached = attached.or(self.cost.as_ref());
        CodexExtraUsageCost::from_credits(
            self.credits.as_ref(),
            self.updated_at,
            self.provider_account_id
                .as_deref()
                .or(self.email.as_deref()),
            attached,
        )
    }
}

#[cfg(test)]
mod extra_usage_tests {
    use super::*;

    fn at(seconds: i64) -> DateTime<Utc> {
        DateTime::<Utc>::from_timestamp(seconds, 0).unwrap()
    }

    #[test]
    fn purchased_balance_keeps_confirmed_zero() {
        let credits = CreditsBalanceSnapshot {
            has_credits: true,
            unlimited: false,
            balance: Some(0.0),
        };
        let result =
            CodexExtraUsageCost::from_credits(Some(&credits), at(200), Some("acct"), None).unwrap();

        assert_eq!(result.balance, Some(0.0));
        assert_eq!(result.balance_updated_at, Some(at(200)));
        assert_eq!(result.account_id.as_deref(), Some("acct"));
    }

    #[test]
    fn purchased_balance_reconciles_with_attached_monthly_cap() {
        let credits = CreditsBalanceSnapshot {
            has_credits: true,
            unlimited: false,
            balance: Some(15.0),
        };
        let mut cap = crate::core::CostSnapshot::new(10.0, "Credits", "Monthly").with_limit(25.0);
        cap.updated_at = at(100);
        cap.account_id = Some("acct".to_string());

        let result =
            CodexExtraUsageCost::from_credits(Some(&credits), at(200), Some("acct"), Some(&cap))
                .unwrap();

        assert_eq!(result.used, 10.0);
        assert_eq!(result.limit, Some(25.0));
        assert_eq!(result.balance, Some(0.0));
        assert_eq!(result.updated_at, at(100));
        assert_eq!(result.balance_updated_at, Some(at(200)));
    }
}
