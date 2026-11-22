use bevy_ecs::entity::Entity;
use egui::{
    Align, Align2, Area, Color32, Context, CornerRadius, Frame, Id, Key, Layout, Order, Rect,
    Response, RichText, Stroke, Ui, Vec2,
};
use std::{path::PathBuf, time::Instant};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NotificationLevel {
    Success,
    Info,
    Warning,
    Error,
}

impl NotificationLevel {
    pub fn icon(&self) -> &'static str {
        match self {
            Self::Success => "✓",
            Self::Info => "ℹ",
            Self::Warning => "⚠",
            Self::Error => "✕",
        }
    }

    pub fn color(&self) -> Color32 {
        match self {
            Self::Success => Color32::from_rgb(60, 180, 100),
            Self::Info => Color32::from_rgb(70, 145, 230),
            Self::Warning => Color32::from_rgb(235, 175, 45),
            Self::Error => Color32::from_rgb(235, 65, 65),
        }
    }

    pub fn bg_color(&self) -> Color32 {
        match self {
            Self::Success => Color32::from_rgb(25, 45, 32),
            Self::Info => Color32::from_rgb(24, 38, 54),
            Self::Warning => Color32::from_rgb(48, 38, 20),
            Self::Error => Color32::from_rgb(48, 22, 22),
        }
    }
}

#[derive(Clone, Debug)]
pub struct Notification {
    pub id: u64,
    pub level: NotificationLevel,
    pub title: String,
    pub message: String,
    pub created_at: Instant,
    pub duration: f32,
}

#[derive(Default)]
pub struct NotificationManager {
    pub notifications: Vec<Notification>,
    next_id: u64,
}

impl NotificationManager {
    pub fn push(
        &mut self,
        level: NotificationLevel,
        title: impl Into<String>,
        message: impl Into<String>,
    ) {
        let notif = Notification {
            id: self.next_id,
            level,
            title: title.into(),
            message: message.into(),
            created_at: Instant::now(),
            duration: 4.5,
        };
        self.next_id = self.next_id.wrapping_add(1);
        self.notifications.push(notif);

        // Cap to latest 5 to avoid overcrowding
        if self.notifications.len() > 5 {
            self.notifications.remove(0);
        }
    }

    pub fn success(&mut self, title: impl Into<String>, message: impl Into<String>) {
        self.push(NotificationLevel::Success, title, message);
    }

    pub fn info(&mut self, title: impl Into<String>, message: impl Into<String>) {
        self.push(NotificationLevel::Info, title, message);
    }

    pub fn warning(&mut self, title: impl Into<String>, message: impl Into<String>) {
        self.push(NotificationLevel::Warning, title, message);
    }

    pub fn error(&mut self, title: impl Into<String>, message: impl Into<String>) {
        self.push(NotificationLevel::Error, title, message);
    }

    pub fn update(&mut self) {
        self.notifications
            .retain(|n| n.created_at.elapsed().as_secs_f32() < n.duration);
    }

    pub fn dismiss(&mut self, id: u64) {
        self.notifications.retain(|n| n.id != id);
    }

