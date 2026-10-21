use super::compare::{Comparison, compare, difference_image};
use super::fractal::box_counting_dimension;
use super::{Action, Scenario, Zoom};
use crate::drip::Hanging;
use crate::engine::{Engine, SIMULATION_DT};
use glam::Vec2;
use crate::gpu::Gpu;
use crate::input::{Hover, PointerPhase, PointerSample};
use serde::Serialize;
use std::path::Path;
use std::time::Instant;

#[derive(Debug, Serialize)]
pub struct CheckpointResult {
    pub name: String,
    pub elapsed_ms: f64,
    pub fractal_dimension: Option<f64>,
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
        self.failures().next().is_none()
    }

    pub fn failures(&self) -> impl Iterator<Item = &CheckpointResult> {
        self.checkpoints.iter().filter(|c| c.comparison.is_some_and(|c| !c.passed))
    }
}

pub fn run(scenario: &Scenario, baseline_dir: &Path, update_baselines: bool) -> Report {
    let started = Instant::now();
    let mut engine = match scenario.palette {
        true => Engine::with_palette(Gpu::headless(), scenario.canvas, scenario.seed),
        false => Engine::new(Gpu::headless(), scenario.canvas, scenario.seed),
    };
    engine.set_show_bristles(scenario.show_bristles);
    engine.set_levelling_enabled(scenario.levelling);
    let mut checkpoints = Vec::new();
    let mut last_position = (0.0, 0.0);

    for step in &scenario.steps {
        if !step.action.is_pointer_input() {
            engine.advance_exact(step.t);
        }
        match &step.action {
            Action::Press { x, y, pressure, tilt, rotation } => {
                last_position = (*x, *y);
                engine.pointer(sample(PointerPhase::Down, *x, *y, *pressure, *tilt, *rotation, step.t));
            }
            Action::Move { x, y, pressure, tilt, rotation } => {
                last_position = (*x, *y);
                engine.pointer(sample(PointerPhase::Move, *x, *y, *pressure, *tilt, *rotation, step.t));
            }
            Action::Release => {
                let (x, y) = last_position;
                engine.pointer(sample(PointerPhase::Up, x, y, 0.0, [0.0; 2], 0.0, step.t));
            }
            Action::Hover { x, y, tilt, rotation } => engine.hover(Hover { x: *x, y: *y, tilt: *tilt, rotation: *rotation }),
            Action::Checkpoint { name, zoom } => {
                let actual = match zoom {
                    Some(zoom) => zoomed(&mut engine, *zoom),
                    None => engine.read_back(),
                };
                let comparison = check(&actual, baseline_dir, name, scenario, update_baselines);
                let size = engine.canvas_size();
                let fractal_dimension = box_counting_dimension(&engine.paint_heights(), size.width as usize, size.height as usize);
                checkpoints.push(CheckpointResult {
                    name: name.clone(),
                    elapsed_ms: started.elapsed().as_secs_f64() * 1000.0,
                    fractal_dimension,
                    comparison,
                });
            }
            Action::Dip { pigment, part } => {
                let pigment = crate::pigment::by_name(pigment).unwrap_or_else(|| panic!("unknown pigment {pigment}"));
                engine.dip_into(pigment, *part);
            }
            Action::DipColour { colour, part } => engine.dip_into(crate::pigment::Coefficients::from_srgb(*colour), *part),
            Action::DipWater => engine.dip_water(),
            Action::Clean => engine.clean(),
            Action::Wipe => engine.wipe(),
            Action::Undo => {
                engine.undo();
            }
            Action::Clear => engine.clear(),
            Action::CleanPalette => engine.clean_palette(),
            Action::Dry => engine.dry(),
            Action::Surface { surface } => engine.set_surface(*surface),
            Action::HangBottle { pivot, release, push } => engine.hang_bottle(Hanging { pivot: Vec2::from(*pivot), release: Vec2::from(*release), push: Vec2::from(*push) }),
            Action::CutStrings => engine.cut_strings(),
            Action::SelectBottle { bottle } => engine.select_bottle(*bottle),
            Action::BottleParam { bottle, name, value } => {
                assert!(engine.set_bottle_param(*bottle, name, *value), "unknown bottle parameter {name}");
            }
            Action::StopBottles { bottle } => engine.stop_bottles(*bottle),
            Action::RemoveBottle { bottle } => engine.remove_bottle(*bottle),
            Action::PourBottles { bottle, pouring } => engine.set_pouring(*bottle, *pouring),
            Action::PauseDrips { paused } => engine.set_drips_paused(*paused),
            Action::Tool { name } => {
                assert!(engine.select_brush(name), "unknown brush {name}");
            }
            Action::Param { name, value } => {
                assert!(engine.set_param(name, *value), "unknown parameter {name}");
            }
        }
    }

    let steps_simulated = (engine.time() / f64::from(SIMULATION_DT)).round() as u64;
    Report { total_ms: started.elapsed().as_secs_f64() * 1000.0, steps_simulated, checkpoints }
}

fn zoomed(engine: &mut Engine, zoom: Zoom) -> image::RgbaImage {
    engine.set_stage(Some(zoom.viewport()));
    engine.render();
    let image = engine.read_stage().expect("a zoomed checkpoint draws the stage");
    engine.set_stage(None);
    image
}

fn sample(phase: PointerPhase, x: f32, y: f32, pressure: f32, tilt: [f32; 2], rotation: f32, time: f64) -> PointerSample {
    PointerSample { phase, x, y, pressure, tilt, rotation, time }
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
    let Ok(expected) = image::open(&baseline_path) else {
        std::fs::create_dir_all(baseline_dir).expect("cannot create baseline dir");
        actual.save(baseline_dir.join(format!("{name}.actual.png"))).expect("cannot write actual");
        return Some(Comparison::missing_baseline());
    };
    let expected = expected.into_rgba8();
    let comparison = compare(actual, &expected, scenario.tolerance);
    if !comparison.passed {
        actual.save(baseline_dir.join(format!("{name}.actual.png"))).expect("cannot write actual");
        difference_image(actual, &expected)
            .save(baseline_dir.join(format!("{name}.diff.png")))
            .expect("cannot write diff");
    }
    Some(comparison)
}
