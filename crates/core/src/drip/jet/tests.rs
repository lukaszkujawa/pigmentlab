use super::*;
use glam::DVec3;

const GRAVITY: f64 = crate::units::GRAVITY as f64;

const WATERY: Fluid = Fluid { viscosity: 1e-3, yield_stress: 0.0 };
const ENAMEL: Fluid = Fluid { viscosity: 2.5, yield_stress: 0.0 };

fn weightless(fluid: Fluid) -> Jet {
    Jet { gravity: DVec3::ZERO, ..Jet::new(fluid, 1) }
}

fn rippled_cylinder(radius: f64, wave_number: f64, wavelengths: f64, ripple: f64) -> Piece {
    let length = wavelengths * std::f64::consts::TAU / wave_number;
    let segments = (length / (0.5 * radius)).round() as usize;
    let spacing = length / segments as f64;
    let nodes = (0..=segments).map(|k| Node { position: DVec3::new(spacing * k as f64 - 0.5 * length, 0.0, 1.0), velocity: DVec3::ZERO }).collect();
    let volumes = (0..segments)
        .map(|k| {
            let x = spacing * (k as f64 + 0.5) - 0.5 * length;
            let h = radius * (1.0 + ripple * (wave_number * x).cos());
            PI * h * h * spacing
        })
        .collect();
    Piece::new(nodes, volumes, false)
}

fn ripple_at(jet: &Jet, wave_number: f64) -> f64 {
    let reach = 3.0 * std::f64::consts::TAU / wave_number;
    let samples: Vec<(f64, f64, f64)> = jet.pieces().iter().flat_map(|piece| (0..piece.segments()).map(move |segment| (0.5 * (piece.nodes[segment].position.x + piece.nodes[segment + 1].position.x), piece.radius(segment), piece.length(segment)))).filter(|(x, _, _)| x.abs() < reach).collect();
    let length: f64 = samples.iter().map(|(_, _, l)| l).sum();
    let mean = samples.iter().map(|(_, h, l)| h * l).sum::<f64>() / length;
    let (along, across) = samples.iter().fold((0.0, 0.0), |(along, across), (x, h, l)| (along + (h - mean) * (wave_number * x).cos() * l, across + (h - mean) * (wave_number * x).sin() * l));
    2.0 * along.hypot(across) / (mean * length)
}

fn slender_rates(fluid: Fluid, radius: f64, wave_number: f64) -> (f64, f64) {
    let damping = 3.0 * f64::from(fluid.viscosity) * wave_number * wave_number;
    let drive = SURFACE_TENSION * wave_number * wave_number * (1.0 - (wave_number * radius).powi(2)) / (2.0 * radius);
    let root = (damping * damping + 4.0 * DENSITY * drive).sqrt();
    ((-damping + root) / (2.0 * DENSITY), (-damping - root) / (2.0 * DENSITY))
}

fn slender_growth_from_rest(fluid: Fluid, radius: f64, wave_number: f64, from: f64, to: f64) -> f64 {
    let (growing, decaying) = slender_rates(fluid, radius, wave_number);
    let amplitude = |t: f64| (decaying * (growing * t).exp() - growing * (decaying * t).exp()) / (decaying - growing);
    (amplitude(to) / amplitude(from)).ln() / (to - from)
}

fn measured_growth(fluid: Fluid, radius: f64, wave_number: f64, from: f64, to: f64, dt: f64) -> f64 {
    let mut jet = weightless(fluid);
    jet.pieces.push(rippled_cylinder(radius, wave_number, 60.0, 1e-3));
    let steps = |seconds: f64| (seconds / dt).round() as usize;
    for _ in 0..steps(from) {
        jet.step(dt, None);
    }
    let start = ripple_at(&jet, wave_number);
    for _ in 0..steps(to - from) {
        jet.step(dt, None);
    }
    (ripple_at(&jet, wave_number) / start).ln() / (to - from)
}