    pub fn show(&mut self, ctx: &Context, bottom_offset: f32) {
        self.update();
        if self.notifications.is_empty() {
            return;
        }

        ctx.request_repaint();

        let toast_width = 320.0;
        let margin = 16.0;

        let mut to_dismiss = None;

        Area::new(Id::new("toast_notification_area"))
            .order(Order::Foreground)
            .anchor(
                Align2::RIGHT_BOTTOM,
                Vec2::new(-margin, -bottom_offset - margin),
            )
            .show(ctx, |ui| {
                ui.set_width(toast_width);
                ui.with_layout(Layout::bottom_up(Align::Max), |ui| {
                    for notif in self.notifications.iter().rev() {
                        let age = notif.created_at.elapsed().as_secs_f32();
                        let remaining = (notif.duration - age).max(0.0);
                        let alpha = if remaining < 0.5 {
                            (remaining / 0.5).clamp(0.0, 1.0)
                        } else if age < 0.2 {
                            (age / 0.2).clamp(0.0, 1.0)
                        } else {
                            1.0
                        };

                        let border_color = notif.level.color();
                        let bg_color = Color32::from_rgb(26, 28, 33);

                        Frame::new()
                            .fill(bg_color.gamma_multiply(alpha))
                            .stroke(Stroke::new(1.0_f32, border_color.gamma_multiply(alpha)))
                            .corner_radius(CornerRadius::same(5))
                            .inner_margin(8.0)
                            .show(ui, |ui| {
                                ui.set_width(toast_width);
                                ui.horizontal(|ui| {
                                    // Accent level badge
                                    let badge = RichText::new(format!(" {} ", notif.level.icon()))
                                        .color(Color32::WHITE)
                                        .background_color(notif.level.color())
                                        .strong();
                                    ui.label(badge);

                                    ui.vertical(|ui| {
                                        ui.horizontal(|ui| {
                                            ui.label(
                                                RichText::new(&notif.title)
                                                    .strong()
                                                    .color(Color32::WHITE)
                                                    .size(12.5),
                                            );
                                            ui.with_layout(
                                                Layout::right_to_left(Align::Center),
                                                |ui| {
                                                    if ui
                                                        .add(egui::Button::new("✕").frame(false))
                                                        .on_hover_text("Dismiss")
                                                        .clicked()
                                                    {
                                                        to_dismiss = Some(notif.id);
                                                    }
                                                },
                                            );
                                        });

                                        if !notif.message.is_empty() {
                                            ui.label(
                                                RichText::new(&notif.message)
                                                    .color(Color32::from_rgb(180, 185, 195))
                                                    .size(11.0),
                                            );
                                        }
                                    });
                                });

                                // Progress bar indicating time left
                                let progress = remaining / notif.duration;
                                let bar_rect = Rect::from_min_size(
                                    ui.cursor().min,
                                    Vec2::new(ui.available_width() * progress, 2.0),
                                );
                                ui.painter().rect_filled(
                                    bar_rect,
                                    CornerRadius::same(1),
                                    border_color,
                                );
                                ui.add_space(2.0);
                            });

                        ui.add_space(6.0);
                    }
                });
            });

        if let Some(id) = to_dismiss {
            self.dismiss(id);
        }
    }
}

pub fn apply_feedback_styles(ctx: &Context) {
    ctx.style_mut(|style| {
        style.interaction.tooltip_delay = 0.35;
        // Fine-tune interaction visual states
        style.visuals.widgets.hovered.bg_stroke =
            Stroke::new(1.0_f32, Color32::from_rgb(82, 166, 235));
        style.visuals.widgets.active.bg_stroke =
            Stroke::new(1.5_f32, Color32::from_rgb(100, 190, 255));
        style.visuals.widgets.inactive.bg_stroke =
            Stroke::new(1.0_f32, Color32::from_rgb(70, 74, 80));
        style.visuals.widgets.open.bg_stroke =
            Stroke::new(1.0_f32, Color32::from_rgb(82, 166, 235));
        style.visuals.selection.bg_fill = Color32::from_rgb(38, 79, 120);
        style.visuals.selection.stroke = Stroke::new(1.0_f32, Color32::from_rgb(82, 166, 235));
        // Disabled text should be clearly muted
        style.visuals.widgets.inactive.fg_stroke =
            Stroke::new(1.0_f32, Color32::from_rgb(110, 115, 125));
    });
}

pub trait TooltipExt {
    fn engine_tooltip(
        self,
        title: &str,
        description: impl AsRef<str>,
        shortcut: Option<&str>,
    ) -> Self;
}

impl TooltipExt for Response {
    fn engine_tooltip(
        self,
        title: &str,
        description: impl AsRef<str>,
        shortcut: Option<&str>,
    ) -> Self {
        self.on_hover_ui(|ui| {
            render_tooltip_content(ui, title, description, shortcut);
        })
    }
}

