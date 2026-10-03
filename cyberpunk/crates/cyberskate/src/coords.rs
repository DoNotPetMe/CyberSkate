//! Night City and the skate engine share metres and a right-handed frame but
//! not an up axis: Cyberpunk is z-up with y north, the skate world is y-up.
//! The skater's local forward is +z in skate space.
use bevy::math::{Mat3, Mat4, Quat, Vec3};

pub fn to_skate(p: Vec3) -> Vec3 {
    Vec3::new(p.x, p.z, -p.y)
}

pub fn from_skate(p: Vec3) -> Vec3 {
    Vec3::new(p.x, -p.z, p.y)
}

/// Skate heading (rotation about skate +y) whose forward is `forward`, a
/// Night City direction; only its horizontal part counts.
pub fn heading(forward: Vec3) -> Option<f32> {
    let f = to_skate(Vec3::new(forward.x, forward.y, 0.)).normalize_or_zero();
    (f != Vec3::ZERO).then(|| f.x.atan2(f.z))
}

/// A rigid frame in Night City coordinates.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Frame {
    pub position: Vec3,
    pub forward: Vec3,
    pub up: Vec3,
}

impl Frame {
    pub const IDENTITY: Self = Self {
        position: Vec3::ZERO,
        forward: Vec3::Y,
        up: Vec3::Z,
    };

    /// Skate-space affine matrix whose z column is forward and y column up.
    pub fn from_skate_matrix(m: Mat4) -> Self {
        Self {
            position: from_skate(m.w_axis.truncate()),
            forward: from_skate(m.z_axis.truncate()).normalize_or(Vec3::Y),
            up: from_skate(m.y_axis.truncate()).normalize_or(Vec3::Z),
        }
    }

    pub fn from_skate_basis(position: Vec3, basis: Mat3) -> Self {
        Self {
            position: from_skate(position),
            forward: from_skate(basis.z_axis).normalize_or(Vec3::Y),
            up: from_skate(basis.y_axis).normalize_or(Vec3::Z),
        }
    }

    /// The frame `t` of the way from `self` to `to`.
    pub fn lerp(&self, to: &Frame, t: f32) -> Frame {
        Frame {
            position: self.position.lerp(to.position, t),
            forward: self.forward.lerp(to.forward, t).normalize_or(to.forward),
            up: self.up.lerp(to.up, t).normalize_or(to.up),
        }
    }

    /// Horizontal heading of `forward`, radians counter-clockwise from +y
    /// seen from above.
    pub fn yaw(&self) -> f32 {
        (-self.forward.x).atan2(self.forward.y)
    }

    /// Elevation of `forward` above the horizon, radians.
    pub fn pitch(&self) -> f32 {
        self.forward.z.clamp(-1., 1.).asin()
    }

    /// Orientation taking Night City's local axes (x right, y forward,
    /// z up) onto this frame, re-orthogonalised around `forward`.
    pub fn rotation(&self) -> Quat {
        let forward = self.forward.normalize_or(Vec3::Y);
        let right = forward.cross(self.up).normalize_or_zero();
        let right = if right == Vec3::ZERO {
            forward.any_orthonormal_vector()
        } else {
            right
        };
        let up = right.cross(forward);
        Quat::from_mat3(&Mat3::from_cols(right, forward, up)).normalize()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f32::consts::{FRAC_PI_2, PI};

    fn close(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-4
    }

    #[test]
    fn conversion_round_trips_and_keeps_handedness() {
        let p = Vec3::new(-1342.5, 211.25, 17.75);
        assert!(from_skate(to_skate(p)).distance(p) < 1e-4);
        assert_eq!(to_skate(Vec3::Z), Vec3::Y);
        let basis = Mat3::from_cols(to_skate(Vec3::X), to_skate(Vec3::Y), to_skate(Vec3::Z));
        assert!(close(basis.determinant(), 1.));
    }

    #[test]
    fn heading_matches_skate_rotation_about_up() {
        for forward in [Vec3::Y, Vec3::X, -Vec3::X, Vec3::new(0.6, -0.8, 0.)] {
            let h = heading(forward).unwrap();
            let skate_forward = Quat::from_rotation_y(h) * Vec3::Z;
            assert!(from_skate(skate_forward).distance(forward.normalize()) < 1e-5);
        }
        assert_eq!(heading(Vec3::Z), None);
    }

    #[test]
    fn frame_angles() {
        let north = Frame::IDENTITY;
        assert!(close(north.yaw(), 0.));
        let west = Frame {
            forward: -Vec3::X,
            ..north
        };
        assert!(close(west.yaw(), FRAC_PI_2));
        let south = Frame {
            forward: -Vec3::Y,
            ..north
        };
        assert!(close(south.yaw().abs(), PI));
        let up = Frame {
            forward: Vec3::new(0., 1., 1.).normalize(),
            up: Vec3::new(0., -1., 1.).normalize(),
            ..north
        };
        assert!(close(up.pitch(), PI / 4.));
        assert!((up.rotation() * Vec3::Y).distance(up.forward) < 1e-5);
        assert!((up.rotation() * Vec3::Z).distance(up.up) < 1e-5);
        assert!((west.rotation() * Vec3::X).distance(Vec3::Y) < 1e-5);
    }

    #[test]
    fn lerp_meets_both_ends_and_stays_unit() {
        let a = Frame::IDENTITY;
        let b = Frame {
            position: Vec3::new(2., 0., 0.),
            forward: Vec3::X,
            up: Vec3::Z,
        };
        assert_eq!(a.lerp(&b, 0.), a);
        assert!(a.lerp(&b, 1.).forward.distance(Vec3::X) < 1e-6);
        let mid = a.lerp(&b, 0.5);
        assert!(mid.position.distance(Vec3::new(1., 0., 0.)) < 1e-6);
        assert!((mid.forward.length() - 1.).abs() < 1e-6);
        // Opposite directions have no midpoint; the destination is kept.
        let back = Frame {
            forward: -Vec3::Y,
            ..a
        };
        assert_eq!(a.lerp(&back, 0.5).forward, -Vec3::Y);
    }

    #[test]
    fn skate_matrix_columns_become_forward_and_up() {
        let m = Mat4::from_rotation_translation(
            Quat::from_rotation_y(FRAC_PI_2),
            to_skate(Vec3::new(5., 6., 7.)),
        );
        let f = Frame::from_skate_matrix(m);
        assert!(f.position.distance(Vec3::new(5., 6., 7.)) < 1e-5);
        assert!(f.forward.distance(Vec3::X) < 1e-5);
        assert!(f.up.distance(Vec3::Z) < 1e-5);
    }
}
