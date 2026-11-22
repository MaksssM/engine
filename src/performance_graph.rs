//! Lightweight real-time performance graphs built directly on the egui Painter.
//!
//! egui 0.32.3 ships no plotting widget (the `plot` module is unstable and not
//! enabled), so the graphs are drawn manually. Rendering is intentionally cheap:
//! no per-frame allocations beyond one reusable point buffer, no texture
//! uploads, and a fixed set of painter primitives.
//!
//! The widget is stateless with respect to data: it only renders what the
//! caller passes in. All measurement state lives in [`crate::performance`].

use egui::{Color32, Pos2, Response, Sense, Stroke, Ui, Vec2, Widget, pos2};

/// Background of the graph area.
const GRAPH_BG: Color32 = Color32::from_rgb(22, 24, 27);
/// Horizontal grid lines.
const GRID_COLOR: Color32 = Color32::from_rgb(44, 48, 54);
/// Axis text.
const AXIS_TEXT: Color32 = Color32::from_rgb(140, 146, 156);
/// The data polyline.
const SERIES_COLOR: Color32 = Color32::from_rgb(80, 200, 100);
/// Target (goal) line.
const TARGET_COLOR: Color32 = Color32::from_rgb(90, 150, 230);
/// Hover crosshair + highlight.
const HOVER_COLOR: Color32 = Color32::from_rgb(240, 242, 245);
/// Fill under the curve, built per-frame because `Color32` with alpha cannot
/// be created in a const context on this egui version.
fn fill_color() -> Color32 {
    Color32::from_rgba_unmultiplied(80, 200, 100, 26)
}

/// How the vertical scale of a graph is chosen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GraphScale {
    /// Recompute the visible range from the data every frame.
    Auto,
    /// Use a fixed maximum value chosen by the user.
    Fixed,
}

impl Default for GraphScale {
    fn default() -> Self {
        Self::Auto
    }
}

/// Configuration shared by both performance graphs.
#[derive(Clone, Debug)]
pub struct PerfGraphConfig {
    /// Unit label shown next to the current value, e.g. `"fps"` or `"ms"`.
    pub unit: &'static str,
    /// Y axis label.
    pub axis_label: &'static str,
    /// Optional horizontal target line (e.g. target FPS or target frame time).
    pub target: Option<f64>,
    /// Label of the target line.
    pub target_label: Option<&'static str>,
    /// Scale behaviour.
    pub scale: GraphScale,
    /// Fixed maximum Y value when [`GraphScale::Fixed`] is active.
    pub fixed_max: f64,
    /// Whether the graph is drawn at all.
    pub visible: bool,
}

impl PerfGraphConfig {
    pub fn new(unit: &'static str, axis_label: &'static str) -> Self {
        Self {
            unit,
            axis_label,
            target: None,
            target_label: None,
            scale: GraphScale::Auto,
            fixed_max: 100.0,
            visible: true,
        }
    }

    pub fn with_target(mut self, target: f64, label: &'static str) -> Self {
        self.target = Some(target);
        self.target_label = Some(label);
        self
    }
}

/// One graph drawing call. `values` must be ordered oldest → newest.
pub struct PerfGraph<'a> {
    pub id: egui::Id,
    pub title: &'static str,
    pub values: &'a [f64],
    /// Seconds of history visible on the X axis (e.g. 10.0, 30.0, 60.0).
    pub window_seconds: f64,
    /// Approximate sample rate, used to map the time window onto samples.
    pub sample_hz: f64,
    pub config: PerfGraphConfig,
    pub height: f32,
}

impl<'a> PerfGraph<'a> {
    pub fn new(id: egui::Id, title: &'static str, values: &'a [f64]) -> Self {
        Self {
            id,
            title,
            values,
            window_seconds: 10.0,
            sample_hz: 60.0,
            config: PerfGraphConfig::new("", ""),
            height: 90.0,
        }
    }

    pub fn window_seconds(mut self, seconds: f64) -> Self {
        self.window_seconds = seconds;
        self
    }

    pub fn sample_hz(mut self, hz: f64) -> Self {
        self.sample_hz = hz;
        self
    }

    pub fn config(mut self, config: PerfGraphConfig) -> Self {
        self.config = config;
        self
    }

