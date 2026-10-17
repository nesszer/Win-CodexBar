//! Reader for the AI Assistant quota XML (`AIAssistantQuotaManager2.xml`).
//!
//! The `AIAssistantQuotaManager2` component stores two options whose `value`
//! attributes hold HTML-encoded JSON: `quotaInfo` (quota state) and
//! `nextRefill` (refill schedule). The XML is read with narrow patterns rather
//! than a full XML parser, matching the macOS/Linux probes.

use std::sync::LazyLock;

use regex_lite::Regex;
use serde_json::{Map, Value};

use super::quota::{QuotaInfo, RefillInfo, TopUpQuota, parse_timestamp};
use crate::core::ProviderError;

const NO_QUOTA_INFO: &str = "No quota information found in the JetBrains AI configuration.";
const INVALID_JSON: &str = "Invalid JSON format";

static COMPONENT_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r#"<component[^>]*name\s*=\s*["']AIAssistantQuotaManager2["'][^>]*>[\s\S]*?</component>"#,
    )
    .expect("valid JetBrains component regex")
});

static OPTION_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"<option\b([^>]*)>").expect("valid JetBrains option regex"));

static ATTRIBUTE_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"([A-Za-z_][\w.-]*)\s*=\s*["']([^"']*)["']"#)
        .expect("valid JetBrains attribute regex")
});

/// Parse the quota state and optional refill schedule from XML bytes.
///
/// Returns [`ProviderError::Parse`] when the component or `quotaInfo` option is
/// missing, or when `quotaInfo` is not a JSON object. An unreadable
/// `nextRefill` is dropped rather than failing the whole quota.
pub(super) fn parse_quota_xml(
    bytes: &[u8],
) -> Result<(QuotaInfo, Option<RefillInfo>), ProviderError> {
    let content =
        std::str::from_utf8(bytes).map_err(|_| ProviderError::Parse(NO_QUOTA_INFO.to_string()))?;
    let component = COMPONENT_RE
        .find(content)
        .map(|found| found.as_str())
        .ok_or_else(|| ProviderError::Parse(NO_QUOTA_INFO.to_string()))?;

    let quota_raw = option_value(component, "quotaInfo")
        .filter(|raw| !raw.is_empty())
        .ok_or_else(|| ProviderError::Parse(NO_QUOTA_INFO.to_string()))?;
    let quota_json = parse_json_object(&decode_entities(&quota_raw))?;

    let refill = option_value(component, "nextRefill")
        .filter(|raw| !raw.is_empty())
        .and_then(|raw| parse_json_object(&decode_entities(&raw)).ok())
        .map(|json| parse_refill(&json));

    Ok((parse_quota(&quota_json), refill))
}

/// Value of the first `<option>` whose `name` attribute is `name`, in either
/// attribute order. The raw (still HTML-encoded) value is returned.
fn option_value(component: &str, name: &str) -> Option<String> {
    OPTION_RE.captures_iter(component).find_map(|option| {
        let attributes = option.get(1)?.as_str();
        let mut option_name = None;
        let mut raw_value = None;
        for attribute in ATTRIBUTE_RE.captures_iter(attributes) {
            let key = attribute.get(1)?.as_str();
            let value = attribute.get(2)?.as_str();
            match key {
                "name" => option_name = Some(value),
                "value" => raw_value = Some(value),
                _ => {}
            }
        }
        (option_name == Some(name))
            .then(|| raw_value.map(str::to_string))
            .flatten()
    })
}