#[test]
fn a_thin_runny_column_grows_ripples_at_the_rayleigh_plateau_rate() {
    for ka in [0.3, 0.9] {
        let expected = slender_growth_from_rest(WATERY, 1e-3, ka / 1e-3, 0.01, 0.03);
        let growth = measured_growth(WATERY, 1e-3, ka / 1e-3, 0.01, 0.03, 1.0 / 240.0);
        assert!((growth - expected).abs() < expected * 0.05, "ripples at ka {ka} grew at {growth:.1} per second, expected {expected:.1}");
    }
    let radius = 1e-3;
    let wave_number = 0.697 / radius;
    let (fastest, _) = slender_rates(WATERY, radius, wave_number);
    let rayleigh = 0.343 * (SURFACE_TENSION / (DENSITY * radius.powi(3))).sqrt();
    assert!((fastest - rayleigh).abs() < rayleigh * 0.05, "the slender model should agree with Rayleigh, {fastest:.1} vs {rayleigh:.1} per second");
    let expected = slender_growth_from_rest(WATERY, radius, wave_number, 0.01, 0.03);
    let growth = measured_growth(WATERY, radius, wave_number, 0.01, 0.03, 1.0 / 240.0);
    assert!((growth - expected).abs() < expected * 0.05, "ripples grew at {growth:.1} per second, expected {expected:.1}");
}

#[test]
fn ripples_shorter_than_the_column_is_round_die_away() {
    let radius = 1e-3;
    let growth = measured_growth(WATERY, radius, 1.4 / radius, 0.005, 0.03, 1.0 / 240.0);
    assert!(growth < 5.0, "short ripples must not grow, grew at {growth:.1} per second");
}

#[test]
fn a_thick_enamel_column_creeps_into_beads_at_the_viscous_rate() {
    let radius = 1e-3;
    let wave_number = 0.4 / radius;
    let (slowest_creep, _) = slender_rates(ENAMEL, radius, wave_number);
    let viscous = SURFACE_TENSION * (1.0 - 0.16) / (6.0 * f64::from(ENAMEL.viscosity) * radius);
    assert!((slowest_creep - viscous).abs() < viscous * 0.02);
    let expected = slender_growth_from_rest(ENAMEL, radius, wave_number, 0.2, 1.0);
    let growth = measured_growth(ENAMEL, radius, wave_number, 0.2, 1.0, 1.0 / 240.0);
    assert!((growth - expected).abs() < expected * 0.1, "enamel ripples grew at {growth:.2} per second, expected {expected:.2}");
}

const HEIGHT: f64 = 0.15;

struct Pour {
    jet: Jet,
    poured: f64,
    landed: Vec<Landing>,
    drops: Vec<(f64, Drop)>,
    first_break: Option<f64>,
}

fn pour(fluid: Fluid, flow: f64, hole_radius: f64, seconds: f64) -> Pour {
    let mut jet = Jet::new(fluid, 1);
    let source = Source { position: DVec3::new(0.0, 0.0, HEIGHT), velocity: DVec3::ZERO, hole_radius, flow };
    let (mut landed, mut drops, mut first_break) = (Vec::new(), Vec::new(), None);
    let steps = (seconds * 240.0) as usize;
    for step in 0..steps {
        let fallen = jet.step(1.0 / 240.0, Some(&source));
        landed.extend(fallen.landed);
        if first_break.is_none() {
            first_break = fallen.drops.iter().filter(|drop| drop.position.z > 0.02).map(|drop| HEIGHT - drop.position.z).reduce(f64::min);
        }
        drops.extend(fallen.drops.into_iter().map(|drop| (step as f64 / 240.0, drop)));
        if first_break.is_none() {
            first_break = jet.pieces().iter().filter(|piece| !piece.pinned && piece.nodes[0].position.z > 0.02).map(|piece| HEIGHT - piece.nodes[0].position.z).reduce(f64::min);
        }
    }
    Pour { jet, poured: flow * steps as f64 / 240.0, landed, drops, first_break }
}

impl Pour {
    fn accounted(&self) -> f64 {
        self.jet.volume() + self.landed.iter().map(|landing| landing.volume).sum::<f64>() + self.drops.iter().map(|(_, drop)| drop.volume).sum::<f64>()
    }

    fn main_drops(&self) -> Vec<f64> {
        self.drops.iter().map(|(_, drop)| drop.volume).filter(|volume| *volume > 1e-9).collect()
    }
}

#[test]
fn every_drop_poured_is_in_the_air_on_the_canvas_or_falling() {
    for (fluid, flow, hole) in [(Fluid::of(0.35), 1e-6, 2e-3), (Fluid::of(1.0), 2e-6, 1.5e-3), (Fluid::of(1.0), 2e-8, 1.5e-3)] {
        let pour = pour(fluid, flow, hole, 3.0);
        assert!((pour.accounted() - pour.poured).abs() < pour.poured * 1e-9, "paint went missing: poured {:e}, found {:e}", pour.poured, pour.accounted());
    }
}

