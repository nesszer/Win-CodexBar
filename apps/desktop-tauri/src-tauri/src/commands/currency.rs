use std::{collections::HashMap, path::PathBuf};

use codexbar::currency::{
    SUPPORTED_CURRENCY_CODES, convert_amount, fallback_rates, fetch_exchange_rates,
    normalize_preferred_currency,
};
use serde::{Deserialize, Serialize};
use tauri::State;
use tokio::sync::Mutex;

const CACHE_MAX_AGE_SECS: i64 = 24 * 60 * 60;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PersistedRates {
    fetched_at_unix: i64,
    rates: HashMap<String, f64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CurrencyRatesSnapshot {
    pub rates: HashMap<String, f64>,
}

#[derive(Default)]
pub struct CurrencyRateCache {
    inner: Mutex<CacheState>,
}

#[derive(Default)]
struct CacheState {
    loaded: bool,
    persisted: Option<PersistedRates>,
}

#[tauri::command]
pub async fn get_currency_rates(
    app: tauri::AppHandle,
    cache: State<'_, CurrencyRateCache>,
    preferred_currency_code: String,
) -> Result<CurrencyRatesSnapshot, String> {
    let preferred = normalize_preferred_currency(&preferred_currency_code);
    let mut state = cache.inner.lock().await;
    if !state.loaded {
        state.persisted = read_persisted_rates();
        state.loaded = true;
    }

    if preferred != "AUTO" {
        let now = unix_now();
        let fresh = state.persisted.as_ref().is_some_and(|cached| {
            let age = now.saturating_sub(cached.fetched_at_unix);
            (0..CACHE_MAX_AGE_SECS).contains(&age)
        });
        if !fresh {
            match fetch_exchange_rates().await {
                Ok(rates) => {
                    let entry = PersistedRates {
                        fetched_at_unix: now,
                        rates,
                    };
                    persist_rates(&entry);
                    state.persisted = Some(entry);
                    crate::tray_bridge::refresh_tray_presentation(&app);
                }
                Err(error) => {
                    tracing::debug!(%error, "currency rates unavailable; using cached or offline rates")
                }
            }
        }
    }

    Ok(CurrencyRatesSnapshot {
        rates: merged_rates(state.persisted.as_ref()),
    })
}

pub(crate) fn convert_preferred_amount(amount: f64, source_code: &str) -> Option<(f64, String)> {
    let preferred =
        normalize_preferred_currency(&codexbar::settings::Settings::load().preferred_currency_code);
    if preferred == "AUTO" {
        return None;
    }
    let cached = read_persisted_rates();
    let rates = merged_rates(cached.as_ref());
    convert_amount(amount, source_code, &preferred, &rates).map(|converted| (converted, preferred))
}

fn unix_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs().min(i64::MAX as u64) as i64)
        .unwrap_or_default()
}

fn cache_path() -> Option<PathBuf> {
    codexbar::settings::Settings::settings_path()?
        .parent()
        .map(|parent| parent.join("currency-rates.json"))
}

fn clean_rates(rates: HashMap<String, f64>) -> HashMap<String, f64> {
    let mut clean = HashMap::new();
    for code in SUPPORTED_CURRENCY_CODES {
        if let Some(rate) = rates.get(*code).copied()
            && rate.is_finite()
            && rate > 0.0
        {
            clean.insert((*code).to_string(), rate);
        }
    }
    if clean
        .get("USD")
        .is_none_or(|rate| (*rate - 1.0).abs() > f64::EPSILON)
    {
        return HashMap::new();
    }
    clean
}

fn read_persisted_rates() -> Option<PersistedRates> {
    let path = cache_path()?;
    let bytes = std::fs::read(path).ok()?;
    let mut cached: PersistedRates = serde_json::from_slice(&bytes).ok()?;
    cached.rates = clean_rates(cached.rates);
    (!cached.rates.is_empty()).then_some(cached)
}

fn merged_rates(cached: Option<&PersistedRates>) -> HashMap<String, f64> {
    let mut rates = fallback_rates();
    if let Some(cached) = cached {
        for (code, rate) in clean_rates(cached.rates.clone()) {
            rates.insert(code, rate);
        }
    }
    rates
}

fn persist_rates(cached: &PersistedRates) {
    let Some(path) = cache_path() else { return };
    let Some(parent) = path.parent() else { return };
    let write_result = (|| -> Result<(), Box<dyn std::error::Error>> {
        std::fs::create_dir_all(parent)?;
        let bytes = serde_json::to_vec(cached)?;
        codexbar::atomic_file::write_atomic(&path, &bytes)?;
        Ok(())
    })();
    if let Err(error) = write_result {
        tracing::debug!(%error, "could not persist currency rate cache");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use codexbar::currency::FALLBACK_RATES;

    #[test]
    fn fallback_table_covers_every_preferred_currency() {
        let rates = fallback_rates();
        assert_eq!(rates.len(), FALLBACK_RATES.len());
        for code in SUPPORTED_CURRENCY_CODES {
            assert!(
                rates
                    .get(*code)
                    .is_some_and(|rate| rate.is_finite() && *rate > 0.0)
            );
        }
    }

    #[test]
    fn persisted_rates_are_sanitized_before_merging() {
        let rates = HashMap::from([
            ("USD".to_string(), 1.0),
            ("TRY".to_string(), 48.0),
            ("EUR".to_string(), f64::NAN),
            ("BTC".to_string(), 100.0),
        ]);
        let clean = clean_rates(rates);
        assert_eq!(clean.get("TRY"), Some(&48.0));
        assert!(!clean.contains_key("EUR"));
        assert!(!clean.contains_key("BTC"));
    }
}
