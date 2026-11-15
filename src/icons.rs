//! Professional icon system for Rust Engine Editor
//! Uses consistent Unicode glyphs with standardized sizing and visual treatment

use egui::{FontId, RichText, Ui};

/// Standard icon sizes used throughout the editor
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IconSize {
    /// Small icons (12pt) - for toolbar buttons, tree items
    Small = 12,
    /// Medium icons (14pt) - for panel headers, list items
    Medium = 14,
    /// Large icons (16pt) - for toolbar primary actions
    Large = 16,
    /// Extra large icons (20pt) - for viewport gizmos, major UI elements
    XLarge = 20,
}

impl IconSize {
    pub fn pt(self) -> f32 {
        self as u8 as f32
    }
}

/// Professional editor icons using carefully selected Unicode glyphs
/// All icons use a consistent visual weight and style
pub struct Icons;

impl Icons {
    // File & Folder icons
    pub const FOLDER: &str = "📁";
    pub const FOLDER_OPEN: &str = "📂";
    pub const FILE: &str = "📄";
    pub const FILE_BLANK: &str = "📃";

    // Scene & Asset type icons
    pub const SCENE: &str = "🌍";
    pub const MATERIAL: &str = "🎨";
    pub const TEXTURE: &str = "🖼️";
    pub const MODEL: &str = "📦";
    pub const SCRIPT: &str = "📜";
    pub const AUDIO: &str = "🔊";
    pub const PREFAB: &str = "🧩";
    pub const BLUEPRINT: &str = "📋";
    pub const SHADER: &str = "✨";
    pub const ANIMATION: &str = "🎬";
    pub const PARTICLE: &str = "✨";
    pub const FONT: &str = "🔤";
    pub const SETTINGS_FILE: &str = "⚙️";

    // Tool icons
    pub const SELECT: &str = "🖱️";
    pub const MOVE: &str = "⤴";
    pub const ROTATE: &str = "🔄";
    pub const SCALE: &str = "⤢";
    pub const TRANSFORM: &str = "⛶";
    pub const TOOLBOX: &str = "🧰";

    // Action icons
    pub const SAVE: &str = "💾";
    pub const SAVE_ALL: &str = "💾";
    pub const UNDO: &str = "↶";
    pub const REDO: &str = "↷";
    pub const PLAY: &str = "▶";
    pub const PAUSE: &str = "⏸";
    pub const STOP: &str = "⏹";
    pub const ADD: &str = "+";
    pub const DELETE: &str = "🗑";
    pub const DUPLICATE: &str = "📋";
    pub const RENAME: &str = "✎";
    pub const SEARCH: &str = "🔍";
    pub const FILTER: &str = "🔽";
    pub const SETTINGS: &str = "⚙";
    pub const IMPORT: &str = "📥";
    pub const EXPORT: &str = "📤";
    pub const REFRESH: &str = "🔄";
    pub const BUILD: &str = "🔨";
    pub const COMPILE: &str = "⚙️";
    pub const RESET: &str = "↺";
    pub const COPY: &str = "⧉";
    pub const PASTE: &str = "⧉";

    // Navigation icons
    pub const BACK: &str = "◀";
    pub const FORWARD: &str = "▶";
    pub const UP: &str = "▲";
    pub const HOME: &str = "⌂";

    // View mode icons
    pub const GRID_VIEW: &str = "⊞";
    pub const LIST_VIEW: &str = "☰";
    pub const PERSPECTIVE: &str = "◈";
    pub const ORTHOGRAPHIC: &str = "▣";
    pub const CAMERA: &str = "📷";
    pub const LIGHT: &str = "💡";
    pub const FRONT: &str = "▤";
    pub const SIDE: &str = "▥";
    pub const TOP: &str = "▦";

    // Viewport & display icons
    pub const VIEW: &str = "◈";
    pub const GRID: &str = "▦";
    pub const GIZMO: &str = "⤢";
    pub const COLLISION: &str = "▣";
    pub const WIREFRAME: &str = "▧";
    pub const LIT: &str = "◉";
    pub const TOOL: &str = "⛶";
    pub const FOCUS: &str = "◎";
    pub const ORTHO: &str = "▣";

    // Log & status icons
    pub const LOG: &str = "☰";
    pub const WARN: &str = "⚠";

    // Entity/Object icons
    pub const ENTITY: &str = "◆";
    pub const CAMERA_ENTITY: &str = "📷";
    pub const LIGHT_ENTITY: &str = "💡";
    pub const MESH_ENTITY: &str = "📦";
    pub const EMPTY_ENTITY: &str = "⬚";
    pub const COMPONENTS: &str = "⚙";

    // State icons
    pub const VISIBLE: &str = "👁";
    pub const HIDDEN: &str = "🙈";
    pub const LOCKED: &str = "🔒";
    pub const UNLOCKED: &str = "🔓";
    pub const EXPANDED: &str = "▼";
    pub const COLLAPSED: &str = "▶";
    pub const CHECKED: &str = "✓";
    pub const UNCHECKED: &str = "☐";
    pub const RADIO_ON: &str = "◉";
    pub const RADIO_OFF: &str = "◯";

    // Notification/Status icons
    pub const SUCCESS: &str = "✓";
    pub const INFO: &str = "ℹ";
    pub const WARNING: &str = "⚠";
    pub const ERROR: &str = "✕";
    pub const LOADING: &str = "⟳";

