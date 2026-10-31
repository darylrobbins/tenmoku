//! Auto-save in a headless workbench: keys typed into its editors, the
//! test executor's clock moved on, focus moved and the window deactivated.

use std::{
    fs,
    path::{Path, PathBuf},
    time::{Duration, SystemTime},
};

use fe_buffer::Buffer;
use fe_command::{ContextFrame, ContextStack, Invocation};
use fe_config::settings::{AutoSave as Setting, SettingsStore};
use fe_editor::EditorModel;
use fe_gpui::{EditorSettings, EditorTheme, EditorView, SettingsEvent, commands::RunCommand};
use gpui::{Entity, Focusable as _, TestAppContext, VisualTestContext};

use crate::{Launch, Workbench, workbench::toast::Level};

/// A workbench on `root` with `settings`, in a headless window that's
/// active, as the app's is.
fn open<'a>(
    cx: &'a mut TestAppContext,
    root: &Path,
    settings: EditorSettings,
) -> (Entity<Workbench>, &'a mut VisualTestContext) {
    cx.update(|cx| {
        fe_gpui::init(cx);
        crate::init(cx);
    });
    let launch = Launch {
        root: Some(root.to_owned()),
        file: None,
    };
    let (workbench, cx) = cx.add_window_view(move |window, cx| {
        let theme = EditorTheme::default_dark(cx);
        Workbench::new(launch, theme, settings, window, cx)
    });
    cx.update(|window, cx| {
        window.activate_window();
        window.focus(&workbench.focus_handle(cx), cx);
    });
    draw(cx);
    (workbench, cx)
}

fn settings(auto_save: Setting) -> EditorSettings {
    EditorSettings {
        auto_save,
        auto_save_delay: 1000,
        ..EditorSettings::default()
    }
}

/// A folder with `a.txt` and `b.txt`.
fn project() -> (tempfile::TempDir, PathBuf, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let (a, b) = (dir.path().join("a.txt"), dir.path().join("b.txt"));
    fs::write(&a, "one\n").unwrap();
    fs::write(&b, "two\n").unwrap();
    (dir, a, b)
}

/// Lets background work land, then draws a frame, which delivers focus
/// changes.
fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.run_until_parked();
}

/// Moves the clock on by `ms`, and lets what that wakes land.
fn wait(ms: u64, cx: &mut VisualTestContext) {
    cx.executor().advance_clock(Duration::from_millis(ms));
    cx.run_until_parked();
}

/// Opens `path` in the active pane, focused.
fn open_file(
    workbench: &Entity<Workbench>,
    path: &Path,
    cx: &mut VisualTestContext,
) -> Entity<EditorView> {
    let pane = workbench.read_with(cx, |workbench, _| workbench.pane().clone());
    let opened = path.to_owned();
    cx.update(|window, cx| pane.update(cx, |pane, cx| pane.open(opened, window, cx)));
    draw(cx);
    let editor = workbench.read_with(cx, |workbench, cx| {
        (workbench.open_editors(cx).into_iter())
            .rfind(|editor| editor.read(cx).model().path() == Some(path))
            .expect("open")
    });
    focus(&editor, cx);
    editor
}

fn focus(editor: &Entity<EditorView>, cx: &mut VisualTestContext) {
    cx.update(|window, cx| window.focus(&editor.focus_handle(cx), cx));
    draw(cx);
}

/// Shows tab `ix` of the active pane, focused.
fn show_tab(workbench: &Entity<Workbench>, ix: usize, cx: &mut VisualTestContext) {
    let pane = workbench.read_with(cx, |workbench, _| workbench.pane().clone());
    cx.update(|window, cx| pane.update(cx, |pane, cx| pane.activate(ix, window, cx)));
    draw(cx);
}

fn type_text(text: &str, cx: &mut VisualTestContext) {
    cx.simulate_input(text);
    draw(cx);
}

fn keys(keys: &str, cx: &mut VisualTestContext) {
    cx.simulate_keystrokes(keys);
    draw(cx);
}

fn text(editor: &Entity<EditorView>, cx: &mut VisualTestContext) -> String {
    editor.read_with(cx, |editor, _| editor.model().text().to_string())
}

