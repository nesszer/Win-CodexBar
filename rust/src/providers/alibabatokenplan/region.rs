//! Alibaba Token Plan API region (upstream 0.46.0).

use serde::{Deserialize, Serialize};

/// Persisted region id: `"cn" | "intl" | "cn-personal" | "intl-personal"`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AlibabaTokenPlanRegion {
    #[default]
    #[serde(rename = "cn")]
    Cn,
    #[serde(rename = "intl")]
    Intl,
    #[serde(rename = "cn-personal")]
    CnPersonal,
    #[serde(rename = "intl-personal")]
    IntlPersonal,
}

impl AlibabaTokenPlanRegion {
    pub const ALL: [Self; 4] = [Self::Cn, Self::Intl, Self::CnPersonal, Self::IntlPersonal];

    pub fn display_name(self) -> &'static str {
        match self {
            Self::Cn => "China Team",
            Self::Intl => "International Team",
            Self::CnPersonal => "China Personal/Solo",
            Self::IntlPersonal => "International Personal/Solo",
        }
    }

    /// Serde/settings raw string exactly as persisted.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Cn => "cn",
            Self::Intl => "intl",
            Self::CnPersonal => "cn-personal",
            Self::IntlPersonal => "intl-personal",
        }
    }

    /// Parse settings / `FetchContext.api_region` value. Unknown → cn.
    pub fn from_settings_value(value: Option<&str>) -> Self {
        match value.unwrap_or("").trim().to_ascii_lowercase().as_str() {
            "intl" | "international" | "ap-southeast-1" => Self::Intl,
            "cn-personal" | "cn_personal" | "personal" | "solo" => Self::CnPersonal,
            "intl-personal"
            | "intl_personal"
            | "international-personal"
            | "international_personal" => Self::IntlPersonal,
            "cn" | "china" | "cn-beijing" | "beijing" | "" => Self::Cn,
            other => {
                tracing::warn!(
                    value = other,
                    "Unknown Alibaba Token Plan region; falling back to cn"
                );
                Self::Cn
            }
        }
    }

    pub fn gateway_base_url(self) -> &'static str {
        match self {
            Self::Intl | Self::IntlPersonal => "https://modelstudio.console.alibabacloud.com",
            Self::Cn | Self::CnPersonal => "https://bailian.console.aliyun.com",
        }
    }

    pub fn quota_base_url(self) -> &'static str {
        match self {
            Self::Cn | Self::Intl => self.gateway_base_url(),
            Self::CnPersonal => "https://bailian-cs.console.aliyun.com",
            Self::IntlPersonal => "https://bailian-singapore-cs.alibabacloud.com",
        }
    }

    pub fn current_region_id(self) -> &'static str {
        match self {
            Self::Intl | Self::IntlPersonal => "ap-southeast-1",
            Self::Cn | Self::CnPersonal => "cn-beijing",
        }
    }

    pub fn cli_console_site(self) -> &'static str {
        match self {
            Self::Intl | Self::IntlPersonal => "international",
            Self::Cn | Self::CnPersonal => "domestic",
        }
    }

    pub fn product_code(self) -> &'static str {
        match self {
            Self::Cn => "sfm_tokenplanteams_dp_cn",
            Self::Intl => "sfm_tokenplanteams_dp_intl",
            Self::CnPersonal => "sfm_tokenplansolo_public_cn",
            Self::IntlPersonal => "sfm_tokenplansolo_public_intl",
        }
    }

    pub fn uses_personal_api(self) -> bool {
        matches!(self, Self::CnPersonal | Self::IntlPersonal)
    }

    pub fn personal_api_action(self) -> &'static str {
        match self {
            Self::Intl | Self::IntlPersonal => "IntlBroadScopeAspnGateway",
            Self::Cn | Self::CnPersonal => "BroadScopeAspnGateway",
        }
    }

    /// Upstream spelling is intentional (`ALBABACLOUD`).
    pub fn personal_console_site(self) -> &'static str {
        match self {
            Self::Intl | Self::IntlPersonal => "MODELSTUDIO_ALBABACLOUD",
            Self::Cn | Self::CnPersonal => "BAILIAN_ALIYUN",
        }
    }

    pub fn dashboard_url(self) -> &'static str {
        match self {
            Self::Cn => {
                "https://bailian.console.aliyun.com/cn-beijing?tab=plan#/efm/subscription/token-plan"
            }
            Self::Intl => {
                "https://modelstudio.console.alibabacloud.com/ap-southeast-1/?tab=plan#/efm/subscription/token-plan"
            }
            Self::CnPersonal => {
                "https://bailian.console.aliyun.com/cn-beijing?tab=plan#/efm/subscription/token-plan/personal"
            }
            Self::IntlPersonal => {
                "https://modelstudio.console.alibabacloud.com/ap-southeast-1/?tab=plan#/efm/subscription/token-plan/personal"
            }
        }
    }

    pub fn cookie_domains(self) -> &'static [&'static str] {
        match self {
            Self::Cn | Self::CnPersonal => CN_COOKIE_DOMAINS,
            Self::Intl | Self::IntlPersonal => INTL_COOKIE_DOMAINS,
        }
    }
}

