use egui::{Color32, Context, CornerRadius};
use egui_dock::{DockState, NodeIndex};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum EditorTab {
    Viewport,
    WorldOutliner,
    Details,
    ContentBrowser,
    AssetPreview,
    AssetDetails,
    OutputLog,
    ScriptEditor,
    GraphicsSettings,
}

impl EditorTab {
    pub const ALL: [Self; 9] = [
        Self::Viewport,
        Self::WorldOutliner,
        Self::Details,
        Self::ContentBrowser,
        Self::AssetPreview,
        Self::AssetDetails,
        Self::OutputLog,
        Self::ScriptEditor,
        Self::GraphicsSettings,
    ];

    pub fn title(&self) -> &'static str {
        match self {
            Self::Viewport => "Viewport",
            Self::WorldOutliner => "World Outliner",
            Self::Details => "Details",
            Self::ContentBrowser => "Content Browser",
            Self::AssetPreview => "Asset Preview",
            Self::AssetDetails => "Asset Details",
            Self::OutputLog => "Output Log",
            Self::ScriptEditor => "Script Editor",
            Self::GraphicsSettings => "Graphics Settings",
        }
    }

    pub fn id_str(&self) -> &'static str {
        match self {
            Self::Viewport => "viewport",
            Self::WorldOutliner => "outliner",
            Self::Details => "details",
            Self::ContentBrowser => "content",
            Self::AssetPreview => "asset-preview",
            Self::AssetDetails => "asset-details",
            Self::OutputLog => "log",
            Self::ScriptEditor => "script",
            Self::GraphicsSettings => "graphics",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum WorkspacePreset {
    #[default]
    Default,
    LevelEditing,
    AssetEditing,
    Scripting,
    Minimal,
}

impl WorkspacePreset {
    pub const ALL: [Self; 5] = [
        Self::Default,
        Self::LevelEditing,
        Self::AssetEditing,
        Self::Scripting,
        Self::Minimal,
    ];

    pub fn name(&self) -> &'static str {
        match self {
            Self::Default => "Default",
            Self::LevelEditing => "Level Editing",
            Self::AssetEditing => "Asset Editing",
            Self::Scripting => "Scripting",
            Self::Minimal => "Minimal",
        }
    }

    pub fn description(&self) -> &'static str {
        match self {
            Self::Default => "Classic layout with Viewport, Outliner, Details, Browser & Log",
            Self::LevelEditing => "Maximizes 3D Viewport real estate for scene assembly",
            Self::AssetEditing => "Content Browser and Details focused for asset management",
            Self::Scripting => "Script Editor focused alongside Viewport and Output Log",
            Self::Minimal => "Clean distraction-free full-screen Viewport",
        }
    }

    pub fn create_dock_state(&self) -> DockState<EditorTab> {
        match self {
            Self::Default => {
                let mut state = DockState::new(vec![EditorTab::Viewport]);
                let [viewport_node, outliner_node] = state.main_surface_mut().split_right(
                    NodeIndex::root(),
                    0.74,
                    vec![EditorTab::WorldOutliner],
                );
                let [_outliner_node, _details_node] = state.main_surface_mut().split_below(
                    outliner_node,
                    0.50,
                    vec![EditorTab::Details],
                );
                let [_viewport_node, _bottom_node] = state.main_surface_mut().split_below(
                    viewport_node,
                    0.70,
                    vec![EditorTab::ContentBrowser, EditorTab::OutputLog],
                );
                state
            }
            Self::LevelEditing => {
                let mut state = DockState::new(vec![EditorTab::Viewport]);
                let [viewport_node, right_node] = state.main_surface_mut().split_right(
                    NodeIndex::root(),
                    0.80,
                    vec![EditorTab::WorldOutliner],
                );
                let [_outliner, _details] = state.main_surface_mut().split_below(
                    right_node,
                    0.48,
                    vec![EditorTab::Details, EditorTab::GraphicsSettings],
                );
                let [_viewport, _bottom] = state.main_surface_mut().split_below(
                    viewport_node,
                    0.82,
                    vec![EditorTab::ContentBrowser],
                );
                state
            }
            Self::AssetEditing => {
                let mut state = DockState::new(vec![EditorTab::ContentBrowser]);
                let [content_node, preview_node] = state.main_surface_mut().split_right(
                    NodeIndex::root(),
                    0.30,
                    vec![EditorTab::AssetPreview],
                );
                let [_preview_node, _details_node] = state.main_surface_mut().split_right(
                    preview_node,
                    0.62,
                    vec![EditorTab::AssetDetails],
                );
                // Output Log starts collapsed (thin strip) and expands when the user drags it.
                let [_content_node, _log_node] = state.main_surface_mut().split_below(
                    content_node,
                    0.95,
                    vec![EditorTab::OutputLog],
                );
                state
            }
            Self::Scripting => {
                let mut state = DockState::new(vec![EditorTab::ScriptEditor]);
                let [script_node, right_node] = state.main_surface_mut().split_right(
                    NodeIndex::root(),
                    0.55,
                    vec![EditorTab::Viewport],
                );
                let [_viewport, _outliner] = state.main_surface_mut().split_right(
                    right_node,
                    0.50,
                    vec![EditorTab::WorldOutliner, EditorTab::Details],
                );
                let [_script, _log] = state.main_surface_mut().split_below(
                    script_node,
                    0.70,
                    vec![EditorTab::OutputLog],
                );
                state
            }
            Self::Minimal => {
                let mut state = DockState::new(vec![EditorTab::Viewport]);
                let [_viewport, _outliner] = state.main_surface_mut().split_right(
                    NodeIndex::root(),
                    0.88,
                    vec![EditorTab::WorldOutliner],
                );
                state
            }
        }
    }
}

