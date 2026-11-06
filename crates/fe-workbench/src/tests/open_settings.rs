//! Open Settings (JSON), `workbench.open_settings`: the user's
//! `settings.toml`, created from the template if missing, in a tab of the
//! active pane. (cmd-comma opens the Settings editor:
//! `crate::settings_editor_tests`.) The config directory is
//! a temporary one, through the test-only override.

use std::{fs, path::PathBuf, sync::Mutex};

use fe_command::{ContextFrame, ContextStack, Invocation};
use fe_config::USER_SETTINGS_TEMPLATE;
use fe_gpui::commands::RunCommand;
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

/// Runs `workbench.open_settings`, as the palette does.
fn run_open_settings(cx: &mut VisualTestContext) {
    let run = RunCommand(
        Invocation::of("workbench.open_settings")
            .unwrap()
            .context(ContextStack::new().with(ContextFrame::new("workbench"))),
    );
    cx.update(|window, cx| window.dispatch_action(Box::new(run), cx));
}

fn open_settings_and_wait(cx: &mut VisualTestContext) {
    run_open_settings(cx);
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
    let dir = config.path().join("tenmoku");
    let file = dir.join("settings.toml");
    let (workbench, cx) = workbench_with(cx, project.path(), Some(dir));

    open_settings_and_wait(cx);
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
    open_settings_and_wait(cx);
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

    open_settings_and_wait(cx);
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

    open_settings_and_wait(cx);
    assert_eq!(tabs(&workbench, cx), before);
    let errors = ERRORS.lock().unwrap();
    assert!(
        errors.iter().any(|e| e.contains("no config directory")),
        "an error is logged: {errors:?}"
    );
}

/// Serializes the tests that set the process-wide hook.
static HOOKED: Mutex<()> = Mutex::new(());

fn hook_path(path: &std::path::Path, hook: crate::open::Hook) {
    *crate::open::HOOK.lock().unwrap() = Some((path.to_owned(), hook));
}

