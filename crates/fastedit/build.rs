//! On Windows, embeds the app icon in the executable.

use std::env;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");

    windows_icon();
}

/// Embeds the app icon in the Windows executable (`assets/windows`), for
/// Explorer, the taskbar and the windows' title bars. Nothing elsewhere: the
/// macOS bundle gets its icon from `scripts/bundle-macos.sh`.
fn windows_icon() {
    println!("cargo:rerun-if-changed=assets/windows/tenmoku.rc");
    println!("cargo:rerun-if-changed=assets/windows/tenmoku.ico");
    if env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        embed_resource::compile("assets/windows/tenmoku.rc", embed_resource::NONE)
            .manifest_optional()
            .expect("the app icon's resources compile");
    }
}
