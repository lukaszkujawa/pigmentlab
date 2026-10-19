use crate::drip::Fluid;
use crate::drip::fluid::{DENSITY, POURED_YIELD_SHARE, SURFACE_TENSION, WETTING_SLOPE};
use crate::units::{GRAVITY, METRES_PER_PIXEL};
use glam::IVec2;
use rayon::prelude::*;
use std::f32::consts::PI;

pub type Pigment = [f32; 7];

const CAPILLARY_AREA: f32 = SURFACE_TENSION / (DENSITY * GRAVITY) / (METRES_PER_PIXEL * METRES_PER_PIXEL);
const SOLVE_TOLERANCE: f32 = 1e-3;
const MOST_ITERATIONS: usize = 8;
const CHUNK: usize = 4096;
const CONTACT_LINE_FRICTION: f32 = 120.0;
const PINNING_SPREAD: f32 = 0.35;
const WEAVE_GUIDANCE: f32 = 0.3;
const FIBRE: f32 = 4.0;

pub const NOWHERE: usize = usize::MAX;

pub struct Blob {
    pixels: Vec<IVec2>,
    links: Vec<[usize; 4]>,
    pub wet: Vec<bool>,
    pub open: Vec<bool>,
    pub height: Vec<f32>,
    pub edge: Vec<f32>,
    pub ground: Vec<f32>,
    pub pigment: Vec<Pigment>,
    pub unit: Vec<f32>,
    pub offset: Vec<f32>,
    pub creep: Vec<f32>,
    pub weight: Vec<f32>,
}

impl Blob {
    pub fn new(pixels: Vec<IVec2>, links: Vec<[usize; 4]>) -> Self {
        let cells = pixels.len();
        Self { pixels, links, wet: vec![false; cells], open: vec![false; cells], height: vec![0.0; cells], edge: vec![0.0; cells], ground: vec![0.0; cells], pigment: vec![[0.0; 7]; cells], unit: vec![0.0; cells], offset: vec![0.0; cells], creep: vec![0.0; cells], weight: vec![1.0; cells] }
    }

    pub fn pixel(&self, index: usize) -> IVec2 {
        self.pixels[index]
    }

