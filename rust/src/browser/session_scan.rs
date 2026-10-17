//! Lazy, one-browser-at-a-time cookie scan.
//!
//! Reading a browser's cookie store copies its SQLite database and, for
//! Chromium browsers, decrypts every matching value with DPAPI/AES-GCM. The
//! scan therefore reads the next browser only when the caller asks for another
//! candidate, so a caller that accepts the first browser's session never
//! touches the others. Callers that validate a session (an HTTP request) and
//! see it rejected simply ask for the next candidate instead of being stuck
//! with the first browser that holds any cookie for the domain.
//!
//! The scan is provider-agnostic: it knows browsers, domains and cookies, not
//! what a valid session looks like.

use super::cookies::{Cookie, CookieError, CookieExtractor};
use super::detection::{BrowserDetector, BrowserType, DetectedBrowser};

/// Where browser installations and their cookies come from. The system store
/// reads real browser profiles; tests substitute an in-memory store.
pub trait BrowserCookieStore {
    /// Installed browsers in scan order. Must be cheap: it only discovers
    /// profile directories and never reads or decrypts cookies.
    fn detect_browsers(&self) -> Vec<DetectedBrowser>;

    /// Read (and decrypt) the cookies `browser` holds for `domain`.
    fn read_cookies(
        &self,
        browser: &DetectedBrowser,
        domain: &str,
    ) -> Result<Vec<Cookie>, CookieError>;
}

/// The real browser profiles on this machine (and the Windows host under WSL).
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemCookieStore;

impl BrowserCookieStore for SystemCookieStore {
    fn detect_browsers(&self) -> Vec<DetectedBrowser> {
        BrowserDetector::detect_all()
    }

    fn read_cookies(
        &self,
        browser: &DetectedBrowser,
        domain: &str,
    ) -> Result<Vec<Cookie>, CookieError> {
        CookieExtractor::extract_for_domain(browser, domain)
    }
}

/// One browser's cookies for one domain.
#[derive(Debug, Clone)]
pub struct BrowserCookieCandidate {
    pub browser: BrowserType,
    pub domain: String,
    pub cookies: Vec<Cookie>,
}

impl BrowserCookieCandidate {
    /// The `Cookie` request-header value for these cookies.
    pub fn header(&self) -> String {
        CookieExtractor::build_cookie_header(&self.cookies)
    }
}

/// How a multi-domain scan walks the browser × domain grid.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ScanOrder {
    /// Every browser for the first domain, then every browser for the next.
    /// The first candidate is exactly the one the older first-match helpers
    /// returned.
    #[default]
    DomainMajor,
    /// Every domain for the first browser, then every domain for the next, so
    /// an earlier browser's session is always tried before a later browser's.
    BrowserMajor,
}

/// Iterator over browser cookie candidates for a list of domains.
///
/// Order is [`ScanOrder::DomainMajor`] unless [`BrowserCookieScan::browser_major`]
/// is used. Browsers follow `BrowserType::all()` order, or the explicit order
/// passed to [`BrowserCookieScan::with_browser_order`]. Browsers without
/// cookies for a domain, and browsers that fail to read, are skipped. A repeat
/// of the same header from the same browser under another domain is skipped
/// too, since it would only repeat a session the caller already tried.
pub struct BrowserCookieScan<S: BrowserCookieStore = SystemCookieStore> {
    store: S,
    domains: Vec<String>,
    browser_order: Option<Vec<BrowserType>>,
    browsers: Option<Vec<DetectedBrowser>>,
    order: ScanOrder,
    outer_index: usize,
    inner_index: usize,
    yielded: Vec<(BrowserType, String)>,
    app_bound_encryption_seen: bool,
}

impl BrowserCookieScan<SystemCookieStore> {
    /// Scan the real browser profiles for `domains`.
    pub fn new(domains: &[&str]) -> Self {
        Self::with_store(SystemCookieStore, domains)
    }
}

impl<S: BrowserCookieStore> BrowserCookieScan<S> {
    /// Scan `store` for `domains`. Nothing is read until the first `next()`.
    pub fn with_store(store: S, domains: &[&str]) -> Self {
        Self {
            store,
            domains: domains.iter().map(|domain| (*domain).to_string()).collect(),
            browser_order: None,
            browsers: None,
            order: ScanOrder::DomainMajor,
            outer_index: 0,
            inner_index: 0,
            yielded: Vec::new(),
            app_bound_encryption_seen: false,
        }
    }

