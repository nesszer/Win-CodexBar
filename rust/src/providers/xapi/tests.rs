#[cfg(test)]
mod xapi_tests {
    use crate::core::{Provider, ProviderId};
    use crate::providers::xapi::{XapiProvider, parse_account_id, parse_credits};
    use serde_json::{Map, Value, json};

    fn object(value: Value) -> Map<String, Value> {
        value.as_object().cloned().expect("JSON object")
    }

    fn detail_values(result: &crate::core::ProviderFetchResult) -> Vec<(String, String)> {
        result
            .display_details
            .iter()
            .map(|d| (d.title().to_string(), d.value().to_string()))
            .collect()
    }

    #[test]
    fn provider_metadata() {
        let provider = XapiProvider::new();
        assert_eq!(provider.id(), ProviderId::Xapi);
        assert_eq!(provider.metadata().display_name, "X API");
        assert_eq!(provider.metadata().session_label, "Balance");
        assert!(!provider.metadata().supports_opus);
        assert!(!provider.metadata().supports_credits);
        assert!(!provider.metadata().default_enabled);
        assert!(!provider.metadata().is_primary);
    }

    #[test]
    fn account_id_accepts_strings_and_safe_integers() {
        let id = |v: Value| parse_account_id(&object(json!({ "account": { "id": v } })));
        assert_eq!(id(json!("fixture-account")).unwrap(), "fixture-account");
        assert_eq!(id(json!(42)).unwrap(), "42");
        assert_eq!(id(json!(42.0)).unwrap(), "42");
        assert_eq!(
            id(json!(9_007_199_254_740_991_u64)).unwrap(),
            "9007199254740991"
        );
    }

    #[test]
    fn account_id_rejects_unsafe_or_malformed_values() {
        let id = |v: Value| parse_account_id(&object(json!({ "account": { "id": v } })));
        assert!(id(json!(9_007_199_254_740_992_u64)).is_err());
        assert!(id(json!(-1)).is_err());
        assert!(id(json!(1.5)).is_err());
        assert!(id(json!("")).is_err());
        assert!(id(json!("a/b")).is_err());
        assert!(id(json!("a".repeat(129))).is_err());
        assert!(parse_account_id(&object(json!({}))).is_err());
    }

    #[test]
    fn credits_sum_paid_and_free_into_a_card_hidden_balance() {
        let result = parse_credits(
            "1",
            &object(json!({ "credits": { "balance": 42.5 }, "freeCredits": { "balance": 7.25 } })),
        )
        .unwrap();

        assert_eq!(
            detail_values(&result),
            [
                ("Balance".to_string(), "$49.75".to_string()),
                ("Purchased credits".to_string(), "$42.50".to_string()),
                ("Free credits".to_string(), "$7.25".to_string()),
            ]
        );
        let primary = &result.usage.primary;
        assert!(primary.is_informational && primary.hidden_on_card);
        assert_eq!(
            primary.reset_description.as_deref(),
            Some("Balance: $49.75")
        );
        assert!(result.cost.is_none(), "upstream hides the X API cost block");
    }

    #[test]
    fn credits_accept_integer_balances_and_missing_free_credits() {
        let result =
            parse_credits("1", &object(json!({ "credits": { "balance": 1234 } }))).unwrap();
        assert_eq!(
            detail_values(&result),
            [
                ("Balance".to_string(), "$1,234.00".to_string()),
                ("Purchased credits".to_string(), "$1,234.00".to_string()),
                ("Free credits".to_string(), "$0.00".to_string()),
            ]
        );
    }

    #[test]
    fn credits_keep_negative_balances_signed() {
        let result =
            parse_credits("1", &object(json!({ "credits": { "balance": -2.15 } }))).unwrap();
        assert_eq!(detail_values(&result)[0].1, "-$2.15");
    }

    #[test]
    fn credits_reject_missing_or_invalid_balances() {
        let parse = |v: Value| parse_credits("1", &object(v));
        assert!(parse(json!({})).is_err());
        assert!(parse(json!({ "credits": { "balance": "10" } })).is_err());
        assert!(parse(json!({ "credits": { "balance": 1 }, "freeCredits": {} })).is_err());
        assert!(parse(json!({ "credits": { "balance": 1 }, "freeCredits": null })).is_err());
    }

    #[test]
    fn credits_reject_a_sum_that_overflows() {
        let result = parse_credits(
            "1",
            &object(json!({
                "credits": { "balance": f64::MAX },
                "freeCredits": { "balance": f64::MAX }
            })),
        );
        assert!(result.is_err());
    }

    #[test]
    fn status_errors_match_upstream() {
        use reqwest::StatusCode;
        let message = |code: u16| {
            crate::providers::xapi::check_status(StatusCode::from_u16(code).unwrap())
                .unwrap_err()
                .to_string()
        };
        assert!(crate::providers::xapi::check_status(StatusCode::OK).is_ok());
        assert!(message(401).contains("X API session expired"));
        assert!(message(403).contains("X developer console access was denied."));
        assert!(message(429).contains("X API console is rate limited."));
        assert!(message(503).contains("X developer console is unavailable."));
        assert!(message(418).contains("X developer console returned HTTP 418."));
    }

    #[test]
    fn provider_supports_web() {
        let provider = XapiProvider::new();
        assert!(provider.supports_web());
    }

