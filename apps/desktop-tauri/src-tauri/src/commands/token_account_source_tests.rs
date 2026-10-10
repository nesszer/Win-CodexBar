//! Source-mode resolution for selected token accounts.

use codexbar::core::{FetchContext, ProviderId, SourceMode, instantiate_provider};

use super::fetch_context_tests::{CtxInput, fetch_ctx};

fn huggingface_context(usage_source: &str, with_account: bool) -> FetchContext {
    let accounts: &[(&str, &str)] = if with_account {
        &[("Work", "hf_account_token")]
    } else {
        &[]
    };
    fetch_ctx(
        ProviderId::HuggingFace,
        CtxInput {
            usage_source: Some(usage_source),
            accounts,
            ..Default::default()
        },
    )
}

#[test]
fn huggingface_token_account_keeps_auto_so_the_wallet_is_still_read() {
    let ctx = huggingface_context("auto", true);

    assert_eq!(ctx.source_mode, SourceMode::Auto);
    assert_eq!(ctx.api_key.as_deref(), Some("hf_account_token"));
}

#[test]
fn huggingface_token_account_with_explicit_api_source_stays_api_only() {
    let ctx = huggingface_context("oauth", true);

    assert_eq!(ctx.source_mode, SourceMode::OAuth);
    assert_eq!(ctx.api_key.as_deref(), Some("hf_account_token"));
}

#[test]
fn huggingface_token_account_with_an_unsupported_stored_source_falls_back_to_api() {
    for stale in ["web", "cli"] {
        let ctx = huggingface_context(stale, true);
        assert_eq!(ctx.source_mode, SourceMode::OAuth, "{stale}");
    }
}

#[test]
fn huggingface_without_a_token_account_follows_the_usage_source() {
    let ctx = huggingface_context("auto", false);
    assert_eq!(ctx.source_mode, SourceMode::Auto);
}

#[test]
fn only_huggingface_keeps_auto_for_token_accounts() {
    for id in ProviderId::all() {
        assert_eq!(
            instantiate_provider(*id).token_account_preserves_auto_source(),
            *id == ProviderId::HuggingFace,
            "{id:?}"
        );
    }
}
