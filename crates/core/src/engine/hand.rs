use super::{Engine, LIFTED_HEIGHT, Tool};
use crate::brush::{BrushPose, MAX_COMPRESSION, working_point};
use crate::paint::NOZZLE_RADIUS;
use glam::{Quat, Vec2, Vec3};

const PEN_TO_BRUSH_LEAN: f32 = 0.6;
const MAX_LEAN: f32 = 0.5;
const BLADE_FLEX: f32 = 0.75;
const KNIFE_RESTING_WEIGHT: f32 = 0.4;

impl Engine {
    pub(super) fn handle_position(&self) -> Vec2 {
        Vec2::new(self.pose.position.x, self.pose.position.y)
    }

    pub(super) fn tool_tip(&self) -> Vec3 {
        match self.tool {
            Tool::Brush => self.pose.position,
            Tool::Tube => self.touch.position.extend(0.0),
        }
    }

    pub fn nozzle_radius(&self) -> f32 {
        NOZZLE_RADIUS * self.adjustments.scale.clamp(0.1, 3.0)
    }

    pub(super) fn handle_heights(&self) -> (f32, f32) {
        let preset = self.brush.preset();
        let (compression, press) = if preset.handle_lean > 0.0 {
            (BLADE_FLEX, KNIFE_RESTING_WEIGHT + (1.0 - KNIFE_RESTING_WEIGHT) * self.touch.pressure)
        } else {
            (MAX_COMPRESSION, self.touch.pressure)
        };
        (preset.length * LIFTED_HEIGHT, preset.length * (1.0 - compression * press))
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
        let preset = self.brush.preset();
        let held_lean = preset.handle_lean;
        let (heading, mut spin_radians) = if held_lean > 0.0 {
            (Vec2::from_angle(self.brush_angle_degrees.to_radians()).rotate(self.touch.heading()), self.touch.rotation.to_radians())
        } else {
            (self.touch.heading(), (self.brush_angle_degrees + self.touch.rotation).to_radians())
        };
        if preset.turns_with_tilt() && heading != Vec2::Y {
            spin_radians += heading.y.atan2(heading.x) - std::f32::consts::FRAC_PI_2;
        }
        let lean = if held_lean > 0.0 { heading * held_lean } else { pen_lean(self.touch.tilt) };
        let handle = handle_direction(lean);
        let spin = Quat::from_rotation_z(spin_radians);
        let mut pose = BrushPose {
            position: self.touch.position.extend(0.0) + handle * (lifted + (pressed - lifted) * self.touch.depth()),
            orientation: Quat::from_rotation_arc(Vec3::Z, handle) * spin,
        };
        if held_lean > 0.0 {
            let working = working_point(&pose, self.brush.preset().length);
            pose.position += (self.touch.position - working.truncate()).extend(0.0);
        }
        pose
    }
}

fn pen_lean(tilt: [f32; 2]) -> Vec2 {
    (Vec2::new(tilt[0], -tilt[1]) * PEN_TO_BRUSH_LEAN).clamp_length_max(MAX_LEAN)
}

fn handle_direction(lean: Vec2) -> Vec3 {
    Vec3::new(lean.x, lean.y, (1.0 - lean.length_squared()).max(0.05).sqrt()).normalize()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    pub(super) fn upright_pen_gives_vertical_handle() {
        assert!((handle_direction(pen_lean([0.0, 0.0])) - Vec3::Z).length() < 1e-6);
    }

    #[test]
    pub(super) fn tilt_is_limited_and_unit_length() {
        let handle = handle_direction(pen_lean([1.0, 1.0]));
        assert!((handle.length() - 1.0).abs() < 1e-5);
        assert!(handle.z > 0.3, "handle should never lie flat, got {handle}");
    }
}
