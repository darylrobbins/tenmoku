//! Settings… (cmd-comma): the user's `settings.toml`, created from the
//! template if missing, in a tab of the active pane. The config directory is
//! a temporary one, through the test-only override.

use std::{fs, path::PathBuf, sync::Mutex};

use fe_config::USER_SETTINGS_TEMPLATE;
use gpui::{Entity, Focusable as _, TestAppContext, VisualTestContext};

use super::open_root;
use crate::{Workbench, open::ConfigDirOverride};

/// A workbench whose config directory is `config`.
fn workbench_with<'a>(
    cx: &'a mut TestAppContext,
    project: &std::path::Path,
    config: Option<PathBuf>,
) -> (Entity<Workbench>, &'a mut VisualTestContext) {
    let (workbench, cx) = open_root(cx, project);
    cx.update(|window, cx| {
        cx.set_global(ConfigDirOverride(config));
        window.focus(&workbench.focus_handle(cx), cx);
    });
    cx.run_until_parked();
    (workbench, cx)
}

/// The open tabs' paths, and the active tab's.
fn tabs(
    workbench: &Entity<Workbench>,
    cx: &mut VisualTestContext,
) -> (Vec<PathBuf>, Option<PathBuf>) {
    workbench.read_with(cx, |workbench, cx| {
        let pane = workbench.pane().read(cx);
        let paths: Vec<_> = pane.tabs().iter().filter_map(|t| t.path.clone()).collect();
        let active = pane.active().and_then(|ix| paths.get(ix).cloned());
        (paths, active)
    })
}

/// Error-level log lines, from every test in the process.
static ERRORS: Mutex<Vec<String>> = Mutex::new(Vec::new());

struct Capture;

impl log::Log for Capture {
    fn enabled(&self, _: &log::Metadata) -> bool {
        true
    }

    fn log(&self, record: &log::Record) {
        if record.level() == log::Level::Error {
            ERRORS.lock().unwrap().push(record.args().to_string());
        }
    }

    fn flush(&self) {}
}

fn capture_errors() {
    if log::set_logger(&Capture).is_ok() {
        log::set_max_level(log::LevelFilter::Error);
    }
}

fn press_settings(cx: &mut VisualTestContext) {
    cx.simulate_keystrokes("cmd-,");
    // The file is written on a background thread.
    for _ in 0..50 {
        cx.run_until_parked();
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    cx.run_until_parked();
}

#[gpui::test]
fn creates_and_opens_the_file(cx: &mut TestAppContext) {
    let project = tempfile::tempdir().unwrap();
    let config = tempfile::tempdir().unwrap();
    let dir = config.path().join("fastedit");
    let file = dir.join("settings.toml");
    let (workbench, cx) = workbench_with(cx, project.path(), Some(dir));

    press_settings(cx);
    assert_eq!(fs::read_to_string(&file).unwrap(), USER_SETTINGS_TEMPLATE);
    assert_eq!(
        tabs(&workbench, cx),
        (vec![file.clone()], Some(file.clone()))
    );

    // Again, with another tab active: settings' tab activates, no second
    // tab opens, and the file isn't written again.
    fs::write(&file, "# mine\n").unwrap();
    let other = project.path().join("other.txt");
    fs::write(&other, "other\n").unwrap();
    workbench.update_in(cx, |workbench, window, cx| {
        workbench
            .pane()
            .update(cx, |pane, cx| pane.open(other.clone(), window, cx))
    });
    cx.run_until_parked();
    assert_eq!(tabs(&workbench, cx).1, Some(other.clone()));
    press_settings(cx);
    assert_eq!(fs::read_to_string(&file).unwrap(), "# mine\n");
    assert_eq!(
        tabs(&workbench, cx),
        (vec![file.clone(), other], Some(file))
    );
}

#[gpui::test]
fn an_existing_file_is_never_overwritten(cx: &mut TestAppContext) {
    let project = tempfile::tempdir().unwrap();
    let config = tempfile::tempdir().unwrap();
    let file = config.path().join("settings.toml");
    fs::write(&file, "[editor]\nfont_size = 16\n").unwrap();
    let (workbench, cx) = workbench_with(cx, project.path(), Some(config.path().to_owned()));

    press_settings(cx);
    assert_eq!(
        fs::read_to_string(&file).unwrap(),
        "[editor]\nfont_size = 16\n"
    );
    assert_eq!(tabs(&workbench, cx), (vec![file.clone()], Some(file)));
}

#[gpui::test]
fn no_config_directory_opens_nothing(cx: &mut TestAppContext) {
    let project = tempfile::tempdir().unwrap();
    capture_errors();
    let existing = project.path().join("existing.txt");
    fs::write(&existing, "x\n").unwrap();
    let (workbench, cx) = workbench_with(cx, project.path(), None);
    workbench.update_in(cx, |workbench, window, cx| {
        workbench
            .pane()
            .update(cx, |pane, cx| pane.open(existing.clone(), window, cx))
    });
    cx.run_until_parked();
    let before = tabs(&workbench, cx);
    assert_eq!(before, (vec![existing.clone()], Some(existing)));
    ERRORS.lock().unwrap().clear();

    press_settings(cx);
    assert_eq!(tabs(&workbench, cx), before);
    let errors = ERRORS.lock().unwrap();
    assert!(
        errors.iter().any(|e| e.contains("no config directory")),
        "an error is logged: {errors:?}"
    );
}

/// A second request that finds the file created but not yet written waits
/// for the write, so what it then opens is complete. (Threads, not the
/// test executor: parking it would wait on the paused write.)
#[test]
fn overlapping_requests_wait_for_the_write() {
    use crate::open::{AFTER_CREATE, create_settings};
    let config = tempfile::tempdir().unwrap();
    let file = config.path().join("settings.toml");

    let (created_tx, created_rx) = std::sync::mpsc::channel();
    let (release_tx, release_rx) = std::sync::mpsc::channel::<()>();
    let (created_tx, release_rx) = (Mutex::new(created_tx), Mutex::new(release_rx));
    *AFTER_CREATE.lock().unwrap() = Some((
        file.clone(),
        std::sync::Arc::new(move || {
            created_tx.lock().unwrap().send(()).unwrap();
            release_rx.lock().unwrap().recv().unwrap();
        }),
    ));
    let first = std::thread::spawn({
        let file = file.clone();
        move || create_settings(&file)
    });
    created_rx.recv().unwrap();
    assert_eq!(fs::read_to_string(&file).unwrap(), "");

    let (read_tx, read_rx) = std::sync::mpsc::channel();
    let second = std::thread::spawn({
        let file = file.clone();
        move || {
            create_settings(&file).unwrap();
            read_tx.send(fs::read_to_string(&file).unwrap()).unwrap();
        }
    });
    assert!(
        read_rx
            .recv_timeout(std::time::Duration::from_millis(200))
            .is_err(),
        "the second request returned before the write finished"
    );
    AFTER_CREATE.lock().unwrap().take();
    release_tx.send(()).unwrap();
    first.join().unwrap().unwrap();
    second.join().unwrap();
    assert_eq!(read_rx.recv().unwrap(), USER_SETTINGS_TEMPLATE);
}
