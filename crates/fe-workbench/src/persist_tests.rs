//! Toggle Vim Mode and Toggle Status Bar saved to the user's
//! `settings.toml`, and hand edits to it applied live. Each test's settings
//! file is in a temporary directory.

use std::{
    fs,
    path::{Path, PathBuf},
    sync::Mutex,
};

use fe_command::{ContextFrame, ContextStack, Invocation};
use fe_config::{Settings, SettingsStore};
use fe_gpui::{EditorSettings, EditorTheme, commands::RunCommand, menu::MenuCommand, vim_enabled};
use fe_keymap::{ItemKind, MenuBar};
use gpui::{Entity, Focusable as _, TestAppContext, VisualTestContext};

use super::{Saver, VIM, save_toggles_to};
use crate::{Launch, Workbench, open::ConfigDirOverride};

/// A workbench on the empty project `project`, focused, run until parked.
fn workbench<'a>(
    cx: &'a mut TestAppContext,
    project: &Path,
) -> (Entity<Workbench>, &'a mut VisualTestContext) {
    let project = project.to_owned();
    cx.update(|cx| {
        fe_gpui::init(cx);
        crate::init(cx);
    });
    let launch = Launch {
        root: Some(project),
        file: None,
    };
    let (workbench, cx) = cx.add_window_view(move |window, cx| {
        let theme = EditorTheme::default_dark(cx);
        Workbench::new(launch, theme, EditorSettings::default(), window, cx)
    });
    cx.update(|window, cx| window.focus(&workbench.focus_handle(cx), cx));
    cx.run_until_parked();
    (workbench, cx)
}

/// Runs command `id` in the workbench's context, as an unbound key would,
/// without running anything that waits.
fn dispatch(id: &str, cx: &mut VisualTestContext) {
    let run = RunCommand(
        Invocation::of(id)
            .unwrap()
            .context(ContextStack::new().with(ContextFrame::new("workbench"))),
    );
    cx.update(|window, cx| window.dispatch_action(Box::new(run), cx));
}

fn visible(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> bool {
    workbench.read_with(cx, |workbench, _| workbench.status_bar_visible())
}

/// The settings a new store loads from the user file at `path`, which
/// load without diagnostics.
fn saved(path: &Path) -> Settings {
    let store = SettingsStore::load(Some(path.to_owned()), None);
    assert_eq!(
        store.diagnostics(),
        [],
        "{}",
        fs::read_to_string(path).unwrap()
    );
    (**store.settings()).clone()
}

/// A temporary directory, and the `settings.toml` path in it.
fn settings_file() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("settings.toml");
    (dir, path)
}

#[gpui::test]
fn status_bar_toggle_persists(cx: &mut TestAppContext, unsaved: &mut TestAppContext) {
    let (dir, path) = settings_file();
    let (workbench, cx) = workbench(cx, dir.path());
    cx.update(|_, cx| save_toggles_to(cx, path.clone()));

    dispatch("view.toggle_status_bar", cx);
    assert!(!visible(&workbench, cx), "hidden at once");
    cx.run_until_parked();
    assert!(!saved(&path).status_bar.visible);

    dispatch("view.toggle_status_bar", cx);
    assert!(visible(&workbench, cx));
    cx.run_until_parked();
    assert!(saved(&path).status_bar.visible);

    for _ in 0..5 {
        dispatch("view.toggle_status_bar", cx);
    }
    assert!(!visible(&workbench, cx));
    cx.run_until_parked();
    assert!(!saved(&path).status_bar.visible);

    // The setter is the host's, and writes nothing.
    let before = fs::read(&path).unwrap();
    workbench.update(cx, |workbench, cx| {
        workbench.set_status_bar_visible(true, cx)
    });
    cx.run_until_parked();
    assert!(visible(&workbench, cx));
    assert_eq!(fs::read(&path).unwrap(), before);

    // Never asked to save: toggles write nothing to the config directory.
    let config = tempfile::tempdir().unwrap();
    let (workbench, cx) = self::workbench(unsaved, dir.path());
    cx.update(|_, cx| cx.set_global(ConfigDirOverride(Some(config.path().to_owned()))));
    dispatch("view.toggle_status_bar", cx);
    dispatch("vim.toggle", cx);
    cx.run_until_parked();
    assert!(!visible(&workbench, cx));
    assert!(cx.update(|_, cx| vim_enabled(cx)));
    assert_eq!(fs::read_dir(config.path()).unwrap().count(), 0);
}

