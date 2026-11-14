use bevy_ecs::{component::Component, entity::Entity, world::World};

pub type EngineEntity = Entity;

#[derive(Component, Clone, Debug, PartialEq)]
pub struct Name(pub String);

impl Default for Name {
    fn default() -> Self {
        Self("entity".into())
    }
}

#[derive(Default)]
pub struct Ecs {
    pub world: World,
}

impl Ecs {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn spawn<B: bevy_ecs::bundle::Bundle>(&mut self, bundle: B) -> EngineEntity {
        self.world.spawn(bundle).id()
    }

    pub fn despawn(&mut self, entity: EngineEntity) -> bool {
        self.world.despawn(entity)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy_ecs::component::Component;

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
}