    pub fn cells(&self) -> impl Iterator<Item = (usize, IVec2)> + '_ {
        self.pixels.iter().copied().enumerate()
    }

    pub fn pixels(&self) -> impl Iterator<Item = IVec2> + '_ {
        self.cells().filter(|(index, _)| self.wet[*index]).map(|(_, pixel)| pixel)
    }

    fn around(&self, index: usize) -> [usize; 4] {
        self.links[index]
    }

    fn wet_cells(&self) -> Vec<usize> {
        (0..self.wet.len()).filter(|index| self.wet[*index]).collect()
    }

    fn links(&self, cells: &[usize]) -> Vec<[usize; 4]> {
        let mut slot = vec![usize::MAX; self.wet.len()];
        for (position, &index) in cells.iter().enumerate() {
            slot[index] = position;
        }
        cells.iter().map(|&index| self.around(index).map(|n| slot[n])).collect()
    }

    fn solve(links: &[[usize; 4]], right: &[f32], guess: Vec<f32>) -> Vec<f32> {
        let apply = |x: &[f32], out: &mut [f32]| {
            out.par_chunks_mut(CHUNK).enumerate().for_each(|(chunk, out)| {
                for (offset, value) in out.iter_mut().enumerate() {
                    let position = chunk * CHUNK + offset;
                    let sum: f32 = links[position].iter().filter(|n| **n != NOWHERE).map(|n| x[*n]).sum();
                    *value = x[position] - CAPILLARY_AREA * (sum - 4.0 * x[position]);
                }
            });
        };
        let mut x = guess;
        let mut applied = vec![0.0; right.len()];
        apply(&x, &mut applied);
        let mut residual: Vec<f32> = right.iter().zip(&applied).map(|(r, a)| r - a).collect();
        let mut direction = residual.clone();
        let mut rr = dot(&residual, &residual);
        let target = dot(right, right) * SOLVE_TOLERANCE * SOLVE_TOLERANCE;
        for _ in 0..MOST_ITERATIONS {
            if rr <= target || rr == 0.0 {
                break;
            }
            apply(&direction, &mut applied);
            let step = rr / dot(&direction, &applied);
            x.par_chunks_mut(CHUNK).zip(residual.par_chunks_mut(CHUNK)).zip(direction.par_chunks(CHUNK).zip(applied.par_chunks(CHUNK))).for_each(|((x, residual), (direction, applied))| {
                for k in 0..x.len() {
                    x[k] += step * direction[k];
                    residual[k] -= step * applied[k];
                }
            });
            let next = dot(&residual, &residual);
            let keep = next / rr;
            direction.par_chunks_mut(CHUNK).zip(residual.par_chunks(CHUNK)).for_each(|(direction, residual)| {
                for k in 0..direction.len() {
                    direction[k] = residual[k] + keep * direction[k];
                }
            });
            rr = next;
        }
        x
    }

    fn surface(&self, index: usize) -> f32 {
        self.ground[index] + if self.wet[index] { self.height[index] } else { self.edge[index] }
    }

    fn equilibrium(&mut self, cells: &[usize], volume: f32) -> Vec<f32> {
        let links = self.links(cells);
        let ones = vec![1.0; cells.len()];
        let shift: Vec<f32> = cells
            .par_iter()
            .with_min_len(CHUNK)
            .map(|&index| {
                let around = self.around(index);
                let ground_around: f32 = around.iter().map(|n| self.ground[*n]).sum();
                let edges: f32 = around.iter().filter(|n| !self.wet[**n]).map(|n| self.edge[*n]).sum();
                self.ground[index] - CAPILLARY_AREA * (ground_around - 4.0 * self.ground[index]) - CAPILLARY_AREA * edges
            })
            .collect();
        let unit = Self::solve(&links, &ones, cells.par_iter().with_min_len(CHUNK).map(|&index| if self.unit[index] == 0.0 { 1.0 } else { self.unit[index] }).collect());
        let offset = Self::solve(&links, &shift, cells.par_iter().with_min_len(CHUNK).map(|&index| if self.offset[index] == 0.0 { self.ground[index] } else { self.offset[index] }).collect());
        let level = (volume + offset.iter().sum::<f32>()) / unit.iter().sum::<f32>().max(f32::EPSILON);

        let mut target: Vec<f32> = unit.par_iter().with_min_len(CHUNK).zip(&offset).map(|(u, q)| (level * u - q).max(0.0)).collect();
        for (position, &index) in cells.iter().enumerate() {
            self.unit[index] = unit[position];
            self.offset[index] = offset[position];
        }
        rescale(&mut target, volume);
        target
    }

    fn yielding(&self, cells: &[usize], yield_stress: f32) -> Vec<f32> {
        let pressure = self.scatter(
            cells,
            cells
                .par_iter()
                .with_min_len(CHUNK)
                .map(|&index| {
                    let around: f32 = self.around(index).iter().map(|n| self.surface(*n)).sum();
                    self.surface(index) - CAPILLARY_AREA * (around - 4.0 * self.surface(index))
                })
                .collect(),
        );
        let flowing = self.scatter(
            cells,
            cells
                .par_iter()
                .with_min_len(CHUNK)
                .map(|&index| {
                    let [left, right, up, down] = self.around(index).map(|n| if self.wet[n] { pressure[n] } else { pressure[index] });
                    let gradient = 0.5 * (right - left).hypot(down - up);
                    let stress = DENSITY * GRAVITY * self.height[index] * METRES_PER_PIXEL * gradient;
                    (1.0 - yield_stress / stress.max(f32::EPSILON)).clamp(0.0, 1.0)
                })
                .collect(),
        );
        cells
            .par_iter()
            .with_min_len(CHUNK)
            .map(|&index| {
                let (sum, count) = self.around(index).iter().filter(|n| self.wet[**n]).fold((2.0 * flowing[index], 2.0), |(sum, count), n| (sum + flowing[*n], count + 1.0));
                sum / count
            })
            .collect()
    }

    fn scatter(&self, cells: &[usize], values: Vec<f32>) -> Vec<f32> {
        let mut spread = vec![0.0; self.wet.len()];
        for (&index, value) in cells.iter().zip(values) {
            spread[index] = value;
        }
        spread
    }

    fn rate(&self, cells: &[usize], volume: f32, fluid: Fluid) -> f32 {
        let area = cells.len() as f32;
        let perimeter = cells.iter().map(|&index| self.around(index).iter().filter(|n| !self.wet[**n]).count()).sum::<usize>().max(1) as f32;
        let width = 2.0 * area / perimeter * METRES_PER_PIXEL;
        let thickness = volume / area * METRES_PER_PIXEL;
        let wave = PI / width;
        thickness.powi(3) / (3.0 * fluid.viscosity) * (DENSITY * GRAVITY * wave * wave + SURFACE_TENSION * wave.powi(4))
    }

    pub fn relax(&mut self, dt: f32) {
        let cells = self.wet_cells();
        let volume: f32 = cells.iter().map(|&index| self.height[index]).sum();
        if volume <= 0.0 {
            return;
        }
        let medium: f32 = cells.iter().map(|&index| self.pigment[index][6]).sum();
        let fluid = Fluid::of(medium / volume);
        let target = self.equilibrium(&cells, volume);
        let flowing = self.yielding(&cells, fluid.yield_stress * POURED_YIELD_SHARE);
        let pace = 1.0 - (-self.rate(&cells, volume, fluid) * dt).exp();
        let before: Vec<f32> = cells.par_iter().with_min_len(CHUNK).map(|&index| self.height[index]).collect();
        let mut after: Vec<f32> = cells.par_iter().with_min_len(CHUNK).enumerate().map(|(position, &index)| before[position] + self.weight[index] * pace * flowing[position] * (target[position] - before[position])).collect();
        let weights: f32 = cells.iter().map(|&index| self.weight[index]).sum();
        let shortfall = (volume - after.iter().sum::<f32>()) / weights.max(f32::EPSILON);
        for (position, &index) in cells.iter().enumerate() {
            after[position] = (after[position] + shortfall * self.weight[index]).max(0.0);
        }
        rescale(&mut after, volume);
        for (position, &index) in cells.iter().enumerate() {
            self.height[index] = after[position];
        }
        self.carry_pigment(&cells, &before);
        self.spread(&cells, &flowing, fluid, dt);
    }

    fn carry_pigment(&mut self, cells: &[usize], before: &[f32]) {
        let mut earlier = vec![0.0; self.wet.len()];
        for (position, &index) in cells.iter().enumerate() {
            earlier[index] = before[position];
        }
        let carried: Vec<Pigment> = cells
            .par_iter()
            .with_min_len(CHUNK)
            .map(|&index| {
                if earlier[index] > 0.0 {
                    let scale = self.height[index] / earlier[index];
                    return self.pigment[index].map(|channel| channel * scale);
                }
                let (near_height, near_pigment) = self.around(index).into_iter().filter(|n| self.wet[*n] && earlier[*n] > 0.0).fold((0.0, [0.0; 7]), |(height, pigment): (f32, Pigment), n| (height + earlier[n], std::array::from_fn(|channel| pigment[channel] + self.pigment[n][channel])));
                if near_height > 0.0 { near_pigment.map(|channel| channel * self.height[index] / near_height) } else { self.pigment[index] }
            })
            .collect();
        for (&index, pigment) in cells.iter().zip(carried) {
            self.pigment[index] = pigment;
        }
    }

    fn spread(&mut self, cells: &[usize], flowing: &[f32], fluid: Fluid, dt: f32) {
        let creep = SURFACE_TENSION / (fluid.viscosity * CONTACT_LINE_FRICTION) * dt / METRES_PER_PIXEL;
        let mut fronts = Vec::new();
        for (position, &index) in cells.iter().enumerate() {
            for n in self.around(index) {
                if self.wet[n] || !self.open[n] {
                    continue;
                }
                let angle = (self.height[index] - WEAVE_GUIDANCE * (self.ground[n] - self.ground[index])).max(0.0).atan();
                let pinned = self.pinning(n);
                let receding = (WETTING_SLOPE * pinned).atan();
                self.creep[n] += (flowing[position] * creep * (angle.powi(3) - receding.powi(3))).max(0.0);
                if self.creep[n] >= pinned {
                    fronts.push(n);
                }
            }
        }
        for front in fronts {
            self.wet[front] = true;
            self.creep[front] = 0.0;
        }
    }

    fn pinning(&self, index: usize) -> f32 {
        1.0 + PINNING_SPREAD * (2.0 * smooth_noise(self.pixels[index].as_vec2() / FIBRE) - 1.0)
    }
}

