use std::future::Future;
use std::path::{Path, PathBuf};

const IMAGE_EXTENSIONS: [&str; 3] = ["png", "jpg", "jpeg"];
pub const UNTITLED: &str = "Untitled";

pub fn choose_save_path(window: &slint::Window, current: Option<&Path>) -> impl Future<Output = Option<PathBuf>> + use<> {
    let mut dialog = rfd::AsyncFileDialog::new()
        .set_title("Save Painting")
        .add_filter("PNG image", &["png"])
        .add_filter("JPEG image", &["jpg", "jpeg"])
        .set_file_name(current.and_then(Path::file_name).map_or(format!("{UNTITLED}.png"), |name| name.to_string_lossy().into_owned()))
        .set_parent(&window.window_handle());
    if let Some(directory) = current.and_then(Path::parent) {
        dialog = dialog.set_directory(directory);
    }
    async move { dialog.save_file().await.map(|file| with_image_extension(file.path().to_path_buf())) }
}

pub fn with_image_extension(path: PathBuf) -> PathBuf {
    let extension = path.extension().and_then(|extension| extension.to_str()).map(str::to_ascii_lowercase);
    if extension.is_some_and(|extension| IMAGE_EXTENSIONS.contains(&extension.as_str())) {
        return path;
    }
    let mut name = path.into_os_string();
    name.push(".png");
    name.into()
}

pub fn display_name(path: &Path) -> String {
    path.file_name().map_or(UNTITLED.into(), |name| name.to_string_lossy().into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn image_paths_keep_a_known_extension_and_gain_png_otherwise() {
        assert_eq!(with_image_extension("a/sunset.JPG".into()), PathBuf::from("a/sunset.JPG"));
        assert_eq!(with_image_extension("a/sunset.png".into()), PathBuf::from("a/sunset.png"));
        assert_eq!(with_image_extension("a/sunset".into()), PathBuf::from("a/sunset.png"));
        assert_eq!(with_image_extension("a/sunset.v2".into()), PathBuf::from("a/sunset.v2.png"));
    }

    #[test]
    fn display_names_are_the_file_name() {
        assert_eq!(display_name(Path::new("/tmp/sunset.jpg")), "sunset.jpg");
    }
}
