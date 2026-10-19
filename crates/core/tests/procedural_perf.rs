use glam::Vec2;
use pigmentlab_core::drip::Hanging;
use pigmentlab_core::pigment::by_name;
use pigmentlab_core::units::cm;
use pigmentlab_core::{CanvasSize, Engine, Gpu};
use std::time::Instant;

const FRAME: f64 = 1.0 / 60.0;

fn engine(medium: f32) -> Engine {
    let mut engine = Engine::new(Gpu::headless(), CanvasSize { width: 2048, height: 1536 }, 1);
    engine.set_param("medium", medium);
    engine.set_param("load", 1.0);
    engine.dip(by_name("cadmium_red").unwrap());
    engine
}

fn frames(label: &str, engine: &mut Engine, seconds: f64) {
    engine.paint_mass();
    let start = engine.time();
    let count = (seconds / FRAME).round() as usize;
    let mut slowest = 0.0f64;
    let started = Instant::now();
    for frame in 1..=count {
        let began = Instant::now();
        engine.advance_exact(start + frame as f64 * FRAME);
        engine.render();
        engine.gpu().device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
        slowest = slowest.max(began.elapsed().as_secs_f64() * 1000.0);
    }
    let mean = started.elapsed().as_secs_f64() * 1000.0 / count as f64;
    println!("PERF {label}: {mean:.2} ms mean, {slowest:.2} ms slowest per 60 Hz frame");
}

fn realtime_frames(label: &str, engine: &mut Engine, seconds: f64) {
    let start = engine.time();
    let clock = Instant::now();
    let mut times = Vec::new();
    while clock.elapsed().as_secs_f64() < seconds {
        let began = Instant::now();
        engine.advance_realtime(start + clock.elapsed().as_secs_f64());
        engine.render();
        engine.gpu().device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
        let took = began.elapsed().as_secs_f64();
        times.push(took * 1000.0);
        std::thread::sleep(std::time::Duration::from_secs_f64((FRAME - took).max(0.0)));
    }
    let mean = times.iter().sum::<f64>() / times.len() as f64;
    let slowest = times.iter().copied().fold(0.0, f64::max);
    println!("PERF realtime {label}: {mean:.2} ms mean, {slowest:.2} ms slowest per frame, simulated {:.2} s in {seconds} s", engine.time() - start);
}

#[test]
#[ignore]
fn report_procedural_frame_times() {
    let mut idle = engine(0.5);
    frames("idle", &mut idle, 1.0);
    for (medium, force) in [(0.2, 9.0), (0.5, 9.0), (1.0, 9.0), (1.0, 3.0)] {
        let mut thrower = engine(medium);
        thrower.select_brush("Splatter");
        thrower.set_param("splatter_force", force);
        thrower.splatter(Vec2::new(300.0, 768.0), Vec2::new(1500.0, 768.0));
        frames(&format!("splatter medium {medium} force {force}"), &mut thrower, 1.5);
    }
    for medium in [0.35, 1.0] {
        let mut pourer = engine(medium);
        pourer.set_param("hole", 4.0);
        for k in 0..4 {
            let hook = Vec2::new(500.0 + 350.0 * k as f32, 768.0);
            pourer.hang_bottle(Hanging { pivot: hook, release: hook + Vec2::new(cm(6.0), 0.0), push: Vec2::new(0.0, cm(20.0)) });
        }
        frames(&format!("four bottles medium {medium}"), &mut pourer, 4.0);
    }
}

#[test]
#[ignore]
fn report_strongest_splatter_frame_times() {
    for medium in [0.2, 0.5, 1.0] {
        let mut thrower = engine(medium);
        thrower.select_brush("Splatter");
        thrower.set_param("splatter_force", 10.0);
        thrower.set_param("splatter_height", 200.0);
        thrower.splatter(Vec2::new(300.0, 768.0), Vec2::new(1500.0, 768.0));
        frames(&format!("strongest splatter medium {medium}"), &mut thrower, 2.5);
    }
}


#[test]
#[ignore]
fn report_runny_bottles_frame_times() {
    let mut pourer = engine(1.0);
    pourer.set_param("hole", 4.0);
    for k in 0..4 {
        let hook = Vec2::new(500.0 + 350.0 * k as f32, 768.0);
        pourer.hang_bottle(Hanging { pivot: hook, release: hook + Vec2::new(cm(6.0), 0.0), push: Vec2::new(0.0, cm(20.0)) });
    }
    frames("four bottles medium 1", &mut pourer, 4.0);
}

#[test]
#[ignore]
fn report_repeated_splatter_frame_times() {
    for medium in [0.5, 1.0] {
        for realtime in [false, true] {
            let mut thrower = engine(medium);
            thrower.select_brush("Splatter");
            thrower.set_param("splatter_force", 10.0);
            let throws = [(Vec2::new(300.0, 768.0), Vec2::new(1500.0, 768.0)), (Vec2::new(1700.0, 400.0), Vec2::new(500.0, 1100.0)), (Vec2::new(1000.0, 200.0), Vec2::new(1000.0, 1300.0)), (Vec2::new(400.0, 300.0), Vec2::new(1600.0, 1200.0)), (Vec2::new(1600.0, 1300.0), Vec2::new(400.0, 300.0))];
            for (throw, (from, to)) in throws.into_iter().enumerate() {
                thrower.dip(by_name("cadmium_red").unwrap());
                thrower.splatter(from, to);
                let label = format!("medium {medium} throw {}", throw + 1);
                if realtime {
                    realtime_frames(&label, &mut thrower, 3.0);
                } else {
                    frames(&label, &mut thrower, 3.0);
                }
            }
        }
    }
}
