//! Content Browser panel: folder tree, asset list with search/filter,
//! list/grid views, create/import actions and context menu.

use std::{fs, path::PathBuf};

use bevy_ecs::world::World;
use egui::{Color32, RichText, Stroke, Vec2};

use crate::{
    assets_ui::{classify, AssetKind},
    editor::Editor,
    feedback::TooltipExt,
    icons::{IconSize, Icons},
};

/// Render the full Content Browser panel into `ui`.
pub fn draw_content_browser(editor: &mut Editor, ui: &mut egui::Ui, world: &mut World) {
    let _ = fs::create_dir_all(&editor.content_root);

    // Toolbar: view toggle + import
    ui.horizontal(|ui| {
        ui.heading("CONTENT BROWSER");
        if ui
            .button(
                RichText::new(Icons::LIST_VIEW)
                    .size(14.0)
                    .color(if editor.content_list_view {
                        Color32::WHITE
                    } else {
                        Color32::from_rgb(140, 145, 155)
                    }),
            )
            .engine_tooltip("List View", "Show assets in a compact list", None)
            .clicked()
        {
            editor.content_list_view = true;
        }
        if ui
            .button(
                RichText::new(Icons::GRID_VIEW)
                    .size(14.0)
                    .color(if !editor.content_list_view {
                        Color32::WHITE
                    } else {
                        Color32::from_rgb(140, 145, 155)
                    }),
            )
            .engine_tooltip("Grid View", "Show assets as thumbnails in a grid", None)
            .clicked()
        {
            editor.content_list_view = false;
        }
        ui.separator();
        if ui
            .button(RichText::new(Icons::IMPORT).size(13.0))
            .engine_tooltip("Import", "Import a file from disk into the current folder", None)
            .clicked()
        {
            editor.import_dummy_resource(&editor.content_dir.clone());
        }
    });

    // Navigation: back / forward / root
    ui.horizontal(|ui| {
        if ui
            .add_enabled(
                editor.history_index > 0,
                egui::Button::new(RichText::new(Icons::BACK).size(13.0)),
            )
            .engine_tooltip("Back", "Return to the previous folder", Some("Alt+Left"))
            .clicked()
        {
            editor.history_index -= 1;
            editor.content_dir = editor.content_history[editor.history_index].clone();
        }
        if ui
            .add_enabled(
                editor.history_index + 1 < editor.content_history.len(),
                egui::Button::new(RichText::new(Icons::FORWARD).size(13.0)),
            )
            .engine_tooltip("Forward", "Return to the next folder", Some("Alt+Right"))
            .clicked()
        {
            editor.history_index += 1;
            editor.content_dir = editor.content_history[editor.history_index].clone();
        }
        if ui
            .button(RichText::new(Icons::HOME).size(13.0))
            .engine_tooltip("Content Root", "Navigate to the root content folder", None)
            .clicked()
        {
            editor.navigate_content(editor.content_root.clone());
        }
    });

    // Breadcrumb
    let relative_parts: Vec<String> = editor
        .content_dir
        .strip_prefix(&editor.content_root)
        .unwrap_or(std::path::Path::new(""))
        .components()
        .map(|part| part.as_os_str().to_string_lossy().to_string())
        .collect();
    ui.horizontal_wrapped(|ui| {
        let mut target = editor.content_root.clone();
        if ui
            .selectable_label(relative_parts.is_empty(), "Content")
            .clicked()
        {
            editor.navigate_content(target.clone());
        }
        for part in &relative_parts {
            target.push(part);
            ui.label("›");
            if ui.selectable_label(false, part).clicked() {
                editor.navigate_content(target.clone());
            }
        }
    });

    // Search
    ui.horizontal(|ui| {
        Icons::show(
            ui,
            Icons::SEARCH,
            IconSize::Small,
            Some(Color32::from_rgb(120, 135, 155)),
        );
        ui.add(
            egui::TextEdit::singleline(&mut editor.filter)
                .hint_text("Search assets…")
                .desired_width(f32::INFINITY),
        );
        if !editor.filter.is_empty() && ui.small_button("×").clicked() {
            editor.filter.clear();
        }
    });

    // Type filter + Create menu
    ui.horizontal(|ui| {
        egui::ComboBox::from_id_salt("content_type_filter")
            .selected_text(&editor.content_type_filter)
            .show_ui(ui, |ui| {
                for kind in [
                    "All types",
                    "Folders",
                    "Scenes",
                    "Materials",
                    "Textures",
                    "Models",
                    "Scripts",
                ] {
                    ui.selectable_value(&mut editor.content_type_filter, kind.into(), kind);
                }
            });
        ui.menu_button(format!("{} Create", Icons::ADD), |ui| {
            if ui.button(format!("{} Folder", Icons::FOLDER)).clicked() {
                editor.create_content_folder();
                ui.close();
            }
            for (label, ext) in [("Scene", "ron"), ("Material", "mat"), ("Script", "rs")] {
                if ui.button(format!("{} {}", Icons::FILE, label)).clicked() {
                    editor.create_content_file(ext);
                    ui.close();
                }
            }
        });
    });
    ui.separator();

    // Keyboard shortcuts for the selected asset (F2 rename, Delete delete, Ctrl+D duplicate)
    if let Some(selected) = editor.content_selected.clone() {
        if ui.input(|i| i.key_pressed(egui::Key::F2)) {
            editor.rename_content(&selected);
        }
        if ui.input(|i| i.key_pressed(egui::Key::Delete)) {
            editor.prompt_delete_resource(selected.clone());
        }
        if ui.input(|i| i.modifiers.ctrl && i.key_pressed(egui::Key::D)) {
            editor.duplicate_content(&selected);
        }
    }

        // F2 on current folder (when no asset selected)
        if editor.content_selected.is_none() && ui.input(|i| i.key_pressed(egui::Key::F2)) {
            if editor.content_dir != editor.content_root {
                let dir = editor.content_dir.clone();
                editor.rename_content(&dir);
            }
        }

    // Main split: folder tree on the left, asset list on the right
    let tree_rect = ui.available_rect_before_wrap();
    let tree_width = 140.0_f32.min(tree_rect.width() * 0.35).max(80.0);
    let list_width = tree_rect.width() - tree_width;

    let mut tree_ui = ui.child_ui(
        egui::Rect::from_min_size(ui.cursor().min, Vec2::new(tree_width, ui.available_height())),
        *ui.layout(),
            None,
    );
    draw_folder_tree(editor, &mut tree_ui);

    let list_x = ui.cursor().min.x + tree_width;
    let mut list_ui = ui.child_ui(
        egui::Rect::from_min_size(
            egui::pos2(list_x, ui.cursor().min.y),
            Vec2::new(list_width, ui.available_height()),
        ),
        *ui.layout(),
            None,
    );
    draw_asset_list(editor, &mut list_ui, world);

    if editor.content_selected.as_ref().is_some_and(|p| !p.exists()) {
        editor.content_selected = None;
    }
}