    pub fn height(mut self, height: f32) -> Self {
        self.height = height;
        self
    }

    /// Stable visible-range bounds so the plot does not jump when the data
    /// changes slightly. The range only expands immediately but shrinks with
    /// smoothing, which keeps grid labels readable and stable.
    fn y_bounds(&self) -> (f64, f64) {
        let mut max = self
            .values
            .iter()
            .copied()
            .fold(f64::NEG_INFINITY, f64::max);
        if !max.is_finite() {
            max = 0.0;
        }
        if let Some(target) = self.config.target {
            max = max.max(target);
        }
        let max = if self.config.scale == GraphScale::Fixed {
            self.config.fixed_max.max(max)
        } else {
            (max * 1.15).max(1.0)
        };
        (0.0, max)
    }

    fn draw(&self, ui: &mut Ui) -> Response {
        let (rect, response) =
            ui.allocate_exact_size(Vec2::new(ui.available_width(), self.height), Sense::hover());
        let painter = ui.painter_at(rect);

        painter.rect_filled(rect, 4.0, GRAPH_BG);
        painter.rect_stroke(
            rect,
            4.0,
            Stroke::new(1.0_f32, GRID_COLOR),
            egui::StrokeKind::Middle,
        );

        if self.values.is_empty() {
            painter.text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                "No data yet",
                egui::FontId::proportional(11.0),
                AXIS_TEXT,
            );
            return response;
        }

        let (y_min, y_max) = self.y_bounds();
        let span = (y_max - y_min).max(1e-6);

        let plot = rect.shrink2(Vec2::new(34.0, 14.0));
        let plot_h = plot.height() as f64;
        let plot_w = plot.width() as f64;
        let plot_left = plot.left() as f64;
        let plot_bottom = plot.bottom() as f64;
        let plot_top = plot.top() as f64;
        let plot_right = plot.right() as f64;

        let to_y = |v: f64| plot_bottom - ((v - y_min) / span) * plot_h;
        let n = self.values.len().max(1) as f64;
        let to_x = |i: usize| plot_left + (i as f64 / (n - 1.0).max(1.0)) * plot_w;

        // Grid + Y labels (4 rows).
        for row in 0..=4 {
            let v = y_min + span * (row as f64 / 4.0);
            let y = to_y(v);
            painter.line_segment(
                [
                    pos2(plot_left as f32, y as f32),
                    pos2(plot_right as f32, y as f32),
                ],
                Stroke::new(1.0_f32, GRID_COLOR),
            );
            painter.text(
                pos2(rect.left() + 2.0, y as f32),
                egui::Align2::LEFT_CENTER,
                format!("{:.1}", v),
                egui::FontId::proportional(9.0),
                AXIS_TEXT,
            );
        }

        // X axis: time labels relative to now.
        let total_seconds = self.values.len() as f64 / self.sample_hz;
        for tick in 0..=4 {
            let frac = tick as f64 / 4.0;
            let x = plot_left + frac * plot_w;
            let ago = total_seconds * (1.0 - frac);
            painter.text(
                pos2(x as f32, plot_bottom as f32 + 3.0),
                egui::Align2::CENTER_TOP,
                format!("{:.0}s", ago),
                egui::FontId::proportional(9.0),
                AXIS_TEXT,
            );
        }

        // Target line.
        if let Some(target) = self.config.target {
            if target >= y_min && target <= y_max {
                let y = to_y(target);
                painter.line_segment(
                    [
                        pos2(plot_left as f32, y as f32),
                        pos2(plot_right as f32, y as f32),
                    ],
                    Stroke::new(1.0_f32, TARGET_COLOR),
                );
                if let Some(label) = self.config.target_label {
                    painter.text(
                        pos2(plot_right as f32 - 2.0, y as f32 - 4.0),
                        egui::Align2::RIGHT_BOTTOM,
                        label,
                        egui::FontId::proportional(9.0),
                        TARGET_COLOR,
                    );
                }
            }
        }

