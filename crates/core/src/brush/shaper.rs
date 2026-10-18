use super::frame::BrushPose;
use glam::{Vec3, Vec4};

pub fn flexed_chain(pose: &BrushPose, rest: &[Vec4], vertices_per_spine: usize) -> Vec<Vec4> {
    let down = pose.orientation * Vec3::NEG_Z;
    let face = (pose.orientation * Vec3::Y).truncate().normalize_or_zero();
    let folds_over = if face.dot(down.truncate()) < 0.0 { -face } else { face };
    rest.iter()
        .enumerate()
        .map(|(index, vertex)| {
            if index % vertices_per_spine == 0 {
                return Vec4::ZERO;
            }
            let root = pose.to_canvas(Vec3::new(vertex.x, vertex.y, 0.0));
            let reach = -vertex.z;
            let straight = root + down * reach;
            if straight.z >= 0.0 {
                return straight.extend(0.0);
            }
            let touchdown_reach = root.z / -down.z;
            let touchdown = (root + down * touchdown_reach).truncate();
            (touchdown + folds_over * (reach - touchdown_reach)).extend(0.0).extend(0.0)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use glam::Quat;

    const VERTICES: usize = 12;

    fn spine(length: f32) -> Vec<Vec4> {
        std::iter::once(Vec4::ZERO).chain((0..VERTICES - 1).map(|i| Vec4::new(0.0, 0.0, -length * i as f32 / (VERTICES - 2) as f32, 0.0))).collect()
    }

    fn held_at(height: f32, lean: f32) -> BrushPose {
        let handle = Vec3::new(0.0, lean, 1.0).normalize();
        BrushPose { position: handle * height, orientation: Quat::from_rotation_arc(Vec3::Z, handle) }
    }

    fn length_of(chain: &[Vec4]) -> f32 {
        chain[1..].windows(2).map(|pair| pair[0].truncate().distance(pair[1].truncate())).sum()
    }

    #[test]
    fn a_lifted_shaper_hangs_straight() {
        let chain = flexed_chain(&held_at(150.0, 0.0), &spine(100.0), VERTICES);
        assert!((chain[VERTICES - 1].z - 50.0).abs() < 1e-3 && chain[VERTICES - 1].x.abs() < 1e-3 && chain[VERTICES - 1].y.abs() < 1e-3);
    }

    #[test]
    fn a_pressed_shaper_lays_its_tip_flat_without_stretching() {
        let chain = flexed_chain(&held_at(60.0, 0.0), &spine(100.0), VERTICES);
        assert!((length_of(&chain) - 100.0).abs() < 1e-3, "chain length {}", length_of(&chain));
        assert_eq!(chain[VERTICES - 1].z, 0.0);
        assert!((chain[VERTICES - 1].truncate().truncate().length() - 40.0).abs() < 1e-3, "the 40 px pressed past the canvas should lie flat, tip at {:?}", chain[VERTICES - 1]);
    }

    #[test]
    fn a_leaning_shaper_folds_on_the_way_its_blade_points() {
        let chain = flexed_chain(&held_at(60.0, -0.3), &spine(100.0), VERTICES);
        assert!(chain[VERTICES - 1].y > chain[6].y + 30.0, "the flat tip should carry on past the touchdown the way the blade points: {:?}", &chain[6..]);
    }
}
