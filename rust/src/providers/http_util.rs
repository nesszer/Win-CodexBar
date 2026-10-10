//! Status and JSON-body handling shared by API-key provider requests.
//!
//! Each provider keeps its own labels, so the error text stays
//! "<label> returned status <status>" and "Failed to parse <label>: <error>".

use reqwest::{RequestBuilder, Response, StatusCode};
use serde::de::DeserializeOwned;

use crate::core::ProviderError;

/// How one endpoint turns a non-success status into a `ProviderError`.
pub(crate) struct StatusPolicy<'a> {
    label: &'a str,
    auth: &'a [StatusCode],
    forbidden: Option<String>,
}

impl<'a> StatusPolicy<'a> {
    /// 401 and 403 both mean the key was rejected.
    pub(crate) fn auth_401_403(label: &'a str) -> Self {
        Self::with_auth(label, &[StatusCode::UNAUTHORIZED, StatusCode::FORBIDDEN])
    }

    /// Only 401 means the key was rejected; 403 is reported as a status error.
    pub(crate) fn auth_401(label: &'a str) -> Self {
        Self::with_auth(label, &[StatusCode::UNAUTHORIZED])
    }

    /// Every non-success status, 401 included, is reported as a status error.
    pub(crate) fn status_only(label: &'a str) -> Self {
        Self::with_auth(label, &[])
    }

    fn with_auth(label: &'a str, auth: &'a [StatusCode]) -> Self {
        Self {
            label,
            auth,
            forbidden: None,
        }
    }

    /// Reports 403 with this message instead of the generic status error.
    pub(crate) fn forbidden(mut self, message: String) -> Self {
        self.forbidden = Some(message);
        self
    }

    pub(crate) fn check(&self, status: StatusCode) -> Result<(), ProviderError> {
        if self.auth.contains(&status) {
            return Err(ProviderError::AuthRequired);
        }
        if status == StatusCode::FORBIDDEN
            && let Some(message) = &self.forbidden
        {
            return Err(ProviderError::Other(message.clone()));
        }
        if !status.is_success() {
            return Err(ProviderError::Other(format!(
                "{} returned status {status}",
                self.label
            )));
        }
        Ok(())
    }
}

/// Decodes a JSON body, reporting failures as "Failed to parse <label>: <error>".
pub(crate) async fn parse_json<T: DeserializeOwned>(
    response: Response,
    label: &str,
) -> Result<T, ProviderError> {
    response
        .json()
        .await
        .map_err(|e| ProviderError::Parse(format!("Failed to parse {label}: {e}")))
}

/// Sends `request`, applies `policy` to the status, then decodes the JSON body.
pub(crate) async fn send_json<T: DeserializeOwned>(
    request: RequestBuilder,
    policy: &StatusPolicy<'_>,
    parse_label: &str,
) -> Result<T, ProviderError> {
    let response = request.send().await?;
    policy.check(response.status())?;
    parse_json(response, parse_label).await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn outcome(policy: &StatusPolicy<'_>, status: u16) -> String {
        match policy.check(StatusCode::from_u16(status).unwrap()) {
            Ok(()) => "ok".to_string(),
            Err(ProviderError::AuthRequired) => "auth".to_string(),
            Err(ProviderError::Other(message)) => message,
            Err(other) => panic!("unexpected error {other:?}"),
        }
    }

    #[tokio::test]
    async fn send_json_matches_inline_decode_error_and_auth_mapping() {
        let mut server = mockito::Server::new_async().await;
        let _bad = server
            .mock("GET", "/bad")
            .with_body("not json")
            .create_async()
            .await;
        let _denied = server
            .mock("GET", "/denied")
            .with_status(403)
            .create_async()
            .await;
        let client = reqwest::Client::new();
        let bad_url = format!("{}/bad", server.url());

        let inline = client
            .get(&bad_url)
            .send()
            .await
            .unwrap()
            .json::<serde_json::Value>()
            .await
            .map_err(|e| format!("Failed to parse Poe balance: {e}"))
            .unwrap_err();
        let shared = send_json::<serde_json::Value>(
            client.get(&bad_url),
            &StatusPolicy::auth_401_403("Poe usage"),
            "Poe balance",
        )
        .await;
        assert!(matches!(shared, Err(ProviderError::Parse(message)) if message == inline));

        let denied = send_json::<serde_json::Value>(
            client.get(format!("{}/denied", server.url())),
            &StatusPolicy::auth_401_403("Poe usage"),
            "Poe balance",
        )
        .await;
        assert!(matches!(denied, Err(ProviderError::AuthRequired)));
    }

    #[test]
    fn provider_status_messages_are_pinned() {
        let policies = [
            StatusPolicy::auth_401_403("CrossModel credits"),
            StatusPolicy::auth_401("Codebuff API"),
            StatusPolicy::auth_401("Deepgram projects API")
                .forbidden("Deepgram API key does not have Management API access.".to_string()),
            StatusPolicy::auth_401("Deepgram usage API")
                .forbidden("Deepgram API key cannot read usage for project p1.".to_string()),
            StatusPolicy::auth_401("NanoGPT API"),
            StatusPolicy::auth_401_403("Poe usage"),
            StatusPolicy::status_only("Poe history"),
        ];
        let rows: &[(usize, u16, &str)] = &[
            (0, 200, "ok"),
            (0, 401, "auth"),
            (0, 403, "auth"),
            (
                0,
                500,
                "CrossModel credits returned status 500 Internal Server Error",
            ),
            (1, 401, "auth"),
            (1, 403, "Codebuff API returned status 403 Forbidden"),
            (1, 429, "Codebuff API returned status 429 Too Many Requests"),
            (2, 200, "ok"),
            (2, 401, "auth"),
            (
                2,
                403,
                "Deepgram API key does not have Management API access.",
            ),
            (
                2,
                502,
                "Deepgram projects API returned status 502 Bad Gateway",
            ),
            (3, 401, "auth"),
            (3, 403, "Deepgram API key cannot read usage for project p1."),
            (3, 404, "Deepgram usage API returned status 404 Not Found"),
            (4, 401, "auth"),
            (4, 403, "NanoGPT API returned status 403 Forbidden"),
            (5, 401, "auth"),
            (5, 403, "auth"),
            (5, 503, "Poe usage returned status 503 Service Unavailable"),
            (6, 401, "Poe history returned status 401 Unauthorized"),
            (6, 403, "Poe history returned status 403 Forbidden"),
            (6, 204, "ok"),
        ];
        for &(policy, status, expected) in rows {
            assert_eq!(
                outcome(&policies[policy], status),
                expected,
                "row {policy}/{status}"
            );
        }
    }
}
