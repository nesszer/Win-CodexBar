//! OpenAI Live Usage settings (upstream 0.75.0 #4495): the per-refresh Tokens/min fetch
//! toggle and the manual per-model organization rate limits.
//!
//! OpenAI has no organization-level limits endpoint, so headroom is measured only against
//! limits the user copies from the organization Limits page. On Windows the OpenAI API
//! provider has no token accounts, so the provider-level list is the only credential scope.

use serde::{Deserialize, Deserializer, Serialize};

use super::Settings;
use crate::core::ProviderId;

/// One manual organization rate limit. Rows are trimmed and validated on load and on
/// every write; see [`ModelRateLimit::validated`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelRateLimit {
    pub model: String,
    #[serde(alias = "tokensPerMinute")]
    pub tokens_per_minute: u64,
    #[serde(
        default,
        alias = "requestsPerMinute",
        skip_serializing_if = "Option::is_none"
    )]
    pub requests_per_minute: Option<u64>,
}

/// Wire shape used only while loading, so negative or zero values are dropped rather than
/// failing the whole settings file.
#[derive(Deserialize)]
struct RawModelRateLimit {
    model: String,
    #[serde(alias = "tokensPerMinute")]
    tokens_per_minute: i64,
    #[serde(default, alias = "requestsPerMinute")]
    requests_per_minute: Option<i64>,
}

impl ModelRateLimit {
    pub fn new(
        model: impl Into<String>,
        tokens_per_minute: u64,
        requests_per_minute: Option<u64>,
    ) -> Self {
        Self {
            model: model.into(),
            tokens_per_minute,
            requests_per_minute,
        }
    }

    /// Upstream `ProviderModelRateLimit.validated`: model IDs are trimmed, empty IDs and
    /// zero-TPM rows are dropped, a zero RPM becomes absent, and the last valid row for a
    /// duplicate ID wins while keeping its position among the survivors.
    pub fn validated(rows: Vec<Self>) -> Vec<Self> {
        let mut seen = std::collections::HashSet::new();
        let mut kept: Vec<Self> = rows
            .into_iter()
            .rev()
            .filter_map(|row| {
                let model = row.model.trim().to_string();
                if model.is_empty() || row.tokens_per_minute == 0 || !seen.insert(model.clone()) {
                    return None;
                }
                Some(Self {
                    model,
                    tokens_per_minute: row.tokens_per_minute,
                    requests_per_minute: row.requests_per_minute.filter(|rpm| *rpm > 0),
                })
            })
            .collect();
        kept.reverse();
        kept
    }
}

/// Lenient loader for `ProviderConfig::model_rate_limits`: a malformed list or row is
/// dropped instead of resetting every setting, and the survivors are validated.
pub(super) fn deserialize_model_rate_limits<'de, D>(
    deserializer: D,
) -> Result<Option<Vec<ModelRateLimit>>, D::Error>
where
    D: Deserializer<'de>,
{
    let value = Option::<serde_json::Value>::deserialize(deserializer)?;
    let Some(serde_json::Value::Array(items)) = value else {
        return Ok(None);
    };
    let rows = items
        .into_iter()
        .filter_map(|item| serde_json::from_value::<RawModelRateLimit>(item).ok())
        .filter_map(|row| {
            Some(ModelRateLimit {
                model: row.model,
                tokens_per_minute: u64::try_from(row.tokens_per_minute).ok()?,
                requests_per_minute: row
                    .requests_per_minute
                    .and_then(|rpm| u64::try_from(rpm).ok()),
            })
        })
        .collect();
    let rows = ModelRateLimit::validated(rows);
    Ok((!rows.is_empty()).then_some(rows))
}

/// Providers with the OpenAI Live Usage settings.
fn supports_live_usage(id: ProviderId) -> bool {
    id == ProviderId::OpenAIApi
}

