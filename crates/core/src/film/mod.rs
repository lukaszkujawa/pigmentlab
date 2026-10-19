mod deltas;
mod raster;
mod settle;

pub use deltas::{FILM_CHANGE_CAPACITY, FilmDeltas};

use crate::canvas::PAINT_SCALE;
use crate::paint::Mark;
use glam::IVec2;
use rayon::prelude::*;
use settle::{Blob, NOWHERE, Pigment};

const VISITED: usize = NOWHERE - 1;
const REACHED: usize = NOWHERE - 2;
use std::collections::BTreeMap;

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
}

pub struct Film {
    width: i32,
    height: i32,
    relief: Vec<f32>,
    tiles: BTreeMap<Key, Tile>,
    laid: Vec<f32>,
    places: Vec<usize>,
    depths: Vec<u32>,
    clock: f32,
    live_cells: usize,
    waited: f32,
    skipped: usize,
}

type Key = (i32, i32);

fn tile_of(pixel: IVec2) -> (Key, usize) {
    let tile = pixel.div_euclid(IVec2::splat(TILE));
    let local = pixel - tile * TILE;
    ((tile.x, tile.y), (local.y * TILE + local.x) as usize)
}

fn tile_origin(key: Key) -> IVec2 {
    IVec2::new(key.0, key.1) * TILE
}

impl Film {
    pub fn new(width: u32, height: u32, relief: Vec<f32>) -> Self {
        Self { width: width as i32, height: height as i32, relief, tiles: BTreeMap::new(), laid: Vec::new(), places: Vec::new(), depths: Vec::new(), clock: 0.0, live_cells: 0, waited: 0.0, skipped: 0 }
    }

    pub fn set_relief(&mut self, relief: Vec<f32>) {
        self.relief = relief;
    }

    pub fn forget(&mut self) {
        self.tiles.clear();
        self.laid.clear();
    }

    pub fn unsettled(&self) -> f32 {
        self.tiles.values().flat_map(|tile| (0..CELLS).map(move |cell| tile.height[cell] - tile.committed[cell][0] as f32 / PAINT_SCALE)).sum()
    }

