use std::time::{Duration, Instant};

#[derive(Debug)]
pub struct Clock {
    previous: Instant,
    elapsed: Duration,
}

impl Clock {
    pub fn new() -> Self {
        Self {
            previous: Instant::now(),
            elapsed: Duration::ZERO,
        }
    }

    pub fn tick(&mut self) -> Duration {
        let now = Instant::now();
        let delta = now.saturating_duration_since(self.previous);
        self.previous = now;
        self.elapsed += delta;
        delta
    }

    pub fn elapsed(&self) -> Duration {
        self.elapsed
    }
}

impl Default for Clock {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug)]
pub struct FixedTimestep {
    step: Duration,
    accumulator: Duration,
    max_frame_time: Duration,
}

impl FixedTimestep {
    pub fn new(step: Duration) -> Self {
        assert!(!step.is_zero(), "fixed timestep must be greater than zero");
        Self {
            step,
            accumulator: Duration::ZERO,
            max_frame_time: Duration::from_millis(250),
        }
    }

    pub fn add_frame(&mut self, frame_time: Duration) -> u32 {
        self.accumulator += frame_time.min(self.max_frame_time);
        let mut updates = 0;
        while self.accumulator >= self.step {
            self.accumulator -= self.step;
            updates += 1;
        }
        updates
    }

    pub fn step(&self) -> Duration {
        self.step
    }
}

#[derive(Debug)]
pub struct FpsMeter {
    frames: u32,
    elapsed: Duration,
    fps: f64,
}

impl FpsMeter {
    pub fn new() -> Self {
        Self {
            frames: 0,
            elapsed: Duration::ZERO,
            fps: 0.0,
        }
    }

    pub fn record_frame(&mut self, frame_time: Duration) -> Option<f64> {
        self.frames += 1;
        self.elapsed += frame_time;
        if self.elapsed < Duration::from_secs(1) {
            return None;
        }
        self.fps = f64::from(self.frames) / self.elapsed.as_secs_f64();
        self.frames = 0;
        self.elapsed = Duration::ZERO;
        Some(self.fps)
    }

    pub fn fps(&self) -> f64 {
        self.fps
    }
}

impl Default for FpsMeter {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixed_timestep_counts_updates() {
        let mut timestep = FixedTimestep::new(Duration::from_millis(16));
        assert_eq!(timestep.add_frame(Duration::from_millis(50)), 3);
        assert_eq!(timestep.add_frame(Duration::from_millis(14)), 1);
    }

    #[test]
    fn fixed_timestep_clamps_long_frames() {
        let mut timestep = FixedTimestep::new(Duration::from_millis(16));
        assert_eq!(timestep.add_frame(Duration::from_secs(2)), 15);
    }

    #[test]
    fn fps_meter_reports_after_one_second() {
        let mut meter = FpsMeter::new();
        for _ in 0..60 {
            assert!(meter.record_frame(Duration::from_millis(16)).is_none());
        }
        let fps = meter.record_frame(Duration::from_millis(40)).unwrap();
        assert!(fps > 50.0 && fps < 65.0);
    }
}