fn dirty(editor: &Entity<EditorView>, cx: &mut VisualTestContext) -> bool {
    editor.read_with(cx, |editor, _| editor.model().is_dirty())
}

fn on_disk(path: &Path) -> String {
    fs::read_to_string(path).unwrap()
}

/// How many auto-saves have started.
fn saves(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> usize {
    workbench.read_with(cx, |workbench, _| workbench.auto_save.saves.len())
}

/// Each toast shown: its level, message and count.
fn toasts(
    workbench: &Entity<Workbench>,
    cx: &mut VisualTestContext,
) -> Vec<(Level, String, usize)> {
    let layer = workbench.read_with(cx, |workbench, _| workbench.toasts().clone());
    layer.read_with(cx, |layer, _| {
        (layer.queue().shown().iter())
            .map(|entry| (entry.toast.level, entry.toast.message.clone(), entry.count))
            .collect()
    })
}

/// Applies `event` as the host does when a settings file changes.
fn reload(workbench: &Entity<Workbench>, event: SettingsEvent, cx: &mut VisualTestContext) {
    workbench.update(cx, |workbench, cx| workbench.reload_settings(&event, cx));
    cx.run_until_parked();
}

/// Writes `editor.auto_save = mode` to the user's settings file `user`,
/// and applies the reload.
fn set_mode(
    store: &mut SettingsStore,
    user: &Path,
    mode: &str,
    workbench: &Entity<Workbench>,
    cx: &mut VisualTestContext,
) {
    fs::write(user, format!("[editor]\nauto_save = \"{mode}\"\n")).unwrap();
    let event = store.reload(&[user.to_owned()]).expect("changed");
    reload(workbench, event, cx);
}

/// Writes `text` to `path` as another program would, its modified time
/// moved on so the change shows however coarse the file system's clock.
fn rewrite(path: &Path, text: &str) {
    fs::write(path, text).unwrap();
    let file = fs::File::options().write(true).open(path).unwrap();
    file.set_modified(SystemTime::now() + Duration::from_secs(10))
        .unwrap();
}

#[gpui::test]
fn after_delay_saves(cx: &mut TestAppContext) {
    let (dir, a, _) = project();
    let (workbench, cx) = open(cx, dir.path(), settings(Setting::AfterDelay));
    let editor = open_file(&workbench, &a, cx);

    type_text("x", cx);
    wait(999, cx);
    assert!(dirty(&editor, cx));
    assert_eq!(on_disk(&a), "one\n");
    wait(1, cx);
    assert_eq!(on_disk(&a), "xone\n");
    assert!(!dirty(&editor, cx));
    assert_eq!(cx.window_title().as_deref(), Some("a.txt — Tenmoku"));
    assert_eq!(saves(&workbench, cx), 1);

    // Each edit puts the save off again.
    type_text("y", cx);
    wait(600, cx);
    type_text("z", cx);
    wait(600, cx);
    assert_eq!(on_disk(&a), "xone\n");
    wait(400, cx);
    assert_eq!(on_disk(&a), "xyzone\n");
    assert_eq!(saves(&workbench, cx), 2);

    // Split: both editors show the buffer, which saves once.
    cx.simulate_keystrokes("cmd-\\");
    draw(cx);
    let shown = workbench.read_with(cx, |workbench, cx| workbench.open_editors(cx).len());
    assert_eq!(shown, 2);
    type_text("w", cx);
    wait(1000, cx);
    assert_eq!(on_disk(&a), "xyzwone\n");
    assert_eq!(saves(&workbench, cx), 3);
    wait(5000, cx);
    assert_eq!(saves(&workbench, cx), 3);
}

#[gpui::test]
fn focus_and_window(cx: &mut TestAppContext) {
    let (dir, a, b) = project();
    let user = dir.path().join("settings.toml");
    let mut store = SettingsStore::load(Some(user.clone()), None);
    let (workbench, cx) = open(cx, dir.path(), settings(Setting::OnFocusChange));
    let set = |store: &mut SettingsStore, mode: &str, workbench: &Entity<Workbench>, cx: &mut _| {
        set_mode(store, &user, mode, workbench, cx)
    };
    set(&mut store, "on_focus_change", &workbench, cx);
    let ea = open_file(&workbench, &a, cx);
    let eb = open_file(&workbench, &b, cx);

    // On focus change: moving to another tab saves.
    type_text("b", cx);
    wait(5000, cx);
    assert_eq!(on_disk(&b), "two\n");
    show_tab(&workbench, 0, cx);
    assert_eq!(on_disk(&b), "btwo\n");
    assert!(!dirty(&eb, cx));
    // And so does the window deactivating.
    type_text("a", cx);
    cx.deactivate_window();
    draw(cx);
    assert_eq!(on_disk(&a), "aone\n");
    assert!(!dirty(&ea, cx));
    cx.update(|window, _| window.activate_window());
    draw(cx);

    // On window change: only deactivating saves.
    set(&mut store, "on_window_change", &workbench, cx);
    focus(&ea, cx);
    type_text("a", cx);
    show_tab(&workbench, 1, cx);
    wait(5000, cx);
    assert_eq!(on_disk(&a), "aone\n");
    assert!(dirty(&ea, cx));
    cx.deactivate_window();
    draw(cx);
    assert_eq!(on_disk(&a), "aaone\n");
    cx.update(|window, _| window.activate_window());
    draw(cx);

    // Off: nothing does.
    set(&mut store, "off", &workbench, cx);
    let before = saves(&workbench, cx);
    type_text("b", cx);
    show_tab(&workbench, 0, cx);
    cx.deactivate_window();
    draw(cx);
    cx.update(|window, _| window.activate_window());
    draw(cx);
    wait(5000, cx);
    assert_eq!(on_disk(&b), "btwo\n");
    assert!(dirty(&eb, cx));
    assert_eq!(saves(&workbench, cx), before);

    // An untitled buffer never saves, in any mode.
    set(&mut store, "on_focus_change", &workbench, cx);
    let pane = workbench.read_with(cx, |workbench, _| workbench.pane().clone());
    cx.update(|window, cx| {
        pane.update(cx, |pane, cx| {
            pane.add(EditorModel::new(Buffer::new(""), None), cx);
            pane.activate(pane.tabs().len() - 1, window, cx);
        })
    });
    draw(cx);
    let before = saves(&workbench, cx);
    type_text("new", cx);
    show_tab(&workbench, 0, cx);
    assert_eq!(saves(&workbench, cx), before);
    // The window deactivating saves b.txt, left dirty while off, but not
    // the untitled buffer.
    cx.deactivate_window();
    draw(cx);
    assert_eq!(on_disk(&b), "bbtwo\n");
    assert_eq!(saves(&workbench, cx), before + 1);
    let before = before + 1;
    cx.update(|window, _| window.activate_window());
    draw(cx);
    set(&mut store, "after_delay", &workbench, cx);
    show_tab(&workbench, 2, cx);
    type_text("er", cx);
    wait(5000, cx);
    assert_eq!(saves(&workbench, cx), before);
}

#[gpui::test]
fn vim_insert(cx: &mut TestAppContext) {
    let (dir, a, b) = project();
    let (workbench, cx) = open(cx, dir.path(), settings(Setting::AfterDelay));
    cx.update(|_, cx| fe_gpui::set_vim_enabled(cx, true));
    let editor = open_file(&workbench, &a, cx);

    // After a delay: nothing saves mid-insert, however long.
    keys("i a b c", cx);
    wait(5000, cx);
    assert_eq!(on_disk(&a), "one\n");
    assert_eq!(saves(&workbench, cx), 0);
    keys("escape", cx);
    wait(999, cx);
    assert_eq!(on_disk(&a), "one\n");
    wait(1, cx);
    assert_eq!(on_disk(&a), "abcone\n");

    // The insert still repeats whole, and undoes as one step.
    keys(".", cx);
    assert_eq!(text(&editor, cx).len(), "abcabcone\n".len());
    keys("u", cx);
    assert_eq!(text(&editor, cx), "abcone\n");

    // On insert leave: Esc saves, and so does a normal-mode change.
    let mut event_settings = fe_config::Settings::default();
    event_settings.editor.auto_save = Setting::OnInsertLeave;
    reload(
        &workbench,
        SettingsEvent {
            settings: std::sync::Arc::new(event_settings),
            previous: std::sync::Arc::new(fe_config::Settings::default()),
            diagnostics: Vec::new(),
        },
        cx,
    );
    let editor = open_file(&workbench, &b, cx);
    let before = saves(&workbench, cx);
    keys("i x", cx);
    wait(5000, cx);
    assert_eq!(on_disk(&b), "two\n");
    keys("escape", cx);
    assert_eq!(on_disk(&b), "xtwo\n");
    assert!(!dirty(&editor, cx));
    keys("d d", cx);
    assert_eq!(text(&editor, cx), "");
    assert_eq!(on_disk(&b), "");
    assert_eq!(saves(&workbench, cx), before + 2);

    // A normal-mode change with no insert before it saves too.
    let c = dir.path().join("c.txt");
    fs::write(&c, "three\nfour\n").unwrap();
    open_file(&workbench, &c, cx);
    keys("d d", cx);
    assert_eq!(on_disk(&c), "four\n");
    assert_eq!(saves(&workbench, cx), before + 3);
}

#[gpui::test]
fn conflict_held(cx: &mut TestAppContext) {
    let (dir, a, _) = project();
    let (workbench, cx) = open(cx, dir.path(), settings(Setting::AfterDelay));
    let editor = open_file(&workbench, &a, cx);
    let skipped = (
        Level::Warning,
        "Auto save skipped: a.txt changed on disk".to_owned(),
        1,
    );

    type_text("mine ", cx);
    rewrite(&a, "theirs\n");
    wait(1000, cx);
    assert_eq!(on_disk(&a), "theirs\n");
    assert!(dirty(&editor, cx));
    assert!(cx.pending_prompt().is_none());
    assert_eq!(toasts(&workbench, cx), std::slice::from_ref(&skipped));
    assert_eq!(saves(&workbench, cx), 1);

    // Held: more edits, and time, try nothing more.
    type_text("more ", cx);
    wait(5000, cx);
    assert_eq!(on_disk(&a), "theirs\n");
    assert_eq!(saves(&workbench, cx), 1);
    assert_eq!(toasts(&workbench, cx), [skipped]);

    // cmd-s asks, and overwriting lets auto-save go on.
    keys("cmd-s", cx);
    assert!(cx.pending_prompt().is_some());
    cx.simulate_prompt_answer("Overwrite");
    draw(cx);
    assert_eq!(on_disk(&a), "mine more one\n");
    type_text("x", cx);
    wait(1000, cx);
    assert_eq!(on_disk(&a), "mine more xone\n");
    assert_eq!(saves(&workbench, cx), 2);

    // Held again, then reloaded: auto-save goes on.
    type_text("y", cx);
    rewrite(&a, "theirs\n");
    wait(1000, cx);
    type_text("z", cx);
    wait(5000, cx);
    assert_eq!(saves(&workbench, cx), 3);
    assert_eq!(on_disk(&a), "theirs\n");
    workbench.update_in(cx, |workbench, window, cx| {
        workbench.check_open_files(window, cx)
    });
    draw(cx);
    cx.simulate_prompt_answer("Reload");
    draw(cx);
    assert_eq!(text(&editor, cx), "theirs\n");
    assert!(!dirty(&editor, cx));
    type_text("w", cx);
    wait(1000, cx);
    assert_eq!(on_disk(&a), "wtheirs\n");
    assert_eq!(saves(&workbench, cx), 4);
}

#[gpui::test]
fn setting_reload(cx: &mut TestAppContext) {
    let (dir, a, _) = project();
    let user = dir.path().join("settings.toml");
    fs::write(&user, "[editor]\nauto_save = \"off\"\n").unwrap();
    let mut store = SettingsStore::load(Some(user.clone()), None);
    let editor_settings = store.settings().editor.clone();
    let (workbench, cx) = open(cx, dir.path(), editor_settings);
    let editor = open_file(&workbench, &a, cx);

    type_text("x", cx);
    wait(5000, cx);
    assert_eq!(on_disk(&a), "one\n");

    fs::write(
        &user,
        "[editor]\nauto_save = \"after_delay\"\nauto_save_delay = 200\n",
    )
    .unwrap();
    let event = store.reload(std::slice::from_ref(&user)).expect("changed");
    reload(&workbench, event, cx);
    // The edit made while off is past its delay already.
    assert_eq!(on_disk(&a), "xone\n");
    type_text("y", cx);
    wait(199, cx);
    assert_eq!(on_disk(&a), "xone\n");
    wait(1, cx);
    assert_eq!(on_disk(&a), "xyone\n");
    assert!(!dirty(&editor, cx));

    fs::write(&user, "[editor]\nauto_save = \"off\"\n").unwrap();
    let event = store.reload(std::slice::from_ref(&user)).expect("changed");
    reload(&workbench, event, cx);
    type_text("z", cx);
    wait(5000, cx);
    assert_eq!(on_disk(&a), "xyone\n");
    assert!(dirty(&editor, cx));
}

/// T518's carried finding: an open panel that fails shows an error toast,
/// on every platform.
#[gpui::test]
fn open_panel_failure_toasts(cx: &mut TestAppContext) {
    let (dir, _, _) = project();
    let (workbench, cx) = open(cx, dir.path(), settings(Setting::Off));
    crate::workbench::OPEN_PANEL_FAILS.set(Some("no portal"));
    let run = RunCommand(
        Invocation::of("workbench.open_folder")
            .unwrap()
            .context(ContextStack::new().with(ContextFrame::new("workbench"))),
    );
    cx.update(|window, cx| window.dispatch_action(Box::new(run), cx));
    draw(cx);
    assert!(cx.pending_prompt().is_none());
    let shown = toasts(&workbench, cx);
    assert_eq!(shown.len(), 1, "{shown:?}");
    assert_eq!(shown[0].0, Level::Error);
}

/// A save hook that holds each save until [`Gate::open`], then types
/// `fixed ` where the save allows edits, as format on save would.
#[derive(Default)]
struct Gate(std::cell::RefCell<Vec<futures::channel::oneshot::Sender<()>>>);

impl Gate {
    /// Lets the saves waiting go on.
    fn open(&self) {
        for waiting in self.0.borrow_mut().drain(..) {
            waiting.send(()).ok();
        }
    }
}

impl fe_gpui::SaveHook for Gate {
    fn before_save(&self, editor: &Entity<EditorView>, cx: &mut gpui::App) -> gpui::Task<()> {
        let (go, wait) = futures::channel::oneshot::channel();
        self.0.borrow_mut().push(go);
        let editor = editor.clone();
        cx.spawn(async move |cx| {
            wait.await.ok();
            editor.update(cx, |editor, cx| {
                if editor.save_edits_allowed(cx) {
                    editor.model_mut().insert("fixed ");
                    cx.notify();
                }
            });
        })
    }
}

/// Types `typed` into `editor`, then has focus leave it, without drawing a
/// frame: a frame's LSP hook would take [`Gate`] off the editor.
fn type_and_leave(
    workbench: &Entity<Workbench>,
    editor: &Entity<EditorView>,
    typed: &str,
    cx: &mut VisualTestContext,
) {
    workbench.update(cx, |workbench, cx| {
        editor.update(cx, |editor, _| editor.model_mut().insert(typed));
        workbench.auto_save_focus_lost(editor.entity_id(), cx);
    });
    cx.run_until_parked();
}

#[gpui::test]
fn one_save_at_a_time(cx: &mut TestAppContext) {
    let (dir, a, _) = project();
    let (workbench, cx) = open(cx, dir.path(), settings(Setting::OnFocusChange));
    let editor = open_file(&workbench, &a, cx);
    let gate = std::rc::Rc::new(Gate::default());
    let hook: std::rc::Rc<dyn fe_gpui::SaveHook> = gate.clone();
    let set_hook = |cx: &mut VisualTestContext| {
        editor.update(cx, |editor, _| editor.set_save_hook(Some(hook.clone())))
    };
    set_hook(cx);
    let waiting = |cx: &mut VisualTestContext| {
        cx.run_until_parked();
        gate.0.borrow().len()
    };

    // A save held by its hook; an edit and a focus change meanwhile wait
    // for it, and the editor is asked for one save.
    type_and_leave(&workbench, &editor, "x", cx);
    assert_eq!((saves(&workbench, cx), waiting(cx)), (1, 1));
    type_and_leave(&workbench, &editor, "y", cx);
    assert_eq!((saves(&workbench, cx), waiting(cx)), (1, 1));
    // The hook's edit goes in the save, which takes the text as it is
    // then: nothing is left to save.
    gate.open();
    cx.run_until_parked();
    assert_eq!(text(&editor, cx), "xyfixed one\n");
    assert_eq!(on_disk(&a), "xyfixed one\n");
    assert!(!dirty(&editor, cx));
    assert_eq!((saves(&workbench, cx), waiting(cx)), (1, 0));

    // Refused for the file changing on disk, with another due behind it:
    // one toast, and no further attempt.
    set_hook(cx);
    type_and_leave(&workbench, &editor, "v", cx);
    rewrite(&a, "theirs\n");
    type_and_leave(&workbench, &editor, "u", cx);
    assert_eq!((saves(&workbench, cx), waiting(cx)), (2, 1));
    gate.open();
    cx.run_until_parked();
    assert_eq!(on_disk(&a), "theirs\n");
    assert!(dirty(&editor, cx));
    type_and_leave(&workbench, &editor, "t", cx);
    assert_eq!((saves(&workbench, cx), waiting(cx)), (2, 0));
    let skipped = (
        Level::Warning,
        "Auto save skipped: a.txt changed on disk".to_owned(),
        1,
    );
    assert_eq!(toasts(&workbench, cx), [skipped]);
}

#[gpui::test]
fn dirty_when_first_seen(cx: &mut TestAppContext) {
    let (dir, a, _) = project();
    let (workbench, cx) = open(cx, dir.path(), settings(Setting::AfterDelay));
    // As a buffer restored from its backup opens.
    let mut model = EditorModel::open(&a).unwrap();
    model.insert("restored ");
    let pane = workbench.read_with(cx, |workbench, _| workbench.pane().clone());
    cx.update(|window, cx| {
        pane.update(cx, |pane, cx| {
            pane.add(model, cx);
            pane.activate(0, window, cx);
        })
    });
    draw(cx);
    wait(999, cx);
    assert_eq!(on_disk(&a), "one\n");
    wait(1, cx);
    assert_eq!(on_disk(&a), "restored one\n");
}

#[gpui::test]
fn facts_without_edits(cx: &mut TestAppContext) {
    let (dir, a, _) = project();
    let (workbench, cx) = open(cx, dir.path(), settings(Setting::AfterDelay));
    cx.update(|_, cx| fe_gpui::set_vim_enabled(cx, true));
    let editor = open_file(&workbench, &a, cx);

    // A normal-mode change, then insert mode with nothing typed: nothing
    // saves until it's left.
    keys("x", cx);
    keys("i", cx);
    wait(5000, cx);
    assert_eq!(on_disk(&a), "one\n");
    keys("escape", cx);
    wait(1000, cx);
    assert_eq!(on_disk(&a), "ne\n");

    // Made read-only while dirty: it doesn't save until writable again.
    keys("x", cx);
    editor.update(cx, |editor, cx| editor.set_read_only(true, cx));
    wait(5000, cx);
    assert_eq!(on_disk(&a), "ne\n");
    editor.update(cx, |editor, cx| editor.set_read_only(false, cx));
    wait(1000, cx);
    assert_eq!(on_disk(&a), "e\n");
}

#[gpui::test]
fn insert_in_splits(cx: &mut TestAppContext) {
    let (dir, a, _) = project();
    let (workbench, cx) = open(cx, dir.path(), settings(Setting::AfterDelay));
    cx.update(|_, cx| fe_gpui::set_vim_enabled(cx, true));
    open_file(&workbench, &a, cx);
    cx.simulate_keystrokes("cmd-\\");
    draw(cx);
    let [left, right] = workbench
        .read_with(cx, |workbench, cx| {
            <[_; 2]>::try_from(workbench.open_editors(cx))
        })
        .expect("two editors");

    // Both in insert mode: leaving it in one doesn't save.
    focus(&right, cx);
    keys("i", cx);
    focus(&left, cx);
    keys("i a escape", cx);
    wait(5000, cx);
    assert_eq!(on_disk(&a), "one\n");
    focus(&right, cx);
    keys("escape", cx);
    wait(1000, cx);
    assert_eq!(on_disk(&a), "aone\n");
}

#[gpui::test]
fn vim_turned_off(cx: &mut TestAppContext) {
    let (dir, a, b) = project();
    let (workbench, cx) = open(cx, dir.path(), settings(Setting::OnInsertLeave));
    cx.update(|_, cx| fe_gpui::set_vim_enabled(cx, true));
    open_file(&workbench, &a, cx);
    let editor = open_file(&workbench, &b, cx);
    keys("i x escape", cx);
    assert_eq!(on_disk(&b), "xtwo\n");

    // Vim off: as on focus change.
    cx.update(|_, cx| fe_gpui::set_vim_enabled(cx, false));
    draw(cx);
    type_text("y", cx);
    wait(5000, cx);
    assert!(dirty(&editor, cx));
    assert_eq!(on_disk(&b), "xtwo\n");
    show_tab(&workbench, 0, cx);
    assert_eq!(on_disk(&b), text(&editor, cx));
    assert!(!dirty(&editor, cx));

    // Turned off mid-insert, or mid-replace: the same.
    for mode in ["i", "shift-r"] {
        show_tab(&workbench, 1, cx);
        cx.update(|_, cx| fe_gpui::set_vim_enabled(cx, true));
        draw(cx);
        keys(&format!("{mode} q"), cx);
        cx.update(|_, cx| fe_gpui::set_vim_enabled(cx, false));
        draw(cx);
        assert_eq!(editor.read_with(cx, |editor, _| editor.vim_mode()), None);
        let saved = on_disk(&b);
        type_text("y", cx);
        wait(5000, cx);
        assert!(dirty(&editor, cx), "{mode}");
        assert_eq!(on_disk(&b), saved, "{mode}");
        show_tab(&workbench, 0, cx);
        assert_eq!(on_disk(&b), text(&editor, cx), "{mode}");
    }
}

#[gpui::test]
fn turned_off_mid_save(cx: &mut TestAppContext) {
    let (dir, a, _) = project();
    let (workbench, cx) = open(cx, dir.path(), settings(Setting::OnFocusChange));
    let editor = open_file(&workbench, &a, cx);

    // A save whose text is taken, its write still to run.
    workbench.update(cx, |workbench, cx| {
        editor.update(cx, |editor, _| editor.model_mut().insert("x"));
        workbench.auto_save_focus_lost(editor.entity_id(), cx);
    });
    while !editor.read_with(cx, |editor, _| editor.is_saving()) {
        assert!(cx.executor().tick(), "the save never started");
    }
    // Typed, and due again, meanwhile; then auto-save goes off.
    workbench.update(cx, |workbench, cx| {
        editor.update(cx, |editor, _| editor.model_mut().insert("y"));
        workbench.auto_save_focus_lost(editor.entity_id(), cx);
    });
    let mut off = fe_config::Settings::default();
    off.editor.auto_save = Setting::Off;
    let event = SettingsEvent {
        settings: std::sync::Arc::new(off),
        previous: std::sync::Arc::new(fe_config::Settings::default()),
        diagnostics: Vec::new(),
    };
    workbench.update(cx, |workbench, cx| workbench.reload_settings(&event, cx));
    cx.run_until_parked();
    assert_eq!(on_disk(&a), "xone\n");
    assert!(dirty(&editor, cx));
    assert_eq!(saves(&workbench, cx), 1);
    wait(5000, cx);
    assert_eq!(saves(&workbench, cx), 1);
}

#[path = "auto_save_split_insert_tests.rs"]
mod split_insert;
