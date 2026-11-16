use bevy_ecs::{
    component::Component,
    entity::Entity,
    schedule::{IntoScheduleConfigs, Schedule, SystemSet},
    system::{IntoSystem, ScheduleSystem},
    world::World,
};
use std::marker::PhantomData;

pub type EngineEntity = Entity;

#[derive(Component, Clone, Debug, PartialEq)]
pub struct Name(pub String);

impl Default for Name {
    fn default() -> Self {
        Self("entity".into())
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum EcsSchedule {
    Startup,
    EditorUpdate,
    FixedUpdate,
    #[default]
    Update,
    PostUpdate,
    Shutdown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, SystemSet)]
pub struct EcsSystemSet<T>(PhantomData<T>);

impl<T> Default for EcsSystemSet<T> {
    fn default() -> Self {
        Self(PhantomData)
    }
}

#[derive(Default)]
pub struct Ecs {
    pub world: World,
    schedules: EcsSchedules,
}

#[derive(Default)]
struct EcsSchedules {
    startup: Schedule,
    editor_update: Schedule,
    fixed_update: Schedule,
    update: Schedule,
    post_update: Schedule,
    shutdown: Schedule,
}

impl Ecs {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert_resource<R: bevy_ecs::resource::Resource>(&mut self, resource: R) -> &mut Self {
        self.world.insert_resource(resource);
        self
    }

    pub fn resource<R: bevy_ecs::resource::Resource>(&self) -> &R {
        self.world.resource::<R>()
    }

    pub fn resource_mut<R: bevy_ecs::resource::Resource>(
        &mut self,
    ) -> bevy_ecs::change_detection::Mut<'_, R> {
        self.world.resource_mut::<R>()
    }

    pub fn spawn<B: bevy_ecs::bundle::Bundle>(&mut self, bundle: B) -> EngineEntity {
        self.world.spawn(bundle).id()
    }

    pub fn despawn(&mut self, entity: EngineEntity) -> bool {
        self.world.despawn(entity)
    }

    pub fn add_systems<M, S>(&mut self, schedule: EcsSchedule, systems: S) -> &mut Self
    where
        S: IntoScheduleConfigs<ScheduleSystem, M>,
    {
        self.schedule_mut(schedule).add_systems(systems);
        self
    }

    pub fn add_system<M, S>(&mut self, schedule: EcsSchedule, system: S) -> &mut Self
    where
        S: IntoSystem<(), (), M> + 'static,
    {
        self.add_systems(schedule, system);
        self
    }

    pub fn run_schedule(&mut self, schedule: EcsSchedule) {
        match schedule {
            EcsSchedule::Startup => self.schedules.startup.run(&mut self.world),
            EcsSchedule::EditorUpdate => self.schedules.editor_update.run(&mut self.world),
            EcsSchedule::FixedUpdate => self.schedules.fixed_update.run(&mut self.world),
            EcsSchedule::Update => self.schedules.update.run(&mut self.world),
            EcsSchedule::PostUpdate => self.schedules.post_update.run(&mut self.world),
            EcsSchedule::Shutdown => self.schedules.shutdown.run(&mut self.world),
        }
    }

    pub fn run_startup(&mut self) {
        self.run_schedule(EcsSchedule::Startup);
    }

    pub fn run_frame(&mut self) {
        for schedule in [
            EcsSchedule::EditorUpdate,
            EcsSchedule::FixedUpdate,
            EcsSchedule::Update,
            EcsSchedule::PostUpdate,
        ] {
            self.run_schedule(schedule);
        }
    }

    pub fn shutdown(&mut self) {
        self.run_schedule(EcsSchedule::Shutdown);
    }

    fn schedule_mut(&mut self, schedule: EcsSchedule) -> &mut Schedule {
        match schedule {
            EcsSchedule::Startup => &mut self.schedules.startup,
            EcsSchedule::EditorUpdate => &mut self.schedules.editor_update,
            EcsSchedule::FixedUpdate => &mut self.schedules.fixed_update,
            EcsSchedule::Update => &mut self.schedules.update,
            EcsSchedule::PostUpdate => &mut self.schedules.post_update,
            EcsSchedule::Shutdown => &mut self.schedules.shutdown,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy_ecs::{component::Component, event::Event};

    #[derive(Component, PartialEq, Debug)]
    struct Health(u32);

    #[test]
    fn entity_component_flow() {
        let mut ecs = Ecs::new();
        let entity = ecs.spawn((Health(100),));
        assert_eq!(ecs.world.get::<Health>(entity), Some(&Health(100)));
        ecs.world.entity_mut(entity).insert(Health(75));
        assert_eq!(ecs.world.get::<Health>(entity), Some(&Health(75)));
        assert!(ecs.despawn(entity));
        assert!(!ecs.despawn(entity));
    }

    #[test]
    fn bevy_systems_use_queries_resources_order_and_events() {
        #[derive(bevy_ecs::resource::Resource)]
        struct Counter(u32);
        #[derive(Event, Debug)]
        struct Ping;

        let mut ecs = Ecs::new();
        ecs.world.insert_resource(Counter(0));
        ecs.world
            .insert_resource(bevy_ecs::event::Events::<Ping>::default());
        ecs.spawn((Health(10),));
        ecs.add_system(
            EcsSchedule::Startup,
            |mut counter: bevy_ecs::prelude::ResMut<Counter>| {
                counter.0 += 1;
            },
        );
        ecs.add_system(
            EcsSchedule::Update,
            |mut counter: bevy_ecs::prelude::ResMut<Counter>,
             mut events: bevy_ecs::prelude::EventWriter<Ping>,
             query: bevy_ecs::prelude::Query<&mut Health>| {
                counter.0 += query.iter().map(|health| health.0).sum::<u32>();
                events.write(Ping);
            },
        );
        ecs.add_system(
            EcsSchedule::PostUpdate,
            |counter: bevy_ecs::prelude::Res<Counter>,
             mut reader: bevy_ecs::prelude::EventReader<Ping>| {
                assert_eq!(counter.0, 11);
                assert_eq!(reader.read().count(), 1);
            },
        );
        ecs.run_startup();
        ecs.run_frame();
        assert_eq!(ecs.world.resource::<Counter>().0, 11);
    }
}
