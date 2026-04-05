//! Provider Switcher Weekly Indicator
//!
//! Small progress bars (4px height) showing weekly usage in the provider switcher.
//! These provide at-a-glance secondary usage information for each provider.

#![allow(dead_code)]

use crate::core::ProviderId;

/// Height of the weekly indicator bar in pixels
pub const WEEKLY_INDICATOR_HEIGHT: u32 = 4;

/// Padding from button edges in pixels
pub const WEEKLY_INDICATOR_PADDING: u32 = 6;

/// Corner radius for capsule shape
pub const WEEKLY_INDICATOR_CORNER_RADIUS: u32 = 2;

/// Configuration for a weekly indicator
#[derive(Debug, Clone)]
pub struct WeeklyIndicatorConfig {
    /// Provider this indicator is for
    pub provider: ProviderId,
    /// Percentage remaining (0-100)
    pub remaining_percent: f64,
    /// Whether the indicator should be visible
    pub visible: bool,
    /// Whether the parent button is selected
    pub is_selected: bool,
}

impl WeeklyIndicatorConfig {
    /// Create a new indicator config
    pub fn new(provider: ProviderId, remaining_percent: f64) -> Self {
        Self {
            provider,
            remaining_percent: remaining_percent.clamp(0.0, 100.0),
            visible: true,
            is_selected: false,
        }
    }

    /// Set visibility
    pub fn with_visibility(mut self, visible: bool) -> Self {
        self.visible = visible;
        self
    }

    /// Set selected state
    pub fn with_selected(mut self, selected: bool) -> Self {
        self.is_selected = selected;
        self
    }

    /// Whether this indicator should be drawn
    pub fn should_draw(&self) -> bool {
        self.visible && !self.is_selected
    }

    /// Get the fill width as a ratio (0.0 to 1.0)
    pub fn fill_ratio(&self) -> f64 {
        self.remaining_percent / 100.0
    }
}

/// Colors for weekly indicators
#[derive(Debug, Clone, Copy)]
pub struct WeeklyIndicatorColors {
    /// Track background color (RGBA)
    pub track: (u8, u8, u8, u8),
    /// Fill color (RGBA)
    pub fill: (u8, u8, u8, u8),
}

impl WeeklyIndicatorColors {
    /// Get colors for a provider
    pub fn for_provider(provider: ProviderId) -> Self {
        let fill = Self::provider_color(provider);
        Self {
            track: (128, 128, 128, 56), // Gray with ~22% alpha
            fill,
        }
    }

    /// Get the brand color for a provider
    fn provider_color(provider: ProviderId) -> (u8, u8, u8, u8) {
        match provider {
            ProviderId::Codex => (0, 200, 83, 255),     // OpenAI green
            ProviderId::Claude => (217, 119, 87, 255),  // Claude terracotta
            ProviderId::Cursor => (59, 130, 246, 255),  // Cursor blue
            ProviderId::Factory => (139, 92, 246, 255), // Factory purple
            ProviderId::Gemini => (66, 133, 244, 255),  // Google blue
            ProviderId::Antigravity => (156, 39, 176, 255), // Purple
            ProviderId::Copilot => (36, 41, 47, 255),   // GitHub dark
            ProviderId::Zai => (255, 107, 0, 255),      // Zai orange
            ProviderId::MiniMax => (255, 193, 7, 255),  // Yellow
            ProviderId::Kiro => (255, 153, 0, 255),     // AWS orange
            ProviderId::VertexAI => (66, 133, 244, 255), // Google blue
            ProviderId::Augment => (46, 125, 50, 255),  // Green
            ProviderId::OpenCode => (99, 102, 241, 255), // Indigo
            ProviderId::Kimi => (255, 87, 34, 255),     // Deep orange
            ProviderId::KimiK2 => (255, 87, 34, 255),   // Deep orange
            ProviderId::Amp => (233, 30, 99, 255),      // Pink
            ProviderId::Synthetic => (158, 158, 158, 255), // Gray
            ProviderId::JetBrains => (255, 128, 0, 255), // JetBrains orange
            ProviderId::Alibaba => (255, 106, 0, 255),  // Alibaba orange
            ProviderId::Warp => (1, 217, 166, 255),     // Warp teal
            ProviderId::Ollama => (255, 255, 255, 255), // Ollama white
            ProviderId::OpenRouter => (110, 65, 226, 255), // OpenRouter purple
            ProviderId::NanoGPT => (59, 130, 246, 255), // Blue
            ProviderId::Infini => (0, 150, 255, 255),   // Infini blue
        }
    }

    /// Get fill color with opacity adjustment
    pub fn fill_with_opacity(&self, opacity: f32) -> (u8, u8, u8, u8) {
        let (r, g, b, a) = self.fill;
        let new_alpha = ((a as f32) * opacity).round() as u8;
        (r, g, b, new_alpha)
    }
}

/// Weekly indicator drawing data for rendering
#[derive(Debug, Clone)]
pub struct WeeklyIndicatorDrawData {
    /// X position in pixels
    pub x: i32,
    /// Y position in pixels
    pub y: i32,
    /// Total width in pixels
    pub width: u32,
    /// Height in pixels (usually 4)
    pub height: u32,
    /// Fill width in pixels
    pub fill_width: u32,
    /// Track color
    pub track_color: (u8, u8, u8, u8),
    /// Fill color
    pub fill_color: (u8, u8, u8, u8),
    /// Corner radius
    pub corner_radius: u32,
}

