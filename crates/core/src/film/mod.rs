mod deltas;
mod raster;
mod settle;
mod worker;

pub use deltas::{FILM_CHANGE_CAPACITY, FilmDeltas};
pub use worker::FilmWorker;

use crate::canvas::PAINT_SCALE;
use crate::paint::Mark;
use glam::IVec2;
use rayon::prelude::*;
use settle::{Blob, NOWHERE, Pigment};

const VISITED: usize = NOWHERE - 1;
const REACHED: usize = NOWHERE - 2;

const TILE: i32 = 32;
const CELLS: usize = (TILE * TILE) as usize;
const WET_SECONDS: f32 = 6.0;
const FLOWING_SECONDS: f32 = 1.5;
const MOVING: f32 = 0.05;
const SETTLING_REACH: u32 = 32;
const UPLOAD_STEP: i32 = (0.01 * PAINT_SCALE) as i32;
const CELL_BUDGET: usize = 250_000;
const NEIGHBOURS: [IVec2; 4] = [IVec2::new(-1, 0), IVec2::new(1, 0), IVec2::new(0, -1), IVec2::new(0, 1)];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Change {
    pub pixel: u32,
    pub delta: [i32; 8],
}

type Drift = Vec<(usize, [i32; 8])>;

#[derive(Clone)]
struct Tile {
    height: Vec<f32>,
    pigment: Vec<Pigment>,
    wet: Vec<bool>,
    unit: Vec<f32>,
    offset: Vec<f32>,
    committed: Vec<[i32; 8]>,
    stirred_at: Vec<f32>,
    creep: Vec<f32>,
    fed_at: f32,
    changed: bool,
}

impl Tile {
    fn new() -> Self {
        Self { height: vec![0.0; CELLS], pigment: vec![[0.0; 7]; CELLS], wet: vec![false; CELLS], unit: vec![0.0; CELLS], offset: vec![0.0; CELLS], committed: vec![[0; 8]; CELLS], stirred_at: vec![f32::NEG_INFINITY; CELLS], creep: vec![0.0; CELLS], fed_at: 0.0, changed: false }
    }

    fn stirred(&self, cell: usize, now: f32) -> bool {
        self.wet[cell] && now - self.stirred_at[cell] < FLOWING_SECONDS
    }

    fn kept(&self, now: f32) -> bool {
        now - self.fed_at < WET_SECONDS
    }

    fn drifted(&self, now: f32) -> Drift {
        let step = if self.kept(now) { UPLOAD_STEP } else { 0 };
        (0..CELLS)
            .filter_map(|cell| {
                let fixed = fixed_point(self.height[cell], &self.pigment[cell]);
                let committed = self.committed[cell];
                let appeared = (fixed[0] > 0) != (committed[0] > 0);
                (appeared || (0..8).any(|channel| (fixed[channel] - committed[channel]).abs() > step)).then_some((cell, fixed))
            })
            .collect()
    }
}

#[derive(Clone, Copy)]
struct Layout {
    columns: i32,
    rows: i32,
}

impl Layout {
    fn new(width: i32, height: i32) -> Self {
        Self { columns: width.div_euclid(TILE) + 3, rows: height.div_euclid(TILE) + 3 }
    }

    fn slots(self) -> usize {
        (self.columns * self.rows) as usize
    }

    fn slot(self, pixel: IVec2) -> Option<(usize, usize)> {
        let tile = pixel.div_euclid(IVec2::splat(TILE));
        let local = pixel - tile * TILE;
        let bordered = tile + 1;
        (bordered.x >= 0 && bordered.y >= 0 && bordered.x < self.columns && bordered.y < self.rows).then(|| ((bordered.x * self.rows + bordered.y) as usize, (local.y * TILE + local.x) as usize))
    }

    fn origin(self, slot: usize) -> IVec2 {
        (IVec2::new(slot as i32 / self.rows, slot as i32 % self.rows) - 1) * TILE
    }

    fn pixel(self, slot: usize, cell: usize) -> IVec2 {
        self.origin(slot) + IVec2::new(cell as i32 % TILE, cell as i32 / TILE)
    }
}

struct Region {
    pixels: Vec<IVec2>,
    links: Vec<[usize; 4]>,
    wet: usize,
}