    #[test]
    fn provider_available_sources() {
        let provider = XapiProvider::new();
        let sources = provider.available_sources();
        assert_eq!(sources.len(), 2);
        assert!(sources.contains(&crate::core::SourceMode::Auto));
        assert!(sources.contains(&crate::core::SourceMode::Web));
    }
}

#[cfg(test)]
mod xapi_fetch_tests {
    use std::sync::{Arc, Mutex};

    use crate::core::{FetchContext, ProviderError};
    use crate::providers::xapi::{SESSION_EXPIRED, XapiProvider};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    /// (request path, Cookie header, X-CSRF-Token header)
    type Recorded = Arc<Mutex<Vec<(String, String, String)>>>;

    /// Serves one canned response per connection, in order.
    async fn serve_in_order(
        responses: Vec<(u16, &'static str)>,
    ) -> (String, Recorded, tokio::task::JoinHandle<()>) {
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
            .await
            .unwrap();
        let base = format!("http://127.0.0.1:{}", listener.local_addr().unwrap().port());
        let recorded: Recorded = Arc::new(Mutex::new(Vec::new()));
        let sink = Arc::clone(&recorded);
        let task = tokio::spawn(async move {
            for (status, body) in responses {
                let Ok((mut stream, _)) = listener.accept().await else {
                    return;
                };
                let mut request = Vec::new();
                let mut chunk = [0u8; 1024];
                while !request.windows(4).any(|w| w == b"\r\n\r\n") {
                    match stream.read(&mut chunk).await {
                        Ok(0) | Err(_) => break,
                        Ok(read) => request.extend_from_slice(&chunk[..read]),
                    }
                }
                let text = String::from_utf8_lossy(&request).to_string();
                let path = text
                    .split_whitespace()
                    .nth(1)
                    .unwrap_or_default()
                    .to_string();
                let header = |name: &str| {
                    text.lines()
                        .find_map(|line| {
                            let (key, value) = line.split_once(':')?;
                            key.eq_ignore_ascii_case(name)
                                .then(|| value.trim().to_string())
                        })
                        .unwrap_or_default()
                };
                sink.lock()
                    .unwrap()
                    .push((path, header("cookie"), header("x-csrf-token")));
                let response = format!(
                    "HTTP/1.1 {status} X\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                    body.len()
                );
                stream
                    .write_all(response.as_bytes())
                    .await
                    .expect("write fixture response");
                stream.shutdown().await.expect("close fixture stream");
            }
        });
        (base, recorded, task)
    }

    fn two_browsers() -> Result<Vec<(String, String)>, ProviderError> {
        Ok(vec![
            ("Chrome".to_string(), "auth_token=a; ct0=c1".to_string()),
            ("Edge".to_string(), "auth_token=b; ct0=c2".to_string()),
        ])
    }

    #[tokio::test]
    async fn an_expired_session_moves_to_the_next_browser() {
        let expired = [
            (
                400,
                r#"{"errors":[{"code":215,"message":"Bad Authentication data."}]}"#,
            ),
            (401, "{}"),
        ];
        for first in expired {
            let (base, recorded, task) = serve_in_order(vec![
                first,
                (200, r#"{"account":{"id":42}}"#),
                (
                    200,
                    r#"{"credits":{"balance":5},"freeCredits":{"balance":2.5}}"#,
                ),
            ])
            .await;
            let provider = XapiProvider::with_base_url(&base);
            let result = provider
                .fetch_web(&FetchContext::default(), &two_browsers)
                .await
                .unwrap_or_else(|error| panic!("status {}: {error:?}", first.0));
            assert_eq!(
                result.usage.primary.reset_description.as_deref(),
                Some("Balance: $7.50")
            );
            let requests = recorded.lock().unwrap().clone();
            assert_eq!(
                requests,
                vec![
                    ("/api/me".into(), "auth_token=a; ct0=c1".into(), "c1".into()),
                    ("/api/me".into(), "auth_token=b; ct0=c2".into(), "c2".into()),
                    (
                        "/api/accounts/42/credits".into(),
                        "auth_token=b; ct0=c2".into(),
                        "c2".into()
                    ),
                ],
                "status {}",
                first.0
            );
            task.abort();
        }
    }

    #[tokio::test]
    async fn a_plain_bad_request_ends_the_fetch() {
        let (base, recorded, task) = serve_in_order(vec![
            (400, r#"{"errors":[{"code":88,"message":"Rate limit"}]}"#),
            (200, r#"{"account":{"id":42}}"#),
        ])
        .await;
        let provider = XapiProvider::with_base_url(&base);
        match provider
            .fetch_web(&FetchContext::default(), &two_browsers)
            .await
        {
            Err(ProviderError::Other(message)) => {
                assert_eq!(message, "X developer console returned HTTP 400.")
            }
            other => panic!("unexpected {other:?}"),
        }
        assert_eq!(recorded.lock().unwrap().len(), 1);
        task.abort();
    }

    #[tokio::test]
    async fn every_expired_session_reports_the_upstream_message() {
        let (base, _recorded, task) = serve_in_order(vec![(401, "{}"), (401, "{}")]).await;
        let provider = XapiProvider::with_base_url(&base);
        match provider
            .fetch_web(&FetchContext::default(), &two_browsers)
            .await
        {
            Err(ProviderError::Other(message)) => assert_eq!(message, SESSION_EXPIRED),
            other => panic!("unexpected {other:?}"),
        }
        task.abort();
    }
}
