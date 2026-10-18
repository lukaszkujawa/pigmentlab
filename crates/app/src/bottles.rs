use crate::{AppWindow, BottleRow};
use pigmentlab_core::Engine;
use pigmentlab_core::drip::Rig;
use pigmentlab_core::scenario::Action;
use pigmentlab_core::units::PIXELS_PER_CM;

pub const BOTTLE_PARAMS: [&str; 4] = ["string", "pivot", "hole", "radius"];

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum BottleAction {
    Select(Option<u32>),
    Param(u32, &'static str, f32),
    Stop(Option<u32>),
    Remove(u32),
    Pour(Option<u32>, bool),
    Pause(bool),
}

impl BottleAction {
    pub fn apply(self, engine: &mut Engine) {
        match self {
            Self::Select(bottle) => engine.select_bottle(bottle),
            Self::Param(bottle, name, value) => {
                engine.set_bottle_param(bottle, name, value);
            }
            Self::Stop(bottle) => engine.stop_bottles(bottle),
            Self::Remove(bottle) => engine.remove_bottle(bottle),
            Self::Pour(bottle, pouring) => engine.set_pouring(bottle, pouring),
            Self::Pause(paused) => engine.set_drips_paused(paused),
        }
    }

    pub fn recorded(self) -> Action {
        match self {
            Self::Select(bottle) => Action::SelectBottle { bottle },
            Self::Param(bottle, name, value) => Action::BottleParam { bottle, name: name.to_string(), value },
            Self::Stop(bottle) => Action::StopBottles { bottle },
            Self::Remove(bottle) => Action::RemoveBottle { bottle },
            Self::Pour(bottle, pouring) => Action::PourBottles { bottle, pouring },
            Self::Pause(paused) => Action::PauseDrips { paused },
        }
    }
}

pub fn chosen(id: i32) -> Option<u32> {
    u32::try_from(id).ok()
}

#[derive(Default, PartialEq)]
pub struct Shown {
    rows: Vec<(u32, u32, bool)>,
    selected: Option<u32>,
}

pub fn show_bottles(engine: &Engine, app: &AppWindow, shown: &mut Shown) {
    let drips = engine.drips();
    let now = Shown { rows: drips.bottles().iter().map(|bottle| (bottle.id, (bottle.fill() * 100.0).round() as u32, bottle.pouring)).collect(), selected: drips.selected() };
    if now == *shown {
        return;
    }
    if now.rows != shown.rows {
        let rows: Vec<BottleRow> = drips
            .bottles()
            .iter()
            .map(|bottle| {
                let [r, g, b] = bottle.paint.srgb();
                BottleRow { id: bottle.id as i32, label: format!("Bottle {}", bottle.id).into(), colour: slint::Color::from_rgb_u8(r, g, b), fill: bottle.fill(), pouring: bottle.pouring }
            })
            .collect();
        app.set_bottles(slint::ModelRc::new(slint::VecModel::from(rows)));
    }
    app.set_selected_pouring(now.selected.and_then(|id| drips.bottle(id)).is_some_and(|bottle| bottle.pouring));
    if now.selected != shown.selected {
        app.set_selected_bottle(now.selected.map_or(-1, |id| id as i32));
        match now.selected.and_then(|id| drips.bottle(id)) {
            Some(bottle) => show_settings(app, [bottle.pendulum.length * 100.0, bottle.pendulum.pivot.z * 100.0, bottle.hole_radius * 2000.0, bottle.radius * 100.0]),
            None => show_settings(app, rig_settings(engine.rig())),
        }
    }
    *shown = now;
}

fn rig_settings(rig: Rig) -> [f32; 4] {
    let in_cm = |pixels: f32| pixels / PIXELS_PER_CM;
    [in_cm(rig.string_length), in_cm(rig.pivot_height), in_cm(rig.hole_radius) * 20.0, in_cm(rig.bottle_radius)]
}

fn show_settings(app: &AppWindow, [string, pivot, hole, radius]: [f32; 4]) {
    app.set_rig_string(string);
    app.set_rig_pivot(pivot);
    app.set_rig_hole(hole);
    app.set_rig_radius(radius);
}
