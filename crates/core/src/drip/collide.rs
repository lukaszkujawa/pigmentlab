use super::bottle::Bottle;
use glam::Vec3;

const RESTITUTION: f32 = 0.3;
const FRICTION: f32 = 0.4;
const SEPARATION_PASSES: usize = 8;

pub fn collide(bottles: &mut [Bottle]) {
    for first in 0..bottles.len() {
        for second in first + 1..bottles.len() {
            let (head, tail) = bottles.split_at_mut(second);
            resolve(&mut head[first], &mut tail[0]);
        }
    }
}

fn axis(bottle: &Bottle) -> (Vec3, Vec3) {
    (bottle.pendulum.bob, bottle.pendulum.bob + up(bottle) * bottle.height())
}

fn closest_points(p: (Vec3, Vec3), q: (Vec3, Vec3)) -> (Vec3, Vec3) {
    let (d1, d2, r) = (p.1 - p.0, q.1 - q.0, p.0 - q.0);
    let (a, e, f) = (d1.dot(d1), d2.dot(d2), d2.dot(r));
    let c = d1.dot(r);
    let b = d1.dot(d2);
    let denominator = a * e - b * b;
    let mut s = if denominator > 1e-12 { ((b * f - c * e) / denominator).clamp(0.0, 1.0) } else { 0.0 };
    let mut t = (b * s + f) / e;
    if t < 0.0 {
        t = 0.0;
        s = (-c / a).clamp(0.0, 1.0);
    } else if t > 1.0 {
        t = 1.0;
        s = ((b - c) / a).clamp(0.0, 1.0);
    }
    (p.0 + d1 * s, q.0 + d2 * t)
}

fn contact(a: &Bottle, b: &Bottle) -> Option<(Vec3, f32)> {
    let (on_a, on_b) = closest_points(axis(a), axis(b));
    let gap = on_a - on_b;
    let distance = gap.length();
    let overlap = a.radius + b.radius - distance;
    (overlap > 0.0 && distance > 1e-9).then_some((gap / distance, overlap))
}

fn rub(a: &mut Bottle, b: &mut Bottle, normal: Vec3, most: f32) {
    let lever_a = up(a).cross(-normal * a.radius);
    let lever_b = up(b).cross(normal * b.radius);
    let slip = a.pendulum.velocity + lever_a * a.spin - b.pendulum.velocity - lever_b * b.spin;
    let sliding = slip - normal * slip.dot(normal);
    let speed = sliding.length();
    if speed < 1e-9 {
        return;
    }
    let along = sliding / speed;
    let (turn_a, turn_b) = (lever_a.dot(along), lever_b.dot(along));
    let resistance = a.inverse_mass() + b.inverse_mass() + turn_a * turn_a * a.inverse_inertia() + turn_b * turn_b * b.inverse_inertia();
    let impulse = (speed / resistance).min(most);
    a.pendulum.velocity -= along * impulse * a.inverse_mass();
    b.pendulum.velocity += along * impulse * b.inverse_mass();
    a.spin -= turn_a * impulse * a.inverse_inertia();
    b.spin += turn_b * impulse * b.inverse_inertia();
}

fn up(bottle: &Bottle) -> Vec3 {
    (bottle.pendulum.pivot - bottle.pendulum.bob).normalize()
}

fn resolve(a: &mut Bottle, b: &mut Bottle) {
    let Some((normal, _)) = contact(a, b) else { return };
    let (inverse_a, inverse_b) = (a.inverse_mass(), b.inverse_mass());
    let share = inverse_a + inverse_b;
    if share <= 0.0 {
        return;
    }
    let closing = (a.pendulum.velocity - b.pendulum.velocity).dot(normal);
    if closing < 0.0 {
        let push = -(1.0 + RESTITUTION) * closing / share;
        a.pendulum.velocity += normal * push * inverse_a;
        b.pendulum.velocity -= normal * push * inverse_b;
        rub(a, b, normal, FRICTION * push);
    }
    for _ in 0..SEPARATION_PASSES {
        let Some((normal, overlap)) = contact(a, b) else { break };
        a.pendulum.bob += normal * overlap * inverse_a / share;
        b.pendulum.bob -= normal * overlap * inverse_b / share;
        for bottle in [&mut *a, &mut *b] {
            let pendulum = &mut bottle.pendulum;
            pendulum.bob = pendulum.pivot + (pendulum.bob - pendulum.pivot).normalize() * pendulum.length;
        }
    }
    for bottle in [a, b] {
        bottle.pendulum.velocity = bottle.pendulum.tangential(bottle.pendulum.velocity);
    }
}