/// Decode the XML entities the IDE writes into attribute values, in one pass.
fn decode_entities(raw: &str) -> String {
    const ENTITIES: [(&str, char); 6] = [
        ("&#10;", '\n'),
        ("&quot;", '"'),
        ("&amp;", '&'),
        ("&lt;", '<'),
        ("&gt;", '>'),
        ("&apos;", '\''),
    ];
    let mut out = String::with_capacity(raw.len());
    let mut rest = raw;
    while let Some(position) = rest.find('&') {
        out.push_str(&rest[..position]);
        rest = &rest[position..];
        match ENTITIES.iter().find(|(entity, _)| rest.starts_with(entity)) {
            Some((entity, decoded)) => {
                out.push(*decoded);
                rest = &rest[entity.len()..];
            }
            None => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

fn parse_json_object(text: &str) -> Result<Map<String, Value>, ProviderError> {
    match serde_json::from_str::<Value>(text) {
        Ok(Value::Object(map)) => Ok(map),
        _ => Err(ProviderError::Parse(format!(
            "Could not parse JetBrains AI quota: {INVALID_JSON}"
        ))),
    }
}

fn number(object: &Map<String, Value>, key: &str) -> Option<f64> {
    object.get(key)?.as_str()?.parse::<f64>().ok()
}

/// Select the monthly tariff balance when both its values are finite, so
/// monthly figures are never paired with totals that include top-ups.
/// Otherwise use the top-level `current` / `maximum` pair together.
fn parse_quota(json: &Map<String, Value>) -> QuotaInfo {
    let tariff = json
        .get("tariffQuota")
        .and_then(Value::as_object)
        .filter(|quota| {
            ["current", "maximum"]
                .iter()
                .all(|key| number(quota, key).is_some_and(f64::is_finite))
        });
    let quota = tariff.unwrap_or(json);
    let top_up = json
        .get("topUpQuota")
        .and_then(Value::as_object)
        .and_then(|top| TopUpQuota::new(number(top, "maximum"), number(top, "available")));

    QuotaInfo {
        used: number(quota, "current").unwrap_or(0.0),
        maximum: number(quota, "maximum").unwrap_or(0.0),
        top_up,
    }
}

fn parse_refill(json: &Map<String, Value>) -> RefillInfo {
    RefillInfo {
        next: json
            .get("next")
            .and_then(Value::as_str)
            .and_then(parse_timestamp),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{TimeZone, Utc};

    /// Synthetic `AIAssistantQuotaManager2.xml` in the IDE's encoded form.
    fn quota_xml(quota_json: &str, refill_json: Option<&str>) -> String {
        let encode = |raw: &str| {
            raw.replace('&', "&amp;")
                .replace('"', "&quot;")
                .replace('<', "&lt;")
                .replace('>', "&gt;")
        };
        let refill = refill_json
            .map(|raw| {
                format!(
                    "\n  <option name=\"nextRefill\" value=\"{}\" />",
                    encode(raw)
                )
            })
            .unwrap_or_default();
        format!(
            "<application>\n<component name=\"AIAssistantQuotaManager2\">\n  \
             <option name=\"quotaInfo\" value=\"{}\" />{refill}\n</component>\n</application>\n",
            encode(quota_json)
        )
    }

    const QUOTA_JSON: &str = r#"{"type":"Available","current":"68433.145","maximum":"6489986.397","tariffQuota":{"current":"68433.145","maximum":"1000000","available":"931566.855"},"topUpQuota":{"current":"0","maximum":"5489986.397","available":"5489986.397"}}"#;
    const REFILL_JSON: &str = r#"{"type":"Known","next":"2026-11-01T00:00:00Z","tariff":{"amount":"1000000","duration":"PT720H"}}"#;

    #[test]
    fn monthly_percent_uses_tariff_and_top_up_is_separate() {
        let (quota, refill) =
            parse_quota_xml(quota_xml(QUOTA_JSON, Some(REFILL_JSON)).as_bytes()).expect("parses");

        assert!((quota.used_percent() - 6.8433145).abs() < 1e-6);
        let top_up = quota.top_up.expect("top-up present");
        assert_eq!(top_up.available, 5_489_986.397);
        assert_eq!(
            refill.and_then(|refill| refill.next),
            Some(Utc.with_ymd_and_hms(2026, 11, 1, 0, 0, 0).unwrap())
        );
    }

    #[test]
    fn top_level_totals_are_used_together_when_tariff_is_not_usable() {
        let json = r#"{"current":"3489986.397","maximum":"6489986.397","tariffQuota":{"current":"nan","maximum":"1000000","available":"1"}}"#;
        let (quota, _) = parse_quota_xml(quota_xml(json, None).as_bytes()).expect("parses");

        assert_eq!(quota.used, 3_489_986.397);
        assert_eq!(quota.maximum, 6_489_986.397);
        assert!(quota.top_up.is_none());
    }

    #[test]
    fn tariff_is_preferred_over_top_level_totals() {
        let json =
            r#"{"current":"9","maximum":"9","tariffQuota":{"current":"25","maximum":"100"}}"#;
        let (quota, _) = parse_quota_xml(quota_xml(json, None).as_bytes()).expect("parses");
        assert_eq!(quota.used_percent(), 25.0);
    }

    #[test]
    fn invalid_top_up_balances_are_never_reconstructed() {
        for available in [None, Some("abc"), Some("nan"), Some("inf"), Some("-1")] {
            let top = match available {
                Some(value) => format!(r#","available":"{value}""#),
                None => String::new(),
            };
            let json = format!(
                r#"{{"current":"1","maximum":"100","tariffQuota":{{"current":"1","maximum":"100"}},"topUpQuota":{{"maximum":"500"{top}}}}}"#
            );
            let (quota, _) = parse_quota_xml(quota_xml(&json, None).as_bytes()).expect("parses");
            assert!(
                quota.top_up.is_none(),
                "available {available:?} must be rejected"
            );
        }
    }

    #[test]
    fn exhausted_top_up_balance_stays_present() {
        let json = r#"{"current":"0","maximum":"100","tariffQuota":{"current":"0","maximum":"100"},"topUpQuota":{"maximum":"5489986.397","available":"0"}}"#;
        let (quota, _) = parse_quota_xml(quota_xml(json, None).as_bytes()).expect("parses");
        assert_eq!(quota.top_up.map(|top| top.available_credits()), Some(0.0));
    }

    #[test]
    fn missing_quota_info_is_a_parse_error() {
        let xml = "<application><component name=\"AIAssistantQuotaManager2\">\
                   <option name=\"nextRefill\" value=\"{}\" /></component></application>";
        let error = parse_quota_xml(xml.as_bytes()).expect_err("no quotaInfo");
        assert!(matches!(error, ProviderError::Parse(message) if message == NO_QUOTA_INFO));
    }

    #[test]
    fn quota_info_outside_the_quota_component_is_ignored() {
        let xml = format!(
            "<application><component name=\"OtherComponent\">\
             <option name=\"quotaInfo\" value=\"{}\" /></component></application>",
            QUOTA_JSON.replace('"', "&quot;")
        );
        assert!(parse_quota_xml(xml.as_bytes()).is_err());
    }

    #[test]
    fn option_attributes_are_matched_in_either_order() {
        let json = QUOTA_JSON.replace('"', "&quot;");
        let xml = format!(
            "<application><component name=\"AIAssistantQuotaManager2\">\
             <option value=\"{json}\" name=\"quotaInfo\" /></component></application>"
        );
        let (quota, _) = parse_quota_xml(xml.as_bytes()).expect("parses");
        assert!(quota.top_up.is_some());
    }

    #[test]
    fn invalid_quota_json_is_a_parse_error() {
        let error = parse_quota_xml(quota_xml("{not json", None).as_bytes()).expect_err("bad json");
        assert!(matches!(error, ProviderError::Parse(message) if message.contains(INVALID_JSON)));
    }

    #[test]
    fn invalid_refill_json_is_dropped_without_failing_quota() {
        let (quota, refill) =
            parse_quota_xml(quota_xml(QUOTA_JSON, Some("{broken")).as_bytes()).expect("parses");
        assert!(refill.is_none());
        assert!(quota.top_up.is_some());
    }

    #[test]
    fn non_utf8_xml_has_no_quota_info() {
        assert!(parse_quota_xml(&[0xff, 0xfe, 0x00]).is_err());
    }

    #[test]
    fn entities_decode_in_a_single_pass() {
        assert_eq!(
            decode_entities("&quot;a&amp;quot;&#10;&lt;b&gt;&apos;&amp;"),
            "\"a&quot;\n<b>'&"
        );
        assert_eq!(
            decode_entities("Tom & Jerry &unknown;"),
            "Tom & Jerry &unknown;"
        );
    }
}