    // Menu/File icons
    pub const NEW: &str = "📄";
    pub const OPEN: &str = "📂";
    pub const FOLDER_NEW: &str = "📁";
    pub const PROJECT: &str = "📁";

    /// Get icon for file extension
    pub fn for_extension(ext: &str) -> &'static str {
        match ext.to_lowercase().as_str() {
            "scene" | "lvl" | "level" | "umap" => Self::SCENE,
            "mat" | "material" | "matx" => Self::MATERIAL,
            "png" | "jpg" | "jpeg" | "tga" | "bmp" | "hdr" | "exr" | "ktx" | "dds" => Self::TEXTURE,
            "fbx" | "obj" | "gltf" | "glb" | "dae" | "blend" | "3ds" | "msh" => Self::MODEL,
            "rs" | "ron" | "toml" | "json" | "lua" | "py" | "js" | "ts" => Self::SCRIPT,
            "wav" | "mp3" | "ogg" | "flac" => Self::AUDIO,
            "prefab" | "prefabx" => Self::PREFAB,
            "shader" | "hlsl" | "glsl" | "wgsl" | "spv" => Self::SHADER,
            "anim" | "animation" => Self::ANIMATION,
            "particle" | "vfx" => Self::PARTICLE,
            "ttf" | "otf" | "woff" | "woff2" => Self::FONT,
            _ => Self::FILE,
        }
    }

    /// Get icon for entity type
    pub fn for_entity_type(entity_type: &str) -> &'static str {
        match entity_type.to_lowercase().as_str() {
            "camera" => Self::CAMERA_ENTITY,
            "light" | "directional_light" | "point_light" | "spot_light" => Self::LIGHT_ENTITY,
            "mesh" | "static_mesh" | "skeletal_mesh" => Self::MESH_ENTITY,
            _ => Self::ENTITY,
        }
    }

    /// Render an icon with consistent styling
    pub fn show(ui: &mut Ui, icon: &str, size: IconSize, color: Option<egui::Color32>) -> egui::Response {
        let text = RichText::new(icon)
            .font(FontId::new(size.pt(), egui::FontFamily::Proportional))
            .color(color.unwrap_or(egui::Color32::WHITE));
        ui.label(text)
    }

    /// Render a clickable icon button
    pub fn button(ui: &mut Ui, icon: &str, size: IconSize) -> egui::Response {
        let text = RichText::new(icon)
            .font(FontId::new(size.pt(), egui::FontFamily::Proportional))
            .color(egui::Color32::WHITE);
        ui.add(egui::Button::new(text).frame(false).min_size(egui::vec2(size.pt() * 2.0, size.pt() * 2.0)))
    }

    /// Render a toolbar icon button with tooltip
    pub fn toolbar_button(ui: &mut Ui, icon: &str, tooltip: &str, shortcut: Option<&str>, selected: bool) -> egui::Response {
        let bg_color = if selected {
            egui::Color32::from_rgb(60, 80, 120)
        } else {
            egui::Color32::TRANSPARENT
        };

        let text = RichText::new(icon)
            .font(FontId::new(IconSize::Large.pt(), egui::FontFamily::Proportional))
            .color(egui::Color32::WHITE);

        let mut btn = egui::Button::new(text)
            .frame(false)
            .min_size(egui::vec2(32.0, 32.0))
            .fill(bg_color)
            .corner_radius(egui::CornerRadius::same(4));

        if let Some(sc) = shortcut {
            btn = btn.shortcut_text(sc);
        }

            let mut response = ui.add(btn);
        if !tooltip.is_empty() {
                response = response.on_hover_text(tooltip);
        }
        response
    }

    /// Render a tree expand/collapse icon
    pub fn tree_toggle(ui: &mut Ui, expanded: bool, size: IconSize) -> egui::Response {
        let icon = if expanded { Self::EXPANDED } else { Self::COLLAPSED };
        let text = RichText::new(icon)
            .font(FontId::new(size.pt(), egui::FontFamily::Proportional))
            .color(egui::Color32::from_rgb(140, 145, 155));
        ui.add(egui::Button::new(text).frame(false).min_size(egui::vec2(size.pt() * 1.5, size.pt() * 1.5)))
    }

    /// Render a visibility toggle icon
    pub fn visibility_toggle(ui: &mut Ui, visible: bool, size: IconSize) -> egui::Response {
        let icon = if visible { Self::VISIBLE } else { Self::HIDDEN };
        let color = if visible { egui::Color32::from_rgb(100, 200, 100) } else { egui::Color32::from_rgb(120, 120, 120) };
        let text = RichText::new(icon)
            .font(FontId::new(size.pt(), egui::FontFamily::Proportional))
            .color(color);
        ui.add(egui::Button::new(text).frame(false).min_size(egui::vec2(size.pt() * 1.5, size.pt() * 1.5)))
    }

    /// Render a lock toggle icon
    pub fn lock_toggle(ui: &mut Ui, locked: bool, size: IconSize) -> egui::Response {
        let icon = if locked { Self::LOCKED } else { Self::UNLOCKED };
        let color = if locked { egui::Color32::from_rgb(220, 180, 60) } else { egui::Color32::from_rgb(120, 120, 120) };
        let text = RichText::new(icon)
            .font(FontId::new(size.pt(), egui::FontFamily::Proportional))
            .color(color);
        ui.add(egui::Button::new(text).frame(false).min_size(egui::vec2(size.pt() * 1.5, size.pt() * 1.5)))
    }
}
