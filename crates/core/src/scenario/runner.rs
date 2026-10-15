use super::compare::{Comparison, compare, difference_image};
use super::{Action, Scenario};
use crate::engine::{Engine, SIMULATION_DT};
use crate::gpu::Gpu;
use crate::input::{PointerPhase, PointerSample};
use serde::Serialize;
use std::path::Path;
use std::time::Instant;

#[derive(Debug, Serialize)]
pub struct CheckpointResult {
    pub name: String,
    pub elapsed_ms: f64,
    pub comparison: Option<Comparison>,
}

#[derive(Debug, Serialize)]
pub struct Report {
    pub total_ms: f64,
    pub steps_simulated: u64,
    pub checkpoints: Vec<CheckpointResult>,
}

impl Report {
    pub fn passed(&self) -> bool {
        self.checkpoints.iter().all(|c| c.comparison.is_none_or(|c| c.passed))
    }
}

pub fn run(scenario: &Scenario, baseline_dir: &Path, update_baselines: bool) -> Report {
    let started = Instant::now();
    let mut engine = Engine::new(Gpu::headless(), scenario.canvas, scenario.seed);
    engine.set_show_bristles(scenario.show_bristles);
    engine.set_fluid_enabled(scenario.fluid);
    let mut checkpoints = Vec::new();
    let mut last_position = (0.0, 0.0);

    for step in &scenario.steps {
        if !step.action.is_pointer_input() {
            engine.advance_exact(step.t);
        }
        match &step.action {
            Action::Press { x, y, pressure } => {
                last_position = (*x, *y);
                engine.pointer(sample(PointerPhase::Down, *x, *y, *pressure, step.t));
            }
            Action::Move { x, y, pressure } => {
                last_position = (*x, *y);
                engine.pointer(sample(PointerPhase::Move, *x, *y, *pressure, step.t));
            }
            Action::Release => {
                let (x, y) = last_position;
                engine.pointer(sample(PointerPhase::Up, x, y, 0.0, step.t));
            }
            Action::Checkpoint { name } => {
                engine.render();
                let actual = engine.read_back();
                let comparison = check(&actual, baseline_dir, name, scenario, update_baselines);
                checkpoints.push(CheckpointResult {
                    name: name.clone(),
                    elapsed_ms: started.elapsed().as_secs_f64() * 1000.0,
                    comparison,
                });
            }
            Action::Dip { pigment } => {
                let pigment = crate::pigment::by_name(pigment).unwrap_or_else(|| panic!("unknown pigment {pigment}"));
                engine.dip(pigment);
            }
            Action::Clean => engine.clean(),
            Action::Undo => {
                engine.undo();
            }
            Action::Clear => engine.clear(),
            Action::Dry => engine.dry(),
            Action::Tool { .. } | Action::Param { .. } => {}
        }
    }

    let steps_simulated = (engine.time() / f64::from(SIMULATION_DT)).round() as u64;
    Report { total_ms: started.elapsed().as_secs_f64() * 1000.0, steps_simulated, checkpoints }
}

fn sample(phase: PointerPhase, x: f32, y: f32, pressure: f32, time: f64) -> PointerSample {
    PointerSample { phase, x, y, pressure, time }
}

fn check(
    actual: &image::RgbaImage,
    baseline_dir: &Path,
    name: &str,
    scenario: &Scenario,
    update_baselines: bool,
) -> Option<Comparison> {
    let baseline_path = baseline_dir.join(format!("{name}.png"));
    if update_baselines {
        std::fs::create_dir_all(baseline_dir).expect("cannot create baseline dir");
        actual.save(&baseline_path).expect("cannot write baseline");
        return None;
    }
    let expected = image::open(&baseline_path)
        .unwrap_or_else(|e| panic!("missing baseline {}: {e}", baseline_path.display()))
        .into_rgba8();
    let comparison = compare(actual, &expected, scenario.tolerance);
    if !comparison.passed {
        actual.save(baseline_dir.join(format!("{name}.actual.png"))).expect("cannot write actual");
        difference_image(actual, &expected)
            .save(baseline_dir.join(format!("{name}.diff.png")))
            .expect("cannot write diff");
    }
    Some(comparison)
}