#[gpui::test]
fn vim_toggle_persists(cx: &mut TestAppContext) {
    let (dir, path) = settings_file();
    fs::write(&path, "# Mine.\n[editor]\ntab_size = 2\n").unwrap();
    let (_workbench, cx) = workbench(cx, dir.path());
    cx.update(|_, cx| save_toggles_to(cx, path.clone()));
    let on = |cx: &mut VisualTestContext| cx.update(|_, cx| vim_enabled(cx));

    dispatch("vim.toggle", cx);
    assert!(on(cx));
    cx.run_until_parked();
    let settings = saved(&path);
    assert!(settings.vim.enabled);
    assert_eq!(settings.editor.tab_size, 2);
    assert!(fs::read_to_string(&path).unwrap().contains("# Mine.\n"));

    dispatch("vim.toggle", cx);
    assert!(!on(cx));
    cx.run_until_parked();
    assert!(!saved(&path).vim.enabled);

    for _ in 0..3 {
        dispatch("vim.toggle", cx);
    }
    assert!(on(cx));
    cx.run_until_parked();
    assert!(saved(&path).vim.enabled);
}

/// A toggle queues its value as it runs: a reload of an earlier write that
/// lands before the next write runs (as the host applies `vim.enabled`)
/// changes the switch, not what the newer toggle saves. And the write is
/// queued at once, for quitting to wait on.
#[gpui::test]
fn a_reload_before_the_write_keeps_the_toggle(cx: &mut TestAppContext) {
    let (dir, path) = settings_file();
    let (_workbench, cx) = workbench(cx, dir.path());
    cx.update(|_, cx| save_toggles_to(cx, path.clone()));
    dispatch("vim.toggle", cx);
    cx.run_until_parked();
    assert!(saved(&path).vim.enabled);

    dispatch("vim.toggle", cx);
    let queued = cx.update(|_, cx| {
        let Saver(writes) = cx.global::<Saver>();
        let queue = writes.queue.lock().unwrap();
        (queue.pending.clone(), writes.writer.borrow().is_some())
    });
    assert_eq!(queued, (vec![(VIM, false)], true));
    // The earlier write's reload: `vim.enabled` went from false to true.
    cx.update(|_, cx| fe_gpui::set_vim_enabled(cx, true));
    cx.run_until_parked();
    assert!(!saved(&path).vim.enabled, "the last toggle's value");
}