fn draw_folder_tree(editor: &mut Editor, ui: &mut egui::Ui) {
    ui.label(RichText::new("Folders").size(10.0).color(Color32::from_rgb(120, 135, 155)));
    ui.separator();
    egui::ScrollArea::vertical().show(ui, |ui| {
        let root = crate::assets_ui::build_folder_tree(&editor.content_root, 8);
        draw_tree_node(editor, ui, &root, 0);
    });
}

fn draw_tree_node(
    editor: &mut Editor,
    ui: &mut egui::Ui,
    node: &crate::assets_ui::FolderNode,
    depth: usize,
) {
    let indent = 8.0 + depth as f32 * 12.0;
    let is_current = editor.content_dir == node.path;

    // Inline rename for folders
    let renaming_this = editor
        .renaming
        .as_ref()
        .is_some_and(|(p, _)| p == &node.path);
    if renaming_this {
        ui.add_space(indent);
        let (_, text) = editor.renaming.as_mut().unwrap();
        let response = ui.text_edit_singleline(text);
        if response.lost_focus() || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            editor.commit_rename(false);
        }
        if response.has_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
            editor.commit_rename(true);
        }
        return;
    }

    let response = ui.selectable_label(is_current, format!("{} {}", Icons::FOLDER, node.name));
    if response.clicked() {
        editor.navigate_content(node.path.clone());
    }
    response.context_menu(|ui| {
        editor.content_context_menu(ui, &node.path);
    });
    if !node.children.is_empty() {
        ui.add_space(indent - 8.0);
        for child in &node.children {
            draw_tree_node(editor, ui, child, depth + 1);
        }
    }
}

