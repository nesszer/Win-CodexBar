//! Theme: macOS-Style Dark
//!
//! A clean dark theme matching macOS dark mode appearance.
//! Uses colors that approximate NSColor system colors.

#![allow(dead_code)]

use egui::Color32;

/// macOS-Style Dark Theme
pub struct Theme;

impl Theme {
    // ═══════════════════════════════════════════════════════════════════
    // BACKGROUNDS - macOS dark mode style
    // ═══════════════════════════════════════════════════════════════════

    /// Window background - matches macOS windowBackgroundColor (dark)
    pub const BG_PRIMARY: Color32 = Color32::from_rgb(30, 30, 30);

    /// Secondary background - elevated layer
    pub const BG_SECONDARY: Color32 = Color32::from_rgb(38, 38, 38);

    /// Tertiary background - for nested elements
    pub const BG_TERTIARY: Color32 = Color32::from_rgb(48, 48, 48);

    /// Card/panel background - slightly elevated
    pub const CARD_BG: Color32 = Color32::from_rgb(44, 44, 46);

    /// Card background on hover
    pub const CARD_BG_HOVER: Color32 = Color32::from_rgb(54, 54, 56);

    /// Elevated surface (modals, popovers)
    pub const SURFACE_ELEVATED: Color32 = Color32::from_rgb(50, 50, 52);

    /// Input field background
    pub const INPUT_BG: Color32 = Color32::from_rgb(28, 28, 30);

    // ═══════════════════════════════════════════════════════════════════
    // ACCENT COLORS - macOS blue accent
    // ═══════════════════════════════════════════════════════════════════

    /// Primary accent - macOS systemBlue
    pub const ACCENT_PRIMARY: Color32 = Color32::from_rgb(10, 132, 255);

    /// Primary accent hover
    pub const ACCENT_HOVER: Color32 = Color32::from_rgb(50, 160, 255);

    /// Primary accent muted
    pub const ACCENT_MUTED: Color32 = Color32::from_rgb(10, 132, 255);

    /// Secondary accent
    pub const ACCENT_SECONDARY: Color32 = Color32::from_rgb(94, 92, 230);

    /// Tertiary accent
    pub const ACCENT_TERTIARY: Color32 = Color32::from_rgb(100, 100, 230);

    // ═══════════════════════════════════════════════════════════════════
    // TAB COLORS
    // ═══════════════════════════════════════════════════════════════════

    /// Tab container background
    pub const TAB_CONTAINER: Color32 = Color32::from_rgb(28, 28, 30);

    /// Tab inactive state
    pub const TAB_INACTIVE: Color32 = Color32::from_rgb(44, 44, 46);

    /// Tab active state
    pub const TAB_ACTIVE: Color32 = Color32::from_rgb(10, 132, 255);

    /// Tab text when inactive - secondaryLabelColor
    pub const TAB_TEXT_INACTIVE: Color32 = Color32::from_rgb(142, 142, 147);

    /// Tab text when active
    pub const TAB_TEXT_ACTIVE: Color32 = Color32::WHITE;

    // ═══════════════════════════════════════════════════════════════════
    // TEXT COLORS - macOS label colors
    // ═══════════════════════════════════════════════════════════════════

    /// Primary text - labelColor (white in dark mode)
    pub const TEXT_PRIMARY: Color32 = Color32::from_rgb(255, 255, 255);

    /// Secondary text - secondaryLabelColor
    pub const TEXT_SECONDARY: Color32 = Color32::from_rgb(142, 142, 147);

    /// Muted text - tertiaryLabelColor
    pub const TEXT_MUTED: Color32 = Color32::from_rgb(84, 84, 88);

    /// Dimmed text - quaternaryLabelColor
    pub const TEXT_DIM: Color32 = Color32::from_rgb(60, 60, 67);

    /// Section header text
    pub const TEXT_SECTION: Color32 = Color32::from_rgb(142, 142, 147);

    // ═══════════════════════════════════════════════════════════════════
    // BORDERS & SEPARATORS - macOS separatorColor
    // ═══════════════════════════════════════════════════════════════════

    /// Separator line - separatorColor
    pub const SEPARATOR: Color32 = Color32::from_rgb(56, 56, 58);

    /// Card/panel border
    pub const CARD_BORDER: Color32 = Color32::from_rgb(56, 56, 58);

    /// Focused/accent border
    pub const CARD_BORDER_ACCENT: Color32 = Color32::from_rgb(10, 132, 255);

    /// Subtle border for inputs
    pub const BORDER_SUBTLE: Color32 = Color32::from_rgb(68, 68, 70);