    /// Try every domain in one browser before moving to the next browser, for
    /// callers that validate each session and must prefer earlier browsers.
    pub fn browser_major(mut self) -> Self {
        self.order = ScanOrder::BrowserMajor;
        self
    }

    /// Restrict the scan to `order` and try those browsers in that order, for
    /// callers that honor an explicit browser selection. Browsers absent from
    /// `order` are never read.
    pub fn with_browser_order(mut self, order: &[BrowserType]) -> Self {
        self.browser_order = Some(order.to_vec());
        self
    }

    /// The error that describes why the scan produced nothing (more): App-Bound
    /// Encryption when it blocked any browser, `BrowserNotInstalled` when no
    /// browser was found, otherwise `NotFound` for the scanned domains.
    pub fn exhaustion_error(&self) -> CookieError {
        if self.app_bound_encryption_seen {
            CookieError::AppBoundEncryption
        } else if self.browsers.as_ref().is_some_and(Vec::is_empty) {
            CookieError::BrowserNotInstalled
        } else {
            CookieError::NotFound(self.domains.join(", "))
        }
    }

    fn browsers(&mut self) -> &[DetectedBrowser] {
        if self.browsers.is_none() {
            let mut detected = self.store.detect_browsers();
            if let Some(order) = &self.browser_order {
                detected.retain(|browser| order.contains(&browser.browser_type));
                detected.sort_by_key(|browser| {
                    order
                        .iter()
                        .position(|selected| *selected == browser.browser_type)
                });
            }
            self.browsers = Some(detected);
        }
        self.browsers.as_deref().unwrap_or_default()
    }
}

impl<S: BrowserCookieStore> Iterator for BrowserCookieScan<S> {
    type Item = BrowserCookieCandidate;

    fn next(&mut self) -> Option<Self::Item> {
        let browser_count = self.browsers().len();
        let domain_count = self.domains.len();
        let (outer_count, inner_count) = match self.order {
            ScanOrder::DomainMajor => (domain_count, browser_count),
            ScanOrder::BrowserMajor => (browser_count, domain_count),
        };
        while self.outer_index < outer_count {
            if self.inner_index >= inner_count {
                self.outer_index += 1;
                self.inner_index = 0;
                continue;
            }
            let (domain_index, browser_index) = match self.order {
                ScanOrder::DomainMajor => (self.outer_index, self.inner_index),
                ScanOrder::BrowserMajor => (self.inner_index, self.outer_index),
            };
            self.inner_index += 1;
            let browser = self.browsers()[browser_index].clone();
            let domain = self.domains[domain_index].clone();
            let browser_name = browser.browser_type.display_name();

            match self.store.read_cookies(&browser, &domain) {
                Ok(cookies) if !cookies.is_empty() => {
                    let candidate = BrowserCookieCandidate {
                        browser: browser.browser_type,
                        domain,
                        cookies,
                    };
                    let header = candidate.header();
                    if header.trim().is_empty()
                        || self.yielded.iter().any(|(seen, seen_header)| {
                            *seen == candidate.browser && *seen_header == header
                        })
                    {
                        continue;
                    }
                    tracing::debug!(
                        browser = browser_name,
                        domain = %candidate.domain,
                        count = candidate.cookies.len(),
                        "Found browser cookie candidate"
                    );
                    self.yielded.push((candidate.browser, header));
                    return Some(candidate);
                }
                Ok(_) => {}
                Err(CookieError::AppBoundEncryption) => {
                    tracing::warn!(
                        browser = browser_name,
                        "App-Bound Encryption prevents automatic cookie import; \
                         trying remaining browsers"
                    );
                    self.app_bound_encryption_seen = true;
                }
                Err(error) => {
                    tracing::debug!(
                        browser = browser_name,
                        "Failed to get cookies from {browser_name}: {error}"
                    );
                }
            }
        }
        None
    }
}

#[cfg(test)]
pub(crate) mod test_support {
    use std::cell::RefCell;
    use std::collections::HashMap;
    use std::path::PathBuf;

    use super::*;
    use crate::browser::detection::BrowserProfile;

    /// What a fake browser returns for a domain.
    #[derive(Clone)]
    pub(crate) enum FakeRead {
        Cookies(Vec<(&'static str, &'static str)>),
        AppBoundEncryption,
        Failure,
    }