#[test]
fn a_slow_trickle_of_runny_paint_drips_drops_of_tates_size_often_trailed_by_a_satellite() {
    let hole = 1.5e-3;
    let pour = pour(Fluid::of(1.0), 2e-8, hole, 6.0);
    let drops = pour.main_drops();
    let tate = 2.0 * PI * hole * SURFACE_TENSION * 0.6 / (DENSITY * GRAVITY);
    let mean = drops.iter().sum::<f64>() / drops.len() as f64;
    assert!(drops.len() >= 4, "six seconds of trickle should drip several drops, got {}", drops.len());
    assert!((mean - tate).abs() < tate * 0.25, "drops of {:.1} ul should be near Tate's {:.1} ul", mean * 1e9, tate * 1e9);
    let satellites = pour.drops.iter().filter(|(_, drop)| drop.volume < mean * 0.05 && drop.volume > mean * 1e-4).count();
    assert!(2 * satellites >= drops.len(), "most drops should leave a satellite behind, {satellites} for {} drops", drops.len());
    assert!(pour.first_break.is_some_and(|depth| depth < 0.012), "dripping drops part right at the hole, first at {:?} m", pour.first_break);
}

fn webers_breakup_length(fluid: Fluid, hole_radius: f64, speed: f64) -> f64 {
    let diameter = 2.0 * hole_radius;
    let weber = DENSITY * speed * speed * diameter / SURFACE_TENSION;
    let ohnesorge = f64::from(fluid.viscosity) / (DENSITY * SURFACE_TENSION * diameter).sqrt();
    12.0 * diameter * weber.sqrt() * (1.0 + 3.0 * ohnesorge)
}

#[test]
fn a_jet_runs_unbroken_for_about_webers_breakup_length_and_further_the_more_viscous_it_is() {
    let (hole, speed) = (5e-4, 1.5);
    let (watery, medium) = (breakup_length(WATERY, hole, speed), breakup_length(Fluid::of(0.7), hole, speed));
    for (name, length, fluid) in [("watery", watery, WATERY), ("medium", medium, Fluid::of(0.7))] {
        let share = length / webers_breakup_length(fluid, hole, speed);
        assert!((0.5..1.6).contains(&share), "a {name} jet broke at {share:.2} of Weber's length");
    }
    assert!(medium > 2.0 * watery, "a viscous jet should run much further, {:.0} vs {:.0} mm", medium * 1e3, watery * 1e3);
}

#[test]
fn enamel_falls_as_one_unbroken_thread() {
    let pour = pour(Fluid::of(0.35), 1e-6, 2e-3, 3.0);
    let drips = pour.drops.iter().filter(|(_, drop)| drop.position.z > 0.005).count();
    assert_eq!(drips, 0, "enamel should not drip on its way down");
    let landed: f64 = pour.landed.iter().map(|landing| landing.volume).sum();
    assert!(landed > pour.poured * 0.85, "the thread should keep landing, {:.2} of the paint did", landed / pour.poured);
}

#[test]
fn a_steady_stream_carries_the_same_flow_through_every_cross_section_as_it_thins() {
    let flow = 1e-6;
    let pour = pour(Fluid::of(0.35), flow, 2e-3, 2.0);
    let piece = pour.jet.pieces().iter().find(|piece| piece.pinned).expect("the stream hangs from the hole");
    let sections: Vec<(f64, f64, f64)> = (HELD_NODES..piece.segments())
        .map(|segment| {
            let (a, b) = (piece.nodes[segment], piece.nodes[segment + 1]);
            (0.5 * (a.position.z + b.position.z), piece.radius(segment), 0.5 * (a.velocity + b.velocity).length())
        })
        .filter(|(z, _, _)| (0.03..0.13).contains(z))
        .collect();
    assert!(sections.len() > 20);
    for (z, radius, speed) in &sections {
        let carried = PI * radius * radius * speed;
        assert!((carried - flow).abs() < flow * 0.1, "at {:.0} mm the stream carries {carried:e} instead of {flow:e}", z * 1e3);
    }
    let (top, bottom) = (sections.first().expect("sections"), sections.last().expect("sections"));
    assert!(bottom.1 < top.1 * 0.8, "the stream should thin as it speeds up, {:.2} -> {:.2} mm", top.1 * 1e3, bottom.1 * 1e3);
}

