use super::{Engine, LIFTED_HEIGHT};
use crate::brush::{BrushPose, MAX_COMPRESSION};
use glam::{Quat, Vec2, Vec3};

const PEN_TO_BRUSH_LEAN: f32 = 0.6;
const MAX_LEAN: f32 = 0.5;

impl Engine {
    pub(super) fn handle_position(&self) -> Vec2 {
        Vec2::new(self.pose.position.x, self.pose.position.y)
    }

    pub(super) fn handle_heights(&self) -> (f32, f32) {
        let length = self.brush.preset().length;
        (length * LIFTED_HEIGHT, length * (1.0 - MAX_COMPRESSION * self.touch.pressure))
    }

    pub(super) fn contact(&self) -> f32 {
        let depth = self.touch.depth();
        if depth >= 1.0 {
            return 1.0;
        }
        let (lifted, pressed) = self.handle_heights();
        let reaching = (lifted - self.brush.preset().length) / (lifted - pressed);
        ((depth - reaching) / (1.0 - reaching).max(1e-3)).clamp(0.0, 1.0)
    }

    pub(super) fn pose_for(&self) -> BrushPose {
        let (lifted, pressed) = self.handle_heights();
        let handle = handle_direction(self.touch.tilt);
        let spin = Quat::from_rotation_z((self.brush_angle_degrees + self.touch.rotation).to_radians());
        BrushPose {
            position: self.touch.position.extend(0.0) + handle * (lifted + (pressed - lifted) * self.touch.depth()),
            orientation: Quat::from_rotation_arc(Vec3::Z, handle) * spin,
        }
    }
}

fn handle_direction(tilt: [f32; 2]) -> Vec3 {
    let lean = (Vec2::new(tilt[0], -tilt[1]) * PEN_TO_BRUSH_LEAN).clamp_length_max(MAX_LEAN);
    Vec3::new(lean.x, lean.y, (1.0 - lean.length_squared()).max(0.05).sqrt()).normalize()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    pub(super) fn upright_pen_gives_vertical_handle() {
        assert!((handle_direction([0.0, 0.0]) - Vec3::Z).length() < 1e-6);
    }

    #[test]
    pub(super) fn tilt_is_limited_and_unit_length() {
        let handle = handle_direction([1.0, 1.0]);
        assert!((handle.length() - 1.0).abs() < 1e-5);
        assert!(handle.z > 0.3, "handle should never lie flat, got {handle}");
    }
}
