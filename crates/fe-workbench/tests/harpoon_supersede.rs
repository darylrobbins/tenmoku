//! Harpoon's pending opens give way to what the user did meanwhile: a
//! newer open or tab navigation, and a dismissed list picker.

use std::{
    fs,
    path::PathBuf,
    sync::{Mutex, MutexGuard},
};

use fe_command::{ContextFrame, ContextStack, Invocation};
use fe_config::harpoon::{MarkList, marks_file};
use fe_gpui::{EditorSettings, EditorTheme, commands::RunCommand};
use fe_workbench::{Launch, Workbench};
use gpui::{Entity, Focusable as _, TestAppContext, VisualTestContext};

static SERIAL: Mutex<()> = Mutex::new(());

/// Runs the rest of a test with no other test of this binary running, and
/// the state directory `state`.
fn serially(state: &std::path::Path) -> MutexGuard<'static, ()> {
    let guard = SERIAL.lock().unwrap_or_else(|p| p.into_inner());
    // SAFETY: every test takes the lock before it starts gpui.
    unsafe { std::env::set_var("TENMOKU_STATE_DIR", state) };
    for var in ["TENMOKU_CONFIG_DIR", "XDG_CONFIG_HOME"] {
        // SAFETY: as above.
        unsafe { std::env::remove_var(var) };
    }
    guard
}

fn scratch() -> PathBuf {
    tempfile::tempdir().unwrap().keep()
}

fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.run_until_parked();
}

fn dispatch(id: &str, cx: &mut VisualTestContext) {
    let context = ContextStack::new().with(ContextFrame::new("workbench"));
    let run = RunCommand(Invocation::of(id).unwrap().context(context));
    cx.update(|window, cx| window.dispatch_action(Box::new(run), cx));
}

fn press(key: &str, cx: &mut VisualTestContext) {
    let key = gpui::Keystroke::parse(key).unwrap();
    cx.update(|window, cx| window.dispatch_keystroke(key, cx));
}

fn run(id: &str, cx: &mut VisualTestContext) {
    dispatch(id, cx);
    draw(cx);
}

/// A window on a root holding `a`, `b` and `c`, all marked.
fn setup(
    cx: &mut TestAppContext,
    state: &std::path::Path,
    root: &std::path::Path,
) -> (Entity<Workbench>, VisualTestContext, Vec<PathBuf>) {
    let names = ["a.txt", "b.txt", "c.txt"];
    let files: Vec<PathBuf> = names
        .iter()
        .map(|n| {
            fs::write(root.join(n), format!("{n}\n")).unwrap();
            root.join(n)
        })
        .collect();
    MarkList::from_files(names.map(PathBuf::from))
        .save(&marks_file(state, root))
        .unwrap();
    cx.update(|cx| {
        fe_gpui::init(cx);
        fe_workbench::init(cx);
    });
    let launch = Launch {
        root: Some(root.to_owned()),
        file: None,
    };
    let window = cx.add_window(move |window, cx| {
        let theme = EditorTheme::default_dark(cx);
        Workbench::new(launch, theme, EditorSettings::default(), window, cx)
    });
    let workbench = window.root(cx).unwrap();
    let mut vcx = VisualTestContext::from_window(window.into(), cx);
    vcx.update(|window, _| window.activate_window());
    vcx.update(|window, cx| window.focus(&workbench.focus_handle(cx), cx));
    draw(&mut vcx);
    (workbench, vcx, files)
}

fn open(workbench: &Entity<Workbench>, path: &std::path::Path, cx: &mut VisualTestContext) {
    let pane = workbench.read_with(cx, |workbench, _| workbench.pane().clone());
    let path = path.to_path_buf();
    cx.update(|window, cx| pane.update(cx, |pane, cx| pane.open(path, window, cx)));
    draw(cx);
}

fn activate(workbench: &Entity<Workbench>, ix: usize, cx: &mut VisualTestContext) {
    let pane = workbench.read_with(cx, |workbench, _| workbench.pane().clone());
    cx.update(|window, cx| pane.update(cx, |pane, cx| pane.activate(ix, window, cx)));
}

fn tabs(
    workbench: &Entity<Workbench>,
    cx: &mut VisualTestContext,
) -> (Vec<PathBuf>, Option<PathBuf>) {
    workbench.read_with(cx, |workbench, cx| {
        let pane = workbench.pane().read(cx);
        let paths = pane.tabs().iter().filter_map(|t| t.path.clone()).collect();
        let active = pane.active().and_then(|ix| pane.tabs()[ix].path.clone());
        (paths, active)
    })
}

#[gpui::test]
fn an_open_while_a_jump_is_pending_beats_the_jump(cx: &mut TestAppContext) {
    // Kept, not removed at the end: the removal would reach the window's
    // file watcher during the next test, whose scheduler rejects it.
    let (state, dir) = (scratch(), scratch());
    let _serial = serially(&state);
    let (workbench, mut vcx, files) = setup(cx, &state, &dir);
    let cx = &mut vcx;
    open(&workbench, &files[1], cx);
    open(&workbench, &files[0], cx);

    // A file not open yet, opened while the jump to b is pending.
    dispatch("workbench.harpoon_jump2", cx);
    open(&workbench, &files[2], cx);
    assert_eq!(tabs(&workbench, cx).1, Some(files[2].clone()));

    // Away and back to a tab, while it is pending.
    activate(&workbench, 0, cx);
    draw(cx);
    dispatch("workbench.harpoon_jump3", cx);
    let back = tabs(&workbench, cx).1.unwrap();
    activate(&workbench, 1, cx);
    let (paths, _) = tabs(&workbench, cx);
    let ix = paths.iter().position(|p| *p == back).unwrap();
    activate(&workbench, ix, cx);
    draw(cx);
    assert_eq!(tabs(&workbench, cx).1, Some(back));
}

#[gpui::test]
fn a_dismissed_list_picker_does_not_open_its_row(cx: &mut TestAppContext) {
    // Kept, not removed at the end: the removal would reach the window's
    // file watcher during the next test, whose scheduler rejects it.
    let (state, dir) = (scratch(), scratch());
    let _serial = serially(&state);
    let (workbench, mut vcx, _files) = setup(cx, &state, &dir);
    let cx = &mut vcx;
    let focus = |cx: &mut VisualTestContext| cx.update(|window, cx| window.focused(cx));
    let before = focus(cx);
    assert!(before.is_some(), "something has focus");
    let dismiss = |how: &str, cx: &mut VisualTestContext| match how {
        "escape" => press("escape", cx),
        _ => dispatch("workbench.harpoon_toggle_list", cx),
    };
    for how in ["escape", "toggle"] {
        // The control: dismissed with no row confirmed, so nothing pending.
        // Escape gives focus back to where it was; toggling off leaves it
        // wherever the modal layer does, with or without an open.
        run("workbench.harpoon_toggle_list", cx);
        dismiss(how, cx);
        draw(cx);
        let expected = focus(cx);
        if how == "escape" {
            assert_eq!(expected, before, "escape returns focus");
        }

        run("workbench.harpoon_toggle_list", cx);
        // Not simulate_keystrokes, which parks: the read must stay pending.
        press("enter", cx);
        dismiss(how, cx);
        draw(cx);
        assert_eq!(tabs(&workbench, cx), (vec![], None), "{how}");
        assert_eq!(focus(cx), expected, "{how}: focus did not move");
        // Back to the start for the next case.
        cx.update(|window, cx| window.focus(&workbench.focus_handle(cx), cx));
        draw(cx);
    }
}