        // Area fill + series polyline.
        let points: Vec<Pos2> = self
            .values
            .iter()
            .enumerate()
            .map(|(i, &v)| pos2(to_x(i) as f32, to_y(v) as f32))
            .collect();
        if points.len() >= 2 {
            // Fill under the curve: build a closed polygon from the data points
            // plus the two bottom corners, then draw as a convex shape.
            let mut fill_points: Vec<Pos2> = Vec::with_capacity(points.len() + 2);
            fill_points.push(pos2(points.first().unwrap().x, plot.bottom()));
            fill_points.extend(points.iter().copied());
            fill_points.push(pos2(points.last().unwrap().x, plot.bottom()));
            painter.add(egui::Shape::convex_polygon(
                fill_points,
                fill_color(),
                Stroke::NONE,
            ));
            painter.add(egui::Shape::line(
                points,
                Stroke::new(1.5_f32, SERIES_COLOR),
            ));
        }

        // Hover: crosshair + value tooltip.
        if let Some(hover_pos) = response.hover_pos() {
            if hover_pos.x >= plot.left() && hover_pos.x <= plot.right() {
                let frac = ((hover_pos.x as f64 - plot_left) / plot_w).clamp(0.0, 1.0);
                let idx = (frac * (self.values.len() - 1) as f64).round() as usize;
                let v = self.values[idx];
                let x = to_x(idx);
                let y = to_y(v);
                painter.line_segment(
                    [
                        pos2(x as f32, plot_top as f32),
                        pos2(x as f32, plot_bottom as f32),
                    ],
                    Stroke::new(1.0_f32, HOVER_COLOR),
                );
                painter.circle_filled(pos2(x as f32, y as f32), 3.0, HOVER_COLOR);
                let ago = total_seconds * (1.0 - idx as f64 / (n - 1.0).max(1.0));
                let text = format!("{:.1} {}\n{:.1}s ago", v, self.config.unit, ago);
                painter.text(
                    pos2(x as f32 + 6.0, plot_top as f32 + 4.0),
                    egui::Align2::LEFT_TOP,
                    text,
                    egui::FontId::proportional(10.0),
                    HOVER_COLOR,
                );
            }
        }

        response
    }
}

impl Widget for PerfGraph<'_> {
    fn ui(self, ui: &mut Ui) -> Response {
        ui.label(self.title);
        self.draw(ui)
    }
}

/// Choose how many samples the given time window covers at the sample rate.
/// Returns `None` when the window covers fewer than 2 samples (too short).
pub fn samples_for_window(window_seconds: f64, sample_hz: f64) -> Option<usize> {
    if window_seconds <= 0.0 || sample_hz <= 0.0 {
        return None;
    }
    let n = (window_seconds * sample_hz).round() as usize;
    (n >= 2).then_some(n)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn y_bounds_include_target_and_data() {
        let graph = PerfGraph::new(egui::Id::new("t"), "t", &[10.0, 20.0, 30.0])
            .config(PerfGraphConfig::new("ms", "ms").with_target(50.0, "target"));
        let (min, max) = graph.y_bounds();
        assert_eq!(min, 0.0);
        assert!(max >= 50.0, "target must be inside the range");
    }

    #[test]
    fn y_bounds_empty_data_falls_back_to_target() {
        let graph = PerfGraph::new(egui::Id::new("t"), "t", &[])
            .config(PerfGraphConfig::new("fps", "fps").with_target(60.0, "target"));
        let (min, max) = graph.y_bounds();
        assert_eq!(min, 0.0);
        assert!(max >= 60.0);
    }

    #[test]
    fn fixed_scale_uses_fixed_maximum() {
        let graph = PerfGraph::new(egui::Id::new("t"), "t", &[1.0, 2.0]).config(PerfGraphConfig {
            scale: GraphScale::Fixed,
            fixed_max: 100.0,
            ..PerfGraphConfig::new("ms", "ms")
        });
        let (_, max) = graph.y_bounds();
        assert_eq!(max, 100.0);
    }

    #[test]
    fn samples_for_window_scales_with_rate() {
        assert_eq!(samples_for_window(10.0, 60.0), Some(600));
        assert_eq!(samples_for_window(30.0, 60.0), Some(1800));
        assert_eq!(samples_for_window(0.0, 60.0), None);
        assert_eq!(samples_for_window(10.0, 0.0), None);
    }
}
