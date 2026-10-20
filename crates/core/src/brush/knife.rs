use super::frame::BrushPose;
use glam::{Vec3, Vec4};

const BISECTION_STEPS: u32 = 24;
const LENGTH_SAMPLES: usize = 48;
const LONGEST_REACH: f32 = 3.0;

const NECK_LENGTH: f32 = 0.15;
const NECK_WIDTH: f32 = 0.35;
const TIP_TAPER: f32 = 0.15;
const POINTED_TIP: f32 = 0.5;
const WORKING_POINT: f32 = 0.92;
const LOADED_FROM: f32 = 0.8;

pub struct Spine {
    samples: Vec<Vec3>,
    travelled: Vec<f32>,
}

impl Spine {
    pub fn at(&self, distance: f32) -> Vec3 {
        let segment = self.travelled.partition_point(|reached| *reached < distance).clamp(1, LENGTH_SAMPLES);
        let span = (self.travelled[segment] - self.travelled[segment - 1]).max(1e-6);
        self.samples[segment - 1].lerp(self.samples[segment], ((distance - self.travelled[segment - 1]) / span).clamp(0.0, 1.0))
    }
}

pub fn spine(pose: &BrushPose, length: f32) -> Spine {
    let root = pose.position;
    let down_the_blade = -(pose.orientation * Vec3::Z);
    let tip = root + down_the_blade * length;
    let points = if tip.z >= 0.0 || down_the_blade.z >= -1e-4 {
        [root, root + down_the_blade * length * 0.5, tip]
    } else {
        let touchdown = root + down_the_blade * (root.z / -down_the_blade.z);
        let flattened_tip = Vec3::new(tip.x, tip.y, 0.0);
        let curve = |reach: f32| [root, touchdown, touchdown + (flattened_tip - touchdown) * reach];
        let (mut short, mut long) = (0.0, LONGEST_REACH);
        for _ in 0..BISECTION_STEPS {
            let middle = 0.5 * (short + long);
            if bezier_length(curve(middle)) < length { short = middle } else { long = middle }
        }
        curve(0.5 * (short + long))
    };
    let samples: Vec<Vec3> = (0..=LENGTH_SAMPLES).map(|i| bezier(points, i as f32 / LENGTH_SAMPLES as f32)).collect();
    let mut travelled = vec![0.0];
    for pair in samples.windows(2) {
        travelled.push(travelled.last().unwrap() + pair[0].distance(pair[1]));
    }
    Spine { samples, travelled }
}

pub fn working_point(pose: &BrushPose, length: f32) -> Vec3 {
    spine(pose, length).at(length * WORKING_POINT)
}

pub fn blade_extent(offset: f32, length: f32, half_width: f32, point: f32) -> (f32, f32) {
    let distance = offset.abs().min(half_width);
    let neck_end = NECK_LENGTH * length;
    let start = neck_end * ((distance / half_width - NECK_WIDTH) / (1.0 - NECK_WIDTH)).max(0.0);
    let tip_half_width = half_width * (1.0 - TIP_TAPER);
    let rounding_starts = (length - tip_half_width).max(neck_end);
    let rounded = if distance <= tip_half_width {
        rounding_starts + (tip_half_width * tip_half_width - distance * distance).sqrt()
    } else {
        neck_end + (rounding_starts - neck_end) * (half_width - distance) / (half_width - tip_half_width)
    };
    let pointed = length * (1.0 - POINTED_TIP * distance / half_width);
    let end = rounded + (pointed - rounded) * point.clamp(0.0, 1.0);
    (start, end.min(length).max(start))
}

pub fn carries_paint(offset: f32, along: f32, length: f32, half_width: f32, point: f32) -> bool {
    let (start, end) = blade_extent(offset, length, half_width, point);
    start + (end - start) * along >= length * LOADED_FROM
}

pub fn blade_chain(pose: &BrushPose, rest: &[Vec4], vertices_per_spine: usize, length: f32, half_width: f32, point: f32) -> Vec<Vec4> {
    let spine = spine(pose, length);
    let across = pose.orientation * Vec3::X;
    let drawn = vertices_per_spine - 1;
    rest.chunks(vertices_per_spine)
        .flat_map(|spine_rest| {
            let offset = spine_rest[1].x;
            let (start, end) = blade_extent(offset, length, half_width, point);
            let points: Vec<Vec4> = (0..drawn)
                .map(|i| {
                    let point = spine.at(start + (end - start) * i as f32 / (drawn - 1) as f32) + across * offset;
                    Vec3::new(point.x, point.y, point.z.max(0.0)).extend(0.0)
                })
                .collect();
            let centre = spine.at(length * 0.5);
            std::iter::once(Vec4::new(centre.x, centre.y, length * 0.5, half_width)).chain(points)
        })
        .collect()
}

