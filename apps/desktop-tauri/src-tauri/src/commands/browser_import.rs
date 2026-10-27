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
/// With a `provider_id`, a detected desktop app store that only that provider
/// may use (the Claude desktop app for Claude) is listed last.
///
/// On non-Windows platforms (e.g. Linux CI) this returns an empty list because
/// DPAPI is unavailable; the UI should hide/disable the import button in that case.
#[tauri::command]
pub fn list_detected_browsers(provider_id: Option<String>) -> Vec<DetectedBrowserBridge> {
    let provider = provider_id
        .as_deref()
        .and_then(|id| parse_provider_arg(id).ok());
    importable_cookie_stores(provider)
        .into_iter()
        .map(|b| DetectedBrowserBridge {
            browser_type: b.browser_type.key().to_string(),
            display_name: b.browser_type.display_name().to_string(),
            profile_count: b.profiles.len(),
        })
        .collect()
}

fn importable_cookie_stores(
    provider: Option<codexbar::core::ProviderId>,
) -> Vec<codexbar::browser::detection::DetectedBrowser> {
    use codexbar::browser::detection::BrowserDetector;

    let mut stores = BrowserDetector::detect_all();
    stores.extend(
        provider
            .and_then(|id| id.app_cookie_store())
            .and_then(BrowserDetector::detect),
    );
    stores
}

/// Where a Settings import reads a provider's browser cookies. A provider
/// with a request scope is read on its scan domain (which may be a parent
/// of the API host) and filtered to the cookies sent to the API host.
struct ImportTarget {
    /// The API host, named in user-facing messages.
    host: &'static str,
    scan_domain: &'static str,
    scope: Option<codexbar::browser::cookies::CookieRequestScope>,
}

impl ImportTarget {
    fn resolve(pid: codexbar::core::ProviderId, settings: &Settings) -> Option<Self> {
        if let Some(scope) = codexbar::core::instantiate_provider(pid).cookie_request_scope() {
            return Some(Self {
                host: scope.host,
                scan_domain: scope.scan_domain,
                scope: Some(scope),
            });
        }
        let host = codexbar::core::provider_cookie_domain(pid, settings)?;
        Some(Self {
            host,
            scan_domain: host,
            scope: None,
        })
    }

    fn retain(&self, cookies: &mut Vec<codexbar::browser::cookies::Cookie>) {
        if let Some(scope) = self.scope {
            scope.retain(cookies);
        }
    }
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
    use codexbar::browser::cookies::CookieExtractor;

    // Resolve the provider to get its cookie domain.
    let pid = parse_provider_arg(&provider_id)?;

    let target = ImportTarget::resolve(pid, &Settings::load())
        .ok_or_else(|| format!("Provider '{provider_id}' does not use cookie authentication"))?;
    let domain = target.host;

    // Find the requested browser.
    let browser = importable_cookie_stores(Some(pid))
        .into_iter()
        .find(|b| b.browser_type.key() == browser_type.as_str())
        .ok_or_else(|| format!("Browser '{browser_type}' not found or not installed"))?;

    // Extract the cookie header.
    let mut cookies = CookieExtractor::extract_for_domain(&browser, target.scan_domain)
        .map_err(|e| e.user_message_for_browser(browser.browser_type))?;
    target.retain(&mut cookies);

    if cookies.is_empty() {
        return Err(format!(
            "No cookies found for {domain} in {}. Make sure you are signed in to that site in the browser.",
            browser.browser_type.display_name()
        ));
    }

    let cookie_header = CookieExtractor::build_cookie_header(&cookies);
    validate_single_line_secret(&cookie_header, "Cookie header", MAX_COOKIE_HEADER_LEN)?;

    // Persist as manual cookie.
    let mut manual = ManualCookies::load();
    manual.set(pid.cli_name(), &cookie_header);
    manual.save().map_err(|e| e.to_string())?;

    Ok(get_manual_cookies())
}

#[cfg(test)]
mod tests {
    use super::*;
    use codexbar::browser::cookies::Cookie;
    use codexbar::core::ProviderId;

    fn cookie(name: &str, domain: &str) -> Cookie {
        Cookie {
            name: name.into(),
            value: "v".into(),
            domain: domain.into(),
            path: "/".into(),
            expires: None,
            is_secure: true,
            is_http_only: true,
        }
    }

    #[test]
    fn trae_cn_import_reads_the_parent_domain_and_keeps_api_cookies() {
        let target = ImportTarget::resolve(ProviderId::TraeCn, &Settings::default()).unwrap();
        assert_eq!(target.host, "api.trae.cn");
        assert_eq!(target.scan_domain, "trae.cn");

        let mut cookies = vec![
            cookie("session", ".trae.cn"),
            cookie("api_only", "api.trae.cn"),
            cookie("www_only", "www.trae.cn"),
        ];
        target.retain(&mut cookies);
        let names: Vec<&str> = cookies.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, vec!["session", "api_only"]);
    }

    #[test]
    fn unscoped_import_reads_and_keeps_the_provider_cookie_domain() {
        let settings = Settings::default();
        let pid = ProviderId::all()
            .iter()
            .copied()
            .find(|pid| {
                pid.cookie_domain().is_some()
                    && codexbar::core::instantiate_provider(*pid)
                        .cookie_request_scope()
                        .is_none()
            })
            .expect("an unscoped cookie provider");
        let target = ImportTarget::resolve(pid, &settings).unwrap();
        assert_eq!(
            Some(target.host),
            codexbar::core::provider_cookie_domain(pid, &settings)
        );
        assert_eq!(target.scan_domain, target.host);

        let mut cookies = vec![cookie("a", "unrelated.example")];
        target.retain(&mut cookies);
        assert_eq!(cookies.len(), 1, "no scope, no filtering");
    }
}