pub struct Film {
    width: i32,
    height: i32,
    relief: Vec<f32>,
    layout: Layout,
    tiles: Vec<Option<Tile>>,
    laid: Vec<f32>,
    places: Vec<usize>,
    depths: Vec<u32>,
    clock: f32,
    live_cells: usize,
    waited: f32,
    skipped: usize,
}

impl Film {
    pub fn new(width: u32, height: u32, relief: Vec<f32>) -> Self {
        let layout = Layout::new(width as i32, height as i32);
        Self { width: width as i32, height: height as i32, relief, layout, tiles: vec![None; layout.slots()], laid: Vec::new(), places: Vec::new(), depths: Vec::new(), clock: 0.0, live_cells: 0, waited: 0.0, skipped: 0 }
    }

    pub fn set_relief(&mut self, relief: Vec<f32>) {
        self.relief = relief;
    }

    pub fn forget(&mut self) {
        self.tiles.fill(None);
        self.laid.clear();
    }

    pub fn unsettled(&self) -> f32 {
        self.tiles.iter().flatten().flat_map(|tile| (0..CELLS).map(move |cell| tile.height[cell] - tile.committed[cell][0] as f32 / PAINT_SCALE)).sum()
    }

    pub fn lay(&mut self, marks: &[Mark]) {
        for mark in marks {
            let pigment = [mark.paint.absorption.x, mark.paint.absorption.y, mark.paint.absorption.z, mark.paint.scattering.x, mark.paint.scattering.y, mark.paint.scattering.z, mark.medium];
            let covered = raster::rasterise(mark);
            for cover in &covered {
                if !self.on_canvas(cover.pixel) {
                    continue;
                }
                let (slot, cell) = self.layout.slot(cover.pixel).expect("a canvas pixel lies in the layout");
                let tile = self.tiles[slot].get_or_insert_with(Tile::new);
                tile.height[cell] += cover.volume;
                for (channel, value) in pigment.iter().enumerate() {
                    tile.pigment[cell][channel] += cover.volume * value;
                }
                tile.wet[cell] = true;
                tile.changed = true;
            }
            for cover in &covered {
                self.stir(cover.pixel);
            }
        }
    }

    fn stir(&mut self, pixel: IVec2) {
        let now = self.clock;
        for at in std::iter::once(pixel).chain(NEIGHBOURS.map(|step| pixel + step)) {
            if let Some((tile, cell)) = self.layout.slot(at).and_then(|(slot, cell)| self.tiles[slot].as_mut().map(|tile| (tile, cell))) {
                tile.stirred_at[cell] = now;
                tile.fed_at = now;
            }
        }
    }

    pub fn settle(&mut self, dt: f32, most: usize) -> Vec<Change> {
        self.waited += dt;
        self.skipped += 1;
        if self.skipped >= self.live_cells.div_ceil(CELL_BUDGET).max(1) {
            self.relax(self.waited);
            self.waited = 0.0;
            self.skipped = 0;
        }
        self.clock += dt;
        let changes = self.changes(most);
        self.retire();
        changes
    }

    fn retire(&mut self) {
        let clock = self.clock;
        let retired: Vec<usize> = (0..self.tiles.len()).filter(|slot| self.tiles[*slot].as_ref().is_some_and(|tile| !tile.kept(clock) && !tile.changed && tile.height.iter().zip(&tile.committed).all(|(height, committed)| fixed_point(*height, &[0.0; 7])[0] == committed[0]))).collect();
        if retired.is_empty() {
            return;
        }
        if self.laid.is_empty() {
            self.laid = vec![0.0; (self.width * self.height) as usize];
        }
        for slot in retired {
            let tile = self.tiles[slot].take().expect("a retired tile exists");
            for cell in (0..CELLS).filter(|cell| tile.committed[*cell][0] != 0) {
                let pixel = self.layout.pixel(slot, cell);
                self.laid[(pixel.y * self.width + pixel.x) as usize] += tile.committed[cell][0] as f32 / PAINT_SCALE;
            }
        }
    }

    fn relax(&mut self, dt: f32) {
        let mut blobs = self.blobs();
        self.live_cells = blobs.iter().map(|blob| blob.pixels().count()).sum();
        blobs.par_iter_mut().for_each(|blob| blob.relax(dt));
        for blob in &blobs {
            self.write_back(blob);
        }
    }

