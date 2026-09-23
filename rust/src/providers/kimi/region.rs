#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KimiRegion {
    China,
    International,
}

impl KimiRegion {
    pub const ALL: [Self; 2] = [Self::China, Self::International];

    pub fn from_settings(value: Option<&str>) -> Self {
        match value.map(str::trim).map(str::to_ascii_lowercase).as_deref() {
            Some("international" | "intl" | "global") => Self::International,
            _ => Self::China,
        }
    }

    pub const fn settings_value(self) -> &'static str {
        match self {
            Self::China => "china",
            Self::International => "international",
        }
    }

    pub const fn display_name(self) -> &'static str {
        match self {
            Self::China => "China (kimi.com)",
            Self::International => "International (kimi.ai)",
        }
    }

    pub const fn code_api_base_url(self) -> &'static str {
        match self {
            Self::China => "https://api.kimi.com",
            Self::International => "https://api.kimi.ai",
        }
    }

    pub const fn web_base_url(self) -> &'static str {
        match self {
            Self::China => "https://www.kimi.com",
            Self::International => "https://www.kimi.ai",
        }
    }

    pub const fn console_url(self) -> &'static str {
        match self {
            Self::China => "https://www.kimi.com/code/console",
            Self::International => "https://www.kimi.ai/code/console",
        }
    }

    pub const fn cookie_domains(self) -> &'static [&'static str] {
        match self {
            Self::China => &["www.kimi.com", "kimi.com"],
            Self::International => &["www.kimi.ai", "kimi.ai"],
        }
    }

    pub const fn desktop_cookie_hosts(self) -> &'static [&'static str; 4] {
        match self {
            Self::China => &["www.kimi.com", ".www.kimi.com", ".kimi.com", "kimi.com"],
            Self::International => &["www.kimi.ai", ".www.kimi.ai", ".kimi.ai", "kimi.ai"],
        }
    }

    pub fn web_api_url(self, service: &str) -> String {
        format!("{}/apiv2/{service}", self.web_base_url())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_and_missing_settings_preserve_china_default() {
        assert_eq!(KimiRegion::from_settings(None), KimiRegion::China);
        assert_eq!(
            KimiRegion::from_settings(Some("unknown")),
            KimiRegion::China
        );
        assert_eq!(
            KimiRegion::from_settings(Some("international")),
            KimiRegion::International
        );
    }

    #[test]
    fn regional_hosts_remain_coherent() {
        for region in KimiRegion::ALL {
            let suffix = match region {
                KimiRegion::China => "kimi.com",
                KimiRegion::International => "kimi.ai",
            };
            assert!(region.code_api_base_url().ends_with(suffix));
            assert!(region.web_base_url().ends_with(suffix));
            assert!(region.console_url().ends_with("/code/console"));
            assert!(
                region
                    .cookie_domains()
                    .iter()
                    .all(|host| host.ends_with(suffix))
            );
        }
    }
}