/// A request that overlaps an initialization waits for it, and what the
/// editor then shows is the whole template. (The first request runs on a
/// thread of its own, paused after creating its file; the second is the
/// Settings command, whose wait releases the first, so nothing depends on
/// timing.)
#[gpui::test]
fn overlapping_requests_wait_for_the_write(cx: &mut TestAppContext) {
    use crate::open::{Stage, create_settings};
    let _hooked = HOOKED.lock().unwrap_or_else(|e| e.into_inner());
    let project = tempfile::tempdir().unwrap();
    let config = tempfile::tempdir().unwrap();
    let file = config.path().join("settings.toml");
    let (workbench, cx) = workbench_with(cx, project.path(), Some(config.path().to_owned()));

    let (paused_tx, paused_rx) = std::sync::mpsc::channel();
    let (waiting_tx, waiting_rx) = std::sync::mpsc::channel();
    let (release_tx, release_rx) = std::sync::mpsc::channel::<()>();
    // Wakes the releaser if the command never reaches `Stage::Wait`.
    let wake = waiting_tx.clone();
    let bound = std::time::Duration::from_secs(10);
    let (paused_tx, waiting_tx, release_rx) = (
        Mutex::new(paused_tx),
        Mutex::new(waiting_tx),
        Mutex::new(release_rx),
    );
    hook_path(
        &file,
        std::sync::Arc::new(move |stage| {
            match stage {
                Stage::Write => {
                    paused_tx.lock().unwrap().send(()).unwrap();
                    let _ = release_rx.lock().unwrap().recv_timeout(bound);
                }
                Stage::Wait => waiting_tx.lock().unwrap().send(()).unwrap(),
                Stage::Cleanup => {}
            }
            Ok(())
        }),
    );
    let first = std::thread::spawn({
        let file = file.clone();
        move || create_settings(&file)
    });
    paused_rx.recv_timeout(bound).unwrap();
    // Releases the first once the command's request is waiting on it, or
    // when woken or timed out, so neither thread can outlive the test.
    let releaser = std::thread::spawn(move || {
        let _ = waiting_rx.recv_timeout(bound);
        let _ = release_tx.send(());
    });

    run_open_settings(cx);
    // The command's request returns once the first is released; wait, with
    // a bound, for its tab.
    let mut opened = false;
    for _ in 0..500 {
        cx.run_until_parked();
        if tabs(&workbench, cx).1.as_ref() == Some(&file) {
            opened = true;
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    if !opened {
        let _ = wake.send(());
    }
    releaser.join().unwrap();
    first.join().unwrap().unwrap();
    crate::open::HOOK.lock().unwrap().take();
    assert!(opened, "the Settings command never opened the file");

    let editor = workbench.read_with(cx, |workbench, cx| {
        workbench.pane().read(cx).active_editor().cloned().unwrap()
    });
    let text = editor.read_with(cx, |view, _| view.model().text().to_string());
    assert_eq!(text, USER_SETTINGS_TEMPLATE);
    assert_eq!(tabs(&workbench, cx), (vec![file.clone()], Some(file)));
}

/// Two requests overlapping a failing write both get its error, and
/// neither opens anything; the file isn't left behind, and a later
/// request, finding the failure over, succeeds. A failing cleanup adds to
/// the error without leaving the file in place.
#[test]
fn a_failed_write_is_every_waiters_error() {
    use crate::open::{Stage, create_settings};
    let _hooked = HOOKED.lock().unwrap_or_else(|e| e.into_inner());
    for cleanup_fails in [false, true] {
        let config = tempfile::tempdir().unwrap();
        let file = config.path().join("settings.toml");
        let (paused_tx, paused_rx) = std::sync::mpsc::channel();
        let (waiting_tx, waiting_rx) = std::sync::mpsc::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel::<()>();
        let (paused_tx, waiting_tx, release_rx) = (
            Mutex::new(paused_tx),
            Mutex::new(waiting_tx),
            Mutex::new(release_rx),
        );
        hook_path(
            &file,
            std::sync::Arc::new(move |stage| match stage {
                Stage::Write => {
                    paused_tx.lock().unwrap().send(()).unwrap();
                    release_rx.lock().unwrap().recv().unwrap();
                    Err(std::io::Error::other("disk full"))
                }
                Stage::Wait => {
                    waiting_tx.lock().unwrap().send(()).unwrap();
                    Ok(())
                }
                Stage::Cleanup if cleanup_fails => Err(std::io::Error::other("busy")),
                Stage::Cleanup => Ok(()),
            }),
        );
        let first = std::thread::spawn({
            let file = file.clone();
            move || create_settings(&file)
        });
        paused_rx.recv().unwrap();
        let waiters: Vec<_> = (0..2)
            .map(|_| {
                let file = file.clone();
                std::thread::spawn(move || create_settings(&file))
            })
            .collect();
        for _ in &waiters {
            waiting_rx.recv().unwrap();
        }
        release_tx.send(()).unwrap();

        let errors: Vec<_> = std::iter::once(first)
            .chain(waiters)
            .map(|t| t.join().unwrap().unwrap_err().to_string())
            .collect();
        assert!(errors.iter().all(|e| e.contains("disk full")), "{errors:?}");
        assert_eq!(errors.iter().any(|e| e.contains("busy")), cleanup_fails);
        assert!(!file.exists(), "no incomplete file at the path");

        crate::open::HOOK.lock().unwrap().take();
        create_settings(&file).unwrap();
        assert_eq!(fs::read_to_string(&file).unwrap(), USER_SETTINGS_TEMPLATE);
    }
}

/// Temporary files already there, a symlink among them, aren't written
/// through or removed, and a leftover link to the finished file (as after
/// a failed cleanup) doesn't let a later initialization truncate it.
#[test]
#[cfg(unix)]
fn temporary_files_are_exclusive() {
    use crate::open::{Stage, create_settings};
    let _hooked = HOOKED.lock().unwrap_or_else(|e| e.into_inner());
    let config = tempfile::tempdir().unwrap();
    let file = config.path().join("settings.toml");
    let victim = config.path().join("victim");
    fs::write(&victim, "keep\n").unwrap();
    for n in 0..8 {
        let temp = config
            .path()
            .join(format!("settings.toml.{}-{n}.tmp", std::process::id()));
        std::os::unix::fs::symlink(&victim, &temp).unwrap();
    }
    create_settings(&file).unwrap();
    assert_eq!(fs::read_to_string(&file).unwrap(), USER_SETTINGS_TEMPLATE);
    assert_eq!(fs::read_to_string(&victim).unwrap(), "keep\n");

    // Published, then cleanup fails: still a success. Back up the file and
    // initialize again: the backup, hard-linked to the leftover, is intact.
    fs::remove_file(&file).unwrap();
    hook_path(
        &file,
        std::sync::Arc::new(|stage| match stage {
            Stage::Cleanup => Err(std::io::Error::other("busy")),
            _ => Ok(()),
        }),
    );
    create_settings(&file).unwrap();
    crate::open::HOOK.lock().unwrap().take();
    let backup = config.path().join("settings.toml.bak");
    fs::write(&backup, "").unwrap();
    fs::rename(&file, &backup).unwrap();
    fs::write(&backup, "# mine\n").unwrap();
    create_settings(&file).unwrap();
    assert_eq!(fs::read_to_string(&backup).unwrap(), "# mine\n");
}
