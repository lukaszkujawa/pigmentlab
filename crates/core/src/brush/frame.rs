use glam::{Quat, Vec3};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BrushPose {
    pub position: Vec3,
    pub orientation: Quat,
}

impl BrushPose {
    pub fn upright(position: Vec3) -> Self {
        Self { position, orientation: Quat::IDENTITY }
    }

    pub fn canvas_to_brush(&self) -> Quat {
        self.orientation.inverse()
    }

    pub fn to_canvas(&self, brush_frame: Vec3) -> Vec3 {
        self.orientation * brush_frame + self.position
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_between_frames() {
        let pose = BrushPose { position: Vec3::new(5.0, -2.0, 3.0), orientation: Quat::from_rotation_z(0.7) };
        let brush_frame = Vec3::new(1.0, 2.0, -3.0);
        let canvas = pose.to_canvas(brush_frame);
        let back = pose.canvas_to_brush() * (canvas - pose.position);
        assert!((back - brush_frame).length() < 1e-5);
    }
}