impl WeeklyIndicatorDrawData {
    /// Create draw data from config and button dimensions
    pub fn from_config(
        config: &WeeklyIndicatorConfig,
        button_x: i32,
        button_y: i32,
        button_width: u32,
        button_height: u32,
    ) -> Option<Self> {
        if !config.should_draw() {
            return None;
        }

        let padding = WEEKLY_INDICATOR_PADDING as i32;
        let width = button_width.saturating_sub(2 * WEEKLY_INDICATOR_PADDING);
        let height = WEEKLY_INDICATOR_HEIGHT;

        // Position at bottom of button with some margin
        let x = button_x + padding;
        let y = button_y + (button_height as i32) - (height as i32) - padding;

        let fill_width = ((width as f64) * config.fill_ratio()).round() as u32;

        let colors = WeeklyIndicatorColors::for_provider(config.provider);

        Some(WeeklyIndicatorDrawData {
            x,
            y,
            width,
            height,
            fill_width,
            track_color: colors.track,
            fill_color: colors.fill,
            corner_radius: WEEKLY_INDICATOR_CORNER_RADIUS,
        })
    }
}

/// Settings for showing used vs remaining
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum UsageDisplayMode {
    /// Show percentage remaining (100% = empty, 0% = full)
    #[default]
    Remaining,
    /// Show percentage used (0% = empty, 100% = full)
    Used,
}

impl UsageDisplayMode {
    /// Convert a usage percent to display percent based on mode
    pub fn to_display_percent(self, used_percent: f64) -> f64 {
        match self {
            UsageDisplayMode::Remaining => 100.0 - used_percent,
            UsageDisplayMode::Used => used_percent,
        }
    }
}

/// Get the weekly remaining percent for a provider from usage data
pub fn calculate_weekly_remaining(
    primary_percent: Option<f64>,
    secondary_percent: Option<f64>,
    provider: ProviderId,
    display_mode: UsageDisplayMode,
) -> Option<f64> {
    // For Factory, use secondary if available, otherwise primary
    let used_percent = if provider == ProviderId::Factory {
        secondary_percent.or(primary_percent)
    } else {
        secondary_percent
    }?;

    Some(display_mode.to_display_percent(used_percent))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_indicator_config_new() {
        let config = WeeklyIndicatorConfig::new(ProviderId::Claude, 75.0);
        assert_eq!(config.provider, ProviderId::Claude);
        assert!((config.remaining_percent - 75.0).abs() < 0.01);
        assert!(config.visible);
        assert!(!config.is_selected);
    }

    #[test]
    fn test_indicator_should_draw() {
        let config = WeeklyIndicatorConfig::new(ProviderId::Claude, 50.0);
        assert!(config.should_draw());

        let config = config.with_selected(true);
        assert!(!config.should_draw());

        let config = WeeklyIndicatorConfig::new(ProviderId::Claude, 50.0).with_visibility(false);
        assert!(!config.should_draw());
    }

    #[test]
    fn test_fill_ratio() {
        let config = WeeklyIndicatorConfig::new(ProviderId::Claude, 75.0);
        assert!((config.fill_ratio() - 0.75).abs() < 0.01);

        let config = WeeklyIndicatorConfig::new(ProviderId::Claude, 0.0);
        assert!(config.fill_ratio().abs() < 0.01);

        let config = WeeklyIndicatorConfig::new(ProviderId::Claude, 100.0);
        assert!((config.fill_ratio() - 1.0).abs() < 0.01);
    }

    #[test]
    fn test_provider_colors() {
        let colors = WeeklyIndicatorColors::for_provider(ProviderId::Claude);
        assert_eq!(colors.fill, (217, 119, 87, 255));

        let colors = WeeklyIndicatorColors::for_provider(ProviderId::Codex);
        assert_eq!(colors.fill, (0, 200, 83, 255));
    }

    #[test]
    fn test_draw_data_creation() {
        let config = WeeklyIndicatorConfig::new(ProviderId::Claude, 50.0);
        let draw_data = WeeklyIndicatorDrawData::from_config(&config, 10, 20, 100, 40);

        assert!(draw_data.is_some());
        let data = draw_data.unwrap();
        assert_eq!(data.width, 88); // 100 - 2*6
        assert_eq!(data.height, 4);
        assert_eq!(data.fill_width, 44); // 50% of 88
    }

    #[test]
    fn test_usage_display_mode() {
        let remaining = UsageDisplayMode::Remaining.to_display_percent(25.0);
        assert!((remaining - 75.0).abs() < 0.01);

        let used = UsageDisplayMode::Used.to_display_percent(25.0);
        assert!((used - 25.0).abs() < 0.01);
    }

    #[test]
    fn test_calculate_weekly_remaining() {
        // Normal case: use secondary
        let result = calculate_weekly_remaining(
            Some(50.0),
            Some(30.0),
            ProviderId::Claude,
            UsageDisplayMode::Remaining,
        );
        assert_eq!(result, Some(70.0));

        // Factory special case: secondary available
        let result = calculate_weekly_remaining(
            Some(50.0),
            Some(30.0),
            ProviderId::Factory,
            UsageDisplayMode::Remaining,
        );
        assert_eq!(result, Some(70.0));

        // Factory special case: no secondary, use primary
        let result = calculate_weekly_remaining(
            Some(50.0),
            None,
            ProviderId::Factory,
            UsageDisplayMode::Remaining,
        );
        assert_eq!(result, Some(50.0));

        // No secondary for non-Factory
        let result = calculate_weekly_remaining(
            Some(50.0),
            None,
            ProviderId::Claude,
            UsageDisplayMode::Remaining,
        );
        assert_eq!(result, None);
    }
}