fn dot(a: &[f32], b: &[f32]) -> f32 {
    let partial: Vec<f32> = a.par_chunks(CHUNK).zip(b.par_chunks(CHUNK)).map(|(a, b)| a.iter().zip(b).map(|(a, b)| a * b).sum()).collect();
    partial.iter().sum()
}

fn hash(pixel: IVec2) -> f32 {
    let mut hash = (pixel.x as u32).wrapping_mul(0x8da6_b343) ^ (pixel.y as u32).wrapping_mul(0xd816_3841);
    hash ^= hash >> 16;
    hash = hash.wrapping_mul(0x7feb_352d);
    hash ^= hash >> 15;
    (hash >> 8) as f32 / 16_777_216.0
}

fn smooth_noise(at: glam::Vec2) -> f32 {
    let corner = at.floor();
    let fraction = at - corner;
    let ease = fraction * fraction * (3.0 - 2.0 * fraction);
    let lattice = |dx: i32, dy: i32| hash(corner.as_ivec2() + IVec2::new(dx, dy));
    let top = lattice(0, 0) + (lattice(1, 0) - lattice(0, 0)) * ease.x;
    let bottom = lattice(0, 1) + (lattice(1, 1) - lattice(0, 1)) * ease.x;
    top + (bottom - top) * ease.y
}

fn rescale(values: &mut [f32], volume: f32) {
    let total: f32 = values.iter().sum();
    if total > 0.0 {
        values.iter_mut().for_each(|value| *value *= volume / total);
    }
}
