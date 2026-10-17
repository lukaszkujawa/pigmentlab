# PigmentLab

A real-time painting simulator written in Rust. Bristle brushes, a palette knife, a colour shaper and a paint tube push physically simulated oil paint and watercolour around a textured canvas, with Kubelka-Munk colour mixing and GPU-accelerated lighting. Wacom pens are supported, including pressure and tilt.

## Requirements

- [Rust](https://www.rust-lang.org/tools/install) 1.85 or newer
- macOS with a Metal-capable GPU

## Installation

```sh
git clone https://github.com/lukaszkujawa/pigmentlab.git
cd pigmentlab
cargo build --release
```

## Running

```sh
cargo run --release -p pigmentlab
```
