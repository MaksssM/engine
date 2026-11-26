//! Repeatable benchmark scenarios and result export.
//!
//! Scenarios describe scene complexity, not renderer rewrites. A run records
//! frames from the existing [`crate::performance::PerformanceMonitor`] and
//! summarizes measured wall-clock and GPU timestamp data.

use serde::Serialize;

use crate::performance::{FrameSnapshot, PerformanceMonitor};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub enum BenchmarkScenario {
    Empty,
    Small,
    Medium,
    Large,
    MeshHeavy,
    DrawCallHeavy,
    Pbr,
}

impl BenchmarkScenario {
    pub const ALL: [Self; 7] = [
        Self::Empty,
        Self::Small,
        Self::Medium,
        Self::Large,
        Self::MeshHeavy,
        Self::DrawCallHeavy,
        Self::Pbr,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Self::Empty => "empty",
            Self::Small => "small",
            Self::Medium => "medium",
            Self::Large => "large",
            Self::MeshHeavy => "mesh_heavy",
            Self::DrawCallHeavy => "draw_call_heavy",
            Self::Pbr => "pbr",
        }
    }

    /// Target entity count. The caller constructs the scene; this is the
    /// deterministic size the benchmark expects.
    pub fn target_entities(self) -> u32 {
        match self {
            Self::Empty => 0,
            Self::Small => 32,
            Self::Medium => 256,
            Self::Large => 2048,
            Self::MeshHeavy => 512,
            Self::DrawCallHeavy => 4096,
            Self::Pbr => 64,
        }
    }

    pub fn description(self) -> &'static str {
        match self {
            Self::Empty => "No renderables. Baseline frame and present cost.",
            Self::Small => "Sparse scene for per-frame overhead.",
            Self::Medium => "Typical editor scene.",
            Self::Large => "Many entities, shared meshes.",
            Self::MeshHeavy => "High triangle count, fewer objects.",
            Self::DrawCallHeavy => "Many unique mesh/material pairs.",
            Self::Pbr => "Representative PBR material mix.",
        }
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct BenchmarkResult {
    pub scenario: BenchmarkScenario,
    pub frames: usize,
    pub warmup_frames: usize,
    pub average_fps: f64,
    pub p50_frame_ms: f64,
    pub p95_frame_ms: f64,
    pub p99_frame_ms: f64,
    pub average_cpu_ms: f64,
    pub average_gpu_ms: Option<f64>,
    pub average_draw_calls: f64,
    pub average_triangles: f64,
}

impl BenchmarkResult {
    pub fn from_snapshots(
        scenario: BenchmarkScenario,
        warmup_frames: usize,
        samples: &[FrameSnapshot],
    ) -> Self {
        let frames = samples.len();
        if frames == 0 {
            return Self {
                scenario,
                frames: 0,
                warmup_frames,
                average_fps: 0.0,
                p50_frame_ms: 0.0,
                p95_frame_ms: 0.0,
                p99_frame_ms: 0.0,
                average_cpu_ms: 0.0,
                average_gpu_ms: None,
                average_draw_calls: 0.0,
                average_triangles: 0.0,
            };
        }
        let mean = |values: &[f64]| values.iter().sum::<f64>() / frames as f64;
        let frame_times: Vec<f64> = samples.iter().map(|s| s.frame_time_ms).collect();
        let cpu: Vec<f64> = samples.iter().map(|s| s.cpu_frame_time_ms).collect();
        let gpu_values: Vec<f64> = samples.iter().filter_map(|s| s.gpu_frame_time_ms).collect();
        let average_gpu_ms = if gpu_values.is_empty() {
            None
        } else {
            Some(gpu_values.iter().sum::<f64>() / gpu_values.len() as f64)
        };
        let average_fps = if frame_times.iter().sum::<f64>() > 0.0 {
            frames as f64 * 1000.0 / frame_times.iter().sum::<f64>()
        } else {
            0.0
        };
        Self {
            scenario,
            frames,
            warmup_frames,
            average_fps,
            p50_frame_ms: crate::performance::calculate_percentile(&frame_times, 0.50),
            p95_frame_ms: crate::performance::calculate_percentile(&frame_times, 0.95),
            p99_frame_ms: crate::performance::calculate_percentile(&frame_times, 0.99),
            average_cpu_ms: mean(&cpu),
            average_gpu_ms,
            average_draw_calls: samples.iter().map(|s| s.draw_calls as f64).sum::<f64>()
                / frames as f64,
            average_triangles: samples.iter().map(|s| s.triangles as f64).sum::<f64>()
                / frames as f64,
        }
    }
}

