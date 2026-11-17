//! The application icon. Run unbundled (`cargo run`, an installed binary),
//! macOS shows the launching terminal's icon in the Dock and Cmd-Tab; setting
//! it at startup replaces that. Source: `assets/icon/fastedit.svg`.

use kiln_l10n::verbatim;

/// The 1024px render of `assets/icon/fastedit.svg`.
#[cfg(target_os = "macos")]
const ICON_PNG: &[u8] = include_bytes!("../assets/icon/fastedit-1024.png");

/// Whether the Dock icon needs setting for the executable at `exe`. Inside a
/// real `.app` bundle (`X.app/Contents/MacOS/exe`, an `Info.plist` naming
/// `CFBundleIconFile`, and `Resources/Tenmoku.icns`) the Dock already shows
/// the bundle's icon, and `setApplicationIconImage` there only makes AppKit
/// render a 32 MB CG Image bitmap (T1907). Anything else, an unrelated `.app`
/// ancestor included, gets the icon set.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub(crate) fn should_set_icon(exe: &std::path::Path) -> bool {
    let contents = exe
        .parent()
        .filter(|macos| macos.file_name().is_some_and(|n| n == "MacOS"))
        .and_then(|macos| macos.parent())
        .filter(|c| c.file_name().is_some_and(|n| n == "Contents"));
    let Some(contents) = contents else {
        return true;
    };
    let is_app = contents
        .parent()
        .and_then(|app| app.extension())
        .is_some_and(|ext| ext == "app");
    let names_icon = std::fs::read_to_string(contents.join(verbatim!("Info.plist")))
        .is_ok_and(|plist| plist.contains("CFBundleIconFile"));
    let bundle_icon = contents
        .join(verbatim!("Resources"))
        .join(verbatim!("Tenmoku.icns"))
        .is_file();
    !(is_app && names_icon && bundle_icon)
}

/// Sets the Dock icon of an unbundled run. Call on the main thread once the
/// app is running.
#[cfg(target_os = "macos")]
pub(crate) fn set() {
    if std::env::current_exe().is_ok_and(|exe| !should_set_icon(&exe)) {
        return;
    }
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