#[test]
fn the_same_pour_falls_the_same_way() {
    let (first, second) = (pour(Fluid::of(1.0), 2e-6, 1.5e-3, 1.0), pour(Fluid::of(1.0), 2e-6, 1.5e-3, 1.0));
    assert_eq!(first.landed, second.landed);
    assert_eq!(first.drops.len(), second.drops.len());
}

fn breakup_length(fluid: Fluid, hole_radius: f64, speed: f64) -> f64 {
    let mut jet = Jet { gravity: DVec3::ZERO, ..Jet::new(fluid, 1) };
    let flow = speed * PI * hole_radius * hole_radius;
    let source = Source { position: DVec3::new(0.0, 0.0, 2.0), velocity: DVec3::ZERO, hole_radius, flow };
    let mut lengths = Vec::new();
    for step in 0..480 {
        jet.step(1.0 / 240.0, Some(&source));
        if step >= 240 {
            lengths.extend(jet.pieces().iter().filter(|piece| piece.pinned).map(Piece::total_length));
        }
    }
    lengths.iter().sum::<f64>() / lengths.len() as f64
}

#[test]
fn a_stream_from_a_moving_nozzle_stays_whole_and_lands_under_where_the_nozzle_has_got_to() {
    let speed = 0.13;
    let mut jet = Jet::new(Fluid::of(0.35), 1);
    let mut gaps = Vec::new();
    for step in 0..240 {
        let nozzle = DVec3::new(speed * step as f64 / 240.0, 0.0, HEIGHT);
        let fallen = jet.step(1.0 / 240.0, Some(&Source { position: nozzle, velocity: DVec3::new(speed, 0.0, 0.0), hole_radius: 2e-3, flow: 1e-6 }));
        assert!(fallen.drops.iter().all(|drop| drop.position.z < 0.005), "the moving stream must not shed drops in the air");
        gaps.extend(fallen.landed.iter().filter(|landing| landing.attached && step > 120).map(|landing| landing.position.x - nozzle.x));
    }
    let airborne = jet.pieces().iter().filter(|piece| !piece.pinned && !piece.is_on_canvas()).count();
    assert_eq!(airborne, 0, "no piece of the stream should break off in the air");
    let mean = gaps.iter().sum::<f64>() / gaps.len() as f64;
    assert!(mean.abs() < 0.005, "paint keeps the nozzle's sideways speed as it falls, so it lands under the nozzle, {:.1} mm away", mean * 1e3);
}

fn clanet_lasheras_weber(hole_radius: f64) -> f64 {
    let bond = 2.0 * hole_radius * (DENSITY * GRAVITY / (2.0 * SURFACE_TENSION)).sqrt();
    let k = 1.0 + 0.37 * bond * bond;
    4.0 * (k - (k * k - 1.0).sqrt()).powi(2)
}

struct Hanging {
    mean_length: f64,
    mean_drop: f64,
}

fn hang(fluid: Fluid, hole_radius: f64, weber: f64) -> Hanging {
    let speed = (weber * SURFACE_TENSION / (DENSITY * 2.0 * hole_radius)).sqrt();
    let mut jet = Jet::new(fluid, 1);
    let source = Source { position: DVec3::new(0.0, 0.0, 1.0), velocity: DVec3::ZERO, hole_radius, flow: speed * PI * hole_radius * hole_radius };
    let (mut lengths, mut drops) = (Vec::new(), Vec::new());
    for step in 0..360 {
        let fallen = jet.step(1.0 / 240.0, Some(&source));
        drops.extend(fallen.drops.iter().map(|drop| drop.volume).filter(|volume| *volume > 1e-9));
        if step >= 120 {
            lengths.extend(jet.pieces().iter().filter(|piece| piece.pinned).map(Piece::total_length));
        }
    }
    Hanging { mean_length: lengths.iter().sum::<f64>() / lengths.len() as f64, mean_drop: drops.iter().sum::<f64>() / drops.len().max(1) as f64 }
}