    pub fn lay(&mut self, marks: &[Mark]) {
        for mark in marks {
            let pigment = [mark.paint.absorption.x, mark.paint.absorption.y, mark.paint.absorption.z, mark.paint.scattering.x, mark.paint.scattering.y, mark.paint.scattering.z, mark.medium];
            let covered = raster::rasterise(mark);
            for cover in &covered {
                if !self.on_canvas(cover.pixel) {
                    continue;
                }
                let (key, cell) = tile_of(cover.pixel);
                let tile = self.tiles.entry(key).or_insert_with(Tile::new);
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
            let (key, cell) = tile_of(at);
            if let Some(tile) = self.tiles.get_mut(&key) {
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
        let retired: Vec<Key> = self.tiles.iter().filter(|(_, tile)| !tile.kept(self.clock) && !tile.changed && tile.height.iter().zip(&tile.committed).all(|(height, committed)| fixed_point(*height, &[0.0; 7])[0] == committed[0])).map(|(key, _)| *key).collect();
        if retired.is_empty() {
            return;
        }
        if self.laid.is_empty() {
            self.laid = vec![0.0; (self.width * self.height) as usize];
        }
        for key in retired {
            let tile = self.tiles.remove(&key).expect("a retired tile exists");
            let origin = tile_origin(key);
            for cell in (0..CELLS).filter(|cell| tile.committed[*cell][0] != 0) {
                let pixel = origin + IVec2::new(cell as i32 % TILE, cell as i32 / TILE);
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
        let mut places = std::mem::take(&mut self.places);
        let mut depths = std::mem::take(&mut self.depths);
        let atlas = Atlas::of(&self.tiles);
        if places.len() < atlas.slots.len() * CELLS {
            places.resize(atlas.slots.len() * CELLS, NOWHERE);
        }
        depths.resize(atlas.slots.len() * CELLS, 0);
        let mut reached = Vec::new();
        let mut depth = Vec::new();
        for (key, tile) in self.tiles.iter().filter(|(_, tile)| self.clock - tile.fed_at < FLOWING_SECONDS) {
            for cell in (0..CELLS).filter(|cell| tile.stirred(*cell, self.clock)) {
                let pixel = tile_origin(*key) + IVec2::new(cell as i32 % TILE, cell as i32 / TILE);
                atlas.mark(&mut places, pixel, REACHED);
                if let Some((slot, at)) = atlas.slot(pixel) {
                    depths[slot * CELLS + at] = 0;
                }
                reached.push(pixel);
                depth.push(0);
            }
        }
        let mut next = 0;
        while next < reached.len() {
            let (pixel, steps) = (reached[next], depth[next]);
            next += 1;
            if steps >= SETTLING_REACH {
                continue;
            }
            for neighbour in NEIGHBOURS.map(|step| pixel + step) {
                if atlas.cell(neighbour).is_some_and(|(tile, cell)| tile.wet[cell]) && atlas.place(&places, neighbour) == Some(NOWHERE) {
                    atlas.mark(&mut places, neighbour, REACHED);
                    if let Some((slot, cell)) = atlas.slot(neighbour) {
                        depths[slot * CELLS + cell] = steps + 1;
                    }
                    reached.push(neighbour);
                    depth.push(steps + 1);
                }
            }
        }
        let mut blobs = Vec::new();
        for &start in &reached {
            if atlas.place(&places, start) != Some(REACHED) {
                continue;
            }
            atlas.mark(&mut places, start, VISITED);
            let mut pixels = vec![start];
            let mut next = 0;
            while next < pixels.len() {
                let pixel = pixels[next];
                next += 1;
                for neighbour in NEIGHBOURS.map(|step| pixel + step) {
                    if atlas.place(&places, neighbour) == Some(REACHED) {
                        atlas.mark(&mut places, neighbour, VISITED);
                        pixels.push(neighbour);
                    }
                }
            }
            let mut blob = self.gather(&atlas, &mut places, pixels);
            for (index, pixel) in blob.cells().filter(|(index, _)| blob.wet[*index]).collect::<Vec<_>>() {
                let (slot, cell) = atlas.slot(pixel).expect("a blob pixel lies in the atlas");
                let depth = depths[slot * CELLS + cell] as f32 / SETTLING_REACH as f32;
                blob.weight[index] = 1.0 - depth * depth * (3.0 - 2.0 * depth);
            }
            blobs.push(blob);
        }
        for pixel in reached {
            atlas.mark(&mut places, pixel, NOWHERE);
        }
        self.places = places;
        self.depths = depths;
        blobs
    }

    fn gather(&self, atlas: &Atlas, places: &mut [usize], mut pixels: Vec<IVec2>) -> Blob {
        let place = |pixel: IVec2| atlas.slot(pixel).map(|(slot, cell)| slot * CELLS + cell);
        let wet = pixels.len();
        for (index, pixel) in pixels.iter().enumerate() {
            places[place(*pixel).expect("a wet pixel lies in the atlas")] = index;
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
        let mut blob = Blob::new(pixels.clone(), links);
        for (index, &pixel) in pixels.iter().enumerate() {
            blob.ground[index] = self.relief_at(pixel);
            let found = atlas.cell(pixel);
            match found.filter(|_| index < wet) {
                Some((tile, cell)) => {
                    blob.wet[index] = true;
                    blob.height[index] = tile.height[cell];
                    blob.pigment[index] = tile.pigment[cell];
                    blob.unit[index] = tile.unit[cell];
                    blob.offset[index] = tile.offset[cell];
                }
                None => {
                    blob.creep[index] = found.map_or(0.0, |(tile, cell)| tile.creep[cell]);
                    let neighbour = found.filter(|(tile, cell)| tile.wet[*cell]);
                    blob.edge[index] = neighbour.map_or(0.0, |(tile, cell)| tile.height[cell]);
                    blob.open[index] = self.on_canvas(pixel) && neighbour.is_none();
                }
            }
        }
        for (index, pixel) in pixels.iter().enumerate() {
            places[place(*pixel).expect("a blob pixel lies in the atlas")] = if index < wet { VISITED } else { NOWHERE };
        }
        blob
    }

    fn write_back(&mut self, blob: &Blob) {
        let mut moving = Vec::new();
        let mut current: Option<(Key, &mut Tile)> = None;
        for (index, pixel) in blob.cells() {
            if !blob.wet[index] {
                if blob.creep[index] > 0.0 {
                    let (key, cell) = tile_of(pixel);
                    current = None;
                    self.tiles.entry(key).or_insert_with(Tile::new).creep[cell] = blob.creep[index];
                }
                continue;
            }
            let (key, cell) = tile_of(pixel);
            if current.as_ref().is_none_or(|(held, _)| *held != key) {
                current = Some((key, self.tiles.entry(key).or_insert_with(Tile::new)));
            }
            let (_, tile) = current.as_mut().expect("the current tile was just set");
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
        let mut changes = Vec::new();
        let clock = self.clock;
        for (key, tile) in self.tiles.iter_mut().filter(|(_, tile)| tile.changed || !tile.kept(clock)) {
            let step = if tile.kept(clock) { UPLOAD_STEP } else { 0 };
            let mut pending = false;
            for cell in 0..CELLS {
                let fixed = fixed_point(tile.height[cell], &tile.pigment[cell]);
                let committed = tile.committed[cell];
                let appeared = (fixed[0] > 0) != (committed[0] > 0);
                if !appeared && (0..8).all(|channel| (fixed[channel] - committed[channel]).abs() <= step) {
                    continue;
                }
                if changes.len() >= most {
                    pending = true;
                    continue;
                }
                let pixel = tile_origin(*key) + IVec2::new(cell as i32 % TILE, cell as i32 / TILE);
                let delta = std::array::from_fn(|channel| fixed[channel] - tile.committed[cell][channel]);
                changes.push(Change { pixel: (pixel.y * width + pixel.x) as u32, delta });
                tile.committed[cell] = fixed;
            }
            tile.changed = pending;
        }
        changes
    }
}

struct Atlas<'a> {
    low: IVec2,
    span: IVec2,
    slots: Vec<Option<&'a Tile>>,
}

impl<'a> Atlas<'a> {
    fn of(tiles: &'a BTreeMap<Key, Tile>) -> Self {
        let keys = || tiles.keys().map(|&(x, y)| IVec2::new(x, y));
        let low = keys().reduce(IVec2::min).unwrap_or(IVec2::ZERO) - 1;
        let span = keys().reduce(IVec2::max).unwrap_or(IVec2::ZERO) + 2 - low;
        let mut slots = vec![None; (span.x * span.y) as usize];
        for (&(x, y), tile) in tiles {
            slots[((y - low.y) * span.x + x - low.x) as usize] = Some(tile);
        }
        Self { low, span, slots }
    }

    fn slot(&self, pixel: IVec2) -> Option<(usize, usize)> {
        let ((x, y), cell) = tile_of(pixel);
        let local = IVec2::new(x, y) - self.low;
        (local.x >= 0 && local.y >= 0 && local.x < self.span.x && local.y < self.span.y).then(|| ((local.y * self.span.x + local.x) as usize, cell))
    }

    fn cell(&self, pixel: IVec2) -> Option<(&'a Tile, usize)> {
        self.slot(pixel).and_then(|(slot, cell)| self.slots[slot].map(|tile| (tile, cell)))
    }

    fn place(&self, places: &[usize], pixel: IVec2) -> Option<usize> {
        self.slot(pixel).map(|(slot, cell)| places[slot * CELLS + cell])
    }

    fn mark(&self, places: &mut [usize], pixel: IVec2, value: usize) {
        if let Some((slot, cell)) = self.slot(pixel) {
            places[slot * CELLS + cell] = value;
        }
    }
}

fn fixed_point(height: f32, pigment: &Pigment) -> [i32; 8] {
    let scaled = |value: f32| (value * PAINT_SCALE).round() as i32;
    std::array::from_fn(|channel| if channel == 0 { scaled(height) } else { scaled(pigment[channel - 1]) })
}

#[cfg(test)]
mod tests;