    fn on_canvas(&self, pixel: IVec2) -> bool {
        pixel.x >= 0 && pixel.y >= 0 && pixel.x < self.width && pixel.y < self.height
    }

    fn relief_at(&self, pixel: IVec2) -> f32 {
        let clamped = pixel.clamp(IVec2::ZERO, IVec2::new(self.width - 1, self.height - 1));
        let index = (clamped.y * self.width + clamped.x) as usize;
        self.relief.get(index).copied().unwrap_or(0.0) + self.laid.get(index).copied().unwrap_or(0.0)
    }

    fn blobs(&mut self) -> Vec<Blob> {
        let layout = self.layout;
        let place = |pixel: IVec2| layout.slot(pixel).map(|(slot, cell)| slot * CELLS + cell);
        let mut places = std::mem::take(&mut self.places);
        let mut depths = std::mem::take(&mut self.depths);
        places.resize(layout.slots() * CELLS, NOWHERE);
        depths.resize(layout.slots() * CELLS, 0);
        let mut reached = self.stirred_pixels();
        for &pixel in &reached {
            let at = place(pixel).expect("a stirred pixel lies in the layout");
            places[at] = REACHED;
            depths[at] = 0;
        }
        let mut next = 0;
        while next < reached.len() {
            let pixel = reached[next];
            next += 1;
            let steps = depths[place(pixel).expect("a reached pixel lies in the layout")];
            if steps >= SETTLING_REACH {
                continue;
            }
            for neighbour in NEIGHBOURS.map(|step| pixel + step) {
                if let Some(at) = place(neighbour).filter(|at| places[*at] == NOWHERE && self.tiles[at / CELLS].as_ref().is_some_and(|tile| tile.wet[at % CELLS])) {
                    places[at] = REACHED;
                    depths[at] = steps + 1;
                    reached.push(neighbour);
                }
            }
        }
        let mut regions = Vec::new();
        for &start in &reached {
            let start_at = place(start).expect("a reached pixel lies in the layout");
            if places[start_at] != REACHED {
                continue;
            }
            places[start_at] = VISITED;
            let mut pixels = vec![start];
            let mut next = 0;
            while next < pixels.len() {
                let pixel = pixels[next];
                next += 1;
                for neighbour in NEIGHBOURS.map(|step| pixel + step) {
                    if let Some(at) = place(neighbour).filter(|at| places[*at] == REACHED) {
                        places[at] = VISITED;
                        pixels.push(neighbour);
                    }
                }
            }
            regions.push(self.outline(&mut places, pixels));
        }
        for pixel in reached {
            places[place(pixel).expect("a reached pixel lies in the layout")] = NOWHERE;
        }
        let blobs = regions.into_par_iter().map(|region| self.fill(region, &depths)).collect();
        self.places = places;
        self.depths = depths;
        blobs
    }

    fn stirred_pixels(&self) -> Vec<IVec2> {
        let clock = self.clock;
        let layout = self.layout;
        let per_tile: Vec<Vec<IVec2>> = self
            .tiles
            .par_iter()
            .enumerate()
            .filter_map(|(slot, tile)| tile.as_ref().filter(|tile| clock - tile.fed_at < FLOWING_SECONDS).map(|tile| (0..CELLS).filter(|cell| tile.stirred(*cell, clock)).map(|cell| layout.pixel(slot, cell)).collect()))
            .collect();
        per_tile.concat()
    }

    fn outline(&self, places: &mut [usize], mut pixels: Vec<IVec2>) -> Region {
        let layout = self.layout;
        let place = |pixel: IVec2| layout.slot(pixel).map(|(slot, cell)| slot * CELLS + cell);
        let wet = pixels.len();
        for (index, pixel) in pixels.iter().enumerate() {
            places[place(*pixel).expect("a wet pixel lies in the layout")] = index;
        }
        for index in 0..wet {
            for neighbour in NEIGHBOURS.map(|step| pixels[index] + step) {
                if let Some(at) = place(neighbour).filter(|at| places[*at] == NOWHERE) {
                    places[at] = pixels.len();
                    pixels.push(neighbour);
                }
            }
        }
        let links = pixels[..wet].iter().map(|pixel| NEIGHBOURS.map(|step| place(*pixel + step).map_or(NOWHERE, |at| places[at]))).collect();
        for (index, pixel) in pixels.iter().enumerate() {
            places[place(*pixel).expect("a blob pixel lies in the layout")] = if index < wet { VISITED } else { NOWHERE };
        }
        Region { pixels, links, wet }
    }

