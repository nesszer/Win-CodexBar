//! Infini AI Coding Plan Provider

use serde::{Deserialize, Serialize};

/// Infini AI Coding Plan 用量数据
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InfiniUsage {
    #[serde(rename = "5_hour")]
    pub five_hour: UsagePeriod,
    #[serde(rename = "7_day")]
    pub seven_day: UsagePeriod,
    #[serde(rename = "30_day")]
    pub thirty_day: UsagePeriod,
}

/// 单个时间周期的用量数据
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsagePeriod {
    pub quota: u64,
    pub used: u64,
    pub remain: u64,
}

/// Infini 套餐类型
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlanType {
    Lite,
    Pro,
    Unknown,
}

impl InfiniUsage {
    /// 根据配额判断套餐类型
    pub fn plan_type(&self) -> PlanType {
        match self.five_hour.quota {
            1000 => PlanType::Lite,
            5000 => PlanType::Pro,
            _ => PlanType::Unknown,
        }
    }

    /// 5小时周期使用百分比
    pub fn five_hour_percentage(&self) -> f64 {
        if self.five_hour.quota == 0 {
            return 0.0;
        }
        (self.five_hour.used as f64 / self.five_hour.quota as f64) * 100.0
    }

    /// 7天周期使用百分比
    pub fn seven_day_percentage(&self) -> f64 {
        if self.seven_day.quota == 0 {
            return 0.0;
        }
        (self.seven_day.used as f64 / self.seven_day.quota as f64) * 100.0
    }

    /// 30天周期使用百分比
    pub fn thirty_day_percentage(&self) -> f64 {
        if self.thirty_day.quota == 0 {
            return 0.0;
        }
        (self.thirty_day.used as f64 / self.thirty_day.quota as f64) * 100.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_usage_period_deserialization() {
        let json = r#"{"quota": 5000, "used": 1000, "remain": 4000}"#;
        let period: UsagePeriod = serde_json::from_str(json).unwrap();
        assert_eq!(period.quota, 5000);
        assert_eq!(period.used, 1000);
        assert_eq!(period.remain, 4000);
    }

    #[test]
    fn test_infini_usage_deserialization() {
        let json = r#"{
            "5_hour": {"quota": 5000, "used": 1000, "remain": 4000},
            "7_day": {"quota": 30000, "used": 5000, "remain": 25000},
            "30_day": {"quota": 60000, "used": 10000, "remain": 50000}
        }"#;
        let usage: InfiniUsage = serde_json::from_str(json).unwrap();
        assert_eq!(usage.five_hour.quota, 5000);
        assert_eq!(usage.seven_day.used, 5000);
        assert_eq!(usage.thirty_day.remain, 50000);
    }

    #[test]
    fn test_plan_type_pro() {
        let usage = InfiniUsage {
            five_hour: UsagePeriod {
                quota: 5000,
                used: 1000,
                remain: 4000,
            },
            seven_day: UsagePeriod {
                quota: 30000,
                used: 5000,
                remain: 25000,
            },
            thirty_day: UsagePeriod {
                quota: 60000,
                used: 10000,
                remain: 50000,
            },
        };
        assert_eq!(usage.plan_type(), PlanType::Pro);
    }

    #[test]
    fn test_plan_type_lite() {
        let usage = InfiniUsage {
            five_hour: UsagePeriod {
                quota: 1000,
                used: 500,
                remain: 500,
            },
            seven_day: UsagePeriod {
                quota: 7000,
                used: 3500,
                remain: 3500,
            },
            thirty_day: UsagePeriod {
                quota: 30000,
                used: 15000,
                remain: 15000,
            },
        };
        assert_eq!(usage.plan_type(), PlanType::Lite);
    }

    #[test]
    fn test_percentage_calculation() {
        let usage = InfiniUsage {
            five_hour: UsagePeriod {
                quota: 5000,
                used: 2500,
                remain: 2500,
            },
            seven_day: UsagePeriod {
                quota: 30000,
                used: 15000,
                remain: 15000,
            },
            thirty_day: UsagePeriod {
                quota: 60000,
                used: 30000,
                remain: 30000,
            },
        };
        assert_eq!(usage.five_hour_percentage(), 50.0);
        assert_eq!(usage.seven_day_percentage(), 50.0);
        assert_eq!(usage.thirty_day_percentage(), 50.0);
    }

    #[test]
    fn test_zero_quota_percentage() {
        let usage = InfiniUsage {
            five_hour: UsagePeriod {
                quota: 0,
                used: 0,
                remain: 0,
            },
            seven_day: UsagePeriod {
                quota: 0,
                used: 0,
                remain: 0,
            },
            thirty_day: UsagePeriod {
                quota: 0,
                used: 0,
                remain: 0,
            },
        };
        assert_eq!(usage.five_hour_percentage(), 0.0);
        assert_eq!(usage.seven_day_percentage(), 0.0);
        assert_eq!(usage.thirty_day_percentage(), 0.0);
    }
}
