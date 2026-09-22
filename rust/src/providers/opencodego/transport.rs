use async_trait::async_trait;
use reqwest::Client;
use std::time::Duration;

use crate::core::ProviderError;

use super::{WebCookieSession, console, legacy};

#[async_trait]
pub(super) trait WebTransport: Send + Sync + 'static {
    type LegacySession: Clone + Send + Sync + 'static;

    async fn fetch_workspace_id(
        &self,
        cookie_header: &str,
        timeout: Duration,
    ) -> Result<String, ProviderError>;

    async fn fetch_console_usage(
        &self,
        workspace_id: &str,
        cookie_header: &str,
        timeout: Duration,
    ) -> Result<console::ConsoleUsage, ProviderError>;

    async fn fetch_console_balance(
        &self,
        workspace_id: &str,
        cookie_header: &str,
        timeout: Duration,
    ) -> Result<Option<f64>, ProviderError>;

    async fn resolve_legacy_session(
        &self,
        cookies: &WebCookieSession,
    ) -> Result<Self::LegacySession, ProviderError>;

    async fn fetch_legacy_usage(
        &self,
        session: &Self::LegacySession,
    ) -> Result<legacy::LegacyUsage, ProviderError>;

    async fn fetch_legacy_balance(
        &self,
        session: &Self::LegacySession,
        timeout: Duration,
    ) -> Result<Option<f64>, ProviderError>;
}

#[derive(Clone)]
pub(super) struct HttpWebTransport {
    client: Client,
}

impl HttpWebTransport {
    pub(super) fn new(client: Client) -> Self {
        Self { client }
    }
}

#[derive(Clone)]
pub(super) struct HttpLegacySession {
    cookie_header: String,
    workspace_id: String,
}

#[async_trait]
impl WebTransport for HttpWebTransport {
    type LegacySession = HttpLegacySession;

    async fn fetch_workspace_id(
        &self,
        cookie_header: &str,
        timeout: Duration,
    ) -> Result<String, ProviderError> {
        console::fetch_workspace_id(&self.client, cookie_header, timeout).await
    }

    async fn fetch_console_usage(
        &self,
        workspace_id: &str,
        cookie_header: &str,
        timeout: Duration,
    ) -> Result<console::ConsoleUsage, ProviderError> {
        console::fetch_usage(&self.client, workspace_id, cookie_header, timeout).await
    }

    async fn fetch_console_balance(
        &self,
        workspace_id: &str,
        cookie_header: &str,
        timeout: Duration,
    ) -> Result<Option<f64>, ProviderError> {
        console::fetch_balance(&self.client, workspace_id, cookie_header, timeout).await
    }

    async fn resolve_legacy_session(
        &self,
        cookies: &WebCookieSession,
    ) -> Result<Self::LegacySession, ProviderError> {
        let workspace_id = legacy::discover_workspace_id(&self.client, cookies.header()).await?;
        Ok(HttpLegacySession {
            cookie_header: cookies.header.clone(),
            workspace_id,
        })
    }

    async fn fetch_legacy_usage(
        &self,
        session: &Self::LegacySession,
    ) -> Result<legacy::LegacyUsage, ProviderError> {
        legacy::fetch_usage(&self.client, &session.cookie_header, &session.workspace_id).await
    }

    async fn fetch_legacy_balance(
        &self,
        session: &Self::LegacySession,
        timeout: Duration,
    ) -> Result<Option<f64>, ProviderError> {
        legacy::fetch_balance(
            &self.client,
            &session.cookie_header,
            &session.workspace_id,
            timeout,
        )
        .await
    }
}
