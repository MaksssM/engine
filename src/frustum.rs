//! Camera frustum extraction and AABB/sphere culling.
//!
//! Used by the renderer to reject renderables whose world-space bounds fall
//! entirely outside the camera frustum before they are submitted for drawing.

use glam::{Mat4, Vec3, Vec4};
use std::ops::{Add, Sub};

/// A plane in the form `normal · p + d = 0`, normalized so `d` is a true distance.
#[derive(Clone, Copy, Debug)]
pub struct Plane {
    pub normal: Vec3,
    pub d: f32,
}

impl Plane {
    #[inline]
    pub fn normalized(self) -> Self {
        let len = self.normal.length();
        if len > 0.0 {
            Self {
                normal: self.normal / len,
                d: self.d / len,
            }
        } else {
            self
        }
    }

    /// Signed distance from the plane to a point. Positive is inside.
    #[inline]
    pub fn signed_distance(&self, point: Vec3) -> f32 {
        self.normal.dot(point) + self.d
    }
}

/// Six planes bounding the visible volume of a camera. Planes point inward.
#[derive(Clone, Copy, Debug)]
pub struct Frustum {
    pub planes: [Plane; 6],
}

impl Frustum {
    /// Extract frustum planes from a combined view-projection matrix using
    /// the Gribb-Hartmann method. Caller should ensure the matrix is the
    /// clip-space transform (WebGPU/WebGL style, depth in `[0, 1]`).
    pub fn from_view_proj(view_proj: &Mat4) -> Self {
        let m = view_proj.to_cols_array();
        // Rows of the matrix.
        let row0 = Vec4::new(m[0], m[4], m[8], m[12]);
        let row1 = Vec4::new(m[1], m[5], m[9], m[13]);
        let row2 = Vec4::new(m[2], m[6], m[10], m[14]);
        let row3 = Vec4::new(m[3], m[7], m[11], m[15]);

        // For depth in [0, 1] (WebGPU), near plane uses row2 directly and
        // far plane uses row3 - row2.
        let left = row3.add(row0);
        let right = row3.sub(row0);
        let bottom = row3.add(row1);
        let top = row3.sub(row1);
        let near = row2;
        let far = row3.sub(row2);

        let planes = [
            Plane::from_row(left).normalized(),
            Plane::from_row(right).normalized(),
            Plane::from_row(bottom).normalized(),
            Plane::from_row(top).normalized(),
            Plane::from_row(near).normalized(),
            Plane::from_row(far).normalized(),
        ];
        Self { planes }
    }

    /// Conservative sphere-vs-frustum test. Returns true if the sphere is at
    /// least partially inside (or touching) the frustum.
    #[inline]
    pub fn intersects_sphere(&self, center: Vec3, radius: f32) -> bool {
        for plane in &self.planes {
            let distance = plane.signed_distance(center);
            if distance < -radius {
                return false;
            }
        }
        true
    }
}

impl Plane {
    #[inline]
    fn from_row(row: Vec4) -> Self {
        Self {
            normal: row.truncate(),
            d: row.w,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use glam::{Mat4, Vec3};

    fn perspective(fov_y: f32, aspect: f32, near: f32, far: f32) -> Mat4 {
        Mat4::perspective_rh(fov_y, aspect, near, far)
    }

    #[test]
    fn origin_visible_in_default_frustum() {
        let proj = perspective(1.0, 1.0, 0.1, 100.0);
        let view = Mat4::look_at_rh(Vec3::new(0.0, 0.0, 5.0), Vec3::ZERO, Vec3::Y);
        let frustum = Frustum::from_view_proj(&(proj * view));
        assert!(frustum.intersects_sphere(Vec3::ZERO, 0.5));
    }

    #[test]
    fn point_behind_camera_is_culled() {
        let proj = perspective(1.0, 1.0, 0.1, 100.0);
        let view = Mat4::look_at_rh(Vec3::new(0.0, 0.0, 5.0), Vec3::ZERO, Vec3::Y);
        let frustum = Frustum::from_view_proj(&(proj * view));
        // Camera looks down -Z from z=5; a point at z=50 is behind it.
        assert!(!frustum.intersects_sphere(Vec3::new(0.0, 0.0, 50.0), 0.1));
    }

    #[test]
    fn point_far_off_axis_is_culled() {
        let proj = perspective(1.0, 1.0, 0.1, 100.0);
        let view = Mat4::look_at_rh(Vec3::new(0.0, 0.0, 5.0), Vec3::ZERO, Vec3::Y);
        let frustum = Frustum::from_view_proj(&(proj * view));
        assert!(!frustum.intersects_sphere(Vec3::new(1000.0, 0.0, 0.0), 0.1));
    }

    #[test]
    fn point_beyond_far_plane_is_culled() {
        let proj = perspective(1.0, 1.0, 0.1, 10.0);
        let view = Mat4::look_at_rh(Vec3::new(0.0, 0.0, 5.0), Vec3::ZERO, Vec3::Y);
        let frustum = Frustum::from_view_proj(&(proj * view));
        // z=-50 is 55 units away, well beyond far=10.
        assert!(!frustum.intersects_sphere(Vec3::new(0.0, 0.0, -50.0), 0.1));
    }

    #[test]
    fn sphere_straddling_plane_is_kept() {
        let proj = perspective(1.0, 1.0, 0.1, 100.0);
        let view = Mat4::look_at_rh(Vec3::new(0.0, 0.0, 5.0), Vec3::ZERO, Vec3::Y);
        let frustum = Frustum::from_view_proj(&(proj * view));
        // A large sphere centered off-screen but overlapping the frustum is kept.
        assert!(frustum.intersects_sphere(Vec3::new(50.0, 0.0, 0.0), 80.0));
    }
}