#[derive(Clone, Debug)]
pub enum DockAction {
    Open(EditorTab),
    Close(EditorTab),
    Detach(EditorTab),
    Focus(EditorTab),
    ApplyPreset(WorkspacePreset),
    ResetDefault,
}

impl DockAction {
    pub fn apply(self, dock_state: &mut DockState<EditorTab>, default_rect: egui::Rect) {
        match self {
            Self::Open(tab) => {
                smart_open_tab(dock_state, tab);
            }
            Self::Close(tab) => {
                if let Some(pos) = dock_state.find_tab(&tab) {
                    dock_state.remove_tab(pos);
                }
            }
            Self::Detach(tab) => {
                if let Some(pos) = dock_state.find_tab(&tab) {
                    dock_state.detach_tab(pos, default_rect);
                }
            }
            Self::Focus(tab) => {
                if let Some(pos) = dock_state.find_tab(&tab) {
                    dock_state.set_active_tab(pos);
                }
            }
            Self::ApplyPreset(preset) => {
                *dock_state = preset.create_dock_state();
            }
            Self::ResetDefault => {
                *dock_state = WorkspacePreset::Default.create_dock_state();
            }
        }
    }
}

#[derive(Serialize, Deserialize, Clone)]
pub struct DockConfig {
    pub current_workspace: WorkspacePreset,
    pub dock_state: DockState<EditorTab>,
}

impl Default for DockConfig {
    fn default() -> Self {
        let current_workspace = WorkspacePreset::Default;
        let dock_state = current_workspace.create_dock_state();
        Self {
            current_workspace,
            dock_state,
        }
    }
}

