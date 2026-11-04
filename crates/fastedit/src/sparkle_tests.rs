//! Where Sparkle is looked for, and when it runs. Loading it takes a signed
//! bundle, so the end-to-end card checks that.

#[cfg(target_os = "macos")]
use std::{
    fs,
    path::{Path, PathBuf},
};

use crate::sparkle::Policy;
#[cfg(target_os = "macos")]
use crate::sparkle::framework_path;

/// [`Policy::decide`] for a bundled release launched plainly, the setting on.
fn plain() -> Policy {
    Policy::decide(true, true, false, false, false, true)
}

/// A `tenmoku` at `exe` under a new folder, with `Sparkle.framework` at
/// `framework` there if it's given. Returns the folder and the exe's path.
#[cfg(target_os = "macos")]
fn laid_out(exe: &str, framework: Option<&str>) -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let exe = dir.path().join(exe);
    fs::create_dir_all(exe.parent().unwrap()).unwrap();
    fs::write(&exe, "").unwrap();
    if let Some(framework) = framework {
        fs::create_dir_all(dir.path().join(framework)).unwrap();
    }
    (dir, exe)
}

#[cfg(target_os = "macos")]
#[test]
fn framework_path_inside_a_bundle() {
    let framework = "Tenmoku.app/Contents/Frameworks/Sparkle.framework";
    let (dir, exe) = laid_out("Tenmoku.app/Contents/MacOS/tenmoku", Some(framework));
    assert_eq!(framework_path(&exe), Some(dir.path().join(framework)));
}

#[cfg(target_os = "macos")]
#[test]
fn framework_path_none_outside_a_bundle() {
    // A bundle without the framework.
    let (_dir, exe) = laid_out("Tenmoku.app/Contents/MacOS/tenmoku", None);
    assert_eq!(framework_path(&exe), None);
    // `cargo run`, and paths only shaped like a bundle's.
    for (exe, framework) in [
        (
            "target/debug/tenmoku",
            "target/Frameworks/Sparkle.framework",
        ),
        (
            "Tenmoku/Contents/MacOS/tenmoku",
            "Tenmoku/Contents/Frameworks/Sparkle.framework",
        ),
        (
            "Tenmoku.app/MacOS/tenmoku",
            "Tenmoku.app/Frameworks/Sparkle.framework",
        ),
    ] {
        let (_dir, exe) = laid_out(exe, Some(framework));
        assert_eq!(framework_path(&exe), None, "{}", exe.display());
    }
    assert_eq!(framework_path(Path::new("tenmoku")), None);
}

#[test]
fn policy_off_for_perf_wait_test_and_missing_framework() {
    assert_ne!(plain(), Policy::Off);
    let decide = Policy::decide;
    for automatic_checks in [true, false] {
        assert_eq!(
            decide(false, true, false, false, false, automatic_checks),
            Policy::Off
        );
        assert_eq!(
            decide(true, true, true, false, false, automatic_checks),
            Policy::Off
        );
        assert_eq!(
            decide(true, true, false, true, false, automatic_checks),
            Policy::Off
        );
        assert_eq!(
            decide(true, true, false, false, true, automatic_checks),
            Policy::Off
        );
    }
}

#[test]
fn policy_manual_when_setting_off() {
    assert_eq!(
        Policy::decide(true, true, false, false, false, false),
        Policy::Manual
    );
}

#[test]
fn policy_scheduled_by_default() {
    assert_eq!(plain(), Policy::Scheduled);
    assert!(fe_config::Settings::default().update.automatic_checks);
}

#[test]
fn policy_off_without_feed_keys() {
    for automatic_checks in [true, false] {
        assert_eq!(
            Policy::decide(true, false, false, false, false, automatic_checks),
            Policy::Off
        );
    }
}