    // ═══════════════════════════════════════════════════════════════════
    // USAGE/STATUS COLORS - macOS system colors
    // ═══════════════════════════════════════════════════════════════════

    /// Green - systemGreen (0-50% usage)
    pub const GREEN: Color32 = Color32::from_rgb(48, 209, 88);
    pub const USAGE_GREEN: Color32 = Self::GREEN;

    /// Blue - systemBlue
    pub const BLUE: Color32 = Color32::from_rgb(10, 132, 255);

    /// Yellow - systemYellow (50-75% usage)
    pub const YELLOW: Color32 = Color32::from_rgb(255, 214, 10);

    /// Orange - systemOrange (75-90% usage)
    pub const ORANGE: Color32 = Color32::from_rgb(255, 159, 10);
    pub const USAGE_ORANGE: Color32 = Self::ORANGE;

    /// Red - systemRed (90-100% usage)
    pub const RED: Color32 = Color32::from_rgb(255, 69, 58);

    /// Cyan - systemCyan
    pub const CYAN: Color32 = Color32::from_rgb(100, 210, 255);

    /// Progress bar track - tertiaryLabelColor with opacity
    pub fn progress_track() -> Color32 {
        Color32::from_rgba_unmultiplied(84, 84, 88, 56)
    }

    // ═══════════════════════════════════════════════════════════════════
    // BADGES - Status indicators
    // ═══════════════════════════════════════════════════════════════════

    /// Success badge - systemGreen
    pub const BADGE_SUCCESS: Color32 = Color32::from_rgb(48, 209, 88);

    /// Warning badge - systemOrange
    pub const BADGE_WARNING: Color32 = Color32::from_rgb(255, 159, 10);

    /// Error badge - systemRed
    pub const BADGE_ERROR: Color32 = Color32::from_rgb(255, 69, 58);

    /// Info badge - systemBlue
    pub const BADGE_INFO: Color32 = Color32::from_rgb(10, 132, 255);

    // ═══════════════════════════════════════════════════════════════════
    // SPECIAL EFFECTS
    // ═══════════════════════════════════════════════════════════════════

    /// Shadow color
    pub fn shadow() -> Color32 {
        Color32::from_rgba_unmultiplied(0, 0, 0, 60)
    }

    /// Selection overlay - selectedContentBackgroundColor with opacity
    pub fn selection_overlay() -> Color32 {
        Color32::from_rgba_unmultiplied(10, 132, 255, 30)
    }

    /// Hover overlay
    pub fn hover_overlay() -> Color32 {
        Color32::from_rgba_unmultiplied(255, 255, 255, 8)
    }

    /// Glow overlay for active elements
    pub fn glow_overlay() -> Color32 {
        Color32::from_rgba_unmultiplied(10, 132, 255, 25)
    }

    /// Progress glow
    pub fn progress_glow() -> Color32 {
        Color32::from_rgba_unmultiplied(10, 132, 255, 35)
    }

    /// Gradient start (for backgrounds)
    pub fn gradient_start() -> Color32 {
        Color32::from_rgba_unmultiplied(10, 132, 255, 10)
    }

    /// Gradient end
    pub fn gradient_end() -> Color32 {
        Color32::from_rgba_unmultiplied(94, 92, 230, 8)
    }

    // ═══════════════════════════════════════════════════════════════════
    // METHODS - Usage-based coloring
    // ═══════════════════════════════════════════════════════════════════

    /// Get usage color based on percentage
    pub fn usage_color(percent: f64) -> Color32 {
        if percent <= 50.0 {
            Self::GREEN
        } else if percent <= 75.0 {
            Self::YELLOW
        } else if percent <= 90.0 {
            Self::ORANGE
        } else {
            Self::RED
        }
    }

    /// Get a dimmed version of usage color for track
    pub fn usage_track_color(_percent: f64) -> Color32 {
        Self::progress_track()
    }

    /// Get subtle glow color for usage
    pub fn usage_glow_color(percent: f64) -> Color32 {
        let base = Self::usage_color(percent);
        Color32::from_rgba_unmultiplied(base.r(), base.g(), base.b(), 35)
    }

    /// Get menu item hover background
    pub fn menu_hover() -> Color32 {
        Color32::from_rgba_unmultiplied(255, 255, 255, 8)
    }

    /// Button gradient top
    pub fn button_gradient_top() -> Color32 {
        Color32::from_rgb(70, 145, 255)
    }

    /// Button gradient bottom
    pub fn button_gradient_bottom() -> Color32 {
        Color32::from_rgb(50, 120, 230)
    }
}

use crate::status::StatusLevel;