/// View > Toggle Vim Mode with no window open runs the command without a
/// `RunCommand`: its value is still read in the dispatch, whether the menu
/// was installed before or after saving was set up, so a reload landing
/// before the write doesn't change it.
#[gpui::test]
fn menu_toggle_with_no_window_keeps_its_value(
    menus_first: &mut TestAppContext,
    saving_first: &mut TestAppContext,
) {
    for (menus_first, cx) in [(true, menus_first), (false, saving_first)] {
        let (_dir, path) = settings_file();
        let catalog = crate::command_catalog();
        let (bar, _) = MenuBar::default_macos(Some(&*catalog));
        let bar = bar.unwrap();
        let item = bar
            .menu("View")
            .unwrap()
            .items
            .iter()
            .find_map(|item| match &item.kind {
                ItemKind::Command(command) if command.title == "Toggle Vim Mode" => {
                    Some(command.id.clone())
                }
                _ => None,
            })
            .expect("View has Toggle Vim Mode");
        cx.update(|cx| {
            fe_gpui::init(cx);
            crate::init(cx);
            if menus_first {
                fe_gpui::menu::install(cx, bar.clone());
            }
            save_toggles_to(cx, path.clone());
            if !menus_first {
                fe_gpui::menu::install(cx, bar.clone());
            }
        });
        let toggle = MenuCommand {
            menu: "View".into(),
            item,
        };

        cx.update(|cx| cx.dispatch_action(&toggle));
        assert!(
            cx.update(|cx| vim_enabled(cx)),
            "menus first: {menus_first}"
        );
        let queued = cx.update(|cx| {
            let Saver(writes) = cx.global::<Saver>();
            let queue = writes.queue.lock().unwrap();
            (queue.pending.clone(), writes.writer.borrow().is_some())
        });
        assert_eq!(
            queued,
            (vec![(VIM, true)], true),
            "menus first: {menus_first}"
        );
        // A reload of an earlier `vim.enabled = false` lands first.
        cx.update(|cx| fe_gpui::set_vim_enabled(cx, false));
        cx.run_until_parked();
        assert!(saved(&path).vim.enabled, "menus first: {menus_first}");
    }
}

/// Quitting right after a toggle, with nothing run meanwhile, waits for
/// its write. (With no window: GPUI's test platform crashes quitting with
/// one.)
#[gpui::test]
fn quitting_waits_for_the_write(cx: &mut TestAppContext) {
    let (_dir, path) = settings_file();
    cx.update(|cx| {
        fe_gpui::init(cx);
        crate::init(cx);
        save_toggles_to(cx, path.clone());
    });
    let toggle = RunCommand(Invocation::of("vim.toggle").unwrap());
    cx.update(|cx| cx.dispatch_action(&toggle));
    assert!(!path.exists(), "not written yet");
    cx.quit();
    assert!(saved(&path).vim.enabled);
}

#[gpui::test]
fn hand_edits_apply_live(cx: &mut TestAppContext) {
    let (dir, path) = settings_file();
    fs::write(&path, "").unwrap();
    let mut store = SettingsStore::load(Some(path.clone()), None);
    let (workbench, cx) = workbench(cx, dir.path());
    let mut edit = |text: &str, cx: &mut VisualTestContext| {
        fs::write(&path, text).unwrap();
        let event = store.reload(std::slice::from_ref(&path)).expect("a change");
        workbench.update(cx, |workbench, cx| workbench.reload_settings(&event, cx));
    };
    assert!(visible(&workbench, cx));

    edit("[status_bar]\nvisible = false\n", cx);
    assert!(!visible(&workbench, cx));
    edit("[status_bar]\nvisible = true\n", cx);
    assert!(visible(&workbench, cx));

    // A reload for another key keeps a toggle.
    dispatch("view.toggle_status_bar", cx);
    assert!(!visible(&workbench, cx));
    edit("[status_bar]\nvisible = true\n[editor]\ntab_size = 2\n", cx);
    assert!(!visible(&workbench, cx));
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

/// Runs [`failed_write_is_logged`] in a process of its own, so its logger
/// is the one installed whichever runner runs the tests: a process has one,
/// and `tests::open_settings` installs another.
#[test]
fn failed_write_is_reported() {
    run_alone("persist::tests::failed_write_is_logged");
}

/// Runs the ignored test `name` in a process of its own, and checks it
/// passed.
fn run_alone(name: &str) {
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", name, "--ignored", "--nocapture"])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success() && stdout.contains("1 passed"),
        "{name}: {}\n{stdout}\n{stderr}",
        output.status
    );
}

