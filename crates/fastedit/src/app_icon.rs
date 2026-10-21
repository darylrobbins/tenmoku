//! The application icon. Run unbundled (`cargo run`, an installed binary),
//! macOS shows the launching terminal's icon in the Dock and Cmd-Tab; setting
//! it at startup replaces that. Source: `assets/icon/fastedit.svg`.

/// The 1024px render of `assets/icon/fastedit.svg`.
#[cfg(target_os = "macos")]
const ICON_PNG: &[u8] = include_bytes!("../assets/icon/fastedit-1024.png");

/// Sets the Dock icon. Call on the main thread once the app is running.
#[cfg(target_os = "macos")]
pub(crate) fn set() {
    use objc2::{ClassType, rc::Retained};
    use objc2_app_kit::{NSApplication, NSImage};
    use objc2_foundation::{MainThreadMarker, NSData};

    let Some(mtm) = MainThreadMarker::new() else {
        log::warn!("app icon: not on the main thread");
        return;
    };
    let data = NSData::with_bytes(ICON_PNG);
    let image: Option<Retained<NSImage>> = NSImage::initWithData(NSImage::alloc(), &data);
    let Some(image) = image else {
        log::warn!("app icon: the PNG did not decode");
        return;
    };
    // SAFETY: on the main thread, with a valid image.
    unsafe { NSApplication::sharedApplication(mtm).setApplicationIconImage(Some(&image)) };
}

#[cfg(not(target_os = "macos"))]
pub(crate) fn set() {}
