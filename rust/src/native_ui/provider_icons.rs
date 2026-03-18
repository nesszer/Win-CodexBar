//! Provider SVG icon loading and rendering
//!
//! Loads provider brand icons from assets/icons/ directory.

use egui::{ColorImage, TextureHandle, TextureOptions};
use std::collections::HashMap;
use std::sync::OnceLock;

/// Static icon data embedded at compile time
static ICON_DATA: OnceLock<HashMap<&'static str, &'static [u8]>> = OnceLock::new();

fn get_icon_data() -> &'static HashMap<&'static str, &'static [u8]> {
    ICON_DATA.get_or_init(|| {
        let mut map = HashMap::new();
        map.insert(
            "amp",
            include_bytes!("../../assets/icons/ProviderIcon-amp.svg").as_slice(),
        );
        map.insert(
            "antigravity",
            include_bytes!("../../assets/icons/ProviderIcon-antigravity.svg").as_slice(),
        );
        map.insert(
            "augment",
            include_bytes!("../../assets/icons/ProviderIcon-augment.svg").as_slice(),
        );
        map.insert(
            "claude",
            include_bytes!("../../assets/icons/ProviderIcon-claude.svg").as_slice(),
        );
        map.insert(
            "codex",
            include_bytes!("../../assets/icons/ProviderIcon-codex.svg").as_slice(),
        );
        map.insert(
            "copilot",
            include_bytes!("../../assets/icons/ProviderIcon-copilot.svg").as_slice(),
        );
        map.insert(
            "cursor",
            include_bytes!("../../assets/icons/ProviderIcon-cursor.svg").as_slice(),
        );
        map.insert(
            "factory",
            include_bytes!("../../assets/icons/ProviderIcon-factory.svg").as_slice(),
        );
        map.insert(
            "gemini",
            include_bytes!("../../assets/icons/ProviderIcon-gemini.svg").as_slice(),
        );
        map.insert(
            "jetbrains",
            include_bytes!("../../assets/icons/ProviderIcon-jetbrains.svg").as_slice(),
        );
        map.insert(
            "kimi",
            include_bytes!("../../assets/icons/ProviderIcon-kimi.svg").as_slice(),
        );
        map.insert(
            "kiro",
            include_bytes!("../../assets/icons/ProviderIcon-kiro.svg").as_slice(),
        );
        map.insert(
            "minimax",
            include_bytes!("../../assets/icons/ProviderIcon-minimax.svg").as_slice(),
        );
        map.insert(
            "opencode",
            include_bytes!("../../assets/icons/ProviderIcon-opencode.svg").as_slice(),
        );
        map.insert(
            "synthetic",
            include_bytes!("../../assets/icons/ProviderIcon-synthetic.svg").as_slice(),
        );
        map.insert(
            "vertexai",
            include_bytes!("../../assets/icons/ProviderIcon-vertexai.svg").as_slice(),
        );
        map.insert(
            "zai",
            include_bytes!("../../assets/icons/ProviderIcon-zai.svg").as_slice(),
        );
        map
    })
}

/// Provider icon cache - stores loaded textures
pub struct ProviderIconCache {
    textures: HashMap<String, TextureHandle>,
}

impl ProviderIconCache {
    pub fn new() -> Self {
        Self {
            textures: HashMap::new(),
        }
    }

    /// Get or load a provider icon texture
    pub fn get_icon(
        &mut self,
        ctx: &egui::Context,
        provider_name: &str,
        size: u32,
    ) -> Option<&TextureHandle> {
        let key = normalize_provider_name(provider_name);
        let cache_key = format!("{}_{}", key, size);

        if !self.textures.contains_key(&cache_key) {
            if let Some(texture) = load_provider_icon(ctx, &key, size) {
                self.textures.insert(cache_key.clone(), texture);
            }
        }

        self.textures.get(&cache_key)
    }
}

/// Normalize provider name to match icon filename
fn normalize_provider_name(name: &str) -> String {
    let lower = name.to_lowercase();
    match lower.as_str() {
        "droid" => "factory".to_string(),
        "z.ai" => "zai".to_string(),
        "vertex ai" => "vertexai".to_string(),
        "jetbrains ai" => "jetbrains".to_string(),
        "kimi k2" | "kimik2" => "kimi".to_string(),
        _ => lower.replace(" ", "").replace("-", ""),
    }
}

/// Load and rasterize an SVG icon at the specified size
fn load_provider_icon(ctx: &egui::Context, provider_key: &str, size: u32) -> Option<TextureHandle> {
    let icon_data = get_icon_data();
    let svg_data = icon_data.get(provider_key)?;

    // Parse SVG
    let options = usvg::Options::default();
    let tree = usvg::Tree::from_data(svg_data, &options).ok()?;

    // Create pixmap for rendering
    let mut pixmap = tiny_skia::Pixmap::new(size, size)?;

    // Calculate scale to fit
    let svg_size = tree.size();
    let scale_x = size as f32 / svg_size.width();
    let scale_y = size as f32 / svg_size.height();
    let scale = scale_x.min(scale_y);

    // Center the icon
    let offset_x = (size as f32 - svg_size.width() * scale) / 2.0;
    let offset_y = (size as f32 - svg_size.height() * scale) / 2.0;

    let transform =
        tiny_skia::Transform::from_scale(scale, scale).post_translate(offset_x, offset_y);

    resvg::render(&tree, transform, &mut pixmap.as_mut());

    // Convert to egui texture
    let image = ColorImage::from_rgba_unmultiplied([size as usize, size as usize], pixmap.data());

    let texture = ctx.load_texture(
        format!("provider_icon_{}", provider_key),
        image,
        TextureOptions::LINEAR,
    );

    Some(texture)
}

/// Check if an icon exists for the given provider
#[allow(dead_code)]
pub fn has_icon(provider_name: &str) -> bool {
    let key = normalize_provider_name(provider_name);
    get_icon_data().contains_key(key.as_str())
}
