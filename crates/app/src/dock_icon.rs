#[cfg(target_os = "macos")]
pub fn install() {
    use objc2::{AllocAnyThread, MainThreadMarker};
    use objc2_app_kit::{NSApplication, NSImage};
    use objc2_foundation::NSData;

    let Some(main_thread) = MainThreadMarker::new() else { return };
    let data = NSData::with_bytes(include_bytes!("../ui/icons/icon.png"));
    if let Some(image) = NSImage::initWithData(NSImage::alloc(), &data) {
        unsafe { NSApplication::sharedApplication(main_thread).setApplicationIconImage(Some(&image)) };
    }
}

#[cfg(not(target_os = "macos"))]
pub fn install() {}