    fn fill(&self, region: Region, depths: &[u32]) -> Blob {
        let Region { pixels, links, wet } = region;
        let mut blob = Blob::new(pixels, links);
        for index in 0..blob.wet.len() {
            let pixel = blob.pixel(index);
            blob.ground[index] = self.relief_at(pixel);
            let found = self.layout.slot(pixel).and_then(|(slot, cell)| self.tiles[slot].as_ref().map(|tile| (tile, slot, cell)));
            match found.filter(|_| index < wet) {
                Some((tile, slot, cell)) => {
                    let depth = depths[slot * CELLS + cell] as f32 / SETTLING_REACH as f32;
                    blob.wet[index] = true;
                    blob.height[index] = tile.height[cell];
                    blob.pigment[index] = tile.pigment[cell];
                    blob.unit[index] = tile.unit[cell];
                    blob.offset[index] = tile.offset[cell];
                    blob.weight[index] = 1.0 - depth * depth * (3.0 - 2.0 * depth);
                }
                None => {
                    blob.creep[index] = found.map_or(0.0, |(tile, _, cell)| tile.creep[cell]);
                    let neighbour = found.filter(|(tile, _, cell)| tile.wet[*cell]);
                    blob.edge[index] = neighbour.map_or(0.0, |(tile, _, cell)| tile.height[cell]);
                    blob.open[index] = self.on_canvas(pixel) && neighbour.is_none();
                }
            }
        }
        blob
    }

    fn write_back(&mut self, blob: &Blob) {
        let mut moving = Vec::new();
        for (index, pixel) in blob.cells() {
            if !blob.wet[index] && blob.creep[index] <= 0.0 {
                continue;
            }
            let (slot, cell) = self.layout.slot(pixel).expect("a blob pixel lies in the layout");
            let tile = self.tiles[slot].get_or_insert_with(Tile::new);
            if !blob.wet[index] {
                tile.creep[cell] = blob.creep[index];
                continue;
            }
            if tile.height[cell] != blob.height[index] || tile.pigment[cell] != blob.pigment[index] || !tile.wet[cell] {
                tile.changed = true;
            }
            if !tile.wet[cell] || (tile.height[cell] - blob.height[index]).abs() > MOVING {
                moving.push(pixel);
            }
            tile.wet[cell] = true;
            tile.height[cell] = blob.height[index];
            tile.pigment[cell] = blob.pigment[index];
            tile.unit[cell] = blob.unit[index];
            tile.offset[cell] = blob.offset[index];
        }
        for pixel in moving {
            self.stir(pixel);
        }
    }

    fn changes(&mut self, most: usize) -> Vec<Change> {
        let width = self.width;
        let layout = self.layout;
        let clock = self.clock;
        let drifted: Vec<(usize, Drift)> = self.tiles.par_iter().enumerate().filter_map(|(slot, tile)| tile.as_ref().filter(|tile| tile.changed || !tile.kept(clock)).map(|tile| (slot, tile.drifted(clock)))).collect();
        let mut changes = Vec::new();
        for (slot, cells) in drifted {
            let tile = self.tiles[slot].as_mut().expect("a drifted tile exists");
            let room = most.saturating_sub(changes.len()).min(cells.len());
            for &(cell, fixed) in &cells[..room] {
                let pixel = layout.pixel(slot, cell);
                let delta = std::array::from_fn(|channel| fixed[channel] - tile.committed[cell][channel]);
                changes.push(Change { pixel: (pixel.y * width + pixel.x) as u32, delta });
                tile.committed[cell] = fixed;
            }
            tile.changed = room < cells.len();
        }
        changes
    }
}

fn fixed_point(height: f32, pigment: &Pigment) -> [i32; 8] {
    let scaled = |value: f32| (value * PAINT_SCALE).round() as i32;
    std::array::from_fn(|channel| if channel == 0 { scaled(height) } else { scaled(pigment[channel - 1]) })
}

#[cfg(test)]
mod tests;

