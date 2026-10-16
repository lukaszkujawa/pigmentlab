use crate::input::PointerState;
use glam::Vec2;

const TOUCHDOWN_SECONDS: f32 = 0.06;
const LIFT_SECONDS: f32 = 0.1;
const VELOCITY_SMOOTHING: f32 = 0.2;
const LIFT_DRIFT_KEPT_PER_SECOND: f32 = 0.02;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Touch {
    engagement: f32,
    pub position: Vec2,
    velocity: Vec2,
    pub pressure: f32,
    pub tilt: [f32; 2],
    pub rotation: f32,
}

impl Touch {
    pub fn update(&mut self, state: &PointerState, dt: f32) {
        if state.pressed {
            let position = Vec2::new(state.x, state.y);
            if state.fresh_press {
                *self = Self { position, ..Self::default() };
            }
            self.velocity = self.velocity.lerp((position - self.position) / dt, VELOCITY_SMOOTHING);
            self.position = position;
            self.pressure = state.pressure.clamp(0.0, 1.0);
            self.tilt = state.tilt;
            self.rotation = state.rotation;
            self.engagement = (self.engagement + dt / TOUCHDOWN_SECONDS).min(1.0);
        } else if self.engaged() {
            self.position += self.velocity * dt;
            self.velocity *= LIFT_DRIFT_KEPT_PER_SECOND.powf(dt);
            self.engagement = (self.engagement - dt / LIFT_SECONDS).max(0.0);
        }
    }

    pub fn engaged(&self) -> bool {
        self.engagement > 0.0
    }

    pub fn depth(&self) -> f32 {
        let e = self.engagement;
        e * e * (3.0 - 2.0 * e)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state(x: f32, pressed: bool, fresh_press: bool) -> PointerState {
        PointerState { x, y: 0.0, pressure: 1.0, tilt: [0.0; 2], rotation: 0.0, pressed, fresh_press }
    }

    #[test]
    fn touches_down_gradually_and_lifts_while_drifting_on() {
        let dt = 1.0 / 240.0;
        let mut touch = Touch::default();
        touch.update(&state(0.0, true, true), dt);
        assert!(touch.depth() > 0.0 && touch.depth() < 0.1);
        for i in 1..=60 {
            touch.update(&state(i as f32 * 2.0, true, false), dt);
        }
        assert_eq!(touch.depth(), 1.0);
        let released_at = touch.position.x;
        touch.update(&state(120.0, false, false), dt);
        assert!(touch.engaged() && touch.depth() < 1.0);
        for _ in 0..30 {
            touch.update(&state(120.0, false, false), dt);
        }
        assert!(!touch.engaged());
        assert!(touch.position.x > released_at + 5.0, "the hand keeps moving while it lifts");
    }
}
