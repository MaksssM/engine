use bevy_ecs::world::World;
use serde::{Deserialize, Serialize};
use std::fmt;

// ---------------------------------------------------------------------------
// System stages — mirrors a simplified UE5 tick model
// ---------------------------------------------------------------------------

/// Named execution stages within a single frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum SystemStage {
    /// Runs before anything else (resource init, pre-frame jobs).
    PreUpdate,
    /// Editor-only work that remains active outside play mode.
    EditorUpdate,
    /// Fixed-rate update for physics and deterministic simulation.
    FixedUpdate,
    /// Main per-frame update.
    Update,
    /// Post-update: resolve hierarchies, propagate transforms.
    PostUpdate,
    /// Late update: camera follow, animation blending.
    LateUpdate,
    /// Pre-render: frustum culling, draw list build.
    PreRender,
}

/// A system is simply a function that receives mutable world access.
pub trait System: Send + Sync {
    fn name(&self) -> &str;
    fn stage(&self) -> SystemStage;
    fn run(&mut self, world: &mut World);
}

/// Wrapper that turns a closure into a System.
pub struct FnSystem<F: FnMut(&mut World) + Send + Sync> {
    name: String,
    stage: SystemStage,
    func: F,
}

impl<F: FnMut(&mut World) + Send + Sync> FnSystem<F> {
    pub fn new(name: impl Into<String>, stage: SystemStage, func: F) -> Self {
        Self {
            name: name.into(),
            stage,
            func,
        }
    }
}

impl<F: FnMut(&mut World) + Send + Sync> System for FnSystem<F> {
    fn name(&self) -> &str {
        &self.name
    }
    fn stage(&self) -> SystemStage {
        self.stage
    }
    fn run(&mut self, world: &mut World) {
        (self.func)(world);
    }
}

// ---------------------------------------------------------------------------
// System scheduler
// ---------------------------------------------------------------------------

/// Collects systems and runs them stage-by-stage each frame.
#[derive(Clone, Debug)]
pub struct SystemTiming {
    pub name: String,
    pub stage: SystemStage,
    pub last_ms: f64,
    pub calls: u64,
}

pub struct SystemScheduler {
    systems: Vec<Box<dyn System>>,
    timings: Vec<SystemTiming>,
    profiling: bool,
}

impl SystemScheduler {
    pub fn new() -> Self {
        Self {
            systems: Vec::new(),
            timings: Vec::new(),
            profiling: true,
        }
    }

    pub fn set_profiling(&mut self, enabled: bool) {
        self.profiling = enabled;
    }

    /// Register a system for a given stage.
    pub fn add_system(&mut self, system: Box<dyn System>) {
        self.systems.push(system);
    }

    /// Convenience: register a closure as a named system.
    pub fn add_fn<F: FnMut(&mut World) + Send + Sync + 'static>(
        &mut self,
        name: impl Into<String>,
        stage: SystemStage,
        func: F,
    ) {
        self.add_system(Box::new(FnSystem::new(name, stage, func)));
    }

    /// Run all systems whose stage matches `stage`, in registration order.
    pub fn run_stage(&mut self, stage: SystemStage, world: &mut World) {
        for sys in self.systems.iter_mut() {
            if sys.stage() == stage {
                let started = self.profiling.then(std::time::Instant::now);
                sys.run(world);
                if let Some(started) = started {
                    let elapsed = started.elapsed().as_secs_f64() * 1000.0;
                    if let Some(timing) = self.timings.iter_mut().find(|timing| timing.name == sys.name()) {
                        timing.last_ms = elapsed;
                        timing.calls = timing.calls.saturating_add(1);
                    } else {
                        self.timings.push(SystemTiming {
                            name: sys.name().to_owned(),
                            stage,
                            last_ms: elapsed,
                            calls: 1,
                        });
                    }
                }
            }
        }
    }

    /// Run a full frame tick: every stage in order.
    pub fn run_frame(&mut self, world: &mut World) {
        use SystemStage::*;
        for stage in [
            PreUpdate,
            FixedUpdate,
            Update,
            PostUpdate,
            LateUpdate,
            PreRender,
        ] {
            self.run_stage(stage, world);
        }
    }

    pub fn system_count(&self) -> usize {
        self.systems.len()
    }

    /// List registered system names grouped by stage (for debug UI).
    pub fn list(&self) -> Vec<(SystemStage, &str)> {
        self.systems.iter().map(|s| (s.stage(), s.name())).collect()
    }

    pub fn timings(&self) -> &[SystemTiming] {
        &self.timings
    }
}

impl Default for SystemScheduler {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Debug for SystemScheduler {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SystemScheduler")
            .field("count", &self.systems.len())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    #[test]
    fn systems_run_in_stage_order() {
        let log = Arc::new(Mutex::new(Vec::new()));
        let mut scheduler = SystemScheduler::new();

        {
            let l = log.clone();
            scheduler.add_fn("late", SystemStage::LateUpdate, move |_w| {
                l.lock().unwrap().push("late");
            });
        }
        {
            let l = log.clone();
            scheduler.add_fn("pre", SystemStage::PreUpdate, move |_w| {
                l.lock().unwrap().push("pre");
            });
        }
        {
            let l = log.clone();
            scheduler.add_fn("update", SystemStage::Update, move |_w| {
                l.lock().unwrap().push("update");
            });
        }

        let mut world = World::new();
        scheduler.run_frame(&mut world);

        let entries = log.lock().unwrap().clone();
        assert_eq!(entries, vec!["pre", "update", "late"]);
    }

    #[test]
    fn scheduler_list() {
        let mut scheduler = SystemScheduler::new();
        scheduler.add_fn("sys_a", SystemStage::Update, |_| {});
        scheduler.add_fn("sys_b", SystemStage::PreUpdate, |_| {});

        let list = scheduler.list();
        assert_eq!(list.len(), 2);
        assert_eq!(list[0], (SystemStage::Update, "sys_a"));
    }
}
