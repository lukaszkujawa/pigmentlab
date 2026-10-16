mod compare;
mod recorder;
mod runner;

pub use compare::{Comparison, Tolerance, compare};
pub use recorder::Recorder;
pub use runner::{CheckpointResult, Report, run};

use crate::canvas::CanvasSize;
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Scenario {
    pub seed: u64,
    pub canvas: CanvasSize,
    #[serde(default)]
    pub tolerance: Tolerance,
    #[serde(default)]
    pub show_bristles: bool,
    #[serde(default = "default_true")]
    pub levelling: bool,
    pub steps: Vec<Step>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Step {
    pub t: f64,
    #[serde(flatten)]
    pub action: Action,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum Action {
    Tool { name: String },
    Param { name: String, value: f32 },
    Press {
        x: f32,
        y: f32,
        pressure: f32,
        #[serde(default)]
        tilt: [f32; 2],
        #[serde(default)]
        rotation: f32,
    },
    Move {
        x: f32,
        y: f32,
        pressure: f32,
        #[serde(default)]
        tilt: [f32; 2],
        #[serde(default)]
        rotation: f32,
    },
    Release,
    Dip { pigment: String },
    DipColour { colour: [u8; 3] },
    DipWater,
    Clean,
    Undo,
    Clear,
    Checkpoint { name: String },
    Dry,
    Surface { surface: crate::canvas::Surface },
}

fn default_true() -> bool {
    true
}

impl Action {
    pub fn is_pointer_input(&self) -> bool {
        matches!(self, Action::Press { .. } | Action::Move { .. } | Action::Release)
    }
}

impl Scenario {
    pub fn load(path: &Path) -> Result<Self, String> {
        let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
        serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))
    }
}
