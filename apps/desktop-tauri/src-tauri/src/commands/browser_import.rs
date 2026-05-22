use super::*;

// ── Browser cookie import commands ────────────────────────────────────

/// Bridge-friendly detected browser entry.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DetectedBrowserBridge {
    /// Stable key used when calling `import_browser_cookies`.
    pub browser_type: String,
    pub display_name: String,
    pub profile_count: usize,
}

/// List all browsers detected on this machine that CodexBar can read cookies from.
///
/// On non-Windows platforms (e.g. Linux CI) this returns an empty list because
/// DPAPI is unavailable; the UI should hide/disable the import button in that case.
#[tauri::command]
pub fn list_detected_browsers() -> Vec<DetectedBrowserBridge> {
    use codexbar::browser::detection::BrowserDetector;

    BrowserDetector::detect_all()
        .into_iter()
        .map(|b| DetectedBrowserBridge {
            browser_type: browser_type_key(b.browser_type).to_string(),
            display_name: b.browser_type.display_name().to_string(),
            profile_count: b.profiles.len(),
        })
        .collect()
}

/// Import cookies for `provider_id` from the named browser and persist them as
/// a manual-cookie override, replacing any existing entry for that provider.
///
/// `browser_type` must be one of the keys returned by `list_detected_browsers`
/// (e.g. `"chrome"`, `"edge"`, `"brave"`).
///
/// Returns the updated manual-cookies list on success.
#[tauri::command]
pub fn import_browser_cookies(
    provider_id: String,
    browser_type: String,
) -> Result<Vec<CookieInfoBridge>, String> {
    use codexbar::browser::cookies::{CookieError, CookieExtractor};
    use codexbar::browser::detection::BrowserDetector;

    // Resolve the provider to get its cookie domain.
    let pid = parse_provider_arg(&provider_id)?;

    let domain = pid
        .cookie_domain()
        .ok_or_else(|| format!("Provider '{provider_id}' does not use cookie authentication"))?;

    // Find the requested browser.
    let browsers = BrowserDetector::detect_all();
    let browser = browsers
        .into_iter()
        .find(|b| browser_type_key(b.browser_type) == browser_type.as_str())
        .ok_or_else(|| format!("Browser '{browser_type}' not found or not installed"))?;

    // Extract the cookie header.
    let cookies = CookieExtractor::extract_for_domain(&browser, domain).map_err(|e| match e {
        CookieError::Dpapi(msg) => format!("DPAPI error: {msg}"),
        other => other.to_string(),
    })?;

    if cookies.is_empty() {
        return Err(format!(
            "No cookies found for {domain} in {}. Make sure you are signed in to that site in the browser.",
            browser.browser_type.display_name()
        ));
    }

    let cookie_header = CookieExtractor::build_cookie_header(&cookies);

    // Persist as manual cookie.
    let mut manual = ManualCookies::load();
    manual.set(pid.cli_name(), &cookie_header);
    manual.save().map_err(|e| e.to_string())?;

    Ok(get_manual_cookies())
}

/// Map `BrowserType` to a stable lowercase string key used in the IPC bridge.
fn browser_type_key(bt: codexbar::browser::detection::BrowserType) -> &'static str {
    use codexbar::browser::detection::BrowserType;
    match bt {
        BrowserType::Chrome => "chrome",
        BrowserType::Edge => "edge",
        BrowserType::Brave => "brave",
        BrowserType::Arc => "arc",
        BrowserType::Firefox => "firefox",
        BrowserType::Chromium => "chromium",
    }
}
