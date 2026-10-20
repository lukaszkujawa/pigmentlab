use serde::{Deserialize, Serialize};

const TIP_START: f32 = 0.55;
const TIP_FULL: f32 = 0.85;
const SIDE_BLEND: f32 = 0.25;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DipPart {
    #[default]
    Whole,
    Tip,
    Side,
}

impl DipPart {
    pub fn is_whole(&self) -> bool {
        *self == DipPart::Whole
    }

    pub fn share(self, across: f32, along: f32) -> f32 {
        match self {
            DipPart::Whole => 1.0,
            DipPart::Tip => smoothstep(TIP_START, TIP_FULL, along),
            DipPart::Side => smoothstep(-SIDE_BLEND, SIDE_BLEND, across),
        }
    }
}

fn smoothstep(edge0: f32, edge1: f32, x: f32) -> f32 {
    let t = ((x - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn whole_loads_everywhere_and_parts_load_only_their_region() {
        assert_eq!(DipPart::Whole.share(-1.0, 0.0), 1.0);
        assert_eq!(DipPart::Tip.share(0.0, 0.2), 0.0);
        assert_eq!(DipPart::Tip.share(0.0, 1.0), 1.0);
        assert_eq!(DipPart::Side.share(-1.0, 1.0), 0.0);
        assert_eq!(DipPart::Side.share(1.0, 0.0), 1.0);
        assert_eq!(DipPart::Side.share(0.0, 0.5), 0.5);
    }
}
