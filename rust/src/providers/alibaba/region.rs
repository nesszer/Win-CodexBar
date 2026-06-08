/// Per-platform request constants for the console data gateway. These differ
/// between the international Model Studio console and the China-mainland
/// Bailian console.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AlibabaRequestProfile {
    /// Base origin of the console data gateway (no trailing slash).
    pub gateway: &'static str,
    /// `action` gateway parameter.
    pub api_action: &'static str,
    /// `product` gateway parameter.
    pub api_product: &'static str,
    /// Fully-qualified gateway API method name.
    pub api_method: &'static str,
    /// Coding Plan commodity code queried.
    pub commodity_code: &'static str,
    /// `switchAgent` cornerstone parameter (fixed per console).
    pub switch_agent: u64,
    /// `switchUserType` cornerstone parameter (fixed per console).
    pub switch_user_type: u64,
    /// `consoleSite` cornerstone parameter.
    pub console_site: &'static str,
    /// `domain` cornerstone parameter (the console host).
    pub console_domain: &'static str,
}

/// Verified request profile for the international Model Studio console
/// (Singapore / US / Germany / Hong Kong share this; only the region code
/// in the referer/feURL differs).
pub(crate) const INTL_PROFILE: AlibabaRequestProfile = AlibabaRequestProfile {
    gateway: "https://modelstudio.console.alibabacloud.com",
    api_action: "IntlBroadScopeAspnGateway",
    api_product: "sfm_bailian",
    api_method: "zeldaEasy.broadscope-bailian.codingPlan.queryCodingPlanInstanceInfoV2",
    commodity_code: "sfm_codingplan_public_intl",
    switch_agent: 313762,
    switch_user_type: 3,
    // NOTE: this is Alibaba's own (misspelled) token, "ALBABACLOUD", not
    // "ALIBABACLOUD". It is copied verbatim from a verified working capture;
    // the gateway rejects the correctly-spelled value. Do not "fix" it.
    console_site: "MODELSTUDIO_ALBABACLOUD",
    console_domain: "modelstudio.console.alibabacloud.com",
};

/// Alibaba Coding Plan region: the single source of truth for settings value,
/// UI label, gateway, API region code, cookie domains, dashboard URL and gateway
/// request constants.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AlibabaRegion {
    Singapore,
    UsEast,
    Germany,
    HongKong,
    ChinaMainland,
}

