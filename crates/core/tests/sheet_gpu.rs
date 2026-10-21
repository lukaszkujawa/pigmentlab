use pigmentlab_core::canvas::Surface;
use pigmentlab_core::gpu::Gpu;
use pigmentlab_core::{CanvasSize, Engine};

const SIZE: CanvasSize = CanvasSize { width: 384, height: 256 };
const RAW_UMBER: [u8; 3] = [107, 90, 69];

fn mean_rgb(image: &image::RgbImage) -> [f32; 3] {
    let total = image.pixels().fold([0.0f32; 3], |sum, pixel| [0, 1, 2].map(|channel| sum[channel] + f32::from(pixel[channel])));
    total.map(|channel| channel / (image.width() * image.height()) as f32)
}

fn assert_close(actual: [f32; 3], expected: [u8; 3], tolerance: f32) {
    for (channel, target) in actual.iter().zip(expected) {
        assert!((channel - f32::from(target)).abs() < tolerance, "{actual:?} vs {expected:?}");
    }
}

#[test]
fn a_new_sheet_takes_its_size_and_ground_and_exports_as_png_and_jpeg() {
    let mut engine = Engine::new(Gpu::headless(), SIZE, 1);
    engine.set_sheet(Surface::HotPressed, Some(RAW_UMBER));
    assert_eq!(engine.canvas_size(), SIZE);
    assert_eq!(engine.surface(), Surface::HotPressed);

    let directory = std::env::temp_dir().join(format!("pigmentlab-sheet-{}", std::process::id()));
    std::fs::create_dir_all(&directory).unwrap();
    for name in ["sheet.png", "sheet.jpg"] {
        let path = directory.join(name);
        engine.save_image(&path).unwrap();
        let saved = image::open(&path).unwrap().to_rgb8();
        assert_eq!((saved.width(), saved.height()), (SIZE.width, SIZE.height), "{name}");
        assert_close(mean_rgb(&saved), RAW_UMBER, 12.0);
    }
    std::fs::remove_dir_all(&directory).unwrap();
}

#[test]
fn a_chosen_ground_survives_surface_changes_until_reset_to_natural() {
    let mut engine = Engine::new(Gpu::headless(), SIZE, 1);
    engine.set_sheet(Surface::Linen, Some(RAW_UMBER));
    engine.set_surface(Surface::ColdPressed);
    engine.render();
    assert_close(mean_rgb(&image::DynamicImage::ImageRgba8(engine.read_back()).to_rgb8()), RAW_UMBER, 12.0);

    engine.set_sheet(Surface::ColdPressed, None);
    engine.render();
    let natural = mean_rgb(&image::DynamicImage::ImageRgba8(engine.read_back()).to_rgb8());
    assert!(natural.iter().all(|channel| *channel > 200.0), "{natural:?}");
}

fn squeeze(engine: &mut Engine, x: f32, y: f32, start: f64) -> f64 {
    use pigmentlab_core::{PointerPhase, PointerSample};
    let at = |phase, time| PointerSample { phase, x, y, pressure: 0.7, tilt: [0.0; 2], rotation: 0.0, time };
    engine.pointer(at(PointerPhase::Down, start));
    engine.pointer(at(PointerPhase::Move, start + 0.6));
    engine.pointer(at(PointerPhase::Up, start + 0.6));
    engine.advance_exact(start + 0.8);
    start + 1.0
}

#[test]
fn the_palette_is_smooth_keeps_its_paint_through_dry_and_clear_and_only_cleaning_scrapes_it() {
    let mut engine = Engine::with_palette(Gpu::headless(), SIZE, 1);
    let width = engine.canvas_size().width as usize;
    assert_eq!(engine.painting_size(), SIZE);
    assert!(width > SIZE.width as usize);
    let relief = engine.canvas_relief();
    assert!((0..SIZE.height as usize).all(|y| (SIZE.width as usize..width).all(|x| relief[y * width + x] == 0.0)));
    assert!(relief[..SIZE.width as usize].iter().any(|height| *height != 0.0));

    engine.select_brush("Paint tube");
    engine.dip(pigmentlab_core::pigment::by_name("cadmium_red").unwrap());
    let painting_pixel = 128 * width + 100;
    let palette_pixel = 128 * width + SIZE.width as usize + 100;
    let time = squeeze(&mut engine, 100.0, 128.0, 0.0);
    squeeze(&mut engine, SIZE.width as f32 + 100.0, 128.0, time);
    let wet_on_palette = engine.paint_heights()[palette_pixel];
    assert!(wet_on_palette > 1.0 && engine.paint_heights()[painting_pixel] > 1.0);

    engine.dry();
    let heights = engine.paint_heights();
    assert_eq!(heights[painting_pixel], 0.0);
    assert!((heights[palette_pixel] - wet_on_palette).abs() < 1e-3);
    assert!(engine.dry_heights()[painting_pixel] > 0.0 && engine.dry_heights()[palette_pixel] == 0.0);

    engine.clear();
    assert!((engine.paint_heights()[palette_pixel] - wet_on_palette).abs() < 1e-3);
    engine.clean_palette();
    assert_eq!(engine.paint_mass(), 0.0);
}

#[test]
fn a_saved_image_holds_the_painting_without_the_palette() {
    let mut engine = Engine::with_palette(Gpu::headless(), SIZE, 1);
    let path = std::env::temp_dir().join(format!("pigmentlab-palette-{}.png", std::process::id()));
    engine.save_image(&path).unwrap();
    let saved = image::open(&path).unwrap().to_rgb8();
    std::fs::remove_file(&path).unwrap();
    assert_eq!((saved.width(), saved.height()), (SIZE.width, SIZE.height));
}