impl Settings {
    /// Whether each refresh also fetches the last hour of 1-minute usage (upstream
    /// `OPENAI_LIVE_MINUTES`). On by default for OpenAI API; false for every other provider.
    pub fn live_usage_enabled(&self, id: ProviderId) -> bool {
        supports_live_usage(id)
            && self
                .provider_configs
                .get(&id)
                .and_then(|config| config.live_usage_enabled)
                .unwrap_or(true)
    }

    pub fn set_live_usage_enabled(&mut self, id: ProviderId, enabled: bool) {
        if supports_live_usage(id) {
            // The default is on, so only an opt-out is persisted.
            self.provider_config_mut(id).live_usage_enabled = (!enabled).then_some(false);
        }
    }

    /// Validated manual organization rate limits for `id`; empty when unset.
    pub fn model_rate_limits(&self, id: ProviderId) -> &[ModelRateLimit] {
        if !supports_live_usage(id) {
            return &[];
        }
        self.provider_configs
            .get(&id)
            .and_then(|config| config.model_rate_limits.as_deref())
            .unwrap_or(&[])
    }

    /// Replace `id`'s manual limits with the validated rows. An empty list clears them.
    pub fn set_model_rate_limits(&mut self, id: ProviderId, limits: Vec<ModelRateLimit>) {
        if supports_live_usage(id) {
            let limits = ModelRateLimit::validated(limits);
            self.provider_config_mut(id).model_rate_limits = (!limits.is_empty()).then_some(limits);
        }
    }
}

/// Upstream `ProviderRateLimitNumber`: the Settings editor's number parser/formatter.
pub mod rate_limit_number {
    /// Accepts plain (`40000000`) or comma-grouped (`40,000,000`) integers, optionally with
    /// a decimal and a `k`/`M` suffix (`150k`, `1.5M`). A fraction without a suffix, or one
    /// that does not land on a whole count, is rejected rather than rounded.
    pub fn parse(text: &str) -> Option<u64> {
        let text = text.trim();
        let (body, multiplier) = match text.chars().last()? {
            'k' | 'K' => (&text[..text.len() - 1], 1_000_u64),
            'm' | 'M' => (&text[..text.len() - 1], 1_000_000_u64),
            _ => (text, 1_u64),
        };
        let (integer, fraction) = match body.split_once('.') {
            Some((integer, fraction)) => (integer, Some(fraction)),
            None => (body, None),
        };
        if fraction.is_some() && multiplier == 1 {
            return None;
        }
        let digits = integer_digits(integer)?;
        let fraction = fraction.unwrap_or("");
        if !fraction.chars().all(|ch| ch.is_ascii_digit())
            || (body.contains('.') && fraction.is_empty())
        {
            return None;
        }
        let scale = 10_u64.checked_pow(u32::try_from(fraction.len()).ok()?)?;
        let whole: u64 = digits.parse().ok()?;
        let fraction_value: u64 = if fraction.is_empty() {
            0
        } else {
            fraction.parse().ok()?
        };
        let numerator = u128::from(whole) * u128::from(scale) + u128::from(fraction_value);
        let scaled = numerator.checked_mul(u128::from(multiplier))?;
        if scaled % u128::from(scale) != 0 {
            return None;
        }
        let value = u64::try_from(scaled / u128::from(scale)).ok()?;
        (value > 0 && value <= i64::MAX as u64).then_some(value)
    }

    /// `[0-9]+` or `[0-9]{1,3}(,[0-9]{3})+`, returned without commas.
    fn integer_digits(text: &str) -> Option<String> {
        if text.is_empty() {
            return None;
        }
        if !text.contains(',') {
            return text
                .chars()
                .all(|ch| ch.is_ascii_digit())
                .then(|| text.to_string());
        }
        let mut groups = text.split(',');
        let head = groups.next()?;
        if head.is_empty() || head.len() > 3 || !head.chars().all(|ch| ch.is_ascii_digit()) {
            return None;
        }
        let mut digits = head.to_string();
        for group in groups {
            if group.len() != 3 || !group.chars().all(|ch| ch.is_ascii_digit()) {
                return None;
            }
            digits.push_str(group);
        }
        Some(digits)
    }