pub fn editor_dock_style(ctx: &Context) -> egui_dock::Style {
    let mut style = egui_dock::Style::from_egui(&ctx.style());

    // Separators between split panes
    style.separator.width = 2.0;
    style.separator.extra_interact_width = 4.0;
    style.separator.color_idle = Color32::from_rgb(32, 34, 38);
    style.separator.color_hovered = Color32::from_rgb(52, 120, 212);
    style.separator.color_dragged = Color32::from_rgb(64, 140, 230);

    // Tab bar header
    style.tab_bar.bg_fill = Color32::from_rgb(22, 24, 27);
    style.tab_bar.height = 26.0;
    style.tab_bar.hline_color = Color32::from_rgb(35, 37, 41);

    // Tab buttons
    style.tab.active.bg_fill = Color32::from_rgb(38, 41, 46);
    style.tab.active.text_color = Color32::from_rgb(240, 242, 245);
    style.tab.active.outline_color = Color32::from_rgb(52, 120, 212);
    style.tab.active.corner_radius = CornerRadius {
        nw: 4,
        ne: 4,
        sw: 0,
        se: 0,
    };

    style.tab.inactive.bg_fill = Color32::from_rgb(26, 28, 32);
    style.tab.inactive.text_color = Color32::from_rgb(150, 155, 165);
    style.tab.inactive.outline_color = Color32::TRANSPARENT;
    style.tab.inactive.corner_radius = CornerRadius {
        nw: 3,
        ne: 3,
        sw: 0,
        se: 0,
    };

    style.tab.hovered.bg_fill = Color32::from_rgb(46, 50, 56);
    style.tab.hovered.text_color = Color32::from_rgb(255, 255, 255);
    style.tab.hovered.outline_color = Color32::from_rgb(60, 64, 72);
    style.tab.hovered.corner_radius = CornerRadius {
        nw: 4,
        ne: 4,
        sw: 0,
        se: 0,
    };

    style.tab.focused.bg_fill = Color32::from_rgb(40, 44, 50);
    style.tab.focused.text_color = Color32::from_rgb(255, 255, 255);
    style.tab.focused.outline_color = Color32::from_rgb(52, 120, 212);

    style.tab.tab_body.bg_fill = Color32::from_rgb(28, 30, 34);

    // Drag-drop dock preview overlay
    style.overlay.selection_color = Color32::from_rgba_unmultiplied(52, 120, 212, 110);
    style.overlay.selection_stroke_width = 2.0;

    // Close button on tabs
    style.buttons.close_tab_color = Color32::from_rgb(160, 165, 175);
    style.buttons.close_tab_active_color = Color32::from_rgb(255, 80, 80);

    style
}