    /// In-memory browsers that record every cookie read.
    #[derive(Default)]
    pub(crate) struct FakeCookieStore {
        browsers: Vec<BrowserType>,
        reads: HashMap<(BrowserType, String), FakeRead>,
        pub(crate) read_log: RefCell<Vec<(BrowserType, String)>>,
    }

    impl FakeCookieStore {
        pub(crate) fn browser(mut self, browser: BrowserType) -> Self {
            self.browsers.push(browser);
            self
        }

        pub(crate) fn with(mut self, browser: BrowserType, domain: &str, read: FakeRead) -> Self {
            if !self.browsers.contains(&browser) {
                self.browsers.push(browser);
            }
            self.reads.insert((browser, domain.to_string()), read);
            self
        }

        pub(crate) fn cookies(
            self,
            browser: BrowserType,
            domain: &str,
            cookies: &[(&'static str, &'static str)],
        ) -> Self {
            self.with(browser, domain, FakeRead::Cookies(cookies.to_vec()))
        }

        pub(crate) fn reads(&self) -> Vec<(BrowserType, String)> {
            self.read_log.borrow().clone()
        }
    }

    impl BrowserCookieStore for &FakeCookieStore {
        fn detect_browsers(&self) -> Vec<DetectedBrowser> {
            self.browsers
                .iter()
                .map(|browser_type| DetectedBrowser {
                    browser_type: *browser_type,
                    user_data_dir: PathBuf::from("fake"),
                    profiles: vec![BrowserProfile {
                        name: "Default".into(),
                        path: PathBuf::from("fake/Default"),
                        is_default: true,
                    }],
                })
                .collect()
        }