    /// Shortens only exactly representable values (`40000000` -> `40M`, `1500000` ->
    /// `1.5M`), so reopening Settings never changes a stored limit.
    pub fn format(value: u64) -> String {
        for (divisor, suffix) in [(1_000_000_u64, "M"), (1_000_u64, "K")] {
            if value >= divisor && value.is_multiple_of(divisor / 10) {
                let whole = value / divisor;
                let tenths = value % divisor / (divisor / 10);
                return if tenths == 0 {
                    format!("{whole}{suffix}")
                } else {
                    format!("{whole}.{tenths}{suffix}")
                };
            }
        }
        value.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::rate_limit_number::{format, parse};
    use super::*;
    use crate::settings::ProviderConfig;

    fn row(model: &str, tpm: u64, rpm: Option<u64>) -> ModelRateLimit {
        ModelRateLimit::new(model, tpm, rpm)
    }

    #[test]
    fn validated_trims_drops_invalid_rows_and_keeps_last_duplicate() {
        let rows = ModelRateLimit::validated(vec![
            row(" gpt-5 ", 1_000, Some(10)),
            row("", 5, None),
            row("gpt-4o", 0, None),
            row("o3", 2_000, Some(0)),
            row("gpt-5", 3_000, None),
        ]);
        assert_eq!(
            rows,
            vec![row("o3", 2_000, None), row("gpt-5", 3_000, None)]
        );
    }

    #[test]
    fn live_usage_defaults_on_for_openai_only_and_persists_only_the_opt_out() {
        let mut settings = Settings::default();
        assert!(settings.live_usage_enabled(ProviderId::OpenAIApi));
        assert!(!settings.live_usage_enabled(ProviderId::Codex));

        settings.set_live_usage_enabled(ProviderId::OpenAIApi, false);
        assert!(!settings.live_usage_enabled(ProviderId::OpenAIApi));
        let config = settings.provider_config(ProviderId::OpenAIApi).unwrap();
        assert_eq!(config.live_usage_enabled, Some(false));

        settings.set_live_usage_enabled(ProviderId::OpenAIApi, true);
        assert!(settings.live_usage_enabled(ProviderId::OpenAIApi));
        let config = settings.provider_config(ProviderId::OpenAIApi).unwrap();
        assert_eq!(config.live_usage_enabled, None);

        settings.set_live_usage_enabled(ProviderId::Codex, true);
        assert!(settings.provider_config(ProviderId::Codex).is_none());
    }

    #[test]
    fn model_rate_limits_set_validates_and_empty_clears() {
        let mut settings = Settings::default();
        settings.set_model_rate_limits(
            ProviderId::OpenAIApi,
            vec![row(" gpt-5 ", 40_000_000, Some(15_000)), row(" ", 1, None)],
        );
        assert_eq!(
            settings.model_rate_limits(ProviderId::OpenAIApi),
            &[row("gpt-5", 40_000_000, Some(15_000))]
        );
        assert!(settings.model_rate_limits(ProviderId::Codex).is_empty());

        settings.set_model_rate_limits(ProviderId::OpenAIApi, Vec::new());
        assert!(settings.model_rate_limits(ProviderId::OpenAIApi).is_empty());
        let config = settings.provider_config(ProviderId::OpenAIApi).unwrap();
        assert!(config.model_rate_limits.is_none());
    }

    #[test]
    fn provider_config_round_trips_and_omits_defaults() {
        let encoded = serde_json::to_value(ProviderConfig::default()).unwrap();
        assert!(encoded.get("model_rate_limits").is_none());
        assert!(encoded.get("live_usage_enabled").is_none());

        let config = ProviderConfig {
            live_usage_enabled: Some(false),
            model_rate_limits: Some(vec![
                row("gpt-5", 40_000_000, Some(15_000)),
                row("o3", 2_000, None),
            ]),
            ..ProviderConfig::default()
        };
        let encoded = serde_json::to_value(&config).unwrap();
        assert_eq!(
            encoded["model_rate_limits"],
            serde_json::json!([
                {"model": "gpt-5", "tokens_per_minute": 40_000_000, "requests_per_minute": 15_000},
                {"model": "o3", "tokens_per_minute": 2_000},
            ])
        );
        let decoded: ProviderConfig = serde_json::from_value(encoded).unwrap();
        assert_eq!(decoded, config);
    }

    #[test]
    fn legacy_config_without_live_fields_loads_with_defaults() {
        let decoded: ProviderConfig =
            serde_json::from_str(r#"{"workspace_id":"proj_abc","api_token":"sk-admin"}"#).unwrap();
        assert_eq!(decoded.live_usage_enabled, None);
        assert_eq!(decoded.model_rate_limits, None);
        assert_eq!(decoded.workspace_id.as_deref(), Some("proj_abc"));
    }

    #[test]
    fn malformed_limits_are_dropped_without_failing_the_config() {
        let decoded: ProviderConfig = serde_json::from_str(
            r#"{
                "workspace_id": "proj_abc",
                "model_rate_limits": [
                    {"model": " gpt-5 ", "tokensPerMinute": 40000000, "requestsPerMinute": 15000},
                    {"model": "neg", "tokens_per_minute": -5},
                    {"model": "rpm", "tokens_per_minute": 10, "requests_per_minute": -1},
                    {"model": 7, "tokens_per_minute": 10},
                    "garbage",
                    {"model": "zero", "tokens_per_minute": 0}
                ]
            }"#,
        )
        .unwrap();
        assert_eq!(
            decoded.model_rate_limits,
            Some(vec![
                row("gpt-5", 40_000_000, Some(15_000)),
                row("rpm", 10, None),
            ])
        );
        assert_eq!(decoded.workspace_id.as_deref(), Some("proj_abc"));

        for bad in [
            r#"{"model_rate_limits": "x"}"#,
            r#"{"model_rate_limits": null}"#,
        ] {
            let decoded: ProviderConfig = serde_json::from_str(bad).unwrap();
            assert_eq!(decoded.model_rate_limits, None);
        }
    }

    #[test]
    fn settings_file_round_trips_live_usage_fields() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        let mut settings = Settings::default();
        settings.set_live_usage_enabled(ProviderId::OpenAIApi, false);
        settings.set_model_rate_limits(
            ProviderId::OpenAIApi,
            vec![row("gpt-4o-mini", 150_000, Some(500))],
        );
        settings.save_to_path(&path).unwrap();

        let loaded = Settings::load_from_path(Some(&path));
        assert!(!loaded.live_usage_enabled(ProviderId::OpenAIApi));
        assert_eq!(
            loaded.model_rate_limits(ProviderId::OpenAIApi),
            &[row("gpt-4o-mini", 150_000, Some(500))]
        );
    }

