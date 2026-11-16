//! The title bar's run widget debugging (`run_widget_debug.rs`, card D8b)
//! against fe-dap's fake adapter: launch configurations listed beside
//! tasks, the Debug button, the session's controls in the title bar and
//! their names.

use std::{fs, path::Path, time::Duration};

use fe_dap::{SessionState, fake::Script};
use gpui::{Modifiers, TestAppContext, px};
use kiln_l10n::tr;
use serde_json::json;

use super::debug_commands::{Debug, main_path, script, spinning, trusted};
use crate::{
    debug_toolbar,
    run_widget_debug::{DISPATCHED, DRAWN, RunChoice},
    title_bar::LAST_TOOLTIP,
};

/// The launch file the harness writes, then one more configuration whose
/// adapter isn't known, so it can't start.
const LAUNCH: &str = r#"
[[launch]]
name = "main"
adapter = "fake"
program = "${workspaceFolder}/target/debug/main"

[[launch]]
name = "other"
adapter = "fake"
program = "${workspaceFolder}/target/debug/other"

[[launch]]
name = "broken"
adapter = "nowhere"
program = "${workspaceFolder}/target/debug/broken"
"#;

const TASKS: &str = r#"
[[task]]
label = "alpha"
command = "true"
group = "build"
default = true
"#;

/// A trusted window on the harness's project, with a task, the launch
/// file's three configurations and a Cargo package (which a
/// configuration is detected from), its title bar shown.
fn window(cx: &mut TestAppContext, script: impl FnOnce(&Path) -> Script) -> Debug<'_> {
    let debug = trusted(cx, script);
    let root = debug.root.clone();
    fs::write(root.join(".tenmoku/launch.toml"), LAUNCH).unwrap();
    fs::write(root.join(".tenmoku/tasks.toml"), TASKS).unwrap();
    fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = \"demo\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )
    .unwrap();
    with_title_bar(debug)
}

