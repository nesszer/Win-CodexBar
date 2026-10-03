use super::*;
use crate::core::ProviderError;
use base64::Engine;
use chrono::{DateTime, Utc};

const TOKEN_ENDPOINT: &str = "https://auth.x.ai/oauth2/token";

#[derive(Deserialize)]
struct TokenResponse {
    access_token: String,
    refresh_token: Option<String>,
    expires_in: i64,
}

impl AccountManager {
    /// Renew a saved login before usage reads, persisting rotated tokens before reuse.
    pub async fn auth_text_for_usage(
        &self,
        id: &str,
        force: bool,
    ) -> Result<String, ProviderError> {
        let _credentials = CREDENTIAL_OPERATION.lock().await;
        self.refresh_locked(id, force, TOKEN_ENDPOINT).await
    }

    pub async fn ambient_auth_text_for_usage(&self) -> Result<String, ProviderError> {
        let _credentials = CREDENTIAL_OPERATION.lock().await;
        let login = read_login(&self.ambient_auth)
            .map_err(storage_error)?
            .ok_or(ProviderError::AuthRequired)?;
        self.refresh_locked(&login.id().map_err(storage_error)?, false, TOKEN_ENDPOINT)
            .await
    }

    async fn refresh_locked(
        &self,
        id: &str,
        force: bool,
        endpoint: &str,
    ) -> Result<String, ProviderError> {
        if let Some(auth) = super::orca::refresh_login(id).await? {
            return Ok(auth);
        }
        let text = self.auth_text_for(id).map_err(storage_error)?;
        let mut login: SavedLogin = SavedLogin {
            auth: serde_json::from_str(&text)
                .map_err(|_| ProviderError::Parse("Invalid Grok login.".into()))?,
        };
        let entry = auth_file(&login.auth)
            .map_err(storage_error)?
            .select_account()
            .map_err(|_| ProviderError::AuthRequired)?;
        let Some(client_id) = entry
            .scope
            .strip_prefix("https://auth.x.ai::")
            .filter(|id| !id.is_empty())
        else {
            return Ok(text);
        };
        let expires = text_field(entry.value(), "expires_at")
            .and_then(|value| DateTime::parse_from_rfc3339(&value).ok());
        if !force && !expires.is_some_and(|date| date <= Utc::now() + chrono::Duration::seconds(60))
        {
            return Ok(text);
        }
        let refresh = text_field(entry.value(), "refresh_token")
            .filter(|token| !token.is_empty())
            .ok_or(ProviderError::AuthRequired)?;
        let scope = entry.scope.to_owned();
        let client = crate::core::credentialed_http_client_builder()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(std::time::Duration::from_secs(15))
            .build()?;
        let response = client
            .post(endpoint)
            .form(&[
                ("grant_type", "refresh_token"),
                ("client_id", client_id),
                ("refresh_token", refresh.as_str()),
            ])
            .send()
            .await?;
        if matches!(response.status().as_u16(), 400 | 401 | 403) {
            return Err(ProviderError::AuthRequired);
        }
        let response = response
            .error_for_status()?
            .json::<TokenResponse>()
            .await
            .map_err(|_| ProviderError::Parse("Invalid Grok token refresh response.".into()))?;
        // These claims come from the fixed, TLS-authenticated token endpoint.
        // Use them only to reject an identity mix-up, never to authenticate a request.
        let claims = response
            .access_token
            .split('.')
            .nth(1)
            .and_then(|part| {
                base64::engine::general_purpose::URL_SAFE_NO_PAD
                    .decode(part)
                    .ok()
            })
            .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
            .ok_or_else(|| {
                ProviderError::Parse("Grok refreshed token has no account identity.".into())
            })?;
        if claims.get("sub").and_then(Value::as_str) != Some(id)
            || claims.get("iss").and_then(Value::as_str) != Some("https://auth.x.ai")
        {
            return Err(ProviderError::Other(
                "Grok refreshed a different account; saved login was preserved.".into(),
            ));
        }
        let expires_at = chrono::Duration::try_seconds(response.expires_in)
            .filter(|duration| *duration > chrono::Duration::zero())
            .and_then(|duration| Utc::now().checked_add_signed(duration))
            .ok_or_else(|| ProviderError::Parse("Invalid Grok token lifetime.".into()))?;
        let entry = login
            .auth
            .get_mut(&scope)
            .and_then(Value::as_object_mut)
            .ok_or(ProviderError::AuthRequired)?;
        entry.insert("key".into(), Value::String(response.access_token));
        entry.insert("expires_at".into(), Value::String(expires_at.to_rfc3339()));
        if let Some(refresh) = response.refresh_token.filter(|token| !token.is_empty()) {
            entry.insert("refresh_token".into(), Value::String(refresh));
        }
        self.reauthenticate(id, login).map_err(storage_error)?;
        self.auth_text_for(id).map_err(storage_error)
    }
}

