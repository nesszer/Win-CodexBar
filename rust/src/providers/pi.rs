//! Local Pi provider.
//!
//! Pi has no remote quota endpoint in the upstream provider model. Its usage
//! and token-cost history come from local Pi/OMP session JSONL files; the
//! ordinary provider refresh therefore exposes an informational local row and
//! leaves cost history to the dedicated scanner path.

use async_trait::async_trait;

use crate::core::{
    FetchContext, Provider, ProviderError, ProviderFetchResult, ProviderId, RateWindow, SourceMode,
    UsageSnapshot,
};

#[derive(Default)]
pub struct PiProvider;

impl PiProvider {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl Provider for PiProvider {
    fn id(&self) -> ProviderId {
        ProviderId::Pi
    }

    async fn fetch_usage(&self, ctx: &FetchContext) -> Result<ProviderFetchResult, ProviderError> {
        if ctx.source_mode != SourceMode::Auto {
            return Err(ProviderError::UnsupportedSource(ctx.source_mode));
        }

        Ok(ProviderFetchResult::new(
            UsageSnapshot::new(RateWindow::informational("Local Pi history")),
            "local",
        )
        .with_non_authoritative_pace())
    }

    fn available_sources(&self) -> Vec<SourceMode> {
        vec![SourceMode::Auto]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exposes_local_only_metadata() {
        let provider = PiProvider::new();
        assert_eq!(provider.id(), ProviderId::Pi);
        assert_eq!(provider.metadata().display_name, "Pi");
        assert!(!provider.metadata().default_enabled);
        assert_eq!(provider.available_sources(), vec![SourceMode::Auto]);
        assert!(!provider.supports_oauth());
        assert!(!provider.supports_web());
        assert!(!provider.supports_cli());
    }
}
