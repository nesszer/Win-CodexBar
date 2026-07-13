//! Rate window model - represents a usage limit window (e.g., 5-hour session, 7-day weekly)

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Represents a rate limit window with usage percentage and reset time
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RateWindow {
    /// Percentage of the window that has been used (0-100)
    pub used_percent: f64,

    /// Duration of the window in minutes (e.g., 300 for 5-hour, 10080 for 7-day)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub window_minutes: Option<u32>,

    /// When the window resets
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resets_at: Option<DateTime<Utc>>,

    /// Human-readable reset description (e.g., "Jan 15 at 3:00pm")
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reset_description: Option<String>,

    /// Whether this row is an informational value rather than a quota.
    #[serde(default)]
    pub is_informational: bool,
}

impl RateWindow {
    /// Create a new rate window
    pub fn new(used_percent: f64) -> Self {
        Self {
            used_percent: Self::finite_percent(used_percent),
            window_minutes: None,
            resets_at: None,
            reset_description: None,
            is_informational: false,
        }
    }

    /// Create an informational row without implying a percentage quota.
    pub fn informational(description: impl Into<String>) -> Self {
        Self {
            reset_description: Some(description.into()),
            is_informational: true,
            ..Self::new(0.0)
        }
    }

    /// Create a rate window with full details
    pub fn with_details(
        used_percent: f64,
        window_minutes: Option<u32>,
        resets_at: Option<DateTime<Utc>>,
        reset_description: Option<String>,
    ) -> Self {
        Self {
            used_percent: Self::finite_percent(used_percent),
            window_minutes,
            resets_at,
            reset_description,
            is_informational: false,
        }
    }

    /// Get the remaining percentage (100 - used)
    pub fn remaining_percent(&self) -> f64 {
        100.0 - self.used_percent
    }

    /// Check if the window is exhausted (>= 100% used)
    pub fn is_exhausted(&self) -> bool {
        self.used_percent >= 100.0
    }

    /// Check if the window is nearly exhausted (>= 90% used)
    pub fn is_nearly_exhausted(&self) -> bool {
        self.used_percent >= 90.0
    }

    /// Format the reset time as a countdown string
    pub fn format_countdown(&self) -> Option<String> {
        let resets_at = self.resets_at?;
        let now = Utc::now();

        if resets_at <= now {
            return Some("now".to_string());
        }

        let duration = resets_at - now;
        let hours = duration.num_hours();
        let total_minutes = ((duration.num_seconds() + 59) / 60).max(1);
        let minutes = total_minutes % 60;

        if hours > 24 {
            let days = hours / 24;
            Some(format!("{}d {}h", days, hours % 24))
        } else if hours > 0 {
            Some(format!("{}h {}m", hours, minutes))
        } else {
            Some(format!("{}m", minutes))
        }
    }

    fn finite_percent(value: f64) -> f64 {
        if value.is_finite() {
            value.clamp(0.0, 100.0)
        } else {
            0.0
        }
    }
}

impl Default for RateWindow {
    fn default() -> Self {
        Self::new(0.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_remaining_percent() {
        let window = RateWindow::new(75.0);
        assert!((window.remaining_percent() - 25.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_clamping() {
        let window = RateWindow::new(150.0);
        assert!((window.used_percent - 100.0).abs() < f64::EPSILON);

        let window = RateWindow::new(-10.0);
        assert!(window.used_percent.abs() < f64::EPSILON);
    }

    #[test]
    fn test_exhausted() {
        assert!(RateWindow::new(100.0).is_exhausted());
        assert!(!RateWindow::new(99.0).is_exhausted());
    }

    #[test]
    fn countdown_uses_one_minute_for_sub_minute_future_reset() {
        let window = RateWindow::with_details(
            10.0,
            None,
            Some(Utc::now() + chrono::Duration::seconds(30)),
            None,
        );

        assert_eq!(window.format_countdown().as_deref(), Some("1m"));
    }
}