fn storage_error(_: io::Error) -> ProviderError {
    ProviderError::Other("Could not read or save the selected Grok login.".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn setup() -> (tempfile::TempDir, AccountManager) {
        let dir = tempfile::tempdir().unwrap();
        let manager = AccountManager {
            root: dir.path().join("store"),
            ambient_auth: dir.path().join("home/auth.json"),
        };
        manager.import(SavedLogin { auth: json!({"https://auth.x.ai::client": {
            "key": "expired", "refresh_token": "old-refresh", "user_id": "user-a",
            "email": "a@example.com", "auth_mode": "oidc", "expires_at": "2000-01-01T00:00:00Z"
        }}) }).unwrap();
        (dir, manager)
    }

    fn token(subject: &str) -> String {
        let claims = json!({"sub":subject,"iss":"https://auth.x.ai"});
        format!(
            "header.{}.signature",
            base64::engine::general_purpose::URL_SAFE_NO_PAD
                .encode(serde_json::to_vec(&claims).unwrap())
        )
    }

    #[tokio::test]
    async fn renewal_persists_rotation_and_survives_a_new_manager() {
        let (_dir, manager) = setup();
        let mut server = mockito::Server::new_async().await;
        let request = server
            .mock("POST", "/token")
            .match_body("grant_type=refresh_token&client_id=client&refresh_token=old-refresh")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(
                json!({"access_token":token("user-a"),"refresh_token":"rotated","expires_in":3600})
                    .to_string(),
            )
            .expect(1)
            .create_async()
            .await;
        let endpoint = format!("{}/token", server.url());
        let text = manager
            .refresh_locked("user-a", false, &endpoint)
            .await
            .unwrap();
        let restored = AccountManager {
            root: manager.root.clone(),
            ambient_auth: manager.ambient_auth.clone(),
        };
        assert_eq!(
            restored
                .refresh_locked("user-a", false, &endpoint)
                .await
                .unwrap(),
            text
        );
        assert!(text.contains("rotated"));
        assert_eq!(restored.list().unwrap().len(), 1);
        assert!(!restored.ambient_auth.exists());
        request.assert_async().await;
    }

    #[tokio::test]
    async fn renewal_updates_the_active_login_without_switching_accounts() {
        let (_dir, manager) = setup();
        let old = manager.auth_text_for("user-a").unwrap();
        std::fs::create_dir_all(manager.ambient_auth.parent().unwrap()).unwrap();
        std::fs::write(&manager.ambient_auth, &old).unwrap();
        let mut server = mockito::Server::new_async().await;
        let request = server
            .mock("POST", "/token")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(json!({"access_token":token("user-a"),"expires_in":3600}).to_string())
            .create_async()
            .await;
        manager
            .refresh_locked("user-a", false, &format!("{}/token", server.url()))
            .await
            .unwrap();
        let ambient = std::fs::read_to_string(&manager.ambient_auth).unwrap();
        assert!(ambient.contains(&token("user-a")));
        assert!(ambient.contains("old-refresh"));
        assert!(manager.list().unwrap()[0].is_active);
        request.assert_async().await;
    }

    #[tokio::test]
    async fn wrong_identity_and_revoked_refresh_preserve_the_saved_login() {
        for (status, body) in [
            (
                200,
                json!({"access_token":token("user-b"),"refresh_token":"wrong","expires_in":3600})
                    .to_string(),
            ),
            (400, "{\"error\":\"invalid_grant\"}".into()),
            (503, "service unavailable".into()),
        ] {
            let (_dir, manager) = setup();
            let original = manager.auth_text_for("user-a").unwrap();
            let mut server = mockito::Server::new_async().await;
            let request = server
                .mock("POST", "/token")
                .with_status(status)
                .with_header("content-type", "application/json")
                .with_body(body)
                .create_async()
                .await;
            let result = manager
                .refresh_locked("user-a", false, &format!("{}/token", server.url()))
                .await;
            assert!(result.is_err());
            if status == 400 {
                assert!(matches!(result, Err(ProviderError::AuthRequired)));
            }
            if status == 503 {
                assert!(matches!(result, Err(ProviderError::Network(_))));
            }
            assert_eq!(manager.auth_text_for("user-a").unwrap(), original);
            request.assert_async().await;
        }
    }
}