#[test]
fn runny_paint_drips_at_a_low_weber_number_and_jets_below_clanet_and_lasheras_threshold_for_water() {
    let hole = 1.5e-3;
    let critical = clanet_lasheras_weber(hole);
    assert!((0.6..0.8).contains(&critical), "Clanet and Lasheras give about 0.7 for a 3 mm hole, got {critical:.2}");
    let tate = 2.0 * PI * hole * SURFACE_TENSION * 0.6 / (DENSITY * GRAVITY);
    let dripping = hang(Fluid::of(1.0), hole, critical / 8.0);
    assert!(dripping.mean_length < 0.015, "slow paint should hang as a drop at the hole, hung {:.1} mm", dripping.mean_length * 1e3);
    assert!(dripping.mean_drop > 0.8 * tate, "dripped drops should be at least Tate's size, {:.1} ul", dripping.mean_drop * 1e9);
    let jetting = hang(Fluid::of(1.0), hole, critical / 2.0);
    assert!(jetting.mean_length > 0.05, "viscous paint jets below the water threshold, the stream only reached {:.1} mm", jetting.mean_length * 1e3);
    assert!(jetting.mean_drop < 0.5 * tate, "a jet breaks into drops smaller than dripped ones, {:.1} ul", jetting.mean_drop * 1e9);
}

#[test]
fn a_long_enamel_stream_steps_well_inside_a_frame() {
    let mut jet = Jet::new(Fluid::of(0.35), 1);
    let source = Source { position: DVec3::new(0.0, 0.0, HEIGHT), velocity: DVec3::ZERO, hole_radius: 2e-3, flow: 1e-6 };
    for _ in 0..240 {
        jet.step(1.0 / 240.0, Some(&source));
    }
    let nodes: usize = jet.pieces().iter().map(|piece| piece.nodes.len()).sum();
    assert!(nodes >= 200, "the stream should be long enough to measure, {nodes} nodes");
    let start = std::time::Instant::now();
    for _ in 0..240 {
        jet.step(1.0 / 240.0, Some(&source));
    }
    let per_frame = start.elapsed().as_secs_f64() / 240.0;
    println!("{nodes} nodes step in {:.3} ms per frame", per_frame * 1e3);
    let budget = if cfg!(debug_assertions) { 0.02 } else { 0.001 };
    assert!(per_frame < budget, "stepping {nodes} nodes took {:.3} ms per frame", per_frame * 1e3);
}

fn bow(piece: &Piece) -> f64 {
    let (top, bottom) = (piece.nodes[0].position, piece.nodes[piece.nodes.len() - 1].position);
    let chord = (bottom - top).normalize();
    piece.nodes.iter().map(|node| (node.position - top).reject_from_normalized(chord).length()).fold(0.0, f64::max)
}

#[test]
fn a_sideways_flick_whips_the_falling_stream_into_an_arc() {
    let mut jet = Jet::new(Fluid::of(0.35), 1);
    let mut nozzle = DVec3::new(0.0, 0.0, HEIGHT);
    let mut widest = 0.0f64;
    for step in 0..270 {
        let velocity = if step >= 240 { DVec3::new(1.5, 0.0, 0.0) } else { DVec3::ZERO };
        nozzle += velocity / 240.0;
        jet.step(1.0 / 240.0, Some(&Source { position: nozzle, velocity, hole_radius: 2e-3, flow: 1e-6 }));
        if step == 239 {
            let still = jet.pieces().iter().find(|piece| piece.pinned).expect("a stream hangs from the hole");
            assert!(bow(still) < 0.002, "a still stream should hang straight, bowed {:.1} mm", bow(still) * 1e3);
        }
        widest = jet.pieces().iter().filter(|piece| piece.pinned).map(bow).fold(widest, f64::max);
    }
    assert!(widest > 0.015, "the flicked stream should bend into an arc, bowed only {:.1} mm", widest * 1e3);
}

const POLLOCK: Fluid = Fluid { viscosity: 5.0, yield_stress: 0.0 };
const POLLOCK_FLOW: f64 = 2e-7;

struct Laid {
    points: Vec<(usize, glam::DVec2)>,
    contact_radius: f64,
}

fn lay_from(speed: f64) -> Laid {
    let mut jet = Jet::new(POLLOCK, 1);
    let (mut points, mut radii) = (Vec::new(), Vec::new());
    for step in 0..600 {
        let at = DVec3::new(speed * step as f64 / 240.0, 0.0, HEIGHT);
        let fallen = jet.step(1.0 / 240.0, Some(&Source { position: at, velocity: DVec3::new(speed, 0.0, 0.0), hole_radius: 1e-3, flow: POLLOCK_FLOW }));
        for landing in fallen.landed.iter().filter(|landing| landing.attached && step > 360) {
            points.push((step, landing.position.truncate()));
            radii.push(landing.radius);
        }
    }
    Laid { points, contact_radius: radii.iter().sum::<f64>() / radii.len() as f64 }
}

