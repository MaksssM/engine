use super::resources::{EngineMode, EngineTime};
use super::systems::SystemScheduler;
use bevy_ecs::world::World;

/// Coordinates the engine's frame lifecycle around the system scheduler.
/// The application remains responsible for window events and rendering;
/// this type owns the deterministic order of simulation systems.
pub struct GameLoop {
    pub scheduler: SystemScheduler,
    pub fixed_accumulator: f64,
    pub fixed_step: f64,
    pub max_fixed_steps: u32,
}

impl Default for GameLoop {
    fn default() -> Self {
        Self {
            scheduler: SystemScheduler::new(),
            fixed_accumulator: 0.0,
            fixed_step: 1.0 / 60.0,
            max_fixed_steps: 8,
        }
    }
}

impl GameLoop {
    pub fn new(fixed_hz: f64) -> Self {
        Self {
            fixed_step: 1.0 / fixed_hz.max(1.0),
            ..Self::default()
        }
    }

    /// Run one frame. FixedUpdate is executed as many times as needed,
    /// capped to avoid a spiral of death after a stalled frame.
    pub fn tick(&mut self, world: &mut World, delta_seconds: f64) {
        if let Some(mut time) = world.get_resource_mut::<EngineTime>() {
            let _ = time.clock.tick();
        }

        self.scheduler
            .run_stage(super::systems::SystemStage::PreUpdate, world);

        self.fixed_accumulator = (self.fixed_accumulator + delta_seconds)
            .min(self.fixed_step * f64::from(self.max_fixed_steps));
        let mut steps = 0;
        while self.fixed_accumulator >= self.fixed_step && steps < self.max_fixed_steps {
            self.scheduler
                .run_stage(super::systems::SystemStage::FixedUpdate, world);
            self.fixed_accumulator -= self.fixed_step;
            steps += 1;
        }

        self.scheduler
            .run_stage(super::systems::SystemStage::Update, world);
        self.scheduler
            .run_stage(super::systems::SystemStage::PostUpdate, world);
        self.scheduler
            .run_stage(super::systems::SystemStage::LateUpdate, world);
        self.scheduler
            .run_stage(super::systems::SystemStage::PreRender, world);
    }

    pub fn is_playing(world: &World) -> bool {
        world
            .get_resource::<EngineMode>()
            .is_some_and(|mode| *mode == EngineMode::Playing)
    }

    pub fn set_playing(world: &mut World, playing: bool) {
        world.insert_resource(if playing {
            EngineMode::Playing
        } else {
            EngineMode::Editor
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::resources::init_core_resources;
    use crate::core::systems::SystemStage;
    use std::sync::{Arc, Mutex};

    #[test]
    fn tick_runs_fixed_and_update() {
        let mut world = World::new();
        init_core_resources(&mut world);
        let fixed = Arc::new(Mutex::new(0));
        let update = Arc::new(Mutex::new(0));
        let mut loop_ = GameLoop::default();
        {
            let fixed = fixed.clone();
            loop_
                .scheduler
                .add_fn("fixed", SystemStage::FixedUpdate, move |_| {
                    *fixed.lock().unwrap() += 1;
                });
        }
        {
            let update = update.clone();
            loop_
                .scheduler
                .add_fn("update", SystemStage::Update, move |_| {
                    *update.lock().unwrap() += 1;
                });
        }
        loop_.tick(&mut world, 0.1);
        assert_eq!(*update.lock().unwrap(), 1);
        assert_eq!(*fixed.lock().unwrap(), 6);
    }

    #[test]
    fn mode_helpers() {
        let mut world = World::new();
        init_core_resources(&mut world);
        assert!(!GameLoop::is_playing(&world));
        GameLoop::set_playing(&mut world, true);
        assert!(GameLoop::is_playing(&world));
    }
}