pub fn render_tooltip_content(
    ui: &mut Ui,
    title: &str,
    description: impl AsRef<str>,
    shortcut: Option<&str>,
) {
    ui.set_max_width(280.0);
    ui.horizontal(|ui| {
        ui.label(
            RichText::new(title)
                .strong()
                .color(Color32::from_rgb(230, 235, 245))
                .size(12.5),
        );
        if let Some(key) = shortcut {
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let badge = RichText::new(format!(" {key} "))
                    .size(10.5)
                    .color(Color32::from_rgb(180, 205, 240))
                    .background_color(Color32::from_rgb(42, 46, 54));
                ui.label(badge);
            });
        }
    });

    let desc = description.as_ref();
    if !desc.is_empty() {
        ui.add_space(2.0);
        ui.label(
            RichText::new(desc)
                .color(Color32::from_rgb(165, 172, 185))
                .size(11.0),
        );
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PendingAction {
    LoadScene,
    NewScene,
    ResetLayout,
}

#[derive(Clone, Debug)]
pub enum ConfirmDialog {
    DeleteEntities {
        targets: Vec<Entity>,
        names: Vec<String>,
    },
    DeleteContent {
        path: PathBuf,
        is_dir: bool,
    },
    UnsavedChanges {
        action: PendingAction,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DialogResult {
    Confirmed,
    Cancelled,
    SaveAndProceed,
    DiscardAndProceed,
}

impl ConfirmDialog {
    pub fn delete_entities(targets: Vec<Entity>, names: Vec<String>) -> Self {
        Self::DeleteEntities { targets, names }
    }

    pub fn delete_content(path: PathBuf) -> Self {
        let is_dir = path.is_dir();
        Self::DeleteContent { path, is_dir }
    }

    pub fn unsaved_changes(action: PendingAction) -> Self {
        Self::UnsavedChanges { action }
    }

    pub fn show(&self, ctx: &Context) -> Option<DialogResult> {
        let mut result = None;

        // Dim backdrop
        let screen_rect = ctx.content_rect();
        ctx.layer_painter(egui::LayerId::new(Order::Middle, Id::new("modal_backdrop")))
            .rect_filled(
                screen_rect,
                CornerRadius::ZERO,
                Color32::from_rgba_unmultiplied(10, 12, 16, 170),
            );

        let title = match self {
            Self::DeleteEntities { targets, .. } => {
                if targets.len() == 1 {
                    "Delete Actor"
                } else {
                    "Delete Actors"
                }
            }
            Self::DeleteContent { is_dir, .. } => {
                if *is_dir {
                    "Delete Folder"
                } else {
                    "Delete File"
                }
            }
            Self::UnsavedChanges { .. } => "Unsaved Scene Changes",
        };

        egui::Window::new(title)
            .id(Id::new("confirm_dialog_window"))
            .collapsible(false)
            .resizable(false)
            .anchor(Align2::CENTER_CENTER, Vec2::ZERO)
            .frame(
                Frame::new()
                    .fill(Color32::from_rgb(32, 34, 40))
                    .stroke(Stroke::new(1.0_f32, Color32::from_rgb(60, 65, 75)))
                    .corner_radius(CornerRadius::same(6))
                    .inner_margin(16.0),
            )
            .order(Order::Foreground)
            .show(ctx, |ui| {
                ui.set_min_width(360.0);
                ui.set_max_width(420.0);

                match self {
                    Self::DeleteEntities { targets, names } => {
                        ui.horizontal(|ui| {
                            ui.label(RichText::new("⚠").size(24.0).color(Color32::from_rgb(235, 80, 80)));
                            ui.vertical(|ui| {
                                if targets.len() == 1 {
                                    let name = names.first().map(|s| s.as_str()).unwrap_or("selected actor");
                                    ui.label(RichText::new(format!("Are you sure you want to delete '{name}'?")).strong());
                                } else {
                                    ui.label(RichText::new(format!("Are you sure you want to delete {} actors?", targets.len())).strong());
                                    ui.label(RichText::new(names.join(", ")).size(11.0).color(Color32::GRAY));
                                }
                                ui.label(RichText::new("This action removes the entity from the active scene.").size(11.0).color(Color32::from_rgb(160, 165, 175)));
                            });
                        });

                        ui.add_space(14.0);
                        ui.separator();
                        ui.add_space(6.0);

                        ui.horizontal(|ui| {
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                let delete_btn = egui::Button::new(
                                    RichText::new("Delete").color(Color32::WHITE),
                                )
                                .fill(Color32::from_rgb(180, 40, 40));
                                if ui.add(delete_btn).clicked() {
                                    result = Some(DialogResult::Confirmed);
                                }
                                if ui.button("Cancel").clicked() {
                                    result = Some(DialogResult::Cancelled);
                                }
                            });
                        });
                    }
                    Self::DeleteContent { path, is_dir } => {
                        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("resource");
                        ui.horizontal(|ui| {
                            ui.label(RichText::new("⚠").size(24.0).color(Color32::from_rgb(235, 80, 80)));
                            ui.vertical(|ui| {
                                ui.label(RichText::new(format!("Permanently delete '{name}'?")).strong());
                                if *is_dir {
                                    ui.label(RichText::new("All files inside this folder will be deleted permanently.").size(11.0).color(Color32::from_rgb(235, 120, 120)));
                                } else {
                                    ui.label(RichText::new("This file will be deleted from disk.").size(11.0).color(Color32::from_rgb(160, 165, 175)));
                                }
                            });
                        });

                        ui.add_space(14.0);
                        ui.separator();
                        ui.add_space(6.0);

                        ui.horizontal(|ui| {
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                let delete_btn = egui::Button::new(
                                    RichText::new("Delete Permanently").color(Color32::WHITE),
                                )
                                .fill(Color32::from_rgb(180, 40, 40));
                                if ui.add(delete_btn).clicked() {
                                    result = Some(DialogResult::Confirmed);
                                }
                                if ui.button("Cancel").clicked() {
                                    result = Some(DialogResult::Cancelled);
                                }
                            });
                        });
                    }
                    Self::UnsavedChanges { action } => {
                        ui.horizontal(|ui| {
                            ui.label(RichText::new("●").size(24.0).color(Color32::from_rgb(235, 175, 45)));
                            ui.vertical(|ui| {
                                ui.label(RichText::new("Scene has unsaved changes!").strong());
                                let action_desc = match action {
                                    PendingAction::LoadScene => "reload the scene",
                                    PendingAction::NewScene => "create a new scene",
                                    PendingAction::ResetLayout => "reset editor",
                                };
                                ui.label(RichText::new(format!("Do you want to save before you {action_desc}?")).size(11.5));
                            });
                        });

                        ui.add_space(14.0);
                        ui.separator();
                        ui.add_space(6.0);

                        ui.horizontal(|ui| {
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                let save_btn = egui::Button::new(
                                    RichText::new("Save & Continue").color(Color32::WHITE),
                                )
                                .fill(Color32::from_rgb(45, 120, 75));
                                if ui.add(save_btn).clicked() {
                                    result = Some(DialogResult::SaveAndProceed);
                                }

                                let discard_btn = egui::Button::new(
                                    RichText::new("Discard").color(Color32::from_rgb(230, 200, 200)),
                                )
                                .fill(Color32::from_rgb(110, 40, 40));
                                if ui.add(discard_btn).clicked() {
                                    result = Some(DialogResult::DiscardAndProceed);
                                }

                                if ui.button("Cancel").clicked() {
                                    result = Some(DialogResult::Cancelled);
                                }
                            });
                        });
                    }
                }

                // Keyboard shortcuts for modal
                ctx.input(|i| {
                    if i.key_pressed(Key::Escape) {
                        result = Some(DialogResult::Cancelled);
                    }
                });
            });

        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_notification_manager_push_and_limit() {
        let mut mgr = NotificationManager::default();

        mgr.success("Title 1", "Msg 1");
        mgr.info("Title 2", "Msg 2");
        mgr.warning("Title 3", "Msg 3");
        mgr.error("Title 4", "Msg 4");
        mgr.info("Title 5", "Msg 5");
        assert_eq!(mgr.notifications.len(), 5);

        // Push 6th notification: should cap at 5 and remove oldest
        mgr.success("Title 6", "Msg 6");
        assert_eq!(mgr.notifications.len(), 5);
        assert_eq!(mgr.notifications[0].title, "Title 2");
        assert_eq!(mgr.notifications[4].title, "Title 6");
    }

    #[test]
    fn test_notification_expiry() {
        let mut mgr = NotificationManager::default();

        mgr.success("Save", "Scene saved");
        assert_eq!(mgr.notifications.len(), 1);

        // Not expired immediately
        mgr.update();
        assert_eq!(mgr.notifications.len(), 1);

        // Manually adjust duration to 0 to simulate expiration
        mgr.notifications[0].duration = 0.0;
        mgr.update();
        assert_eq!(mgr.notifications.len(), 0);
    }

    #[test]
    fn test_notification_dismiss() {
        let mut mgr = NotificationManager::default();

        mgr.info("Test", "Dismiss me");
        let id = mgr.notifications[0].id;
        mgr.dismiss(id);
        assert_eq!(mgr.notifications.len(), 0);
    }

    #[test]
    fn test_notification_levels() {
        assert_eq!(NotificationLevel::Success.icon(), "✓");
        assert_eq!(NotificationLevel::Info.icon(), "ℹ");
        assert_eq!(NotificationLevel::Warning.icon(), "⚠");
        assert_eq!(NotificationLevel::Error.icon(), "✕");
    }
}