fn with_title_bar(mut debug: Debug<'_>) -> Debug<'_> {
    let workbench = debug.workbench.clone();
    workbench.update(debug.cx, |workbench, cx| workbench.set_title_bar(true, cx));
    debug.draw();
    debug
}

/// The widget's debugging controls the last frame drew: id, name, enabled,
/// key.
fn controls(debug: &mut Debug) -> Vec<(&'static str, String, bool, Option<String>)> {
    debug.draw();
    DRAWN.with_borrow(|drawn| {
        (drawn.controls.iter())
            .map(|(id, name, enabled, key)| (*id, name.to_string(), *enabled, key.clone()))
            .collect()
    })
}

/// The run widget's buttons the last frame drew: id and name.
fn run_buttons(debug: &mut Debug) -> Vec<(&'static str, String)> {
    crate::title_bar::DRAWN.with_borrow_mut(Vec::clear);
    debug.draw();
    crate::title_bar::DRAWN.with_borrow(|drawn| {
        (drawn.iter())
            .filter(|(id, _)| id.starts_with("title-run"))
            .map(|(id, name)| (*id, name.to_string()))
            .collect()
    })
}

fn click(debug: &mut Debug, selector: &'static str) {
    debug.draw();
    let at = debug.cx.debug_bounds(selector).expect(selector).center();
    debug.cx.simulate_click(at, Modifiers::none());
    debug.draw();
}

/// The run picker's rows, if it shows.
fn picker_rows(debug: &mut Debug) -> Option<Vec<String>> {
    debug.draw();
    let workbench = debug.workbench.clone();
    debug
        .cx
        .update(|_, cx| workbench.read(cx).run_picker_rows(cx))
}

/// Opens the picker from the label and picks the row reading `row`.
fn choose(debug: &mut Debug, row: &str) {
    click(debug, "title-run");
    let rows = picker_rows(debug).expect("the picker shows");
    let ix = rows.iter().position(|shown| shown == row).expect(row);
    let first = rows
        .iter()
        .position(|shown| !shown.starts_with('#'))
        .unwrap();
    for _ in first..ix {
        debug.cx.simulate_keystrokes("down");
    }
    debug.cx.simulate_keystrokes("enter");
    debug.draw();
}

/// The tooltip shown once the mouse rests on `selector`'s element.
fn hover(debug: &mut Debug, selector: &'static str) -> Option<String> {
    debug
        .cx
        .simulate_mouse_move(gpui::point(px(600.), px(400.)), None, Modifiers::none());
    debug.draw();
    LAST_TOOLTIP.with_borrow_mut(Option::take);
    let at = debug.cx.debug_bounds(selector).expect(selector).center();
    debug.cx.simulate_mouse_move(at, None, Modifiers::none());
    debug
        .cx
        .executor()
        .advance_clock(Duration::from_millis(600));
    debug.draw();
    LAST_TOOLTIP.with_borrow(Clone::clone)
}

/// The detected configuration's name, as detection gives it.
fn detected() -> String {
    tr!("workbench-debug-launch-detected-bin", bin = "demo").to_string()
}

/// C1: the picker lists the tasks, then the launch configurations under
/// their own heading, each with its source: the launch file's and the
/// detected one. Typing filters both, and picking a configuration chooses
/// it, as a task is chosen.
#[gpui::test]
fn the_picker_lists_launch_configurations_beside_tasks(cx: &mut TestAppContext) {
    let mut debug = window(cx, script);
    // A task is chosen, the default build task, and the root has
    // configurations: Debug… shows after play.
    assert_eq!(
        run_buttons(&mut debug),
        [
            (
                "title-run",
                tr!("workbench-run-task", name = "alpha").to_string()
            ),
            ("title-run-play", tr!("workbench-run-play").to_string()),
            (
                "title-run-debug-pick",
                tr!("workbench-debug-run-debug-pick").to_string()
            ),
        ]
    );
    click(&mut debug, "title-run");
    let rows = picker_rows(&mut debug).expect("the picker shows");
    let tasks = format!("# {}", tr!("workbench-debug-run-tasks"));
    let configs = format!("# {}", tr!("workbench-debug-run-configs"));
    let launch_toml = "launch.toml";
    let detected_source = tr!("workbench-debug-picker-detected").to_string();
    let at = rows
        .iter()
        .position(|row| *row == configs)
        .expect("the heading");
    assert_eq!(rows[0], tasks);
    // The Cargo package implies its own tasks too.
    assert!(rows[1..at].contains(&"alpha".to_owned()), "{rows:?}");
    assert_eq!(
        rows[at + 1..],
        [
            format!("main · {launch_toml}"),
            format!("other · {launch_toml}"),
            format!("broken · {launch_toml}"),
            format!("{} · {detected_source}", detected()),
        ]
    );
    // Typing filters both groups.
    debug.cx.simulate_keystrokes("a l p h");
    assert_eq!(
        picker_rows(&mut debug).unwrap(),
        [tasks.clone(), "alpha".to_owned()]
    );
    debug
        .cx
        .simulate_keystrokes("backspace backspace backspace backspace o t h");
    assert_eq!(
        picker_rows(&mut debug).unwrap(),
        [configs.clone(), format!("other · {launch_toml}")]
    );
    // Picking it chooses it: its label carries the bug, and only Debug
    // shows.
    debug.cx.simulate_keystrokes("enter");
    debug.draw();
    assert_eq!(picker_rows(&mut debug), None);
    assert_eq!(
        run_buttons(&mut debug),
        [
            (
                "title-run",
                tr!("workbench-debug-run-config", name = "other").to_string()
            ),
            (
                "title-run-debug",
                tr!("workbench-debug-run-debug").to_string()
            ),
        ]
    );
    assert!(
        (debug.cx.debug_bounds("icon-button:title-run-play")).is_none(),
        "no play for a configuration"
    );
    // The detected one is chosen as one from the file is.
    choose(&mut debug, &format!("{} · {detected_source}", detected()));
    assert_eq!(
        run_buttons(&mut debug)[0].1,
        tr!("workbench-debug-run-config", name = detected().as_str()).to_string()
    );
}

/// C2: Debug dispatches `debug.start` with the configuration chosen, which
/// starts it; one that can't start leaves Debug disabled, its tooltip
/// saying why, and a task chosen has Debug… show `debug.start`'s picker.
#[gpui::test]
fn debug_starts_the_chosen_configuration(cx: &mut TestAppContext) {
    let mut debug = window(cx, |root| spinning(script(root)));
    DISPATCHED.with_borrow_mut(Vec::clear);

    // A task chosen: Debug… is `debug.start` alone, its picker.
    let tip = hover(&mut debug, "icon-button:title-run-debug-pick").expect("a tooltip");
    let name = tr!("workbench-debug-run-debug-pick").to_string();
    assert!(tip.starts_with(&name), "{tip}");
    click(&mut debug, "icon-button:title-run-debug-pick");
    assert_eq!(
        DISPATCHED.with_borrow_mut(std::mem::take),
        [("debug.start".to_owned(), serde_json::Value::Null)]
    );
    let workbench = debug.workbench.clone();
    let shown = debug
        .cx
        .update(|_, cx| workbench.read(cx).debug_picker(cx).is_some());
    assert!(shown, "debug.start's picker shows");
    debug.cx.simulate_keystrokes("escape");
    debug.draw();

    // A configuration that can't start: disabled, why in its tooltip.
    choose(&mut debug, "broken · launch.toml");
    let broken = controls(&mut debug);
    assert_eq!(broken.len(), 1, "{broken:?}");
    assert_eq!(broken[0].0, "title-run-debug");
    assert!(!broken[0].2, "disabled");
    let why = read_blockers(&mut debug, "broken");
    assert!(!why.is_empty());
    let expected = tr!("workbench-debug-run-blocked", why = why.join("; ")).to_string();
    assert_eq!(
        hover(&mut debug, "icon-button:title-run-debug"),
        Some(expected)
    );
    click(&mut debug, "icon-button:title-run-debug");
    assert_eq!(DISPATCHED.with_borrow_mut(std::mem::take), []);
    assert_eq!(debug.state(), None, "nothing started");

    // One that can: Debug starts it, through `debug.start`.
    choose(&mut debug, "other · launch.toml");
    assert!(controls(&mut debug)[0].2, "enabled");
    click(&mut debug, "icon-button:title-run-debug");
    assert_eq!(
        DISPATCHED.with_borrow_mut(std::mem::take),
        [(
            "debug.start".to_owned(),
            json!({ "configuration": "other" })
        )]
    );
    debug.wait_state("running", |state| *state == SessionState::Running);
    let debugger = debug.debugger();
    let name = debugger.read_with(debug.cx, |debugger, _| {
        debugger.configuration().map(ToOwned::to_owned)
    });
    assert_eq!(name.as_deref(), Some("other"));
    debug.run("debug.stop");
    debug.wait_state("the end", SessionState::is_ended);
}

/// The blockers of the root's configuration `name`, as `debug.start`
/// reads them.
fn read_blockers(debug: &mut Debug, name: &str) -> Vec<String> {
    let workbench = debug.workbench.clone();
    let reading = debug
        .cx
        .update(|_, cx| crate::run_widget_debug::read_configs(&workbench, cx));
    let configs = std::rc::Rc::new(std::cell::RefCell::new(None));
    let into = configs.clone();
    debug
        .cx
        .update(|_, cx| {
            cx.spawn(async move |_| {
                *into.borrow_mut() = Some(reading.await);
            })
        })
        .detach();
    debug.until("the configurations", |_| configs.borrow().is_some());
    let configs = configs.borrow_mut().take().unwrap();
    let config = configs.into_iter().find(|config| config.name == name);
    config.expect(name).blockers
}

/// C3: while a session runs the widget is its controls, as D0 places them
/// (the label, Continue or Pause, the three steps, 6 px, Restart and
/// Stop, 22 px buttons 2 px apart), each dispatching its command, and the
/// floating toolbar doesn't show; the label opens the session menu. The
/// choice is the root's, kept across a root change and in the layout.
#[gpui::test]
fn a_session_shows_its_controls_in_the_title_bar(cx: &mut TestAppContext) {
    let mut debug = window(cx, |root| spinning(script(root)));
    let main = main_path(&debug.root);
    debug.toggle(&main, 1);
    choose(&mut debug, "main · launch.toml");
    click(&mut debug, "icon-button:title-run-debug");
    debug.wait_stop_at(1);

    let ids =
        |controls: &[(&'static str, String, bool, Option<String>)]| -> Vec<(&'static str, bool)> {
            controls
                .iter()
                .map(|(id, _, enabled, _)| (*id, *enabled))
                .collect()
        };
    assert_eq!(
        ids(&controls(&mut debug)),
        [
            ("title-run-session", true),
            ("title-run-debug-continue", true),
            ("title-run-debug-step-over", true),
            ("title-run-debug-step-into", true),
            ("title-run-debug-step-out", true),
            ("title-run-debug-restart", true),
            ("title-run-debug-stop", true),
        ]
    );
    // The title bar has them; the toolbar doesn't float.
    assert!(debug_toolbar::DRAWN.with_borrow(Option::is_none));
    assert_eq!(debug.cx.debug_bounds(debug_toolbar::TOOLBAR_ID), None);
    // The label shows the stopped state's icon.
    assert!(
        (debug.cx.debug_bounds("title-run-session-chevron")).is_some(),
        "the label's state icon"
    );
    let bounds = |debug: &mut Debug, id: &str| {
        let selector: &'static str = Box::leak(format!("icon-button:{id}").into_boxed_str());
        debug.cx.debug_bounds(selector).expect(selector)
    };
    let over = bounds(&mut debug, "title-run-debug-step-over");
    let into = bounds(&mut debug, "title-run-debug-step-into");
    let out = bounds(&mut debug, "title-run-debug-step-out");
    let restart = bounds(&mut debug, "title-run-debug-restart");
    assert_eq!(over.size, gpui::size(px(22.), px(22.)));
    assert_eq!(into.left() - over.right(), px(2.));
    assert_eq!(restart.left() - out.right(), px(6.));

    // Each dispatches its command: Step Over steps.
    click(&mut debug, "icon-button:title-run-debug-step-over");
    debug.wait_stop_at(2);
    // Continue runs, and Pause takes its place.
    click(&mut debug, "icon-button:title-run-debug-continue");
    debug.wait_state("running", |state| *state == SessionState::Running);
    let running = ids(&controls(&mut debug));
    assert_eq!(running[1], ("title-run-debug-pause", true));
    assert_eq!(running[2], ("title-run-debug-step-over", false));

    // The label opens the session menu.
    click(&mut debug, "title-run-session");
    let workbench = debug.workbench.clone();
    let items = debug
        .cx
        .update(|_, cx| workbench.read(cx).session_menu_items(cx));
    let items: Vec<String> = (items.expect("the session menu shows").iter())
        .map(ToString::to_string)
        .collect();
    assert_eq!(
        items,
        [
            tr!("workbench-debug-run-show-debugger").to_string(),
            tr!("workbench-debug-run-show-log").to_string(),
            tr!("workbench-debug-run-choose").to_string(),
        ]
    );
    // Choose Task or Configuration… is the widget's picker.
    debug.cx.simulate_keystrokes("down down enter");
    debug.draw();
    assert!(picker_rows(&mut debug).is_some(), "the run picker shows");
    debug.cx.simulate_keystrokes("escape");
    debug.draw();

    // Stop ends it, and the widget is the choice again.
    click(&mut debug, "icon-button:title-run-debug-stop");
    debug.wait_state("the end", SessionState::is_ended);
    assert_eq!(
        run_buttons(&mut debug)[0].1,
        tr!("workbench-debug-run-config", name = "main").to_string()
    );

    // The choice is the root's: another root has its own, and this one's
    // is back on return, and in the saved layout.
    let root = debug.root.clone();
    let choice = |debug: &mut Debug| {
        let workbench = debug.workbench.clone();
        let root = root.clone();
        debug
            .cx
            .update(|_, cx| workbench.read(cx).run_task_choice(&root))
    };
    assert_eq!(
        choice(&mut debug),
        Some(RunChoice::Debug("main".to_owned()))
    );
    let other = tempfile::tempdir().unwrap();
    fs::write(other.path().join("Makefile"), "build:\n\ttrue\n").unwrap();
    set_root(&mut debug, other.path());
    assert_eq!(run_buttons(&mut debug)[0].0, "title-run");
    assert_ne!(
        run_buttons(&mut debug)[0].1,
        tr!("workbench-debug-run-config", name = "main").to_string()
    );
    set_root(&mut debug, &root);
    assert_eq!(
        run_buttons(&mut debug)[0].1,
        tr!("workbench-debug-run-config", name = "main").to_string()
    );
    let entry = debug.cx.update(|_, cx| workbench.read(cx).layout_entry(cx));
    let saved = &entry["run_tasks"][root.to_str().unwrap()];
    assert_eq!(*saved, json!({ "debug": "main" }));
    assert_eq!(
        RunChoice::from_value(saved),
        Some(RunChoice::Debug("main".to_owned()))
    );
    // A task's choice is saved as before: its label.
    assert_eq!(
        RunChoice::Task("alpha".to_owned()).to_value(),
        json!("alpha")
    );
    choose(&mut debug, "alpha");
    debug.cx.update(|window, cx| {
        workbench.update(cx, |workbench, cx| {
            workbench.restore_layout(&entry, window, cx)
        })
    });
    debug.draw();
    assert_eq!(
        run_buttons(&mut debug)[0].1,
        tr!("workbench-debug-run-config", name = "main").to_string()
    );
}

fn set_root(debug: &mut Debug, root: &Path) {
    let workbench = debug.workbench.clone();
    let root = root.to_owned();
    debug.cx.update(|window, cx| {
        workbench.update(cx, |workbench, cx| workbench.set_root(root, window, cx))
    });
    workbench.update(debug.cx, |workbench, cx| workbench.set_root_trust(true, cx));
    debug.draw();
}

/// C4: every control the widget adds names itself, as a screen reader
/// hears it, and its tooltip names its key.
#[gpui::test]
fn the_widgets_debug_controls_have_accessible_names(cx: &mut TestAppContext) {
    let mut debug = window(cx, |root| spinning(script(root)));
    let label = |keys: &str| {
        let keys = fe_keymap::KeySequence::parse(keys).unwrap();
        fe_keymap::shortcut_label(&keys)
    };
    let buttons = |debug: &mut Debug| -> Vec<String> {
        debug.draw();
        let nodes = debug.cx.update(|window, cx| fe_ui::nodes(window, cx));
        (nodes.iter())
            .filter(|node| node.role == fe_ui::Role::Button)
            .map(|node| node.label.to_string())
            .collect()
    };

    // Idle, a task chosen: Debug… with F5, debug.start's key.
    let idle = controls(&mut debug);
    assert_eq!(
        idle,
        [(
            "title-run-debug-pick",
            tr!("workbench-debug-run-debug-pick").to_string(),
            true,
            Some(label("f5"))
        )]
    );
    assert!(buttons(&mut debug).contains(&tr!("workbench-debug-run-debug-pick").to_string()));

    // A configuration chosen: its label and Debug.
    choose(&mut debug, "main · launch.toml");
    assert_eq!(
        controls(&mut debug)[0].1,
        tr!("workbench-debug-run-debug").to_string()
    );
    let names = buttons(&mut debug);
    assert!(
        names.contains(&tr!("workbench-debug-run-debug").to_string()),
        "{names:?}"
    );
    assert!(
        names.contains(&tr!("workbench-debug-run-config", name = "main").to_string()),
        "{names:?}"
    );

    // A session: the label names it and where it's stopped, each button
    // what it does, with its key.
    let main = main_path(&debug.root);
    debug.toggle(&main, 2);
    click(&mut debug, "icon-button:title-run-debug");
    debug.wait_stop_at(2);
    let session = controls(&mut debug);
    let named = |key: &str, keys: Option<&str>| (tr!(key).to_string(), keys.map(label));
    assert_eq!(
        (session.iter())
            .map(|(_, name, _, key)| (name.clone(), key.clone()))
            .collect::<Vec<_>>(),
        [
            (
                tr!(
                    "workbench-debug-toolbar-session-stopped-at",
                    name = "main",
                    line = 3
                )
                .to_string(),
                None
            ),
            named("workbench-debug-toolbar-continue", Some("f5")),
            named("workbench-debug-toolbar-step-over", Some("f10")),
            named("workbench-debug-toolbar-step-into", Some("f11")),
            named("workbench-debug-toolbar-step-out", Some("shift-f11")),
            named("workbench-debug-toolbar-restart", Some("cmd-shift-f5")),
            named("workbench-debug-toolbar-stop", Some("shift-f5")),
        ]
    );
    // The label names where it stopped (asserted above through the catalogue).
    let names = buttons(&mut debug);
    for (_, name, _, _) in &session {
        assert!(names.contains(name), "{name} in {names:?}");
    }
    // A tooltip names the button and its key.
    let tip = hover(&mut debug, "icon-button:title-run-debug-step-over").unwrap();
    assert!(
        tip.contains(tr!("workbench-debug-toolbar-step-over").as_ref())
            && tip.contains(&label("f10")),
        "{tip}"
    );

    // Running, Pause names itself with F6.
    debug.run("debug.continue");
    debug.wait_state("running", |state| *state == SessionState::Running);
    let running = controls(&mut debug);
    assert_eq!(
        (running[1].0, running[1].1.as_str(), running[1].3.clone()),
        (
            "title-run-debug-pause",
            tr!("workbench-debug-toolbar-pause").as_ref(),
            Some(label("f6"))
        )
    );
    assert_eq!(
        running[0].1,
        tr!("workbench-debug-toolbar-session-running", name = "main").to_string()
    );
    debug.run("debug.stop");
    debug.wait_state("the end", SessionState::is_ended);
}

/// Review: the detected configurations include the active Python file's,
/// so the widget reads them again when the active editor's language
/// changes, not only when a file defining them does.
#[gpui::test]
fn the_active_python_file_is_a_configuration_once_open(cx: &mut TestAppContext) {
    let debug = trusted(cx, script);
    let root = debug.root.clone();
    // Nothing to run or debug yet.
    fs::write(root.join(".tenmoku/launch.toml"), "").unwrap();
    fs::write(root.join("tool.py"), "print('hi')\n").unwrap();
    let mut debug = with_title_bar(debug);
    assert_eq!(run_buttons(&mut debug), []);
    debug.open_at(&root.join("tool.py"), 0);
    let current = tr!("workbench-debug-launch-current-file").to_string();
    let name = tr!("workbench-debug-run-config", name = current.as_str()).to_string();
    debug.until("the current file's configuration", |debug| {
        run_buttons(debug).first().map(|(_, shown)| shown.clone()) == Some(name.clone())
    });
}

/// Review F1: what can start follows the `[debug]` settings, and the
/// detected configurations follow Cargo's binary targets, with no picker
/// opened in between.
#[gpui::test]
fn configurations_follow_adapter_settings_and_cargo_targets(cx: &mut TestAppContext) {
    let mut debug = window(cx, script);
    choose(&mut debug, "broken · launch.toml");
    assert!(!controls(&mut debug)[0].2, "no adapter `nowhere` yet");
    // The user adds the adapter.
    let mut settings = fe_config::Settings::default();
    settings.debug.adapters.insert(
        "nowhere".to_owned(),
        fe_config::debug_settings::DebugAdapterSettings {
            command: Some("nowhere".to_owned()),
            ..Default::default()
        },
    );
    let event = fe_config::SettingsEvent {
        settings: std::sync::Arc::new(settings),
        previous: std::sync::Arc::new(fe_config::Settings::default()),
        diagnostics: Vec::new(),
    };
    let workbench = debug.workbench.clone();
    workbench.update(debug.cx, |workbench, cx| {
        workbench.reload_settings(&event, cx)
    });
    debug.until("Debug enabled", |debug| {
        controls(debug).first().is_some_and(|control| control.2)
    });

    // The detected configuration's binary goes, then comes back.
    let detected_row = format!(
        "{} · {}",
        detected(),
        tr!("workbench-debug-picker-detected")
    );
    choose(&mut debug, &detected_row);
    let root = debug.root.clone();
    let panel = debug.cx.update(|_, cx| workbench.read(cx).panel().clone());
    let changed = |debug: &mut Debug| {
        let changes = [fe_project::FsChange::Path(main_path(&root))];
        panel.update(debug.cx, |panel, cx| panel.apply_fs_changes(&changes, cx));
    };
    let main = fs::read_to_string(main_path(&root)).unwrap();
    fs::remove_file(main_path(&root)).unwrap();
    changed(&mut debug);
    // The choice is gone: the widget falls back to the default build task.
    let alpha = tr!("workbench-run-task", name = "alpha").to_string();
    debug.until("the fallback", |debug| {
        run_buttons(debug).first().map(|(_, name)| name.clone()) == Some(alpha.clone())
    });
    fs::write(main_path(&root), main).unwrap();
    changed(&mut debug);
    let chosen = tr!("workbench-debug-run-config", name = detected().as_str()).to_string();
    debug.until("the choice back", |debug| {
        run_buttons(debug).first().map(|(_, name)| name.clone()) == Some(chosen.clone())
    });
}

/// Review F2: a task plays, and the picker opens, while the configurations
/// are still being read.
#[gpui::test]
fn tasks_do_not_wait_for_the_configurations(cx: &mut TestAppContext) {
    let debug = trusted(cx, script);
    let root = debug.root.clone();
    fs::write(root.join(".tenmoku/tasks.toml"), TASKS).unwrap();
    debug
        .cx
        .update(|_, cx| cx.set_global(crate::run_widget_debug::HoldConfigReads));
    let mut debug = with_title_bar(debug);
    assert_eq!(
        run_buttons(&mut debug),
        [
            (
                "title-run",
                tr!("workbench-run-task", name = "alpha").to_string()
            ),
            ("title-run-play", tr!("workbench-run-play").to_string()),
        ]
    );
    click(&mut debug, "icon-button:title-run-play");
    let terminal = debug
        .cx
        .update(|_, cx| debug.workbench.read(cx).terminal().clone());
    let runs = terminal.read_with(debug.cx, |terminal, _| terminal.views().count());
    assert_eq!(runs, 1);
    click(&mut debug, "title-run");
    assert_eq!(picker_rows(&mut debug).unwrap(), ["alpha"]);
}