fn bezier([a, b, c]: [Vec3; 3], t: f32) -> Vec3 {
    let u = 1.0 - t;
    a * u * u + b * 2.0 * u * t + c * t * t
}

fn bezier_length(points: [Vec3; 3]) -> f32 {
    (1..=LENGTH_SAMPLES).map(|i| bezier(points, i as f32 / LENGTH_SAMPLES as f32).distance(bezier(points, (i - 1) as f32 / LENGTH_SAMPLES as f32))).sum()
}

#[cfg(test)]
mod tests {
    use super::*;
    use glam::Quat;

    fn leaning_knife(height: f32) -> BrushPose {
        let handle = Vec3::new(0.0, 0.7, 0.714).normalize();
        BrushPose { position: handle * height, orientation: Quat::from_rotation_arc(Vec3::Z, handle) }
    }

    fn bent_spine(pose: &BrushPose, length: f32, points: usize) -> Vec<Vec3> {
        let spine = spine(pose, length);
        (0..points).map(|i| spine.at(length * i as f32 / (points - 1) as f32)).collect()
    }

    fn spine_length(points: &[Vec3]) -> f32 {
        points.windows(2).map(|pair| pair[0].distance(pair[1])).sum()
    }

    #[test]
    fn a_lifted_blade_stays_straight() {
        let spine = bent_spine(&leaning_knife(200.0), 100.0, 9);
        let straight = spine[0].distance(spine[8]);
        assert!((straight - 100.0).abs() < 1e-3);
    }

    #[test]
    fn a_pressed_blade_bends_flat_onto_the_canvas_without_stretching() {
        let spine = bent_spine(&leaning_knife(60.0), 100.0, 9);
        assert!((spine_length(&spine) - 100.0).abs() < 1.5, "spine length {}", spine_length(&spine));
        assert!(spine[8].z.abs() < 0.5 && spine[7].z < 3.0, "the tip should lie on the canvas: {:?}", &spine[6..]);
        assert!(spine[0].distance(spine[8]) < 99.0, "a bent blade is shorter end to end than its length");
    }

    #[test]
    fn pressing_harder_lays_more_blade_flat() {
        let touching = |height: f32| bent_spine(&leaning_knife(height), 100.0, 33).iter().filter(|point| point.z < 1.0).count();
        assert!(touching(40.0) > touching(70.0));
    }

    #[test]
    fn the_blade_is_a_gently_tapered_rectangle_with_a_rounded_end() {
        assert_eq!(blade_extent(0.0, 100.0, 10.0, 0.0), (0.0, 100.0));
        let (start, end) = blade_extent(3.0, 100.0, 10.0, 0.0);
        assert!(start == 0.0 && end > 99.0, "the neck is narrow but the middle runs the whole length: {start}..{end}");
        let (start, end) = blade_extent(8.0, 100.0, 10.0, 0.0);
        assert!(start > 5.0 && start < 15.0 && end > 92.0 && end < 97.0, "near the side it starts past the neck and ends on the rounded tip: {start}..{end}");
        let (start, end) = blade_extent(9.5, 100.0, 10.0, 0.0);
        assert!(end > 30.0 && end < 92.0, "the outermost edge tapers in gently along the blade: {start}..{end}");
    }

    #[test]
    fn the_point_control_narrows_the_rounded_end_to_a_sharp_point() {
        let reach = |offset: f32, point: f32| blade_extent(offset, 100.0, 10.0, point).1;
        assert_eq!(reach(0.0, 1.0), 100.0, "the centre line always runs to the tip");
        assert!(reach(5.0, 1.0) < reach(5.0, 0.5) && reach(5.0, 0.5) < reach(5.0, 0.0), "a pointier blade ends sooner away from its centre line");
        assert!((reach(5.0, 1.0) - 75.0).abs() < 1e-3, "a sharp blade tapers in straight lines to its point, got {}", reach(5.0, 1.0));
    }
}