pub fn smart_open_tab(dock_state: &mut DockState<EditorTab>, tab: EditorTab) {
    if let Some(pos) = dock_state.find_tab(&tab) {
        dock_state.set_active_tab(pos);
        return;
    }

    if dock_state.iter_all_tabs().count() == 0 {
        *dock_state = DockState::new(vec![tab]);
        return;
    }

    // Try to dock relative to a related tab
    let preferred_neighbor = match tab {
        EditorTab::Viewport => None,
        EditorTab::WorldOutliner => Some((EditorTab::Details, egui_dock::Split::Above, 0.5)),
        EditorTab::Details => Some((EditorTab::WorldOutliner, egui_dock::Split::Below, 0.5)),
        EditorTab::ContentBrowser => Some((EditorTab::AssetPreview, egui_dock::Split::Right, 0.5)),
        EditorTab::AssetPreview => Some((EditorTab::ContentBrowser, egui_dock::Split::Left, 0.5)),
        EditorTab::AssetDetails => Some((EditorTab::AssetPreview, egui_dock::Split::Right, 0.5)),
        EditorTab::OutputLog => Some((EditorTab::ContentBrowser, egui_dock::Split::Right, 0.5)),
        EditorTab::ScriptEditor => Some((EditorTab::Viewport, egui_dock::Split::Below, 0.35)),
        EditorTab::GraphicsSettings => Some((EditorTab::Details, egui_dock::Split::Below, 0.5)),
    };

    if let Some((neighbor, split, frac)) = preferred_neighbor {
        if let Some((surface, node, _)) = dock_state.find_tab(&neighbor) {
            dock_state.split((surface, node), split, frac, egui_dock::Node::leaf(tab));
            return;
        }
    }

    // Fallback: if viewport exists, split relative to it
    if let Some((surface, node, _)) = dock_state.find_tab(&EditorTab::Viewport) {
        let (split, frac) = match tab {
            EditorTab::WorldOutliner | EditorTab::Details => (egui_dock::Split::Right, 0.28),
            EditorTab::ContentBrowser | EditorTab::OutputLog => (egui_dock::Split::Below, 0.3),
            _ => (egui_dock::Split::Right, 0.25),
        };
        dock_state.split((surface, node), split, frac, egui_dock::Node::leaf(tab));
        return;
    }
    dock_state.push_to_first_leaf(tab);
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::Rect;

    #[test]
    fn test_all_workspace_presets_contain_core_tabs() {
        for preset in WorkspacePreset::ALL {
            let state = preset.create_dock_state();
            let tabs: Vec<EditorTab> = state.iter_all_tabs().map(|(_, t)| *t).collect();
            assert!(!tabs.is_empty(), "Preset {:?} must contain tabs", preset);

            // Viewport should exist in all presets except AssetEditing,
            // which is an asset-management workspace without a 3D viewport.
            if preset != WorkspacePreset::AssetEditing {
                assert!(
                    tabs.contains(&EditorTab::Viewport),
                    "Preset {:?} must contain Viewport",
                    preset
                );
            }
        }
    }

    #[test]
    fn test_scripting_preset_contains_script_editor() {
        let state = WorkspacePreset::Scripting.create_dock_state();
        let tabs: Vec<EditorTab> = state.iter_all_tabs().map(|(_, t)| *t).collect();
        assert!(tabs.contains(&EditorTab::ScriptEditor));
        assert!(tabs.contains(&EditorTab::OutputLog));
    }

    #[test]
    fn test_smart_open_tab_behavior() {
        let mut state = WorkspacePreset::Minimal.create_dock_state();
        assert!(state.find_tab(&EditorTab::ScriptEditor).is_none());

        // Opening missing tab adds it
        smart_open_tab(&mut state, EditorTab::ScriptEditor);
        assert!(state.find_tab(&EditorTab::ScriptEditor).is_some());

        // Opening already existing tab activates it without duplication
        let count_before = state
            .iter_all_tabs()
            .filter(|(_, t)| **t == EditorTab::ScriptEditor)
            .count();
        assert_eq!(count_before, 1);
        smart_open_tab(&mut state, EditorTab::ScriptEditor);
        let count_after = state
            .iter_all_tabs()
            .filter(|(_, t)| **t == EditorTab::ScriptEditor)
            .count();
        assert_eq!(count_after, 1);
    }

    #[test]
    fn test_dock_action_close_and_detach() {
        let mut state = WorkspacePreset::Default.create_dock_state();
        assert!(state.find_tab(&EditorTab::OutputLog).is_some());

        let rect = Rect::from_min_size(egui::pos2(100.0, 100.0), egui::vec2(400.0, 300.0));

        // Test Close
        DockAction::Close(EditorTab::OutputLog).apply(&mut state, rect);
        assert!(state.find_tab(&EditorTab::OutputLog).is_none());

        // Test Detach
        assert!(state.find_tab(&EditorTab::Details).is_some());
        DockAction::Detach(EditorTab::Details).apply(&mut state, rect);
        // After detach, the tab is in a secondary surface (floating window)
        assert!(state.find_tab(&EditorTab::Details).is_some());
    }

    #[test]
    fn test_dock_config_ron_round_trip() {
        let config = DockConfig {
            current_workspace: WorkspacePreset::LevelEditing,
            dock_state: WorkspacePreset::LevelEditing.create_dock_state(),
        };

        let serialized = ron::to_string(&config).expect("Failed to serialize DockConfig");
        let deserialized: DockConfig =
            ron::from_str(&serialized).expect("Failed to deserialize DockConfig");

        assert_eq!(
            deserialized.current_workspace,
            WorkspacePreset::LevelEditing
        );
        let orig_tabs: Vec<EditorTab> =
            config.dock_state.iter_all_tabs().map(|(_, t)| *t).collect();
        let deser_tabs: Vec<EditorTab> = deserialized
            .dock_state
            .iter_all_tabs()
            .map(|(_, t)| *t)
            .collect();
        assert_eq!(orig_tabs, deser_tabs);
    }
}
