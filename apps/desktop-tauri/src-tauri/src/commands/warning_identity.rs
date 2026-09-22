use codexbar::core::ProviderId;

#[derive(Debug, Clone, PartialEq, Eq)]
enum WarningSourceLane {
    ClaudeOauth,
    ClaudeCli,
    Named(String),
}

impl WarningSourceLane {
    fn from_label(provider: ProviderId, source_label: &str) -> Option<Self> {
        let source = source_label.trim().to_ascii_lowercase();
        if source.is_empty() {
            return None;
        }
        if provider == ProviderId::Claude {
            if source == "oauth" {
                return Some(Self::ClaudeOauth);
            }
            if source == "cli" || source.starts_with("cli ") {
                return Some(Self::ClaudeCli);
            }
        }
        Some(Self::Named(source))
    }

    fn key(&self) -> &str {
        match self {
            Self::ClaudeOauth => "oauth",
            Self::ClaudeCli => "cli",
            Self::Named(source) => source,
        }
    }

    fn supports_unresolved_account(&self) -> bool {
        matches!(self, Self::ClaudeOauth | Self::ClaudeCli)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum WarningAccountState {
    Token(uuid::Uuid),
    Email(String),
    Organization(String),
    Unresolved,
    Missing,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct WarningIdentity {
    provider: ProviderId,
    source_lane: Option<WarningSourceLane>,
    account: WarningAccountState,
}

impl WarningIdentity {
    pub(super) fn new(
        provider: ProviderId,
        source_label: &str,
        account_email: Option<&str>,
        account_organization: Option<&str>,
        token_account_id: Option<uuid::Uuid>,
    ) -> Self {
        let source_lane = WarningSourceLane::from_label(provider, source_label);
        let normalized = |value: Option<&str>| {
            value
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_ascii_lowercase)
        };
        let account = if let Some(id) = token_account_id {
            WarningAccountState::Token(id)
        } else if let Some(email) = normalized(account_email) {
            WarningAccountState::Email(email)
        } else if let Some(organization) = normalized(account_organization) {
            WarningAccountState::Organization(organization)
        } else if provider == ProviderId::Claude
            && source_lane
                .as_ref()
                .is_some_and(WarningSourceLane::supports_unresolved_account)
        {
            WarningAccountState::Unresolved
        } else {
            WarningAccountState::Missing
        };
        Self {
            provider,
            source_lane,
            account,
        }
    }

    pub(super) fn unresolved_key(&self) -> Option<String> {
        let lane = self.source_lane.as_ref()?;
        (self.provider == ProviderId::Claude && lane.supports_unresolved_account())
            .then(|| format!("{}:{}:unknown", self.provider.cli_name(), lane.key()))
    }

    pub(super) fn threshold_key(&self) -> String {
        match &self.account {
            WarningAccountState::Token(id) => format!("token-account:{}", id.as_hyphenated()),
            WarningAccountState::Email(email) => email.clone(),
            WarningAccountState::Organization(organization) => format!("org:{organization}"),
            WarningAccountState::Unresolved => self.unresolved_key().unwrap_or_default(),
            WarningAccountState::Missing => String::new(),
        }
    }

    pub(super) fn predictive_key(&self) -> Option<String> {
        match &self.account {
            WarningAccountState::Token(id) => Some(format!("token-account:{}", id.as_hyphenated())),
            WarningAccountState::Email(email) => self
                .source_lane
                .as_ref()
                .map(|lane| format!("{}:{email}", lane.key())),
            WarningAccountState::Unresolved => self.unresolved_key(),
            WarningAccountState::Organization(_) | WarningAccountState::Missing => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_lanes_and_token_accounts_remain_separate() {
        let account_id = uuid::Uuid::parse_str("aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa").unwrap();

        assert_eq!(
            WarningIdentity::new(
                ProviderId::Claude,
                "cli",
                Some("Person@Example.com"),
                None,
                None,
            )
            .predictive_key()
            .as_deref(),
            Some("cli:person@example.com")
        );
        assert_eq!(
            WarningIdentity::new(
                ProviderId::Claude,
                "oauth",
                Some("Person@Example.com"),
                None,
                None,
            )
            .predictive_key()
            .as_deref(),
            Some("oauth:person@example.com")
        );
        assert_eq!(
            WarningIdentity::new(
                ProviderId::Claude,
                "oauth",
                Some("Person@Example.com"),
                None,
                Some(account_id),
            )
            .predictive_key()
            .as_deref(),
            Some("token-account:aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa")
        );
    }

    #[test]
    fn unresolved_claude_sources_are_scoped_to_their_lane() {
        assert_eq!(
            WarningIdentity::new(ProviderId::Claude, "oauth", None, None, None).predictive_key(),
            Some("claude:oauth:unknown".to_string())
        );
        assert_eq!(
            WarningIdentity::new(
                ProviderId::Claude,
                "cli (reduced fidelity)",
                None,
                None,
                None,
            )
            .predictive_key(),
            Some("claude:cli:unknown".to_string())
        );
        assert_eq!(
            WarningIdentity::new(ProviderId::Claude, "web", None, None, None).predictive_key(),
            None
        );
        assert_eq!(
            WarningIdentity::new(ProviderId::Codex, "cli", Some("  "), None, None).predictive_key(),
            None
        );
    }

    #[test]
    fn unresolved_and_resolved_accounts_keep_separate_warning_history() {
        let oauth = WarningIdentity::new(ProviderId::Claude, "oauth", None, None, None);
        let cli = WarningIdentity::new(
            ProviderId::Claude,
            "cli (reduced fidelity)",
            None,
            None,
            None,
        );
        let resolved_cli = WarningIdentity::new(
            ProviderId::Claude,
            "cli",
            Some("person@example.com"),
            None,
            None,
        );

        assert_eq!(
            oauth.unresolved_key().as_deref(),
            Some("claude:oauth:unknown")
        );
        assert_eq!(cli.unresolved_key().as_deref(), Some("claude:cli:unknown"));
        assert_ne!(cli.threshold_key(), resolved_cli.threshold_key());
        assert_ne!(cli.predictive_key(), resolved_cli.predictive_key());
        assert_ne!(oauth.predictive_key(), resolved_cli.predictive_key());
        assert_eq!(
            resolved_cli.predictive_key().as_deref(),
            Some("cli:person@example.com")
        );
    }
}