#[gpui::test]
#[ignore = "run by failed_write_is_reported, in a process of its own"]
fn failed_write_is_logged(cx: &mut TestAppContext) {
    log::set_logger(&Capture).expect("the process's only logger");
    log::set_max_level(log::LevelFilter::Error);
    let (dir, path) = settings_file();
    let broken = "[status_bar\nvisible = true\n";
    fs::write(&path, broken).unwrap();
    let (workbench, cx) = workbench(cx, dir.path());
    cx.update(|_, cx| save_toggles_to(cx, path.clone()));

    dispatch("view.toggle_status_bar", cx);
    cx.run_until_parked();
    assert!(!visible(&workbench, cx), "hidden for the session");
    assert_eq!(fs::read_to_string(&path).unwrap(), broken);
    let errors = ERRORS.lock().unwrap();
    let named = path.display().to_string();
    assert!(
        errors.iter().any(|error| error.contains(&named)),
        "an error names {named}: {errors:?}"
    );
}

/// Runs [`toggle_while_settings_is_created`] in a process of its own: the
/// hook it puts in Settings… is a global, which `tests::open_settings`
/// sets too.
#[test]
fn a_toggle_while_settings_is_created_is_kept() {
    run_alone("persist::tests::toggle_while_settings_is_created");
}

/// Settings… holds after making its temporary file, a toggle is saved, and
/// then Settings… finishes, or fails: the saved file stays as it is either
/// way, with the whole template and the toggle. (Settings… runs on a
/// thread, as in `tests::open_settings`: on the test executor it couldn't
/// overlap.)
#[gpui::test]
#[ignore = "run by a_toggle_while_settings_is_created_is_kept, in a process of its own"]
fn toggle_while_settings_is_created(cx: &mut TestAppContext) {
    use std::{
        sync::{Arc, mpsc},
        thread,
        time::Duration,
    };

    use crate::open::{HOOK, Stage, create_settings};

    let (dir, path) = settings_file();
    let (workbench, cx) = workbench(cx, dir.path());
    cx.update(|_, cx| save_toggles_to(cx, path.clone()));
    let template = fe_config::USER_SETTINGS_TEMPLATE;
    let comments = |text: &str| -> Vec<String> {
        text.lines()
            .filter(|line| line.starts_with('#'))
            .map(str::to_owned)
            .collect()
    };
    let wait = Duration::from_secs(30);
    // The first toggle hides the bar, the second shows it again.
    for (fails, shown) in [(false, false), (true, true)] {
        let (paused_tx, paused_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel::<()>();
        let (paused_tx, release_rx) = (Mutex::new(paused_tx), Mutex::new(release_rx));
        let hook = move |stage: Stage| match stage {
            Stage::Write => {
                paused_tx.lock().unwrap().send(()).unwrap();
                let released = release_rx.lock().unwrap().recv_timeout(wait);
                released.expect("the test releases Settings…");
                if fails {
                    Err(std::io::Error::other("disk full"))
                } else {
                    Ok(())
                }
            }
            Stage::Wait | Stage::Cleanup => Ok(()),
        };
        *HOOK.lock().unwrap() = Some((path.clone(), Arc::new(hook)));
        let creating = thread::spawn({
            let path = path.clone();
            move || create_settings(&path)
        });
        let paused = paused_rx.recv_timeout(wait);
        paused.expect("Settings… holds before writing the template");

        dispatch("view.toggle_status_bar", cx);
        assert_eq!(visible(&workbench, cx), shown);
        cx.run_until_parked();
        let written = fs::read_to_string(&path).unwrap();
        release_tx.send(()).unwrap();
        let created = creating.join().unwrap();
        assert_eq!(created.is_err(), fails, "{created:?}");

        let text = fs::read_to_string(&path).unwrap();
        assert_eq!(text, written, "fails: {fails}");
        assert_eq!(comments(&text), comments(template), "fails: {fails}");
        let line = format!("\nvisible = {shown}\n");
        assert!(text.contains(&line), "fails: {fails}: {text:?}");
        assert_eq!(saved(&path).status_bar.visible, shown, "fails: {fails}");
        fs::remove_file(&path).unwrap();
    }
    HOOK.lock().unwrap().take();
}