/// Get color for a provider status level
pub fn status_color(level: StatusLevel) -> Color32 {
    match level {
        StatusLevel::Operational => Theme::GREEN,
        StatusLevel::Degraded => Theme::YELLOW,
        StatusLevel::Partial => Theme::ORANGE,
        StatusLevel::Major => Theme::RED,
        StatusLevel::Unknown => Theme::TEXT_MUTED,
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// PROVIDER ICONS - Clean symbols with personality
// ═══════════════════════════════════════════════════════════════════════════

/// Provider icons - distinctive symbols
pub fn provider_icon(name: &str) -> &'static str {
    match name.to_lowercase().as_str() {
        "codex" => "◆",
        "claude" => "◈",
        "cursor" => "▸",
        "gemini" => "✦",
        "copilot" => "⬡",
        "antigravity" => "◉",
        "factory" | "droid" => "◎",
        "zai" | "z.ai" => "Z",
        "kiro" => "K",
        "kilo" | "kilo-ai" => "K",
        "vertexai" | "vertex ai" => "△",
        "augment" => "A",
        "minimax" => "M",
        "opencode" => "○",
        "kimi" => "☽",
        "kimik2" | "kimi k2" => "☽",
        "amp" => "⚡",
        "synthetic" => "◇",
        "jetbrains" | "jetbrains ai" => "J",
        _ => "●",
    }
}

/// Provider brand colors - matching original CodexBar
pub fn provider_color(name: &str) -> Color32 {
    match name.to_lowercase().as_str() {
        "claude" => Color32::from_rgb(204, 124, 94), // #CC7C5E - Warm terracotta
        "codex" => Color32::from_rgb(73, 163, 176),  // #49A3B0 - Teal
        "gemini" => Color32::from_rgb(171, 135, 234), // #AB87EA - Purple
        "cursor" => Color32::from_rgb(0, 191, 165),  // #00BFA5 - Teal green
        "copilot" => Color32::from_rgb(168, 85, 247), // #A855F7 - Vibrant purple
        "jetbrains" | "jetbrains ai" => Color32::from_rgb(255, 51, 153), // #FF3399 - Hot pink
        "antigravity" => Color32::from_rgb(96, 186, 126), // #60BA7E - Soft green
        "augment" => Color32::from_rgb(99, 102, 241), // #6366F1 - Indigo
        "amp" => Color32::from_rgb(220, 38, 38),     // #DC2626 - Red
        "factory" | "droid" => Color32::from_rgb(255, 107, 53), // #FF6B35 - Orange
        "kimi" => Color32::from_rgb(254, 96, 60),    // #FE603C - Coral
        "kimik2" | "kimi k2" => Color32::from_rgb(76, 0, 255), // #4C00FF - Electric blue
        "kiro" => Color32::from_rgb(255, 153, 0),    // #FF9900 - Amber
        "kilo" | "kilo-ai" => Color32::from_rgb(242, 112, 39), // #F27027 - Kilo orange
        "opencode" => Color32::from_rgb(59, 130, 246), // #3B82F6 - Blue
        "minimax" => Color32::from_rgb(254, 96, 60), // #FE603C - Coral (same as Kimi)
        "vertexai" | "vertex ai" => Color32::from_rgb(66, 133, 244), // #4285F4 - Google blue
        "zai" | "z.ai" => Color32::from_rgb(232, 90, 106), // #E85A6A - Rose
        "synthetic" => Color32::from_rgb(20, 20, 20), // #141414 - Near black
        _ => Theme::ACCENT_PRIMARY,
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// SPACING CONSTANTS - Generous, comfortable layout
// ═══════════════════════════════════════════════════════════════════════════

/// Spacing constants for consistent layout
pub struct Spacing;

impl Spacing {
    pub const XXS: f32 = 4.0;
    pub const XS: f32 = 6.0;
    pub const SM: f32 = 10.0;
    pub const MD: f32 = 12.0;
    pub const LG: f32 = 16.0;
    pub const XL: f32 = 24.0;
    pub const XXL: f32 = 32.0;
}

/// Rounding constants - softer, modern feel
pub struct Radius;

impl Radius {
    pub const XS: f32 = 4.0;
    pub const SM: f32 = 6.0;
    pub const MD: f32 = 10.0;
    pub const LG: f32 = 14.0;
    pub const XL: f32 = 18.0;
    pub const PILL: f32 = 100.0;
}

/// Font sizes - macOS-inspired clear hierarchy
pub struct FontSize;

impl FontSize {
    pub const XS: f32 = 11.0;
    pub const SM: f32 = 13.0;
    pub const BASE: f32 = 13.0;
    pub const MD: f32 = 14.0;
    pub const LG: f32 = 17.0;
    pub const XL: f32 = 18.0;
    pub const XXL: f32 = 22.0;
}