#[cfg(test)]
mod tests {
    use super::super::bottle::Filling;
    use super::super::pendulum::Pendulum;
    use super::*;
    use crate::pigment::Coefficients;
    use glam::Vec2;

    fn bottle(pivot_x: f32, push: f32) -> Bottle {
        let pendulum = Pendulum::released(Vec3::new(pivot_x, 0.0, 1.2), 1.0, Vec2::new(pivot_x, 0.0), Vec2::new(push, 0.0));
        Bottle::filled(pendulum, Filling { paint: Coefficients { absorption: Vec3::ONE, scattering: Vec3::ONE }, medium: 0.35, hole_radius: 0.002, radius: 0.03, pouring: true }, 1, 1)
    }

    fn momentum(bottles: &[Bottle]) -> f32 {
        bottles.iter().map(|bottle| bottle.mass() * bottle.pendulum.velocity.x).sum()
    }

    #[test]
    fn bottles_swinging_into_each_other_bounce_apart_keeping_their_momentum_and_never_overlap() {
        let mut bottles = vec![bottle(-0.0295, 0.6), bottle(0.0295, -0.2)];
        let before = momentum(&bottles);
        collide(&mut bottles);
        let (first, second) = (&bottles[0], &bottles[1]);
        assert!(first.pendulum.velocity.x < second.pendulum.velocity.x, "the bottles should now move apart");
        assert!((momentum(&bottles) - before).abs() < before.abs() * 0.02, "the knock should keep the momentum ({before:.4} -> {:.4})", momentum(&bottles));
        let separating = second.pendulum.velocity.x - first.pendulum.velocity.x;
        assert!((separating - RESTITUTION * 0.8).abs() < 0.02, "they should part at {} of the closing speed, at {separating:.3} m/s", RESTITUTION);
        let (on_first, on_second) = closest_points(axis(first), axis(second));
        assert!(on_first.distance(on_second) >= first.radius + second.radius - 1e-5, "the bottles must not stay inside each other");
        for bottle in &bottles {
            assert!(((bottle.pendulum.bob - bottle.pendulum.pivot).length() - 1.0).abs() < 1e-5, "the strings must keep their length");
        }
    }

    #[test]
    fn a_held_bottle_pushes_a_hanging_one_aside_without_flinging_it() {
        let mut bottles = vec![bottle(-0.0295, 0.0), bottle(0.0295, 0.0)];
        bottles[1].held_over = Some(bottles[1].pendulum.bob.truncate());
        let held = bottles[1].pendulum.bob;
        collide(&mut bottles);
        assert_eq!(bottles[1].pendulum.bob, held, "the hand holds its bottle where it is");
        let (on_first, on_second) = closest_points(axis(&bottles[0]), axis(&bottles[1]));
        assert!(on_first.distance(on_second) >= bottles[0].radius + bottles[1].radius - 1e-5, "the hanging bottle should be pushed clear");
        assert!(bottles[0].pendulum.velocity.length() < 1e-6, "pushing it clear must not throw it");
    }

    #[test]
    fn a_glancing_rub_spins_the_light_bottles_instead_of_stopping_their_sliding() {
        let mut bottles = vec![bottle(-0.0295, 0.3), bottle(0.0295, -0.3)];
        for bottle in &mut bottles {
            bottle.pendulum.velocity.y = if bottle.pendulum.pivot.x < 0.0 { 0.4 } else { -0.4 };
            bottle.pendulum.velocity = bottle.pendulum.tangential(bottle.pendulum.velocity);
        }
        collide(&mut bottles);
        let sliding = bottles[0].pendulum.velocity.y - bottles[1].pendulum.velocity.y;
        assert!(sliding > 0.8 * 0.6, "a round bottle on a string turns as it rubs, keeping most of its sliding, {sliding:.3} of 0.8 m/s");
        assert!(bottles[0].spin.abs() > 1.0 && bottles[0].spin * bottles[1].spin > 0.0, "the rub should set both bottles turning the same way, {:.2} and {:.2} rad/s", bottles[0].spin, bottles[1].spin);
    }

    #[test]
    fn bottles_that_do_not_touch_are_left_alone() {
        let mut bottles = vec![bottle(-0.2, 0.6), bottle(0.2, -0.2)];
        let untouched = bottles.clone();
        collide(&mut bottles);
        assert_eq!(bottles, untouched);
    }
}