fn draw_asset_list(editor: &mut Editor, ui: &mut egui::Ui, world: &mut World) {
    ui.label(RichText::new("Assets").size(10.0).color(Color32::from_rgb(120, 135, 155)));
    ui.separator();

    let mut entries: Vec<_> = fs::read_dir(&editor.content_dir)
        .ok()
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .collect();
    entries.sort_by_key(|entry| {
        (
            !entry.path().is_dir(),
            entry.file_name().to_string_lossy().to_lowercase(),
        )
    });

    let query = editor.filter.to_lowercase();
    let matches = |path: &std::path::Path| {
        let name = path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_lowercase();
        if !name.contains(&query) {
            return false;
        }
        let is_dir = path.is_dir();
        let ext = path
            .extension()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_lowercase();
        match editor.content_type_filter.as_str() {
            "Folders" => is_dir,
            "Scenes" => ext == "ron" || ext == "scene",
            "Materials" => ext == "mat" || ext == "material",
            "Textures" => ["png", "jpg", "jpeg", "bmp", "tga", "dds"].contains(&ext.as_str()),
            "Models" => ["obj", "fbx", "gltf", "glb"].contains(&ext.as_str()),
            "Scripts" => ["rs", "lua", "py"].contains(&ext.as_str()),
            _ => true,
        }
    };
    let filtered: Vec<PathBuf> = entries
        .into_iter()
        .map(|entry| entry.path())
        .filter(|path| matches(path))
        .collect();

    egui::ScrollArea::vertical().show(ui, |ui| {
        if editor.content_list_view {
            for path in filtered {
                let name = path
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_string();
                let kind = classify(&path);
                let is_dir = path.is_dir();
                let icon = content_icon(&path);

                // Inline rename editor
                let renaming_this = editor
                    .renaming
                    .as_ref()
                    .is_some_and(|(p, _)| p == &path);
                if renaming_this {
                                    let (_, text) = editor.renaming.as_mut().unwrap();
                    let response = ui.text_edit_singleline(text);
                    if response.lost_focus() || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                        editor.commit_rename(false);
                    }
                    if response.has_focus()
                        && ui.input(|i| i.key_pressed(egui::Key::Enter))
                    {
                        editor.commit_rename(true);
                    }
                    ui.label(format!("{}  · {}", icon, kind.label()));
                    continue;
                }

                // Use dnd_drag_source for drag & drop support
                let item_id = egui::Id::new(format!("content_item_{}", path.display()));
                let drag_result = ui.dnd_drag_source(item_id, path.clone(), |ui| {
                    let selected = editor.content_selected.as_ref() == Some(&path);
                    let response = ui.selectable_label(
                        selected,
                        format!("{}  {}   · {}", icon, name, kind.label()),
                    );
                    if response.clicked() {
                        editor.content_selected = Some(path.clone());
                        if is_dir {
                            editor.navigate_content(path.clone());
                        }
                    }
                    if response.double_clicked() {
                        editor.open_asset(world, &path);
                    }
                                    response
                });
                drag_result.inner.context_menu(|ui| editor.content_context_menu(ui, &path));
            }
        } else {
            let columns = ((ui.available_width() / 92.0).floor() as usize).max(1);
            egui::Grid::new("content_grid")
                .num_columns(columns)
                .spacing([6.0, 8.0])
                .show(ui, |ui| {
                    for (index, path) in filtered.iter().enumerate() {
                        let name = path
                            .file_name()
                            .unwrap_or_default()
                            .to_string_lossy()
                            .to_string();
                        let renaming_this = editor
                            .renaming
                            .as_ref()
                            .is_some_and(|(p, _)| p == path);
                        if renaming_this {
                                                    let (_, text) = editor.renaming.as_mut().unwrap();
                            let response = ui.text_edit_singleline(text);
                            if response.lost_focus()
                                || ui.input(|i| i.key_pressed(egui::Key::Escape))
                            {
                                editor.commit_rename(false);
                            }
                            if response.has_focus()
                                && ui.input(|i| i.key_pressed(egui::Key::Enter))
                            {
                                editor.commit_rename(true);
                            }
                            continue;
                        }
                        let item_id = egui::Id::new(format!("content_grid_item_{}", path.display()));
                        let path_clone = path.clone();
                        let drag_result = ui.dnd_drag_source(item_id, path_clone, |ui| {
                            let selected = editor.content_selected.as_ref() == Some(path);
                            let fill = if selected {
                                Color32::from_rgb(52, 76, 102)
                            } else {
                                Color32::from_rgb(39, 41, 46)
                            };
                            let border = if selected {
                                Color32::from_rgb(82, 166, 235)
                            } else {
                                Color32::from_rgb(50, 53, 58)
                            };
                            let frame_response = egui::Frame::new()
                                .fill(fill)
                                .corner_radius(4)
                                .inner_margin(6.0)
                                .stroke(Stroke::new(1.0, border))
                                .show(ui, |ui| {
                                    ui.set_min_width(68.0);
                                    ui.set_min_height(70.0);
                                    ui.vertical_centered(|ui| {
                                        ui.label(
                                            RichText::new(content_icon(path)).size(29.0),
                                        );
                                        ui.label(egui::RichText::new(name).size(11.0));
                                        ui.label(
                                            RichText::new(classify(path).label())
                                                .size(9.0)
                                                .color(Color32::GRAY),
                                        );
                                    });
                                });
                            if frame_response.response.clicked() {
                                editor.content_selected = Some(path.clone());
                                if path.is_dir() {
                                    editor.navigate_content(path.clone());
                                }
                            }
                            if frame_response.response.double_clicked() {
                                editor.open_asset(world, path);
                            }
                                                        frame_response.response
                        });
                        drag_result.inner.context_menu(|ui| editor.content_context_menu(ui, path));
                        if (index + 1) % columns == 0 {
                            ui.end_row();
                        }
                    }
                });
        }
    });

    // Drag & drop target: allow dropping assets onto the current folder to move them
    let _ = world;
    let (_, dropped) = ui.dnd_drop_zone::<PathBuf, _>(
        egui::Frame::none(),
            |_| {
            // invisible target area
        },
    );
        if let Some(arc_path) = dropped {
            let dropped_path = arc_path.as_ref();
            let dest = editor.content_dir.clone();
            if let Some(file_name) = dropped_path.file_name() {
                let target = dest.join(file_name);
                if target != *dropped_path {
                    if let Err(e) = fs::rename(dropped_path, &target) {
                        editor.log.push((
                            "ERROR".into(),
                            format!("Move failed: {e}"),
                        ));
                    } else {
                        editor.log.push((
                            "INFO".into(),
                            format!("Moved '{}'", target.file_name().unwrap_or_default().to_string_lossy()),
                        ));
                }
            }
        }
    }
}

/// Icon for a given path based on its classified kind.
pub fn content_icon(path: &std::path::Path) -> &'static str {
    match classify(path) {
        AssetKind::Folder => Icons::FOLDER,
        AssetKind::Scene => Icons::SCENE,
        AssetKind::Texture => Icons::TEXTURE,
        AssetKind::Model => Icons::MODEL,
        AssetKind::Material => Icons::MATERIAL,
        AssetKind::Script => Icons::SCRIPT,
        AssetKind::Shader => Icons::SHADER,
        AssetKind::Audio => Icons::AUDIO,
        AssetKind::Unsupported => Icons::FILE,
    }
}
