use super::*;

fn parse(body: &str) -> Result<BalanceResponse, ProviderError> {
    parse_platform_balance(body.as_bytes())
}

fn summary_body(normal: &str, bonus: &str) -> String {
    format!(
        r#"{{"code":0,"msg":"","data":{{"biz_code":0,"biz_msg":"","biz_data":{{"normal_wallets":{normal},"bonus_wallets":{bonus}}}}}}}"#
    )
}

#[test]
fn sums_wallets_per_currency_and_prefers_funded_usd() {
    let body = summary_body(
        r#"[{"currency":"USD","balance":"2.50"},{"currency":"CNY","balance":"9"}]"#,
        r#"[{"currency":"USD","balance":1.25}]"#,
    );
    let response = parse(&body).unwrap();
    assert!(response.is_available);

    let snapshot = crate::providers::deepseek::DeepSeekProvider::snapshot_from_balance(response);
    assert_eq!(snapshot.primary.reset_description.as_deref(), Some("$3.75"));
}

#[test]
fn falls_back_to_funded_cny_when_usd_is_empty() {
    let body = summary_body(
        r#"[{"currency":"USD","balance":"0"},{"currency":"CNY","balance":"40"}]"#,
        r#"[{"currency":"CNY","balance":"2.25"}]"#,
    );
    let snapshot =
        crate::providers::deepseek::DeepSeekProvider::snapshot_from_balance(parse(&body).unwrap());
    assert_eq!(
        snapshot.login_method.as_deref(),
        Some("CNY balance: ¥42.25")
    );
}

#[test]
fn empty_wallets_report_an_unavailable_usd_balance() {
    let response = parse(&summary_body("[]", "[]")).unwrap();
    assert!(!response.is_available);
    assert_eq!(response.balance_infos.len(), 1);
    assert_eq!(response.balance_infos[0].currency, "USD");
}

#[test]
fn zero_balance_is_not_available() {
    let response = parse(&summary_body(r#"[{"currency":"USD","balance":"0"}]"#, "[]")).unwrap();
    assert!(!response.is_available);
}

#[test]
fn auth_envelope_codes_are_session_rejections() {
    for code in [40002, 40003] {
        let body = format!(r#"{{"code":{code},"msg":"auth","data":{{"unexpected":true}}}}"#);
        assert!(matches!(parse(&body), Err(ProviderError::AuthRequired)));

        let body = format!(
            r#"{{"code":0,"data":{{"biz_code":{code},"biz_msg":"auth","biz_data":"not a summary"}}}}"#
        );
        assert!(matches!(parse(&body), Err(ProviderError::AuthRequired)));
    }
}

#[test]
fn other_envelope_codes_are_not_session_rejections() {
    let error = parse(r#"{"code":50000,"msg":"busy","data":null}"#).unwrap_err();
    assert!(matches!(error, ProviderError::Other(_)), "{error:?}");
    assert!(!error.is_transport_failure());

    let error = parse(r#"{"code":0,"data":{"biz_code":1,"biz_data":null}}"#).unwrap_err();
    assert!(matches!(error, ProviderError::Other(_)), "{error:?}");
}

#[test]
fn malformed_or_missing_payloads_are_parse_errors() {
    for body in [
        "not json",
        r#"{"code":0}"#,
        r#"{"code":0,"data":{"biz_code":0}}"#,
        r#"{"code":0,"data":{"biz_code":0,"biz_data":{"normal_wallets":[{"currency":"USD","balance":"abc"}]}}}"#,
    ] {
        assert!(
            matches!(parse(body), Err(ProviderError::Parse(_))),
            "{body}"
        );
    }
}
