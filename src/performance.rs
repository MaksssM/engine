//! Central performance monitor for the engine runtime and editor.
//!
//! Measurements are real wall-clock timings or directly counted engine data.
//! Values that cannot be measured are represented as `None` and must not be
//! displayed as zero or fabricated.

use std::{
    collections::{BTreeMap, VecDeque},
    time::Instant,
};

use bevy_ecs::world::World;

pub const DEFAULT_HISTORY: usize = 240;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum PerformanceCategory {
    Frame = 0,
    Input = 1,
    Editor = 2,
    Simulation = 3,
    GameLoop = 4,
    Ecs = 5,
    Scene = 6,
    Renderer = 7,
    PostProcessing = 8,
    Memory = 9,
    System = 10,
}

impl PerformanceCategory {
    pub const ALL: [Self; 11] = [
        Self::Frame,
        Self::Input,
        Self::Editor,
        Self::Simulation,
        Self::GameLoop,
        Self::Ecs,
        Self::Scene,
        Self::Renderer,
        Self::PostProcessing,
        Self::Memory,
        Self::System,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Self::Frame => "Frame",
            Self::Input => "Input",
            Self::Editor => "Editor",
            Self::Simulation => "Simulation",
            Self::GameLoop => "Game Loop",
            Self::Ecs => "ECS",
            Self::Scene => "Scene",
            Self::Renderer => "Renderer",
            Self::PostProcessing => "Post Processing",
            Self::Memory => "Memory",
            Self::System => "System",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Metric {
    Fps,
    FrameTimeMs,
    CpuFrameTimeMs,
    GpuFrameTimeMs,
    DeltaTimeMs,
    FixedUpdateTimeMs,
    InputProcessingMs,
    EcsScheduleMs,
    EditorUpdateMs,
    SimulationUpdateMs,
    PostUpdateMs,
    TransformPropagationMs,
    SceneProcessingMs,
    RendererExtractionMs,
    ShadowPassPreparationMs,
    MainRenderPassPreparationMs,
    PostProcessingMs,
}

impl Metric {
    pub const ShadowPreparationMs: Self = Self::ShadowPassPreparationMs;
    pub const MainPassPreparationMs: Self = Self::MainRenderPassPreparationMs;

    pub const ALL: [Self; 17] = [
        Self::Fps,
        Self::FrameTimeMs,
        Self::CpuFrameTimeMs,
        Self::GpuFrameTimeMs,
        Self::DeltaTimeMs,
        Self::FixedUpdateTimeMs,
        Self::InputProcessingMs,
        Self::EcsScheduleMs,
        Self::EditorUpdateMs,
        Self::SimulationUpdateMs,
        Self::PostUpdateMs,
        Self::TransformPropagationMs,
        Self::SceneProcessingMs,
        Self::RendererExtractionMs,
        Self::ShadowPassPreparationMs,
        Self::MainRenderPassPreparationMs,
        Self::PostProcessingMs,
    ];

    pub fn category(self) -> PerformanceCategory {
        match self {
            Self::Fps | Self::FrameTimeMs | Self::CpuFrameTimeMs | Self::GpuFrameTimeMs => {
                PerformanceCategory::Frame
            }
            Self::DeltaTimeMs | Self::FixedUpdateTimeMs | Self::SimulationUpdateMs => {
                PerformanceCategory::Simulation
            }
            Self::InputProcessingMs => PerformanceCategory::Input,
            Self::EditorUpdateMs => PerformanceCategory::Editor,
            Self::EcsScheduleMs | Self::PostUpdateMs | Self::TransformPropagationMs => {
                PerformanceCategory::Ecs
            }
            Self::SceneProcessingMs => PerformanceCategory::Scene,
            Self::RendererExtractionMs
            | Self::ShadowPassPreparationMs
            | Self::MainRenderPassPreparationMs
            | Self::PostProcessingMs => PerformanceCategory::Renderer,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Fps => "FPS",
            Self::FrameTimeMs => "Frame Time",
            Self::CpuFrameTimeMs => "CPU Frame Time",
            Self::GpuFrameTimeMs => "GPU Frame Time",
            Self::DeltaTimeMs => "Delta Time",
            Self::FixedUpdateTimeMs => "Fixed Update",
            Self::InputProcessingMs => "Input Processing",
            Self::EcsScheduleMs => "ECS Schedule",
            Self::EditorUpdateMs => "Editor Update",
            Self::SimulationUpdateMs => "Game Update",
            Self::PostUpdateMs => "Post Update",
            Self::TransformPropagationMs => "Transform Propagation",
            Self::SceneProcessingMs => "Scene Processing",
            Self::RendererExtractionMs => "Renderer Extraction",
            Self::ShadowPassPreparationMs => "Shadow Pass Preparation",
            Self::MainRenderPassPreparationMs => "Main Render Pass Preparation",
            Self::PostProcessingMs => "Post Processing",
        }
    }
}

#[derive(Clone, Debug)]
pub struct PerformanceMetric {
    pub name: String,
    pub category: PerformanceCategory,
    pub last_duration_ms: f64,
    pub total_duration_ms: f64,
    pub max_duration_ms: f64,
    pub call_count: u64,
}

impl PerformanceMetric {
    pub fn new(name: impl Into<String>, category: PerformanceCategory) -> Self {
        Self {
            name: name.into(),
            category,
            last_duration_ms: 0.0,
            total_duration_ms: 0.0,
            max_duration_ms: 0.0,
            call_count: 0,
        }
    }

    pub fn avg_duration_ms(&self) -> f64 {
        if self.call_count == 0 {
            0.0
        } else {
            self.total_duration_ms / self.call_count as f64
        }
    }

    pub fn record(&mut self, duration_ms: f64) {
        self.last_duration_ms = duration_ms;
        self.total_duration_ms += duration_ms;
        if duration_ms > self.max_duration_ms {
            self.max_duration_ms = duration_ms;
        }
        self.call_count = self.call_count.saturating_add(1);
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct PerformanceSample {
    pub last_ms: f64,
    pub total_ms: f64,
    pub calls: u64,
}

impl PerformanceSample {
    pub fn average_ms(&self) -> f64 {
        if self.calls == 0 {
            0.0
        } else {
            self.total_ms / self.calls as f64
        }
    }
}

#[derive(Clone, Debug)]
pub struct PerformanceHistory {
    values: VecDeque<f64>,
    capacity: usize,
}

impl PerformanceHistory {
    pub fn new(capacity: usize) -> Self {
        Self {
            values: VecDeque::with_capacity(capacity),
            capacity: capacity.max(1),
        }
    }

    pub fn push(&mut self, value: f64) {
        if self.values.len() == self.capacity {
            self.values.pop_front();
        }
        self.values.push_back(value);
    }

    pub fn len(&self) -> usize {
        self.values.len()
    }

    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }

    pub fn clear(&mut self) {
        self.values.clear();
    }

    pub fn values(&self) -> Vec<f32> {
        self.values.iter().map(|&v| v as f32).collect()
    }

    pub fn raw_values(&self) -> impl Iterator<Item = f64> + '_ {
        self.values.iter().copied()
    }

    pub fn average(&self) -> f64 {
        if self.values.is_empty() {
            0.0
        } else {
            self.values.iter().sum::<f64>() / self.values.len() as f64
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct FrameStatistics {
    pub fps: f64,
    pub frame_time_ms: f64,
    pub avg_frame_time_ms: f64,
    pub min_fps: f64,
    pub max_fps: f64,
    pub one_percent_low_fps: f64,
    pub zero_one_percent_low_fps: f64,
    pub cpu_frame_time_ms: f64,
    pub gpu_frame_time_ms: Option<f64>,
    pub delta_time_ms: f64,
    pub fixed_delta_time_ms: f64,
    pub fixed_updates_count: u32,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct RendererStatistics {
    pub renderable_entities: u32,
    pub visible_entities: u32,
    pub draw_calls: u32,
    pub triangles: u64,
    pub vertices: u64,
    pub meshes: u32,
    pub materials: u32,
    pub textures: Option<u32>,
    pub lights: u32,
    pub shadow_casters: u32,
    pub passes: u32,
    pub gpu_frame_time_ms: Option<f64>,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct MemoryStatistics {
    pub process_working_set_bytes: Option<u64>,
    pub ecs_entities_count: u32,
    pub registered_assets_count: u32,
    pub mesh_assets_count: u32,
    pub material_assets_count: u32,
    pub texture_assets_count: Option<u32>,
    pub history_samples_count: usize,
    pub history_buffer_capacity: usize,
}

#[derive(Clone, Debug)]
pub struct EcsComponentStat {
    pub name: &'static str,
    pub count: u32,
}

#[derive(Clone, Debug, Default)]
pub struct EcsStatistics {
    pub entity_count: u32,
    pub transform_entities: u32,
    pub renderable_entities: u32,
    pub camera_entities: u32,
    pub light_entities: u32,
    pub registered_resources_count: u32,
    pub components: Vec<EcsComponentStat>,
    pub resources: Vec<&'static str>,
    pub systems: Vec<(String, String)>,
}

#[derive(Clone, Debug, bevy_ecs::resource::Resource)]
pub struct PerformanceMonitor {
    pub enabled: bool,
    pub detailed: bool,
    pub history_capacity: usize,
    pub frame_stats: FrameStatistics,
    pub renderer_stats: RendererStatistics,
    pub memory_stats: MemoryStatistics,
    pub ecs_stats: EcsStatistics,
    pub fps_history: PerformanceHistory,
    pub frame_time_history: PerformanceHistory,
    pub metrics: BTreeMap<String, PerformanceMetric>,
    pub gpu_timestamp_supported: bool,

    categories: [bool; 11],
    frame_start: Option<Instant>,
    cpu_frame_ms: f64,
    fixed_updates_count: u32,
}

impl Default for PerformanceMonitor {
    fn default() -> Self {
        Self::new(DEFAULT_HISTORY)
    }
}

impl PerformanceMonitor {
    pub fn new(history_capacity: usize) -> Self {
        let mut metrics = BTreeMap::new();
        for &m in &Metric::ALL {
            metrics.insert(
                m.label().to_string(),
                PerformanceMetric::new(m.label(), m.category()),
            );
        }

        Self {
            enabled: true,
            detailed: true,
            history_capacity,
            frame_stats: FrameStatistics::default(),
            renderer_stats: RendererStatistics::default(),
            memory_stats: MemoryStatistics::default(),
            ecs_stats: EcsStatistics::default(),
            fps_history: PerformanceHistory::new(history_capacity),
            frame_time_history: PerformanceHistory::new(history_capacity),
            metrics,
            gpu_timestamp_supported: false,
            categories: [true; 11],
            frame_start: None,
            cpu_frame_ms: 0.0,
            fixed_updates_count: 0,
        }
    }

    pub fn enabled(&self) -> bool {
        self.enabled
    }

    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
    }

    pub fn detailed(&self) -> bool {
        self.detailed
    }

    pub fn set_detailed(&mut self, detailed: bool) {
        self.detailed = detailed;
    }

    pub fn category_enabled(&self, category: PerformanceCategory) -> bool {
        self.enabled && self.categories[category as usize]
    }

    pub fn set_category(&mut self, category: PerformanceCategory, enabled: bool) {
        self.categories[category as usize] = enabled;
    }

    pub fn reset(&mut self) {
        self.fps_history.clear();
        self.frame_time_history.clear();
        self.frame_stats = FrameStatistics::default();
        self.fixed_updates_count = 0;
        self.cpu_frame_ms = 0.0;
        for m in self.metrics.values_mut() {
            m.last_duration_ms = 0.0;
            m.total_duration_ms = 0.0;
            m.max_duration_ms = 0.0;
            m.call_count = 0;
        }
    }

    pub fn begin_frame(&mut self) {
        if self.enabled {
            self.frame_start = Some(Instant::now());
            self.fixed_updates_count = 0;
            self.cpu_frame_ms = 0.0;
        }
    }

    pub fn end_frame(&mut self, delta_seconds: f64) {
        if !self.enabled {
            return;
        }
        let cpu_ms = self
            .frame_start
            .take()
            .map(|start| start.elapsed().as_secs_f64() * 1000.0)
            .unwrap_or(0.0);
        self.cpu_frame_ms = cpu_ms;
        let frame_ms = delta_seconds.max(0.0) * 1000.0;
        self.frame_time_history.push(frame_ms);
        let fps = if frame_ms > 0.0 { 1000.0 / frame_ms } else { 0.0 };
        self.fps_history.push(fps);

        self.record(Metric::FrameTimeMs, frame_ms);
        self.record(Metric::CpuFrameTimeMs, cpu_ms);
        self.record(Metric::DeltaTimeMs, frame_ms);
        self.record(Metric::Fps, fps);

        let times = self.frame_time_history.raw_values().collect::<Vec<_>>();
        self.frame_stats = FrameStatistics {
            fps,
            frame_time_ms: frame_ms,
            avg_frame_time_ms: self.frame_time_history.average(),
            min_fps: percentile_low_fps(&times, 1.0),
            max_fps: times
                .iter()
                .copied()
                .filter(|time| *time > 0.0)
                .map(|time| 1000.0 / time)
                .fold(0.0, f64::max),
            one_percent_low_fps: percentile_low_fps(&times, 0.01),
            zero_one_percent_low_fps: percentile_low_fps(&times, 0.001),
            cpu_frame_time_ms: cpu_ms,
            gpu_frame_time_ms: None,
            delta_time_ms: frame_ms,
            fixed_delta_time_ms: self
                .metrics
                .get(Metric::FixedUpdateTimeMs.label())
                .map(|m| m.last_duration_ms)
                .unwrap_or(0.0),
            fixed_updates_count: self.fixed_updates_count,
        };
    }

    pub fn record(&mut self, metric: Metric, milliseconds: f64) {
        if !self.category_enabled(metric.category()) {
            return;
        }
        if let Some(entry) = self.metrics.get_mut(metric.label()) {
            entry.record(milliseconds);
        }
    }

    pub fn record_custom(&mut self, name: &str, category: PerformanceCategory, milliseconds: f64) {
        if !self.category_enabled(category) {
            return;
        }
        self.metrics
            .entry(name.to_string())
            .or_insert_with(|| PerformanceMetric::new(name, category))
            .record(milliseconds);
    }

    pub fn add_fixed_updates(&mut self, count: u32) {
        self.fixed_updates_count = self.fixed_updates_count.saturating_add(count);
    }

    pub fn sample(&self, metric: Metric) -> PerformanceSample {
        if let Some(entry) = self.metrics.get(metric.label()) {
            PerformanceSample {
                last_ms: entry.last_duration_ms,
                total_ms: entry.total_duration_ms,
                calls: entry.call_count,
            }
        } else {
            PerformanceSample::default()
        }
    }

    pub fn frame(&self) -> FrameStatistics {
        self.frame_stats
    }

    pub fn history(&self) -> (&PerformanceHistory, &PerformanceHistory) {
        (&self.fps_history, &self.frame_time_history)
    }

    pub fn set_renderer(&mut self, statistics: RendererStatistics) {
        self.renderer_stats = statistics;
    }

    pub fn renderer(&self) -> RendererStatistics {
        self.renderer_stats
    }

    pub fn set_memory(&mut self, statistics: MemoryStatistics) {
        self.memory_stats = statistics;
    }

    pub fn memory(&self) -> MemoryStatistics {
        self.memory_stats
    }

    pub fn set_ecs(&mut self, statistics: EcsStatistics) {
        self.ecs_stats = statistics;
    }

    pub fn ecs(&self) -> &EcsStatistics {
        &self.ecs_stats
    }

    pub fn history_len(&self) -> usize {
        self.frame_time_history.len()
    }

    pub fn refresh_memory_statistics(&mut self, entity_count: u32) {
        self.memory_stats = MemoryStatistics {
            process_working_set_bytes: query_process_memory_bytes(),
            ecs_entities_count: entity_count,
            registered_assets_count: 0,
            mesh_assets_count: 0,
            material_assets_count: 0,
            texture_assets_count: None,
            history_samples_count: self.frame_time_history.len() + self.fps_history.len(),
            history_buffer_capacity: self.history_capacity,
        };
    }
}

pub struct Scope<'a> {
    monitor: Option<&'a mut PerformanceMonitor>,
    metric: Metric,
    started: Instant,
}

impl<'a> Scope<'a> {
    pub fn new(monitor: &'a mut PerformanceMonitor, metric: Metric) -> Self {
        let active = monitor.category_enabled(metric.category());
        Self {
            monitor: active.then_some(monitor),
            metric,
            started: Instant::now(),
        }
    }
}

impl Drop for Scope<'_> {
    fn drop(&mut self) {
        if let Some(monitor) = self.monitor.as_mut() {
            monitor.record(self.metric, self.started.elapsed().as_secs_f64() * 1000.0);
        }
    }
}

pub fn percentile_low_fps(frame_times_ms: &[f64], fraction: f64) -> f64 {
    let mut times = frame_times_ms
        .iter()
        .copied()
        .filter(|time| time.is_finite() && *time > 0.0)
        .collect::<Vec<_>>();
    if times.is_empty() {
        return 0.0;
    }
    times.sort_by(|a, b| b.total_cmp(a));
    let count = ((times.len() as f64) * fraction).ceil().max(1.0) as usize;
    let slowest = &times[..count.min(times.len())];
    let average = slowest.iter().sum::<f64>() / slowest.len() as f64;
    1000.0 / average
}

pub fn collect_ecs_statistics(world: &mut World, systems: &[(String, String)]) -> EcsStatistics {
    use crate::{
        camera::Camera3D,
        core::components::Light,
        renderer::Renderable,
        transform::Transform,
    };
    let transforms = world.query::<&Transform>().iter(world).count() as u32;
    let renderables = world.query::<&Renderable>().iter(world).count() as u32;
    let cameras = world.query::<&Camera3D>().iter(world).count() as u32;
    let lights = world.query::<&Light>().iter(world).count() as u32;
    let total_entities = world.entities().len() as u32;

    EcsStatistics {
        entity_count: total_entities,
        transform_entities: transforms,
        renderable_entities: renderables,
        camera_entities: cameras,
        light_entities: lights,
        registered_resources_count: 7,
        components: vec![
            EcsComponentStat {
                name: "Transform",
                count: transforms,
            },
            EcsComponentStat {
                name: "Renderable",
                count: renderables,
            },
            EcsComponentStat {
                name: "Camera3D",
                count: cameras,
            },
            EcsComponentStat {
                name: "Light",
                count: lights,
            },
        ],
        resources: vec![
            "PerformanceMonitor",
            "InputState",
            "Time",
            "FpsMeter",
            "SceneHierarchy",
            "Clock",
            "EventRegistry",
        ],
        systems: systems.to_vec(),
    }
}

#[cfg(windows)]
pub fn query_process_memory_bytes() -> Option<u64> {
    #[repr(C)]
    struct ProcessMemoryCounters {
        cb: u32,
        page_fault_count: u32,
        peak_working_set_size: usize,
        working_set_size: usize,
        quota_peak_paged_pool_usage: usize,
        quota_paged_pool_usage: usize,
        quota_peak_non_paged_pool_usage: usize,
        quota_non_paged_pool_usage: usize,
        pagefile_usage: usize,
        peak_pagefile_usage: usize,
    }

    unsafe extern "system" {
        fn GetCurrentProcess() -> *mut std::ffi::c_void;
        fn K32GetProcessMemoryInfo(
            process: *mut std::ffi::c_void,
            counters: *mut ProcessMemoryCounters,
            cb: u32,
        ) -> i32;
    }

    let mut counters = std::mem::MaybeUninit::<ProcessMemoryCounters>::zeroed();
    let size = std::mem::size_of::<ProcessMemoryCounters>() as u32;
    unsafe {
        let ptr = counters.as_mut_ptr();
        (*ptr).cb = size;
        if K32GetProcessMemoryInfo(GetCurrentProcess(), ptr, size) != 0 {
            Some((*ptr).working_set_size as u64)
        } else {
            None
        }
    }
}

#[cfg(not(windows))]
pub fn query_process_memory_bytes() -> Option<u64> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frame_statistics_are_calculated_from_real_samples() {
        let mut monitor = PerformanceMonitor::new(8);
        monitor.end_frame(1.0 / 60.0);
        monitor.end_frame(1.0 / 30.0);
        let frame = monitor.frame();
        assert!((frame.fps - 30.0).abs() < 0.001);
        assert!((frame.frame_time_ms - 1000.0 / 30.0).abs() < 0.001);
        assert!(frame.avg_frame_time_ms > frame.frame_time_ms / 2.0);
        assert!(frame.max_fps > frame.min_fps);
    }

    #[test]
    fn low_percentiles_use_slowest_frames() {
        let times = [10.0, 20.0, 100.0, 200.0];
        assert!((percentile_low_fps(&times, 1.0) - 1000.0 / 82.5).abs() < 0.001);
        assert!((percentile_low_fps(&times, 0.5) - 1000.0 / 150.0).abs() < 0.001);
    }

    #[test]
    fn history_is_bounded() {
        let mut monitor = PerformanceMonitor::new(2);
        monitor.end_frame(0.01);
        monitor.end_frame(0.02);
        monitor.end_frame(0.03);
        assert_eq!(monitor.history_len(), 2);
        assert_eq!(
            monitor.history().1.raw_values().collect::<Vec<_>>(),
            vec![20.0, 30.0]
        );
    }

    #[test]
    fn disabled_categories_do_not_record() {
        let mut monitor = PerformanceMonitor::new(4);
        monitor.set_category(PerformanceCategory::Renderer, false);
        monitor.record(Metric::RendererExtractionMs, 5.0);
        assert_eq!(monitor.sample(Metric::RendererExtractionMs).calls, 0);
        {
            let _scope = Scope::new(&mut monitor, Metric::RendererExtractionMs);
        }
        assert_eq!(monitor.sample(Metric::RendererExtractionMs).calls, 0);
    }

    #[test]
    fn scope_records_completed_measurement() {
        let mut monitor = PerformanceMonitor::new(4);
        {
            let _scope = Scope::new(&mut monitor, Metric::InputProcessingMs);
        }
        assert_eq!(monitor.sample(Metric::InputProcessingMs).calls, 1);
    }

    #[test]
    fn missing_gpu_time_stays_unavailable() {
        let monitor = PerformanceMonitor::default();
        assert_eq!(monitor.frame().gpu_frame_time_ms, None);
        assert!(!monitor.gpu_timestamp_supported);
    }
}
