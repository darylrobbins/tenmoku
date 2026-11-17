use std::path::{Path, PathBuf};

use crate::app_icon::should_set_icon;

/// A scratch `<root>/<app>/Contents/{MacOS,Resources}` tree; returns the exe path.
fn layout(name: &str, app: &str, plist: Option<&str>, icns: bool) -> PathBuf {
    let root = std::env::temp_dir().join(format!("tenmoku-app-icon-{name}-{}", std::process::id()));
    let contents = root.join(app).join("Contents");
    std::fs::create_dir_all(contents.join("MacOS")).unwrap();
    std::fs::create_dir_all(contents.join("Resources")).unwrap();
    if let Some(plist) = plist {
        std::fs::write(contents.join("Info.plist"), plist).unwrap();
    }
    if icns {
        std::fs::write(contents.join("Resources/Tenmoku.icns"), b"icns").unwrap();
    }
    contents.join("MacOS/tenmoku")
}

const PLIST: &str =
    "<plist><dict><key>CFBundleIconFile</key><string>Tenmoku</string></dict></plist>";

#[test]
fn a_complete_bundle_keeps_its_icns() {
    let exe = layout("full", "Tenmoku.app", Some(PLIST), true);
    assert!(!should_set_icon(&exe));
}

#[test]
fn a_bundle_without_an_icon_resource_gets_the_icon() {
    let no_icns = layout("noicns", "Tenmoku.app", Some(PLIST), false);
    assert!(should_set_icon(&no_icns));
    let no_key = layout("nokey", "Tenmoku.app", Some("<plist/>"), true);
    assert!(should_set_icon(&no_key));
    let no_plist = layout("noplist", "Tenmoku.app", None, true);
    assert!(should_set_icon(&no_plist));
}

#[test]
fn an_unrelated_app_directory_gets_the_icon() {
    assert!(should_set_icon(Path::new(
        "/code/project.app/target/debug/tenmoku"
    )));
    let not_app = layout("notapp", "Tenmoku", Some(PLIST), true);
    assert!(should_set_icon(&not_app));
    assert!(should_set_icon(Path::new("/usr/local/bin/tenmoku")));
}