    #[test]
    fn rate_limit_number_parses_upstream_forms() {
        assert_eq!(parse("40000000"), Some(40_000_000));
        assert_eq!(parse(" 40,000,000 "), Some(40_000_000));
        assert_eq!(parse("40M"), Some(40_000_000));
        assert_eq!(parse("150k"), Some(150_000));
        assert_eq!(parse("1.5M"), Some(1_500_000));
        assert_eq!(parse("1.25k"), Some(1_250));
        for invalid in [
            "",
            "0",
            "1.5",
            "1.0005k",
            "40,00",
            "1,0000",
            ",100",
            "1.",
            "-5",
            "5x",
            "k",
            "1.5.5M",
            "99999999999999999999",
        ] {
            assert_eq!(parse(invalid), None, "{invalid}");
        }
    }

    #[test]
    fn rate_limit_number_formats_only_exact_values() {
        assert_eq!(format(40_000_000), "40M");
        assert_eq!(format(1_500_000), "1.5M");
        assert_eq!(format(150_000), "150K");
        assert_eq!(format(1_250), "1250");
        assert_eq!(format(1_234_567), "1234567");
        assert_eq!(format(999), "999");
        for value in [40_000_000, 1_500_000, 150_000, 1_250, 1_234_567] {
            assert_eq!(parse(&format(value)), Some(value));
        }
    }
}
