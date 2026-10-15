use super::{Action, Scenario, Step, Tolerance};
use crate::canvas::CanvasSize;
use crate::input::{PointerPhase, PointerSample};
use std::path::Path;

pub struct Recorder {
    canvas: CanvasSize,
    seed: u64,
    start: f64,
    steps: Vec<Step>,
}

impl Recorder {
    pub fn start(canvas: CanvasSize, seed: u64, start: f64) -> Self {
        Self { canvas, seed, start, steps: Vec::new() }
    }

    pub fn pointer(&mut self, sample: PointerSample) {
        let action = match sample.phase {
            PointerPhase::Down => Action::Press { x: sample.x, y: sample.y, pressure: sample.pressure, tilt: sample.tilt, rotation: sample.rotation },
            PointerPhase::Move => Action::Move { x: sample.x, y: sample.y, pressure: sample.pressure, tilt: sample.tilt, rotation: sample.rotation },
            PointerPhase::Up => Action::Release,
        };
        self.push(sample.time, action);
    }

    pub fn dip(&mut self, time: f64, pigment: &str) {
        self.push(time, Action::Dip { pigment: pigment.to_string() });
    }

    pub fn tool(&mut self, time: f64, name: &str) {
        self.push(time, Action::Tool { name: name.to_string() });
    }

    pub fn param(&mut self, time: f64, name: &str, value: f32) {
        self.push(time, Action::Param { name: name.to_string(), value });
    }

    pub fn clean(&mut self, time: f64) {
        self.push(time, Action::Clean);
    }

    pub fn dry(&mut self, time: f64) {
        self.push(time, Action::Dry);
    }

    pub fn undo(&mut self, time: f64) {
        self.push(time, Action::Undo);
    }

    pub fn clear(&mut self, time: f64) {
        self.push(time, Action::Clear);
    }

    fn push(&mut self, time: f64, action: Action) {
        self.steps.push(Step { t: time - self.start, action });
    }

    pub fn finish(mut self, time: f64) -> Scenario {
        self.push(time + 0.5, Action::Checkpoint { name: "final".into() });
        Scenario {
            seed: self.seed,
            canvas: self.canvas,
            tolerance: Tolerance::default(),
            show_bristles: false,
            fluid: true,
            steps: self.steps,
        }
    }

    pub fn save(self, time: f64, path: &Path) -> std::io::Result<()> {
        let scenario = self.finish(time);
        let text = serde_json::to_string_pretty(&scenario).expect("scenario serialises");
        std::fs::write(path, text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recorded_scenario_round_trips() {
        let mut recorder = Recorder::start(CanvasSize { width: 64, height: 64 }, 1, 10.0);
        recorder.dip(10.0, "cadmium_red");
        recorder.pointer(PointerSample { phase: PointerPhase::Down, x: 1.0, y: 2.0, pressure: 0.5, tilt: [0.0; 2], rotation: 0.0, time: 10.5 });
        recorder.pointer(PointerSample { phase: PointerPhase::Up, x: 1.0, y: 2.0, pressure: 0.5, tilt: [0.0; 2], rotation: 0.0, time: 11.0 });
        let scenario = recorder.finish(11.0);
        let text = serde_json::to_string(&scenario).unwrap();
        let parsed: Scenario = serde_json::from_str(&text).unwrap();
        assert_eq!(parsed.steps.len(), 4);
        assert_eq!(parsed.steps[1].t, 0.5);
        assert!(matches!(parsed.steps[3].action, Action::Checkpoint { .. }));
    }
}
