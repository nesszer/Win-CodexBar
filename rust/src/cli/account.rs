//! Account command implementation
//!
//! Manages token accounts for providers that support multiple accounts.

use clap::{Parser, Subcommand};

use crate::core::{
    ProviderAccountData, ProviderId, TokenAccount, TokenAccountStore, TokenAccountSupport,
};

/// Arguments for the account command
#[derive(Parser, Debug)]
pub struct AccountArgs {
    #[command(subcommand)]
    pub command: AccountCommand,
}

#[derive(Subcommand, Debug)]
pub enum AccountCommand {
    /// List accounts for a provider
    List {
        /// Provider name (e.g., claude, cursor, zai)
        provider: String,
    },
    /// Add a new account
    Add {
        /// Provider name (e.g., claude, cursor, zai)
        provider: String,
        /// Label for the account (e.g., "Personal", "Work")
        #[arg(short, long)]
        label: String,
        /// Token or cookie value
        #[arg(short, long)]
        token: String,
    },
    /// Remove an account
    Remove {
        /// Provider name (e.g., claude, cursor, zai)
        provider: String,
        /// Account label or ID to remove
        account: String,
    },
    /// Switch active account
    Switch {
        /// Provider name (e.g., claude, cursor, zai)
        provider: String,
        /// Account label or ID to switch to
        account: String,
    },
}

/// Run the account command
pub async fn run(args: AccountArgs) -> anyhow::Result<()> {
    match args.command {
        AccountCommand::List { provider } => list_accounts(&provider).await,
        AccountCommand::Add {
            provider,
            label,
            token,
        } => add_account(&provider, &label, &token).await,
        AccountCommand::Remove { provider, account } => remove_account(&provider, &account).await,
        AccountCommand::Switch { provider, account } => switch_account(&provider, &account).await,
    }
}

/// List accounts for a provider
async fn list_accounts(provider_name: &str) -> anyhow::Result<()> {
    let provider = parse_provider(provider_name)?;

    if !TokenAccountSupport::is_supported(provider) {
        println!(
            "{} does not support token accounts.",
            provider.display_name()
        );
        return Ok(());
    }

    let store = TokenAccountStore::new();
    let data = store.load_provider(provider)?;

    if data.accounts.is_empty() {
        println!("No accounts configured for {}.", provider.display_name());
        println!(
            "Use 'codexbar account add {}' to add one.",
            provider.cli_name()
        );
        return Ok(());
    }

    println!("{} accounts:", provider.display_name());
    for (i, account) in data.accounts.iter().enumerate() {
        let active = if i == data.clamped_active_index() {
            " (active)"
        } else {
            ""
        };
        let masked_token = mask_token(&account.token);
        println!("  {}. {}{}", i + 1, account.label, active);
        println!("     Token: {}", masked_token);
        println!(
            "     Added: {}",
            account.added_at_datetime().format("%Y-%m-%d %H:%M")
        );
        if let Some(last_used) = account.last_used_datetime() {
            println!("     Last used: {}", last_used.format("%Y-%m-%d %H:%M"));
        }
    }

    Ok(())
}

/// Add a new account
async fn add_account(provider_name: &str, label: &str, token: &str) -> anyhow::Result<()> {
    let provider = parse_provider(provider_name)?;

    if !TokenAccountSupport::is_supported(provider) {
        anyhow::bail!(
            "{} does not support token accounts.",
            provider.display_name()
        );
    }

    let store = TokenAccountStore::new();
    let mut data = store.load_provider(provider)?;

    // Check for duplicate label
    if data
        .accounts
        .iter()
        .any(|a| a.label.eq_ignore_ascii_case(label))
    {
        anyhow::bail!("An account with label '{}' already exists.", label);
    }

    let account = TokenAccount::new(label, token);
    data.add_account(account);
    store.save_provider(provider, &data)?;

    println!("Added account '{}' for {}.", label, provider.display_name());
    Ok(())
}