/// Existing cn superset (byte-stable for default region).
const CN_COOKIE_DOMAINS: &[&str] = &[
    "bailian-cs.console.aliyun.com",
    "bailian.console.aliyun.com",
    "aliyun.com",
];

/// Bailian + Model Studio hosts for intl regions.
const INTL_COOKIE_DOMAINS: &[&str] = &[
    "bailian-cs.console.aliyun.com",
    "bailian.console.aliyun.com",
    "aliyun.com",
    "modelstudio.console.alibabacloud.com",
    "bailian-singapore-cs.alibabacloud.com",
    "alibabacloud.com",
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn region_serde_roundtrip() {
        for region in [
            AlibabaTokenPlanRegion::Cn,
            AlibabaTokenPlanRegion::Intl,
            AlibabaTokenPlanRegion::CnPersonal,
            AlibabaTokenPlanRegion::IntlPersonal,
        ] {
            let raw = serde_json::to_string(&region).unwrap();
            assert_eq!(raw, format!("\"{}\"", region.as_str()));
            let back: AlibabaTokenPlanRegion = serde_json::from_str(&raw).unwrap();
            assert_eq!(back, region);
        }
    }

    #[test]
    fn region_gateway_and_commodity_mapping() {
        use AlibabaTokenPlanRegion::*;
        const CN_GATEWAY: &str = "https://bailian.console.aliyun.com";
        const INTL_GATEWAY: &str = "https://modelstudio.console.alibabacloud.com";
        // (region, gateway, quota base, product code, region id, personal (action, site))
        let rows = [
            (
                Cn,
                CN_GATEWAY,
                CN_GATEWAY,
                "sfm_tokenplanteams_dp_cn",
                "cn-beijing",
                None,
            ),
            (
                Intl,
                INTL_GATEWAY,
                INTL_GATEWAY,
                "sfm_tokenplanteams_dp_intl",
                "ap-southeast-1",
                None,
            ),
            (
                CnPersonal,
                CN_GATEWAY,
                "https://bailian-cs.console.aliyun.com",
                "sfm_tokenplansolo_public_cn",
                "cn-beijing",
                Some(("BroadScopeAspnGateway", "BAILIAN_ALIYUN")),
            ),
            (
                IntlPersonal,
                INTL_GATEWAY,
                "https://bailian-singapore-cs.alibabacloud.com",
                "sfm_tokenplansolo_public_intl",
                "ap-southeast-1",
                Some(("IntlBroadScopeAspnGateway", "MODELSTUDIO_ALBABACLOUD")),
            ),
        ];
        for (region, gateway, quota, product, region_id, personal) in rows {
            assert_eq!(region.gateway_base_url(), gateway, "{region:?}");
            assert_eq!(region.quota_base_url(), quota, "{region:?}");
            assert_eq!(region.product_code(), product, "{region:?}");
            assert_eq!(region.current_region_id(), region_id, "{region:?}");
            assert_eq!(region.uses_personal_api(), personal.is_some(), "{region:?}");
            if let Some((action, site)) = personal {
                assert_eq!(region.personal_api_action(), action, "{region:?}");
                assert_eq!(region.personal_console_site(), site, "{region:?}");
            }
        }
    }
}
