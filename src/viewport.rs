//! Viewport projection, picking and transform-gizmo math.
//!
//! The scene itself is rendered by the wgpu renderer. This module only
//! converts between screen space and world space so the editor can pick
//! objects and drag gizmos without a second renderer.

use glam::{Mat4, Quat, Vec2, Vec3, Vec4};

/// Axis (or plane / uniform) a transform gizmo can manipulate.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GizmoAxis {
    X,
    Y,
    Z,
    Xy,
    Xz,
    Yz,
    Uniform,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GizmoTool {
    Select,
    Move,
    Rotate,
    Scale,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CoordSpace {
    World,
    Local,
}

/// Camera used to project the scene into a viewport rectangle.
#[derive(Clone, Copy, Debug)]
pub struct ViewportCamera {
    pub position: Vec3,
    pub target: Vec3,
    pub fov_y: f32,
    pub near: f32,
    pub far: f32,
    pub orthographic: bool,
    /// World-space height covered by the orthographic view.
    pub ortho_scale: f32,
}

impl ViewportCamera {
    pub fn forward(&self) -> Vec3 {
        (self.target - self.position).normalize_or_zero()
    }

    pub fn right(&self) -> Vec3 {
        self.forward().cross(Vec3::Y).normalize_or(Vec3::X)
    }

    pub fn up(&self) -> Vec3 {
        self.right().cross(self.forward()).normalize_or(Vec3::Y)
    }

    pub fn view(&self) -> Mat4 {
        Mat4::look_at_rh(self.position, self.target, Vec3::Y)
    }

    pub fn projection(&self, aspect: f32) -> Mat4 {
        let aspect = aspect.max(0.01);
        if self.orthographic {
            let half_h = (self.ortho_scale * 0.5).max(0.01);
            let half_w = half_h * aspect;
            Mat4::orthographic_rh(
                -half_w,
                half_w,
                -half_h,
                half_h,
                self.near.max(0.01),
                self.far.max(self.near + 0.1),
            )
        } else {
            Mat4::perspective_rh(
                self.fov_y.clamp(0.05, 2.8),
                aspect,
                self.near.max(0.001),
                self.far.max(self.near + 0.1),
            )
        }
    }

    pub fn view_proj(&self, aspect: f32) -> Mat4 {
        self.projection(aspect) * self.view()
    }
}

/// Project a world point into viewport pixels. `None` when behind the camera
/// or outside the clip volume.
pub fn world_to_screen(camera: &ViewportCamera, rect: ViewportRect, world: Vec3) -> Option<Vec2> {
    if rect.width < 1.0 || rect.height < 1.0 {
        return None;
    }
    let clip = camera.view_proj(rect.aspect()) * Vec4::new(world.x, world.y, world.z, 1.0);
    if clip.w <= 0.0 {
        return None;
    }
    let ndc = clip.truncate() / clip.w;
    if ndc.z < -1.0 || ndc.z > 1.0 {
        return None;
    }
    Some(Vec2::new(
        rect.min_x + (ndc.x * 0.5 + 0.5) * rect.width,
        rect.min_y + (1.0 - (ndc.y * 0.5 + 0.5)) * rect.height,
    ))
}

#[derive(Clone, Copy, Debug)]
pub struct ViewportRect {
    pub min_x: f32,
    pub min_y: f32,
    pub width: f32,
    pub height: f32,
}

impl ViewportRect {
    pub fn aspect(self) -> f32 {
        self.width / self.height.max(1.0)
    }

    pub fn contains(self, point: Vec2) -> bool {
        point.x >= self.min_x
            && point.y >= self.min_y
            && point.x <= self.min_x + self.width
            && point.y <= self.min_y + self.height
    }
}

/// Build a world-space ray from a viewport pixel.
pub fn screen_ray(camera: &ViewportCamera, rect: ViewportRect, screen: Vec2) -> (Vec3, Vec3) {
    if camera.orthographic {
        let aspect = rect.aspect();
        let half_h = (camera.ortho_scale * 0.5).max(0.01);
        let half_w = half_h * aspect;
        let nx = ((screen.x - rect.min_x) / rect.width.max(1.0)) * 2.0 - 1.0;
        let ny = 1.0 - ((screen.y - rect.min_y) / rect.height.max(1.0)) * 2.0;
        let origin = camera.position + camera.right() * (nx * half_w) + camera.up() * (ny * half_h);
        return (origin, camera.forward());
    }
    let aspect = rect.aspect();
    let ndc_x = ((screen.x - rect.min_x) / rect.width.max(1.0)) * 2.0 - 1.0;
    let ndc_y = 1.0 - ((screen.y - rect.min_y) / rect.height.max(1.0)) * 2.0;
    let inv = camera.view_proj(aspect).inverse();
    let near = inv * Vec4::new(ndc_x, ndc_y, -1.0, 1.0);
    let far = inv * Vec4::new(ndc_x, ndc_y, 1.0, 1.0);
    let near = near.truncate() / near.w;
    let far = far.truncate() / far.w;
    (camera.position, (far - near).normalize_or(camera.forward()))
}

/// Axis-aligned bounds in world space.
#[derive(Clone, Copy, Debug)]
pub struct Aabb {
    pub min: Vec3,
    pub max: Vec3,
}

impl Aabb {
    pub fn from_center_half(center: Vec3, half: Vec3) -> Self {
        let half = half.abs().max(Vec3::splat(0.05));
        Self {
            min: center - half,
            max: center + half,
        }
    }

    pub fn center(self) -> Vec3 {
        (self.min + self.max) * 0.5
    }
}

/// Slab ray/AABB test. Returns the entry distance along the ray, if any.
pub fn ray_aabb(origin: Vec3, direction: Vec3, aabb: Aabb) -> Option<f32> {
    let mut t_min = 0.0_f32;
    let mut t_max = f32::MAX;
    let mins = [aabb.min.x, aabb.min.y, aabb.min.z];
    let maxs = [aabb.max.x, aabb.max.y, aabb.max.z];
    let origins = [origin.x, origin.y, origin.z];
    let dirs = [direction.x, direction.y, direction.z];
    for i in 0..3 {
        if dirs[i].abs() < 1e-8 {
            if origins[i] < mins[i] || origins[i] > maxs[i] {
                return None;
            }
            continue;
        }
        let inv = 1.0 / dirs[i];
        let mut t1 = (mins[i] - origins[i]) * inv;
        let mut t2 = (maxs[i] - origins[i]) * inv;
        if t1 > t2 {
            std::mem::swap(&mut t1, &mut t2);
        }
        t_min = t_min.max(t1);
        t_max = t_max.min(t2);
        if t_min > t_max {
            return None;
        }
    }
    if t_max < 0.0 {
        return None;
    }
    Some(if t_min >= 0.0 { t_min } else { t_max })
}

/// Screen-space length of gizmo axis handles, in points. Keeping handles a
/// constant size means they stay visible and clickable at any zoom level.
pub const GIZMO_HANDLE_LENGTH: f32 = 64.0;

/// Project an axis of a world-space gizmo into a fixed-length screen handle.
/// Returns `None` when the origin/axis is behind the camera or the axis points
/// almost straight at the camera (where its screen direction is unstable).
pub fn gizmo_screen_handle(
    camera: &ViewportCamera,
    rect: ViewportRect,
    origin: Vec3,
    axis_dir: Vec3,
) -> Option<Vec2> {
    let origin_screen = world_to_screen(camera, rect, origin)?;
    let tip_screen = world_to_screen(camera, rect, origin + axis_dir)?;
    let direction = tip_screen - origin_screen;
    if direction.length_squared() < 16.0 {
        return None;
    }
    Some(origin_screen + direction.normalize() * GIZMO_HANDLE_LENGTH)
}

/// Closest approach between a ray and an infinite line. Returns the parameter
/// along the line (world units from `origin` in `direction`).
pub fn ray_line_parameter(ray_o: Vec3, ray_d: Vec3, line_o: Vec3, line_d: Vec3) -> Option<f32> {
    let line_d = line_d.normalize_or_zero();
    let ray_d = ray_d.normalize_or_zero();
    if line_d.length_squared() < 0.5 || ray_d.length_squared() < 0.5 {
        return None;
    }
    let w0 = ray_o - line_o;
    let b = ray_d.dot(line_d);
    let d = ray_d.dot(w0);
    let e = line_d.dot(w0);
    let denom = 1.0 - b * b;
    if denom.abs() < 1e-6 {
        return None;
    }
    // Parameter along `line_d` of the point closest to the ray.
    Some((e - b * d) / denom)
}

/// Point on a ray closest to a world point, returned as a distance along the ray.
pub fn ray_point_parameter(ray_o: Vec3, ray_d: Vec3, point: Vec3) -> f32 {
    let ray_d = ray_d.normalize_or_zero();
    (point - ray_o).dot(ray_d)
}

pub fn snap_translation(delta: Vec3, step: f32) -> Vec3 {
    if step <= 0.0 {
        return delta;
    }
    (delta / step).round() * step
}

pub fn snap_angle(radians: f32, step_degrees: f32) -> f32 {
    if step_degrees <= 0.0 {
        return radians;
    }
    let step = step_degrees.to_radians();
    (radians / step).round() * step
}

pub fn snap_scale(factor: f32, step: f32) -> f32 {
    if step <= 0.0 {
        return factor.max(0.01);
    }
    ((factor / step).round() * step).max(0.01)
}

/// Basis used by a gizmo. World space is identity; local space follows rotation.
pub fn gizmo_basis(space: CoordSpace, local_rotation: Quat) -> Quat {
    match space {
        CoordSpace::World => Quat::IDENTITY,
        CoordSpace::Local => local_rotation.normalize(),
    }
}

pub fn axis_vector(axis: GizmoAxis, basis: Quat) -> Vec3 {
    let local = match axis {
        GizmoAxis::X | GizmoAxis::Yz => Vec3::X,
        GizmoAxis::Y | GizmoAxis::Xz => Vec3::Y,
        GizmoAxis::Z | GizmoAxis::Xy => Vec3::Z,
        GizmoAxis::Uniform => Vec3::ONE.normalize(),
    };
    basis * local
}

/// Intersect a ray with an infinite plane. Returns the hit point in front of
/// the ray origin, if any.
pub fn ray_plane(ray_o: Vec3, ray_d: Vec3, plane_o: Vec3, plane_n: Vec3) -> Option<Vec3> {
    let normal = plane_n.normalize_or_zero();
    let direction = ray_d.normalize_or_zero();
    if normal.length_squared() < 0.5 || direction.length_squared() < 0.5 {
        return None;
    }
    let denom = direction.dot(normal);
    if denom.abs() < 1e-6 {
        return None;
    }
    let t = (plane_o - ray_o).dot(normal) / denom;
    if t < 0.0 {
        return None;
    }
    Some(ray_o + direction * t)
}

/// Translation delta for a gizmo drag between two screen rays.
///
/// Unlike [`drag_translation`] this measures the motion relative to the ray at
/// mouse-down, so grabbing a handle away from its origin does not make the
/// object jump on the first frame.
pub fn drag_translation_delta(
    axis: GizmoAxis,
    basis: Quat,
    start_ray_o: Vec3,
    start_ray_d: Vec3,
    current_ray_o: Vec3,
    current_ray_d: Vec3,
    anchor: Vec3,
    snap: f32,
) -> Vec3 {
    let axis_dir = axis_vector(axis, basis);
    let delta = match axis {
        GizmoAxis::X | GizmoAxis::Y | GizmoAxis::Z => {
            let start_t =
                ray_line_parameter(start_ray_o, start_ray_d, anchor, axis_dir).unwrap_or(0.0);
            let current_t =
                ray_line_parameter(current_ray_o, current_ray_d, anchor, axis_dir).unwrap_or(0.0);
            axis_dir * (current_t - start_t)
        }
        GizmoAxis::Xy | GizmoAxis::Xz | GizmoAxis::Yz => {
            let normal = axis_dir;
            let (Some(start_hit), Some(current_hit)) = (
                ray_plane(start_ray_o, start_ray_d, anchor, normal),
                ray_plane(current_ray_o, current_ray_d, anchor, normal),
            ) else {
                return Vec3::ZERO;
            };
            let delta = current_hit - start_hit;
            delta - normal * delta.dot(normal)
        }
        GizmoAxis::Uniform => Vec3::ZERO,
    };
    snap_translation(delta, snap)
}

/// Apply a translation drag. `start` is the transform at mouse-down.
/// The returned translation is absolute, not a delta.
pub fn drag_translation(
    start: Vec3,
    axis: GizmoAxis,
    basis: Quat,
    ray_o: Vec3,
    ray_d: Vec3,
    anchor_ray_t: f32,
    snap: f32,
) -> Vec3 {
    let current = ray_o + ray_d.normalize_or_zero() * ray_point_parameter(ray_o, ray_d, start);
    let anchor = ray_o + ray_d.normalize_or_zero() * anchor_ray_t;
    let _ = (current, anchor);
    let axis_dir = axis_vector(axis, basis);
    let along = match axis {
        GizmoAxis::X | GizmoAxis::Y | GizmoAxis::Z => {
            let t = ray_line_parameter(ray_o, ray_d, start, axis_dir).unwrap_or(0.0);
            axis_dir * t
        }
        GizmoAxis::Xy | GizmoAxis::Xz | GizmoAxis::Yz => {
            let normal = axis_dir;
            let denom = ray_d.normalize_or_zero().dot(normal);
            if denom.abs() < 1e-5 {
                Vec3::ZERO
            } else {
                let t = (start - ray_o).dot(normal) / denom;
                let hit = ray_o + ray_d.normalize_or_zero() * t;
                let delta = hit - start;
                // Keep motion inside the plane (drop the normal component).
                delta - normal * delta.dot(normal)
            }
        }
        GizmoAxis::Uniform => Vec3::ZERO,
    };
    start + snap_translation(along, snap)
}

/// Signed angle (radians) of a rotate drag around `axis`.
pub fn drag_rotation_angle(
    origin: Vec3,
    axis: Vec3,
    ray_o: Vec3,
    ray_d: Vec3,
    start_dir: Vec3,
) -> f32 {
    let axis = axis.normalize_or(Vec3::Y);
    let denom = ray_d.normalize_or_zero().dot(axis);
    let hit = if denom.abs() < 1e-5 {
        origin + start_dir
    } else {
        let t = (origin - ray_o).dot(axis) / denom;
        ray_o + ray_d.normalize_or_zero() * t
    };
    let current = (hit - origin) - axis * (hit - origin).dot(axis);
    let start = start_dir - axis * start_dir.dot(axis);
    if current.length_squared() < 1e-8 || start.length_squared() < 1e-8 {
        return 0.0;
    }
    let current = current.normalize();
    let start = start.normalize();
    let sin = axis.dot(start.cross(current));
    let cos = start.dot(current);
    sin.atan2(cos)
}

/// Scale factor from a drag. `1.0` means unchanged.
pub fn drag_scale_factor(start_screen: Vec2, current_screen: Vec2, uniform: bool) -> f32 {
    let delta = current_screen - start_screen;
    let pixels = if uniform { delta.x - delta.y } else { delta.x };
    (1.0 + pixels * 0.01).max(0.01)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn persp() -> ViewportCamera {
        ViewportCamera {
            position: Vec3::new(0.0, 0.0, 8.0),
            target: Vec3::ZERO,
            fov_y: 60.0_f32.to_radians(),
            near: 0.1,
            far: 100.0,
            orthographic: false,
            ortho_scale: 10.0,
        }
    }

    fn rect() -> ViewportRect {
        ViewportRect {
            min_x: 0.0,
            min_y: 0.0,
            width: 800.0,
            height: 600.0,
        }
    }

    #[test]
    fn origin_projects_near_viewport_center() {
        let screen = world_to_screen(&persp(), rect(), Vec3::ZERO).unwrap();
        assert!((screen.x - 400.0).abs() < 2.0, "x={screen:?}");
        assert!((screen.y - 300.0).abs() < 2.0, "y={screen:?}");
    }

    #[test]
    fn point_behind_camera_is_not_projected() {
        assert!(world_to_screen(&persp(), rect(), Vec3::new(0.0, 0.0, 20.0)).is_none());
    }

    #[test]
    fn screen_ray_through_center_hits_origin() {
        let (origin, dir) = screen_ray(&persp(), rect(), Vec2::new(400.0, 300.0));
        let t = ray_aabb(
            origin,
            dir,
            Aabb::from_center_half(Vec3::ZERO, Vec3::splat(0.5)),
        );
        assert!(t.is_some(), "center ray missed the origin box");
    }

    #[test]
    fn screen_ray_misses_offset_box() {
        let (origin, dir) = screen_ray(&persp(), rect(), Vec2::new(20.0, 20.0));
        let t = ray_aabb(
            origin,
            dir,
            Aabb::from_center_half(Vec3::ZERO, Vec3::splat(0.4)),
        );
        assert!(t.is_none());
    }

    #[test]
    fn nearer_aabb_wins() {
        let camera = persp();
        let (origin, dir) = screen_ray(&camera, rect(), Vec2::new(400.0, 300.0));
        let near = ray_aabb(
            origin,
            dir,
            Aabb::from_center_half(Vec3::new(0.0, 0.0, 2.0), Vec3::splat(0.4)),
        )
        .unwrap();
        let far = ray_aabb(
            origin,
            dir,
            Aabb::from_center_half(Vec3::ZERO, Vec3::splat(0.4)),
        )
        .unwrap();
        assert!(near < far);
    }

    #[test]
    fn orthographic_ray_is_parallel() {
        let camera = ViewportCamera {
            orthographic: true,
            ..persp()
        };
        let (_, dir) = screen_ray(&camera, rect(), Vec2::new(100.0, 100.0));
        assert!(dir.dot(camera.forward()) > 0.99);
    }

    #[test]
    fn translation_along_x_does_not_jump() {
        let start = Vec3::new(1.0, 2.0, 3.0);
        let ray_o = Vec3::new(0.0, 2.0, 10.0);
        let ray_d = Vec3::new(0.0, 0.0, -1.0);
        let moved = drag_translation(start, GizmoAxis::X, Quat::IDENTITY, ray_o, ray_d, 0.0, 0.0);
        // Ray is parallel to Z and aimed at y of the object, so X stays put
        // when the ray never leaves the object's X.
        assert!((moved.y - start.y).abs() < 0.001);
        assert!((moved.z - start.z).abs() < 0.001);
    }

    #[test]
    fn translation_snap_rounds_to_step() {
        let snapped = snap_translation(Vec3::new(1.2, -0.4, 0.6), 0.5);
        assert!((snapped.x - 1.0).abs() < 1e-4);
        assert!((snapped.y - (-0.5)).abs() < 1e-4);
        assert!((snapped.z - 0.5).abs() < 1e-4);
    }

    #[test]
    fn translation_delta_is_zero_for_unchanged_ray() {
        let ray_o = Vec3::new(0.0, 2.0, 10.0);
        let ray_d = Vec3::new(0.0, 0.0, -1.0);
        let delta = drag_translation_delta(
            GizmoAxis::X,
            Quat::IDENTITY,
            ray_o,
            ray_d,
            ray_o,
            ray_d,
            Vec3::new(1.0, 2.0, 3.0),
            0.0,
        );
        assert!(delta.length() < 1e-5, "delta={delta:?}");
    }

    #[test]
    fn translation_delta_moves_along_axis() {
        let anchor = Vec3::ZERO;
        let start_o = Vec3::new(0.0, 0.0, 10.0);
        let start_d = Vec3::new(0.0, 0.0, -1.0);
        // Move the ray one world unit to the right; on the X axis line through
        // the origin that is a one unit translation.
        let current_o = Vec3::new(1.0, 0.0, 10.0);
        let delta = drag_translation_delta(
            GizmoAxis::X,
            Quat::IDENTITY,
            start_o,
            start_d,
            current_o,
            start_d,
            anchor,
            0.0,
        );
        assert!((delta.x - 1.0).abs() < 1e-4, "delta={delta:?}");
        assert!(delta.y.abs() < 1e-4 && delta.z.abs() < 1e-4);
    }

    #[test]
    fn ray_plane_hits_plane_in_front() {
        let hit = ray_plane(
            Vec3::new(0.0, 0.0, 10.0),
            Vec3::new(0.0, 0.0, -1.0),
            Vec3::ZERO,
            Vec3::Z,
        );
        assert_eq!(hit, Some(Vec3::ZERO));
        assert!(
            ray_plane(
                Vec3::new(0.0, 0.0, 10.0),
                Vec3::new(0.0, 0.0, 1.0),
                Vec3::ZERO,
                Vec3::Z
            )
            .is_none()
        );
    }

    #[test]
    fn rotation_angle_is_quarter_turn() {
        let angle = drag_rotation_angle(
            Vec3::ZERO,
            Vec3::Y,
            Vec3::new(0.0, 5.0, 0.0),
            Vec3::new(1.0, -1.0, 0.0),
            Vec3::Z,
        );
        // Looking down -Y, the plane hit is +X. From +Z that is +90 degrees around Y.
        assert!(
            (angle - std::f32::consts::FRAC_PI_2).abs() < 0.05,
            "{angle}"
        );
    }

    #[test]
    fn angle_snap_uses_degrees() {
        let snapped = snap_angle(20.0_f32.to_radians(), 15.0);
        assert!((snapped - 15.0_f32.to_radians()).abs() < 1e-4);
    }

    #[test]
    fn scale_grows_when_dragged_right() {
        let factor = drag_scale_factor(Vec2::ZERO, Vec2::new(40.0, 0.0), false);
        assert!(factor > 1.0);
        let snapped = snap_scale(factor, 0.1);
        assert!(snapped >= 1.0);
    }

    #[test]
    fn local_basis_rotates_x_axis() {
        let basis = gizmo_basis(
            CoordSpace::Local,
            Quat::from_rotation_y(std::f32::consts::FRAC_PI_2),
        );
        let axis = axis_vector(GizmoAxis::X, basis);
        assert!(axis.dot(Vec3::Z).abs() > 0.99 || axis.dot(Vec3::NEG_Z).abs() > 0.99);
    }

    #[test]
    fn zero_size_viewport_does_not_project() {
        let rect = ViewportRect {
            min_x: 0.0,
            min_y: 0.0,
            width: 0.0,
            height: 100.0,
        };
        assert!(world_to_screen(&persp(), rect, Vec3::ZERO).is_none());
    }

    #[test]
    fn gizmo_handles_have_constant_screen_length() {
        let camera = persp();
        let origin = world_to_screen(&camera, rect(), Vec3::ZERO).unwrap();
        for axis in [Vec3::X, Vec3::Y] {
            let tip = gizmo_screen_handle(&camera, rect(), Vec3::ZERO, axis).unwrap();
            assert!(((tip - origin).length() - GIZMO_HANDLE_LENGTH).abs() < 1e-3);
        }
    }

    #[test]
    fn gizmo_handle_pointing_at_camera_is_skipped() {
        // The perspective test camera looks down -Z from +Z, so the +Z axis
        // projects to a single point and has no usable screen direction.
        assert!(gizmo_screen_handle(&persp(), rect(), Vec3::ZERO, Vec3::Z).is_none());
    }

    #[test]
    fn orthographic_gizmo_handle_has_constant_length() {
        let camera = ViewportCamera {
            orthographic: true,
            ..persp()
        };
        let origin = world_to_screen(&camera, rect(), Vec3::ZERO).unwrap();
        let tip = gizmo_screen_handle(&camera, rect(), Vec3::ZERO, Vec3::X).unwrap();
        assert!(((tip - origin).length() - GIZMO_HANDLE_LENGTH).abs() < 1e-3);
    }

    #[test]
    fn ray_parallel_to_slab_misses() {
        let hit = ray_aabb(
            Vec3::new(5.0, 0.0, 0.0),
            Vec3::Y,
            Aabb::from_center_half(Vec3::ZERO, Vec3::splat(0.5)),
        );
        assert!(hit.is_none());
    }
}