/// Remove an account
async fn remove_account(provider_name: &str, account_ref: &str) -> anyhow::Result<()> {
    let provider = parse_provider(provider_name)?;

    if !TokenAccountSupport::is_supported(provider) {
        anyhow::bail!(
            "{} does not support token accounts.",
            provider.display_name()
        );
    }

    let store = TokenAccountStore::new();
    let mut data = store.load_provider(provider)?;

    let account = find_account(&data, account_ref)?;
    let label = account.label.clone();
    let id = account.id;

    data.remove_account(id);
    store.save_provider(provider, &data)?;

    println!(
        "Removed account '{}' from {}.",
        label,
        provider.display_name()
    );
    Ok(())
}

/// Switch active account
async fn switch_account(provider_name: &str, account_ref: &str) -> anyhow::Result<()> {
    let provider = parse_provider(provider_name)?;

    if !TokenAccountSupport::is_supported(provider) {
        anyhow::bail!(
            "{} does not support token accounts.",
            provider.display_name()
        );
    }

    let store = TokenAccountStore::new();
    let mut data = store.load_provider(provider)?;

    let account = find_account(&data, account_ref)?;
    let label = account.label.clone();
    let id = account.id;

    data.set_active_by_id(id);
    store.save_provider(provider, &data)?;

    println!(
        "Switched to account '{}' for {}.",
        label,
        provider.display_name()
    );
    Ok(())
}

/// Parse provider name to ProviderId
fn parse_provider(name: &str) -> anyhow::Result<ProviderId> {
    ProviderId::from_cli_name(name)
        .ok_or_else(|| anyhow::anyhow!("Unknown provider: '{}'. Use 'codexbar usage --provider all' to see available providers.", name))
}

/// Find account by label or index
fn find_account<'a>(
    data: &'a ProviderAccountData,
    account_ref: &str,
) -> anyhow::Result<&'a TokenAccount> {
    // Try parsing as index first (1-based)
    if let Ok(idx) = account_ref.parse::<usize>()
        && idx > 0
        && idx <= data.accounts.len()
    {
        return Ok(&data.accounts[idx - 1]);
    }

    // Try matching by label (case-insensitive)
    if let Some(account) = data
        .accounts
        .iter()
        .find(|a| a.label.eq_ignore_ascii_case(account_ref))
    {
        return Ok(account);
    }

    // Try matching by UUID prefix
    if let Some(account) = data
        .accounts
        .iter()
        .find(|a| a.id.to_string().starts_with(account_ref))
    {
        return Ok(account);
    }

    anyhow::bail!(
        "Account '{}' not found. Use 'codexbar account list' to see available accounts.",
        account_ref
    )
}

/// Mask a token for display (show first 4 and last 4 chars).
///
/// Operates on `char`s, not bytes: a stored token containing multibyte
/// characters would otherwise panic here on a non-char-boundary byte slice.
fn mask_token(token: &str) -> String {
    let trimmed = token.trim();
    let char_count = trimmed.chars().count();
    let first4: String = trimmed.chars().take(4).collect();
    if char_count > 12 {
        let mut last: Vec<char> = trimmed.chars().rev().take(4).collect();
        last.reverse();
        let last4: String = last.into_iter().collect();
        format!("{first4}...{last4}")
    } else if char_count > 4 {
        format!("{first4}...")
    } else {
        "****".to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mask_token_handles_multibyte_without_panic() {
        // Non-ASCII token longer than 12 chars must not panic on byte slicing.
        assert_eq!(mask_token("😀😀😀😀abcdefghij😀😀😀😀"), "😀😀😀😀...😀😀😀😀");
        assert_eq!(mask_token("aaaaa😀"), "aaaa...");
        assert_eq!(mask_token("😀"), "****");
        assert_eq!(mask_token("sk-1234567890abcdef"), "sk-1...cdef");
    }
}
