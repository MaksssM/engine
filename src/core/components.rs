use bevy_ecs::component::Component;
use glam::Vec3;
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Visibility & Editor markers
// ---------------------------------------------------------------------------

/// Controls whether this entity is rendered.
#[derive(Component, Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Visible(pub bool);

impl Default for Visible {
    fn default() -> Self {
        Self(true)
    }
}

/// Prevents the entity from being selected or edited.
#[derive(Component, Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Locked(pub bool);

impl Default for Locked {
    fn default() -> Self {
        Self(false)
    }
}

/// Static marker — object does not move at runtime (enables optimisations).
#[derive(Component, Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct StaticObject;

/// Dynamic marker — object participates in physics / scripting movement.
#[derive(Component, Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct DynamicObject;

/// Arbitrary tag string, like UE5's actor tags.
#[derive(Component, Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Tag(pub String);

/// Editor-only folder group for organising the World Outliner.
#[derive(Component, Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FolderGroup(pub String);

impl Default for FolderGroup {
    fn default() -> Self {
        Self("Default".into())
    }
}

// ---------------------------------------------------------------------------
// Rendering components
// ---------------------------------------------------------------------------

/// Identifies which mesh asset this entity uses.
#[derive(Component, Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MeshFilter {
    pub mesh_path: String,
}

/// Material assignment: references a material asset by path.
#[derive(Component, Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MeshRenderer {
    pub material_path: String,
    pub cast_shadows: bool,
    pub receive_shadows: bool,
}

impl Default for MeshRenderer {
    fn default() -> Self {
        Self {
            material_path: String::new(),
            cast_shadows: true,
            receive_shadows: true,
        }
    }
}

// ---------------------------------------------------------------------------
// Lights
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum LightKind {
    Directional,
    Point,
    Spot { inner_angle: f32, outer_angle: f32 },
}

impl Default for LightKind {
    fn default() -> Self {
        Self::Directional
    }
}

#[derive(Component, Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Light {
    pub kind: LightKind,
    pub color: [f32; 3],
    pub intensity: f32,
    pub range: f32,
    pub cast_shadows: bool,
}

impl Default for Light {
    fn default() -> Self {
        Self {
            kind: LightKind::Directional,
            color: [1.0, 1.0, 1.0],
            intensity: 1.0,
            range: 10.0,
            cast_shadows: true,
        }
    }
}

// ---------------------------------------------------------------------------
// Audio
// ---------------------------------------------------------------------------

#[derive(Component, Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AudioSource {
    pub clip_path: String,
    pub volume: f32,
    pub pitch: f32,
    pub looping: bool,
    pub spatial: bool,
    pub min_distance: f32,
    pub max_distance: f32,
}

impl Default for AudioSource {
    fn default() -> Self {
        Self {
            clip_path: String::new(),
            volume: 1.0,
            pitch: 1.0,
            looping: false,
            spatial: true,
            min_distance: 1.0,
            max_distance: 50.0,
        }
    }
}

// ---------------------------------------------------------------------------
// Camera marker
// ---------------------------------------------------------------------------

/// Marks this entity as a camera for the engine.  The actual projection
/// parameters live in the existing `Camera3D` component.
#[derive(Component, Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct CameraMarker {
    pub is_main: bool,
}

// ---------------------------------------------------------------------------
// Physics placeholders (extensible)
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum ColliderShape {
    Box { half_extents: Vec3 },
    Sphere { radius: f32 },
    Capsule { radius: f32, half_height: f32 },
    Mesh,
}

impl Default for ColliderShape {
    fn default() -> Self {
        Self::Box {
            half_extents: Vec3::splat(0.5),
        }
    }
}

#[derive(Component, Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Collider {
    pub shape: ColliderShape,
    pub is_trigger: bool,
}

impl Default for Collider {
    fn default() -> Self {
        Self {
            shape: ColliderShape::default(),
            is_trigger: false,
        }
    }
}

#[derive(Component, Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct RigidBody {
    pub mass: f32,
    pub use_gravity: bool,
    pub is_kinematic: bool,
    pub linear_damping: f32,
    pub angular_damping: f32,
}

impl Default for RigidBody {
    fn default() -> Self {
        Self {
            mass: 1.0,
            use_gravity: true,
            is_kinematic: false,
            linear_damping: 0.01,
            angular_damping: 0.05,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_visible_is_true() {
        assert!(Visible::default().0);
    }

    #[test]
    fn light_serde_round_trip() {
        let light = Light {
            kind: LightKind::Spot {
                inner_angle: 30.0,
                outer_angle: 45.0,
            },
            color: [1.0, 0.8, 0.6],
            intensity: 2.5,
            range: 20.0,
            cast_shadows: false,
        };
        let ron = ron::to_string(&light).unwrap();
        let back: Light = ron::from_str(&ron).unwrap();
        assert_eq!(light, back);
    }

    #[test]
    fn rigid_body_defaults() {
        let rb = RigidBody::default();
        assert!(rb.use_gravity);
        assert!(!rb.is_kinematic);
    }
}
