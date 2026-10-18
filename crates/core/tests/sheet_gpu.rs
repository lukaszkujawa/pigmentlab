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
