use super::fluid::{DENSITY, Fluid};
use crate::units::GRAVITY;
use std::f32::consts::PI;

const DISCHARGE_COEFFICIENT: f32 = 0.61;

pub fn orifice_flow(head: f32, hole_radius: f32, spout_length: f32, fluid: Fluid) -> f32 {
    let plug_length = spout_length.max(hole_radius);
    let driving = DENSITY * GRAVITY * head - fluid.yield_stress * 2.0 * plug_length / hole_radius;
    if driving <= 0.0 {
        return 0.0;
    }
    let resistance = 3.0 * fluid.viscosity / hole_radius.powi(3) + 8.0 * fluid.viscosity * spout_length / (PI * hole_radius.powi(4));
    let viscous = driving / resistance;
    let inertial = DISCHARGE_COEFFICIENT * PI * hole_radius * hole_radius * (2.0 * driving / DENSITY).sqrt();
    viscous.min(inertial)
}

#[cfg(test)]
mod tests {
    use super::*;

    const ENAMEL: Fluid = Fluid { viscosity: 3.0, yield_stress: 0.0 };

    #[test]
    fn viscous_paint_seeps_through_a_hole_at_the_sampson_rate() {
        let flow = orifice_flow(0.1, 0.002, 0.0, ENAMEL);
        assert!((flow * 1e6 - 0.87).abs() < 0.05, "3 Pa s paint under 10 cm through a 4 mm hole flows {:.2} cm3/s, Sampson gives 0.87", flow * 1e6);
    }

    #[test]
    fn water_pours_at_the_torricelli_rate() {
        let water = Fluid { viscosity: 0.001, yield_stress: 0.0 };
        let flow = orifice_flow(0.1, 0.002, 0.0, water);
        let torricelli = DISCHARGE_COEFFICIENT * PI * 0.002f32.powi(2) * (2.0 * GRAVITY * 0.1f32).sqrt();
        assert!((flow - torricelli).abs() < torricelli * 1e-3, "{flow} vs {torricelli}");
    }

    #[test]
    fn a_stiff_paste_barely_oozes_a_yield_stress_holds_a_low_head_and_flow_slows_as_the_bottle_empties() {
        let enamel = Fluid::of(0.35);
        assert!(orifice_flow(0.1, 0.002, 0.003, Fluid::of(0.0)) < orifice_flow(0.1, 0.002, 0.003, enamel) * 0.15);
        assert_eq!(orifice_flow(0.02, 0.002, 0.003, Fluid::of(0.0)), 0.0);
        assert!(orifice_flow(0.1, 0.002, 0.003, enamel) > 2.0 * orifice_flow(0.04, 0.002, 0.003, enamel));
        assert!(orifice_flow(0.1, 0.003, 0.003, enamel) > orifice_flow(0.1, 0.002, 0.003, enamel));
    }
}
