#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PointerPhase {
    Down,
    Move,
    Up,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PointerSample {
    pub phase: PointerPhase,
    pub x: f32,
    pub y: f32,
    pub pressure: f32,
    pub tilt: [f32; 2],
    pub time: f64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PointerState {
    pub x: f32,
    pub y: f32,
    pub pressure: f32,
    pub tilt: [f32; 2],
    pub pressed: bool,
    pub fresh_press: bool,
}

#[derive(Default)]
pub struct PointerTrack {
    samples: std::collections::VecDeque<PointerSample>,
    current: Option<PointerSample>,
}

impl PointerTrack {
    pub fn push(&mut self, sample: PointerSample) {
        self.samples.push_back(sample);
    }

    pub fn state_at(&mut self, time: f64) -> Option<PointerState> {
        let mut fresh_press = false;
        while self.samples.front().is_some_and(|s| s.time <= time) {
            self.current = self.samples.pop_front();
            fresh_press |= self.current.is_some_and(|s| s.phase == PointerPhase::Down);
        }
        let current = self.current?;
        let pressed = current.phase != PointerPhase::Up;
        let next = self.samples.front().filter(|n| pressed && n.phase != PointerPhase::Down);
        let blend = |a: f32, b: f32, t: f32| a + (b - a) * t;
        let (x, y, pressure, tilt) = match next {
            Some(next) if next.time > current.time => {
                let t = ((time - current.time) / (next.time - current.time)).clamp(0.0, 1.0) as f32;
                (
                    blend(current.x, next.x, t),
                    blend(current.y, next.y, t),
                    blend(current.pressure, next.pressure, t),
                    [blend(current.tilt[0], next.tilt[0], t), blend(current.tilt[1], next.tilt[1], t)],
                )
            }
            _ => (current.x, current.y, current.pressure, current.tilt),
        };
        Some(PointerState { x, y, pressure, tilt, pressed, fresh_press })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(phase: PointerPhase, x: f32, time: f64) -> PointerSample {
        PointerSample { phase, x, y: 0.0, pressure: 1.0, tilt: [0.0; 2], time }
    }

    #[test]
    fn interpolates_between_samples_while_pressed() {
        let mut track = PointerTrack::default();
        track.push(sample(PointerPhase::Down, 0.0, 0.0));
        track.push(sample(PointerPhase::Move, 10.0, 1.0));
        let state = track.state_at(0.25).unwrap();
        assert!((state.x - 2.5).abs() < 1e-6);
        assert!(state.pressed);
    }

    #[test]
    fn holds_last_position_after_release() {
        let mut track = PointerTrack::default();
        track.push(sample(PointerPhase::Down, 0.0, 0.0));
        track.push(sample(PointerPhase::Move, 10.0, 1.0));
        track.push(sample(PointerPhase::Up, 10.0, 2.0));
        let state = track.state_at(3.0).unwrap();
        assert_eq!(state.x, 10.0);
        assert!(!state.pressed);
    }

    #[test]
    fn does_not_interpolate_towards_a_new_press() {
        let mut track = PointerTrack::default();
        track.push(sample(PointerPhase::Up, 0.0, 0.0));
        track.push(sample(PointerPhase::Down, 100.0, 1.0));
        assert_eq!(track.state_at(0.5).unwrap().x, 0.0);
    }

    #[test]
    fn reports_a_fresh_press_once() {
        let mut track = PointerTrack::default();
        track.push(sample(PointerPhase::Down, 0.0, 1.0));
        assert!(track.state_at(0.5).is_none());
        assert!(track.state_at(1.0).unwrap().fresh_press);
        assert!(!track.state_at(1.5).unwrap().fresh_press);
    }

    #[test]
    fn no_state_before_any_sample() {
        let mut track = PointerTrack::default();
        assert!(track.state_at(1.0).is_none());
    }
}