        fn read_cookies(
            &self,
            browser: &DetectedBrowser,
            domain: &str,
        ) -> Result<Vec<Cookie>, CookieError> {
            self.read_log
                .borrow_mut()
                .push((browser.browser_type, domain.to_string()));
            match self.reads.get(&(browser.browser_type, domain.to_string())) {
                None => Ok(Vec::new()),
                Some(FakeRead::Cookies(cookies)) => Ok(cookies
                    .iter()
                    .map(|(name, value)| Cookie {
                        name: (*name).to_string(),
                        value: (*value).to_string(),
                        domain: domain.to_string(),
                        path: "/".into(),
                        expires: None,
                        is_secure: true,
                        is_http_only: true,
                    })
                    .collect()),
                Some(FakeRead::AppBoundEncryption) => Err(CookieError::AppBoundEncryption),
                Some(FakeRead::Failure) => Err(CookieError::Decryption("fake".into())),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::test_support::{FakeCookieStore, FakeRead};
    use super::*;

    fn headers<S: BrowserCookieStore>(scan: BrowserCookieScan<S>) -> Vec<(BrowserType, String)> {
        scan.map(|candidate| (candidate.browser, candidate.header()))
            .collect()
    }

    #[test]
    fn reads_nothing_until_first_candidate_requested() {
        let store =
            FakeCookieStore::default().cookies(BrowserType::Chrome, "a.test", &[("s", "1")]);
        let _scan = BrowserCookieScan::with_store(&store, &["a.test"]);
        assert!(store.reads().is_empty());
    }

    #[test]
    fn accepting_first_browser_never_reads_later_browsers() {
        let store = FakeCookieStore::default()
            .cookies(BrowserType::Chrome, "a.test", &[("s", "chrome")])
            .cookies(BrowserType::Edge, "a.test", &[("s", "edge")])
            .cookies(BrowserType::Firefox, "a.test", &[("s", "firefox")]);
        let mut scan = BrowserCookieScan::with_store(&store, &["a.test"]);

        let first = scan.next().expect("first candidate");
        assert_eq!(first.browser, BrowserType::Chrome);
        assert_eq!(store.reads(), vec![(BrowserType::Chrome, "a.test".into())]);
    }

    #[test]
    fn stale_first_browser_moves_to_next_browser_with_cookies() {
        let store = FakeCookieStore::default()
            .browser(BrowserType::Chrome)
            .cookies(BrowserType::Edge, "a.test", &[("consent", "stale")])
            .cookies(BrowserType::Brave, "a.test", &[("session", "valid")]);
        let mut scan = BrowserCookieScan::with_store(&store, &["a.test"]);

        let first = scan.next().expect("stale candidate");
        assert_eq!(first.browser, BrowserType::Edge);
        // The caller rejected Edge's session; only now is Brave read.
        assert_eq!(store.reads().len(), 2);
        let second = scan.next().expect("valid candidate");
        assert_eq!(second.browser, BrowserType::Brave);
        assert_eq!(second.header(), "session=valid");
        assert!(scan.next().is_none());
    }

    #[test]
    fn explicit_browser_order_is_respected_and_excludes_unselected_browsers() {
        let store = FakeCookieStore::default()
            .cookies(BrowserType::Chrome, "a.test", &[("s", "chrome")])
            .cookies(BrowserType::Edge, "a.test", &[("s", "edge")])
            .cookies(BrowserType::Firefox, "a.test", &[("s", "firefox")]);
        let scan = BrowserCookieScan::with_store(&store, &["a.test"])
            .with_browser_order(&[BrowserType::Firefox, BrowserType::Edge]);

        assert_eq!(
            headers(scan),
            vec![
                (BrowserType::Firefox, "s=firefox".into()),
                (BrowserType::Edge, "s=edge".into()),
            ]
        );
        assert!(
            store
                .reads()
                .iter()
                .all(|(browser, _)| *browser != BrowserType::Chrome)
        );
    }

    #[test]
    fn domains_are_scanned_domain_major_and_same_browser_repeats_are_skipped() {
        let store = FakeCookieStore::default()
            .cookies(BrowserType::Chrome, "app.a.test", &[("s", "1")])
            .cookies(BrowserType::Chrome, "a.test", &[("s", "1")])
            .cookies(BrowserType::Firefox, "a.test", &[("s", "2")]);
        let scan = BrowserCookieScan::with_store(&store, &["app.a.test", "a.test"]);

        assert_eq!(
            headers(scan),
            vec![
                (BrowserType::Chrome, "s=1".into()),
                (BrowserType::Firefox, "s=2".into()),
            ]
        );
    }

    #[test]
    fn browser_major_tries_every_domain_of_an_earlier_browser_first() {
        let store = FakeCookieStore::default()
            .cookies(BrowserType::Chrome, "b.test", &[("s", "chrome")])
            .cookies(BrowserType::Firefox, "a.test", &[("s", "firefox")]);

        let domain_major = BrowserCookieScan::with_store(&store, &["a.test", "b.test"]);
        assert_eq!(
            headers(domain_major),
            vec![
                (BrowserType::Firefox, "s=firefox".into()),
                (BrowserType::Chrome, "s=chrome".into()),
            ]
        );

        let reads_before = store.reads().len();
        let mut browser_major =
            BrowserCookieScan::with_store(&store, &["a.test", "b.test"]).browser_major();
        let first = browser_major.next().expect("chrome candidate");
        assert_eq!(first.browser, BrowserType::Chrome);
        assert_eq!(first.domain, "b.test");
        // Firefox is not read until Chrome's session is rejected.
        assert_eq!(
            store.reads()[reads_before..],
            [
                (BrowserType::Chrome, "a.test".to_string()),
                (BrowserType::Chrome, "b.test".to_string()),
            ]
        );
        let second = browser_major.next().expect("firefox candidate");
        assert_eq!(second.browser, BrowserType::Firefox);
        assert!(browser_major.next().is_none());
    }

    #[test]
    fn read_failures_are_skipped_and_app_bound_encryption_is_reported() {
        let store = FakeCookieStore::default()
            .with(BrowserType::Chrome, "a.test", FakeRead::AppBoundEncryption)
            .with(BrowserType::Edge, "a.test", FakeRead::Failure);
        let mut scan = BrowserCookieScan::with_store(&store, &["a.test"]);

        assert!(scan.next().is_none());
        assert!(matches!(
            scan.exhaustion_error(),
            CookieError::AppBoundEncryption
        ));
    }

    #[test]
    fn exhaustion_reports_missing_browsers_and_missing_cookies() {
        let none = FakeCookieStore::default();
        let mut scan = BrowserCookieScan::with_store(&none, &["a.test"]);
        assert!(scan.next().is_none());
        assert!(matches!(
            scan.exhaustion_error(),
            CookieError::BrowserNotInstalled
        ));

        let empty = FakeCookieStore::default().browser(BrowserType::Chrome);
        let mut scan = BrowserCookieScan::with_store(&empty, &["a.test", "b.test"]);
        assert!(scan.next().is_none());
        assert!(
            matches!(scan.exhaustion_error(), CookieError::NotFound(domains) if domains == "a.test, b.test")
        );
    }
}