/// Collect measured frames after `warmup_frames`. Returns the samples and the
/// summary. Does not fabricate timings when the monitor is disabled.
pub fn run_scenario(
    scenario: BenchmarkScenario,
    monitor: &mut PerformanceMonitor,
    warmup_frames: usize,
    measure_frames: usize,
    mut step: impl FnMut(),
) -> BenchmarkResult {
    monitor.set_execution_mode(crate::performance::ExecutionMode::Benchmark);
    for _ in 0..warmup_frames {
        step();
    }
    monitor.clear_graph_samples();
    for _ in 0..measure_frames {
        step();
    }
    let samples: Vec<FrameSnapshot> = monitor.snapshots().iter().cloned().collect();
    BenchmarkResult::from_snapshots(scenario, warmup_frames, &samples)
}

pub fn results_to_json(results: &[BenchmarkResult]) -> String {
    let mut out = String::from("[\n");
    for (i, r) in results.iter().enumerate() {
        out.push_str(&format!(
            "  {{\"scenario\":\"{}\",\"frames\":{},\"warmup_frames\":{},\"average_fps\":{:.3},\"p50_ms\":{:.3},\"p95_ms\":{:.3},\"p99_ms\":{:.3},\"average_cpu_ms\":{:.3},\"average_gpu_ms\":{},\"average_draw_calls\":{:.2},\"average_triangles\":{:.2}}}{}",
            r.scenario.name(),
            r.frames,
            r.warmup_frames,
            r.average_fps,
            r.p50_frame_ms,
            r.p95_frame_ms,
            r.p99_frame_ms,
            r.average_cpu_ms,
            r.average_gpu_ms.map_or("null".into(), |v| format!("{v:.3}")),
            r.average_draw_calls,
            r.average_triangles,
            if i + 1 == results.len() { "\n" } else { ",\n" },
        ));
    }
    out.push(']');
    out
}

pub fn results_to_csv(results: &[BenchmarkResult]) -> String {
    let mut out = String::from(
        "scenario,frames,warmup_frames,average_fps,p50_ms,p95_ms,p99_ms,average_cpu_ms,average_gpu_ms,average_draw_calls,average_triangles\n",
    );
    for r in results {
        out.push_str(&format!(
            "{},{},{},{:.3},{:.3},{:.3},{:.3},{:.3},{},{:.2},{:.2}\n",
            r.scenario.name(),
            r.frames,
            r.warmup_frames,
            r.average_fps,
            r.p50_frame_ms,
            r.p95_frame_ms,
            r.p99_frame_ms,
            r.average_cpu_ms,
            r.average_gpu_ms.map_or("".into(), |v| format!("{v:.3}")),
            r.average_draw_calls,
            r.average_triangles,
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::performance::ExecutionMode;

    #[test]
    fn scenario_sizes_are_deterministic() {
        assert_eq!(BenchmarkScenario::Empty.target_entities(), 0);
        assert!(
            BenchmarkScenario::DrawCallHeavy.target_entities()
                > BenchmarkScenario::Large.target_entities()
        );
    }

    #[test]
    fn empty_run_does_not_fabricate_samples() {
        let r = BenchmarkResult::from_snapshots(BenchmarkScenario::Empty, 0, &[]);
        assert_eq!(r.frames, 0);
        assert_eq!(r.average_gpu_ms, None);
    }

    #[test]
    fn export_contains_scenario_rows() {
        let r = BenchmarkResult::from_snapshots(BenchmarkScenario::Pbr, 2, &[]);
        let csv = results_to_csv(&[r]);
        assert!(csv.contains("pbr"));
        assert!(csv.starts_with("scenario,"));
    }

    #[test]
    fn benchmark_mode_is_set() {
        let mut m = PerformanceMonitor::new(crate::performance::DEFAULT_HISTORY);
        m.set_execution_mode(ExecutionMode::Editor);
        let _ = run_scenario(BenchmarkScenario::Small, &mut m, 0, 1, || {});
        assert_eq!(m.execution_mode(), ExecutionMode::Benchmark);
    }
}