impl AlibabaRegion {
    /// Regions exposed in the settings UI.
    pub const ALL: &'static [AlibabaRegion] = &[
        Self::Singapore,
        Self::UsEast,
        Self::Germany,
        Self::HongKong,
        Self::ChinaMainland,
    ];

    /// Parse the persisted `api_region` settings value.
    pub fn from_settings_value(value: Option<&str>) -> Self {
        match value.unwrap_or_default().trim().to_lowercase().as_str() {
            "us" | "us-east-1" | "useast" | "us-west-1" => Self::UsEast,
            "germany" | "eu" | "eu-central-1" | "frankfurt" => Self::Germany,
            "hongkong" | "hong-kong" | "hk" | "cn-hongkong" => Self::HongKong,
            "cn" | "china" | "china-mainland" | "china_mainland" | "mainland" => {
                Self::ChinaMainland
            }
            "" | "singapore" | "intl" | "ap-southeast-1" | "international" => Self::Singapore,
            other => {
                tracing::warn!(
                    value = other,
                    "Unknown Alibaba api_region value; falling back to Singapore",
                );
                Self::Singapore
            }
        }
    }

    /// Canonical persisted settings value.
    pub fn settings_value(self) -> &'static str {
        match self {
            Self::Singapore => "singapore",
            Self::UsEast => "us",
            Self::Germany => "germany",
            Self::HongKong => "hongkong",
            Self::ChinaMainland => "cn",
        }
    }

    /// Human-readable label for the settings region picker.
    pub fn display_name(self) -> &'static str {
        match self {
            Self::Singapore => "International – Singapore (ap-southeast-1)",
            Self::UsEast => "International – US (us-east-1)",
            Self::Germany => "International – Germany (eu-central-1)",
            Self::HongKong => "International – Hong Kong (cn-hongkong)",
            Self::ChinaMainland => "China Mainland (Bailian)",
        }
    }

    /// Alibaba Cloud region code used in the dashboard path and referer.
    pub fn region_code(self) -> &'static str {
        match self {
            Self::Singapore => "ap-southeast-1",
            Self::UsEast => "us-east-1",
            Self::Germany => "eu-central-1",
            Self::HongKong => "cn-hongkong",
            Self::ChinaMainland => "cn-hangzhou",
        }
    }

    pub fn is_china(self) -> bool {
        matches!(self, Self::ChinaMainland)
    }

    /// Browser cookie domains to try, in priority order, for auto-import.
    pub fn cookie_domains(self) -> &'static [&'static str] {
        match self {
            Self::ChinaMainland => &[
                "bailian.console.alibabacloud.com",
                "bailian.console.aliyun.com",
                "alibabacloud.com",
                "aliyun.com",
            ],
            _ => &["modelstudio.console.alibabacloud.com", "alibabacloud.com"],
        }
    }

    /// Primary cookie domain shown in the browser-import hint.
    pub fn primary_cookie_domain(self) -> &'static str {
        self.cookie_domains()[0]
    }

    /// Per-platform gateway request constants.
    pub fn request_profile(self) -> AlibabaRequestProfile {
        match self {
            Self::ChinaMainland => AlibabaRequestProfile {
                gateway: "https://bailian.console.alibabacloud.com",
                console_domain: "bailian.console.alibabacloud.com",
                ..INTL_PROFILE
            },
            _ => INTL_PROFILE,
        }
    }

    /// Console data-gateway origin.
    pub fn gateway(self) -> &'static str {
        self.request_profile().gateway
    }

    /// Dashboard URL for the browser-import hint and the "open dashboard" link.
    pub fn dashboard_url(self) -> String {
        match self {
            Self::ChinaMainland => self.gateway().to_string(),
            _ => format!("{}/{}", self.gateway(), self.region_code()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn region_from_settings_value_round_trips() {
        for region in AlibabaRegion::ALL {
            assert_eq!(
                AlibabaRegion::from_settings_value(Some(region.settings_value())),
                *region
            );
        }
        assert_eq!(
            AlibabaRegion::from_settings_value(None),
            AlibabaRegion::Singapore
        );
        assert_eq!(
            AlibabaRegion::from_settings_value(Some("intl")),
            AlibabaRegion::Singapore
        );
        assert_eq!(
            AlibabaRegion::from_settings_value(Some("cn")),
            AlibabaRegion::ChinaMainland
        );
    }

    #[test]
    fn region_codes_are_correct() {
        assert_eq!(AlibabaRegion::Singapore.region_code(), "ap-southeast-1");
        assert_eq!(AlibabaRegion::UsEast.region_code(), "us-east-1");
        assert_eq!(AlibabaRegion::Germany.region_code(), "eu-central-1");
        assert_eq!(AlibabaRegion::HongKong.region_code(), "cn-hongkong");
    }

    #[test]
    fn china_routes_to_its_own_gateway_and_cookies() {
        let cn = AlibabaRegion::ChinaMainland;
        assert!(cn.is_china());
        assert_eq!(cn.gateway(), "https://bailian.console.alibabacloud.com");
        assert_ne!(cn.gateway(), AlibabaRegion::Singapore.gateway());
        assert_eq!(
            cn.primary_cookie_domain(),
            "bailian.console.alibabacloud.com"
        );
        assert_ne!(
            cn.primary_cookie_domain(),
            AlibabaRegion::Singapore.primary_cookie_domain()
        );
    }

    #[test]
    fn international_regions_share_one_gateway() {
        let intl = "https://modelstudio.console.alibabacloud.com";
        for region in [
            AlibabaRegion::Singapore,
            AlibabaRegion::UsEast,
            AlibabaRegion::Germany,
            AlibabaRegion::HongKong,
        ] {
            assert_eq!(region.gateway(), intl);
            assert!(!region.is_china());
        }
    }

    #[test]
    fn exposed_regions_include_china() {
        assert!(AlibabaRegion::ALL.contains(&AlibabaRegion::ChinaMainland));
        assert_eq!(AlibabaRegion::ALL.len(), 5);
    }

    #[test]
    fn china_cookie_domains_include_legacy_aliyun() {
        let domains = AlibabaRegion::ChinaMainland.cookie_domains();
        assert!(
            domains.contains(&"bailian.console.aliyun.com"),
            "legacy aliyun.com fallback must remain for upgrading users; got {domains:?}",
        );
        assert!(domains.contains(&"bailian.console.alibabacloud.com"));
    }

    #[test]
    fn intl_console_site_matches_verified_capture() {
        assert_eq!(INTL_PROFILE.console_site, "MODELSTUDIO_ALBABACLOUD");
    }

    #[test]
    fn ap_east_1_no_longer_aliases_to_hongkong() {
        assert_eq!(
            AlibabaRegion::from_settings_value(Some("ap-east-1")),
            AlibabaRegion::Singapore,
            "ap-east-1 is an AWS region code, not Alibaba's; removed as HK alias",
        );
    }

    #[test]
    fn dashboard_url_with_tab_is_consistent_across_regions() {
        let intl_url = format!("{}?tab=plan", AlibabaRegion::Singapore.dashboard_url());
        assert_eq!(
            intl_url,
            "https://modelstudio.console.alibabacloud.com/ap-southeast-1?tab=plan",
        );
        let cn_url = format!("{}?tab=plan", AlibabaRegion::ChinaMainland.dashboard_url());
        assert_eq!(
            cn_url, "https://bailian.console.alibabacloud.com?tab=plan",
            "CN dashboard URL must NOT contain a region-code path segment",
        );
    }
}