fn sideways_spread(laid: &Laid) -> f64 {
    let across = laid.points.iter().map(|(_, point)| point.y);
    across.clone().fold(f64::MIN, f64::max) - across.fold(f64::MAX, f64::min)
}

#[test]
fn a_still_nozzle_coils_at_the_gravitational_coiling_radius_and_rate() {
    let laid = lay_from(0.0);
    let centre = laid.points.iter().map(|(_, point)| *point).sum::<glam::DVec2>() / laid.points.len() as f64;
    let radius = laid.points.iter().map(|(_, point)| point.distance(centre)).sum::<f64>() / laid.points.len() as f64;
    let turned: f64 = laid.points.windows(2).map(|pair| ((pair[1].1 - centre).to_angle() - (pair[0].1 - centre).to_angle() + PI).rem_euclid(2.0 * PI) - PI).sum();
    let rate = turned.abs() / ((laid.points[laid.points.len() - 1].0 - laid.points[0].0) as f64 / 240.0);
    let arrival = POLLOCK_FLOW / (PI * laid.contact_radius * laid.contact_radius);
    let ribe = (GRAVITY * POLLOCK_FLOW.powi(3) / (f64::from(POLLOCK.kinematic_viscosity()) * laid.contact_radius.powi(8))).powf(0.25);
    println!("coils of {:.2} mm at {rate:.0} rad/s; Ribe gives {:.2} mm at {ribe:.0} rad/s", radius * 1e3, arrival / ribe * 1e3);
    assert!((radius / (arrival / ribe) - 1.0).abs() < 0.3, "coils of {:.2} mm should match the gravitational coiling radius {:.2} mm", radius * 1e3, arrival / ribe * 1e3);
    assert!((0.5..1.5).contains(&(rate / ribe)), "coiling at {rate:.0} rad/s should match the gravitational rate {ribe:.0} rad/s");
}

#[test]
fn a_moving_nozzle_lays_translated_coils_then_meanders_then_a_straight_line() {
    let still = lay_from(0.0);
    let arrival = POLLOCK_FLOW / (PI * still.contact_radius * still.contact_radius);
    let coil = sideways_spread(&still) * 0.5;
    let spreads = [0.2, 0.8, 1.1].map(|ratio| sideways_spread(&lay_from(ratio * arrival)) / coil);
    println!("sideways spread in coil radii at 0.2, 0.8 and 1.1 of the arrival speed {arrival:.2} m/s: {spreads:?}");
    assert!(spreads[0] > 1.5, "slowly translated coils should sweep about two coil radii: {spreads:?}");
    assert!(spreads[1] < spreads[0], "meanders should be narrower than loops: {spreads:?}");
    assert!(spreads[2] < 0.25, "faster than the thread arrives it should lay a straight line: {spreads:?}");
}

#[test]
fn a_thread_drawn_out_to_nothing_in_many_places_parts_at_all_of_them_at_once() {
    let (segments, spacing, gaps) = (200, 4e-4, 20);
    let nodes = (0..=segments).map(|k| Node { position: DVec3::new(spacing * k as f64, 0.0, 1.0), velocity: DVec3::ZERO }).collect();
    let radius = |k: usize| if k % (segments / gaps) == segments / gaps / 2 { 0.5 * VANISHED_RADIUS } else { 5e-4 };
    let volumes = (0..segments).map(|k| PI * radius(k).powi(2) * spacing).collect();
    let thread = Piece::new(nodes, volumes, false);
    let poured = thread.volume();
    let mut jet = weightless(ENAMEL);
    jet.pieces.push(thread);
    let fallen = jet.step(1.0 / 240.0, None);
    let vanished = jet.pieces().iter().any(|piece| (1..piece.segments().saturating_sub(1)).any(|segment| piece.radius(segment) < VANISHED_RADIUS));
    assert!(!vanished, "no piece should keep a vanished stretch inside it");
    assert!(jet.pieces().len() + fallen.drops.len() > gaps, "the thread should part at every vanished stretch");
    let kept = jet.volume() + fallen.drops.iter().map(|drop| drop.volume).sum::<f64>();
    assert!((kept - poured).abs() < poured * 1e-9, "parting the thread keeps its paint, {kept:e} of {poured:e}");
}
