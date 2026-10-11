//! Config problems in the workbench: the status item following kiln's
//! [`ConfigDiagnostics`] in every window, the list it opens, a broken
//! settings file through kiln's config service, and picks that must not
//! open what they shouldn't or where they shouldn't.

use std::{
    fs,
    path::{Path, PathBuf},
};

use fe_command::{ContextFrame, ContextStack, Invocation};
use fe_config::{Diagnostic, Settings, Severity};
use fe_gpui::{EditorSettings, EditorTheme, commands::RunCommand};
use gpui::{Entity, Focusable as _, Modifiers, MouseButton, TestAppContext, VisualTestContext};
use kiln_platform::config::{self, ConfigDiagnostics, ConfigService};

use super::ITEM;
use crate::{Launch, StatusSide, Workbench};

/// A workbench rooted at `root` in a window of its own, focused, drawn.
fn open(root: &Path, cx: &mut TestAppContext) -> (Entity<Workbench>, VisualTestContext) {
    let launch = Launch {
        root: Some(root.to_owned()),
        file: None,
    };
    let window = cx.add_window(move |window, cx| {
        let theme = EditorTheme::default_dark(cx);
        Workbench::new(launch, theme, EditorSettings::default(), window, cx)
    });
    let workbench = window.root(cx).unwrap();
    let mut cx = VisualTestContext::from_window(*window, cx);
    cx.update(|window, cx| window.focus(&workbench.focus_handle(cx), cx));
    draw(&mut cx);
    (workbench, cx)
}

fn init(cx: &mut TestAppContext) {
    cx.update(|cx| {
        fe_gpui::init(cx);
        crate::init(cx);
    });
}

/// Lets background work land, then draws a frame.
fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
}

fn problem(
    severity: Severity,
    file: &Path,
    line: usize,
    column: usize,
    message: &str,
) -> Diagnostic {
    Diagnostic {
        severity,
        file: file.to_owned(),
        key: None,
        line,
        column,
        message: message.to_owned(),
    }
}

fn set_problems(problems: Vec<Diagnostic>, cx: &mut TestAppContext) {
    cx.update(|cx| cx.set_global(ConfigDiagnostics(problems)));
}

/// The config problems item's text, if the last frame drew it.
fn item(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Option<String> {
    let bar = workbench.read_with(cx, |workbench, _| workbench.status_bar().clone());
    let left = bar.read_with(cx, |bar, _| bar.drawn(StatusSide::Left));
    let drawn = cx.debug_bounds("status-item:config.problems").is_some();
    left.into_iter()
        .find(|(id, _)| id == ITEM)
        .filter(|_| drawn)
        .map(|(_, text)| text.to_string())
}

fn dispatch(id: &str, cx: &mut VisualTestContext) {
    let run = RunCommand(
        Invocation::of(id)
            .unwrap()
            .context(ContextStack::new().with(ContextFrame::new("workbench"))),
    );
    cx.update(|window, cx| window.dispatch_action(Box::new(run), cx));
    draw(cx);
}

fn list_open(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> bool {
    workbench.read_with(cx, |workbench, _| workbench.config_problems_open())
}

fn labels(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Vec<(String, String)> {
    let picker = workbench.read_with(cx, |workbench, _| workbench.config_problems().clone());
    picker.read_with(cx, |picker, _| picker.delegate.labels())
}

/// The active editor's file, and its caret's byte offset.
fn caret(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> (PathBuf, usize) {
    workbench.read_with(cx, |workbench, cx| {
        let editor = workbench.active_editor(cx).expect("a file is shown");
        let model = editor.read(cx).model();
        let path = model.path().expect("a file").to_owned();
        (path, model.primary_selection().range.start)
    })
}

/// The status bar's `Ln, Col`, the column in chars.
fn cursor(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Option<String> {
    let bar = workbench.read_with(cx, |workbench, _| workbench.status_bar().clone());
    let right = bar.read_with(cx, |bar, _| bar.drawn(StatusSide::Right));
    right
        .into_iter()
        .find(|(id, _)| id == crate::StatusItem::CURSOR)
        .map(|(_, text)| text.to_string())
}

#[gpui::test]
fn status_item_follows_the_diagnostics(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let settings = dir.path().join("settings.toml");
    let keymap = dir.path().join("keymap.toml");
    init(cx);
    let (first, mut first_cx) = open(dir.path(), cx);
    let (second, mut second_cx) = open(dir.path(), cx);
    assert_eq!(item(&first, &mut first_cx), None);
    assert_eq!(item(&second, &mut second_cx), None);

    set_problems(
        vec![
            problem(Severity::Error, &settings, 1, 1, "expected a number"),
            problem(Severity::Warning, &settings, 2, 1, "unknown key"),
            problem(Severity::Error, &keymap, 3, 5, "unknown command"),
        ],
        cx,
    );
    draw(&mut first_cx);
    draw(&mut second_cx);
    assert_eq!(
        item(&first, &mut first_cx).as_deref(),
        Some("3 config problems")
    );
    assert_eq!(
        item(&second, &mut second_cx).as_deref(),
        Some("3 config problems")
    );

    set_problems(
        vec![problem(Severity::Warning, &settings, 2, 1, "unknown key")],
        cx,
    );
    draw(&mut first_cx);
    draw(&mut second_cx);
    assert_eq!(
        item(&first, &mut first_cx).as_deref(),
        Some("1 config problem")
    );
    assert_eq!(
        item(&second, &mut second_cx).as_deref(),
        Some("1 config problem")
    );

    set_problems(Vec::new(), cx);
    draw(&mut first_cx);
    draw(&mut second_cx);
    assert_eq!(item(&first, &mut first_cx), None);
    assert_eq!(item(&second, &mut second_cx), None);

    // A window opened while there are problems shows them at once.
    set_problems(
        vec![problem(Severity::Error, &keymap, 3, 5, "unknown command")],
        cx,
    );
    let (third, mut third_cx) = open(dir.path(), cx);
    assert_eq!(
        item(&third, &mut third_cx).as_deref(),
        Some("1 config problem")
    );
}

#[gpui::test]
fn list_opens_at_the_problem(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let settings = dir.path().join("settings.toml");
    let keymap = dir.path().join("keymap.toml");
    // Line 2's `junk` is char 12, byte 13: each `é` is two bytes.
    let text = "a = 1\nkey = \"éé\" junk\n";
    fs::write(&settings, text).unwrap();
    fs::write(&keymap, "[[bindings]]\nkey = \"cmd-k\"\nnot = a_command\n").unwrap();
    init(cx);
    let (workbench, mut cx_) = open(dir.path(), cx);
    let cx_ = &mut cx_;

    // With none, the command does nothing.
    dispatch(super::SHOW, cx_);
    assert!(!list_open(&workbench, cx_));

    set_problems(
        vec![
            problem(Severity::Warning, &settings, 1, 1, "unknown key `a`"),
            problem(Severity::Error, &keymap, 3, 7, "expected a string"),
            problem(Severity::Error, &settings, 2, 12, "invalid value"),
        ],
        cx,
    );
    draw(cx_);

    // A click on the item opens the list: errors first, each in file order.
    let bounds = cx_
        .debug_bounds("status-item:config.problems")
        .expect("drawn");
    cx_.simulate_click(bounds.center(), Modifiers::none());
    draw(cx_);
    assert!(list_open(&workbench, cx_));
    let row = |message: &str, detail: &str| (message.to_owned(), detail.to_owned());
    assert_eq!(
        labels(&workbench, cx_),
        [
            row("expected a string", "keymap.toml:3:7 · error"),
            row("invalid value", "settings.toml:2:12 · error"),
            row("unknown key `a`", "settings.toml:1:1 · warning"),
        ]
    );
    cx_.simulate_keystrokes("escape");
    draw(cx_);
    assert!(!list_open(&workbench, cx_));

    // The command opens it too; typing filters by file name and message.
    dispatch(super::SHOW, cx_);
    assert!(list_open(&workbench, cx_));
    cx_.simulate_input("settings");
    draw(cx_);
    assert_eq!(
        labels(&workbench, cx_),
        [
            row("invalid value", "settings.toml:2:12 · error"),
            row("unknown key `a`", "settings.toml:1:1 · warning"),
        ]
    );
    cx_.simulate_keystrokes("escape");
    draw(cx_);
    dispatch(super::SHOW, cx_);
    cx_.simulate_input("INVALID");
    draw(cx_);
    assert_eq!(
        labels(&workbench, cx_),
        [row("invalid value", "settings.toml:2:12 · error")]
    );

    // Confirming opens the file, the caret at line 2, char 12.
    cx_.simulate_keystrokes("enter");
    draw(cx_);
    assert!(!list_open(&workbench, cx_));
    let junk = text.find("junk").unwrap();
    assert_eq!(junk, 6 + 13);
    assert_eq!(caret(&workbench, cx_), (settings.clone(), junk));
    assert_eq!(cursor(&workbench, cx_).as_deref(), Some("Ln 2, Col 12"));
    let focused = cx_.update(|window, cx| {
        let editor = workbench.read(cx).active_editor(cx).unwrap().clone();
        editor.focus_handle(cx).is_focused(window)
    });
    assert!(focused);

    // Its file open, another row moves the caret there.
    dispatch(super::SHOW, cx_);
    cx_.simulate_input("unknown");
    draw(cx_);
    cx_.simulate_keystrokes("enter");
    draw(cx_);
    assert_eq!(caret(&workbench, cx_), (settings.clone(), 0));
    assert_eq!(cursor(&workbench, cx_).as_deref(), Some("Ln 1, Col 1"));

    // The keymap's row opens it in the same pane.
    dispatch(super::SHOW, cx_);
    cx_.simulate_keystrokes("enter");
    draw(cx_);
    assert_eq!(cursor(&workbench, cx_).as_deref(), Some("Ln 3, Col 7"));
    assert_eq!(caret(&workbench, cx_).0, keymap);

    // The problems going takes the open list with them.
    dispatch(super::SHOW, cx_);
    assert!(list_open(&workbench, cx_));
    set_problems(Vec::new(), cx);
    draw(cx_);
    assert!(!list_open(&workbench, cx_));
    assert_eq!(item(&workbench, cx_), None);
}

#[gpui::test]
fn a_broken_settings_file_shows_and_clears(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let user = dir.path().join("config").join("settings.toml");
    fs::create_dir_all(user.parent().unwrap()).unwrap();
    fs::write(&user, "[editor]\ntab_size = 2\n").unwrap();
    init(cx);
    let store = fe_config::SettingsStore::load(Some(user.clone()), None);
    let (sender, batches) = config::batch_channel();
    let _service = cx.update(|cx| {
        ConfigService::new()
            .settings(store.into_inner())
            .start_with(batches, cx)
    });
    let tab_size =
        |cx: &mut TestAppContext| cx.update(|cx| config::settings::<Settings>(cx).editor.tab_size);
    let (workbench, mut cx_) = open(dir.path(), cx);
    assert_eq!(item(&workbench, &mut cx_), None);
    assert_eq!(tab_size(cx), 2);

    fs::write(&user, "[editor]\ntab_size = \"wide\"\n").unwrap();
    sender.send(vec![user.clone()]);
    draw(&mut cx_);
    assert_eq!(
        item(&workbench, &mut cx_).as_deref(),
        Some("1 config problem")
    );
    // The last good settings stay.
    assert_eq!(tab_size(cx), 2);
    // The list names the file and the bad value's place.
    dispatch(super::SHOW, &mut cx_);
    let rows = labels(&workbench, &mut cx_);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].1, "settings.toml:2:12 · error");

    fs::write(&user, "[editor]\ntab_size = 4\n").unwrap();
    sender.send(vec![user.clone()]);
    draw(&mut cx_);
    assert_eq!(item(&workbench, &mut cx_), None);
    assert!(!list_open(&workbench, &mut cx_));
    assert_eq!(tab_size(cx), 4);
}

#[test]
fn the_command_is_in_the_catalog_and_the_palette() {
    let command = fe_command::CommandId::parse(super::SHOW).unwrap();
    let catalog = crate::command_catalog();
    assert_eq!(catalog.check(&command, &fe_command::Value::Null), Ok(()));
    let listed = crate::command_list();
    let listed = listed
        .iter()
        .find(|meta| meta.id == command)
        .expect("listed");
    assert_eq!(listed.title, "Show Config Problems");
}

/// The active editor's file, if a file shows.
fn shown(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Option<PathBuf> {
    workbench.read_with(cx, |workbench, cx| {
        let editor = workbench.active_editor(cx)?;
        editor.read(cx).model().path().map(Path::to_owned)
    })
}

/// Confirms match `ix` of the list that shows, without letting anything
/// land.
fn confirm(workbench: &Entity<Workbench>, ix: usize, cx: &mut VisualTestContext) {
    let picker = workbench.read_with(cx, |workbench, _| workbench.config_problems().clone());
    cx.update(|window, cx| picker.update(cx, |picker, cx| picker.confirm(ix, window, cx)));
}

/// Makes a FIFO at `path`.
fn mkfifo(path: &Path) {
    let name = std::ffi::CString::new(path.as_os_str().as_encoded_bytes()).unwrap();
    // SAFETY: `name` is a NUL-terminated path.
    assert_eq!(unsafe { libc::mkfifo(name.as_ptr(), 0o600) }, 0);
}

#[gpui::test]
fn a_file_that_cant_be_opened_says_why(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    // A FIFO would block the read, /dev/zero never end it, and a file over
    // kiln's limit is one its config service rejects too.
    let fifo = dir.path().join("fifo.toml");
    mkfifo(&fifo);
    let zero = dir.path().join("zero.toml");
    std::os::unix::fs::symlink("/dev/zero", &zero).unwrap();
    let big = dir.path().join("big.toml");
    fs::write(&big, vec![b'a'; 2 << 20]).unwrap();
    init(cx);
    let (workbench, mut cx_) = open(dir.path(), cx);
    let cx_ = &mut cx_;

    for path in [&fifo, &zero, &big] {
        set_problems(vec![problem(Severity::Error, path, 1, 3, "bad")], cx);
        dispatch(super::SHOW, cx_);
        cx_.simulate_keystrokes("enter");
        draw(cx_);
        assert!(cx_.has_pending_prompt(), "{}", path.display());
        cx_.simulate_prompt_answer("OK");
        draw(cx_);
        assert_eq!(shown(&workbench, cx_), None, "{}", path.display());
    }
}

#[gpui::test]
fn a_later_open_wins_over_a_pick_still_checking(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let settings = dir.path().join("settings.toml");
    let keymap = dir.path().join("keymap.toml");
    let other = dir.path().join("other.txt");
    fs::write(&settings, "a = 1\n").unwrap();
    fs::write(&keymap, "b = 2\n").unwrap();
    fs::write(&other, "other\n").unwrap();
    init(cx);
    let (workbench, mut cx_) = open(dir.path(), cx);
    let cx_ = &mut cx_;
    set_problems(
        vec![
            problem(Severity::Error, &keymap, 1, 1, "keymap"),
            problem(Severity::Error, &settings, 1, 3, "settings"),
        ],
        cx,
    );
    draw(cx_);
    let pane = workbench.read_with(cx_, |workbench, _| workbench.pane().clone());

    // A file opened while the pick's check runs stays.
    dispatch(super::SHOW, cx_);
    confirm(&workbench, 1, cx_);
    cx_.update(|window, cx| pane.update(cx, |pane, cx| pane.open(other.clone(), window, cx)));
    draw(cx_);
    assert_eq!(shown(&workbench, cx_), Some(other.clone()));
    let tabs = pane.read_with(cx_, |pane, _| pane.tabs().len());
    assert_eq!(tabs, 1, "the settings file never opened");

    // Of two picks, the later lands, whichever check ends first.
    dispatch(super::SHOW, cx_);
    confirm(&workbench, 1, cx_);
    confirm(&workbench, 0, cx_);
    draw(cx_);
    assert_eq!(shown(&workbench, cx_), Some(keymap.clone()));
    let tabs = pane.read_with(cx_, |pane, _| pane.tabs().len());
    assert_eq!(tabs, 2);
}

#[gpui::test]
fn the_column_is_the_editors_own(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let settings = dir.path().join("settings.toml");
    let keymap = dir.path().join("keymap.toml");
    fs::write(&settings, "a = 1\nkey = \"éé\" junk\n").unwrap();
    fs::write(&keymap, "b = 2\n").unwrap();
    init(cx);
    let (workbench, mut cx_) = open(dir.path(), cx);
    let cx_ = &mut cx_;
    let pane = workbench.read_with(cx_, |workbench, _| workbench.pane().clone());
    let open_file = |path: &Path, cx: &mut VisualTestContext| {
        let path = path.to_owned();
        cx.update(|window, cx| pane.update(cx, |pane, cx| pane.open(path, window, cx)));
        draw(cx);
    };

    // The `éé` made `ee` and not saved: on disk `junk` is still byte 13 of
    // its line, in the editor byte 11; char 12 in both.
    open_file(&settings, cx_);
    let editor = workbench.read_with(cx_, |workbench, cx| workbench.active_editor(cx).cloned());
    editor.unwrap().update(cx_, |editor, cx| {
        let start = editor.model().text().to_string().find("éé").unwrap();
        editor.model_mut().select_range(start..start + 4, false);
        editor.model_mut().insert("ee");
        cx.notify();
    });
    set_problems(
        vec![problem(Severity::Error, &settings, 2, 12, "invalid value")],
        cx,
    );
    // Its tab open: the caret moves at once.
    dispatch(super::SHOW, cx_);
    cx_.simulate_keystrokes("enter");
    draw(cx_);
    assert_eq!(caret(&workbench, cx_), (settings.clone(), 6 + 11));
    assert_eq!(cursor(&workbench, cx_).as_deref(), Some("Ln 2, Col 12"));

    // In a new pane, which shares the edited buffer from the first.
    open_file(&keymap, cx_);
    dispatch("pane.split_right", cx_);
    let split = workbench.read_with(cx_, |workbench, _| workbench.pane().clone());
    assert_ne!(split, pane);
    dispatch(super::SHOW, cx_);
    cx_.simulate_keystrokes("enter");
    draw(cx_);
    assert_eq!(caret(&workbench, cx_), (settings.clone(), 6 + 11));
    assert_eq!(cursor(&workbench, cx_).as_deref(), Some("Ln 2, Col 12"));
}

#[gpui::test]
fn a_row_replaced_mid_click_isnt_picked(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let settings = dir.path().join("settings.toml");
    let keymap = dir.path().join("keymap.toml");
    fs::write(&settings, "a = 1\n").unwrap();
    fs::write(&keymap, "b = 2\n").unwrap();
    init(cx);
    let (workbench, mut cx_) = open(dir.path(), cx);
    let cx_ = &mut cx_;
    let settings_error = problem(Severity::Error, &settings, 1, 1, "settings");
    let keymap_error = problem(Severity::Error, &keymap, 1, 1, "keymap");
    set_problems(vec![settings_error, keymap_error.clone()], cx);
    dispatch(super::SHOW, cx_);
    let picker = workbench.read_with(cx_, |workbench, _| workbench.config_problems().clone());
    let row = picker.read_with(cx_, |picker, _| picker.row_bounds(0).expect("drawn"));
    // Near its start, as a row is only as wide as its text.
    let at = row.origin + gpui::point(gpui::px(4.), row.size.height / 2.);

    // Pressed on the settings error; the keymap error takes its place
    // before the release.
    cx_.simulate_mouse_down(at, MouseButton::Left, Modifiers::none());
    set_problems(vec![keymap_error], cx);
    draw(cx_);
    assert_eq!(
        labels(&workbench, cx_),
        [("keymap".to_owned(), "keymap.toml:1:1 · error".to_owned())]
    );
    cx_.simulate_mouse_up(at, MouseButton::Left, Modifiers::none());
    draw(cx_);
    assert_eq!(shown(&workbench, cx_), None);
    assert!(list_open(&workbench, cx_));
}

/// Picks the only problem, at `path`, and lets `meanwhile` run before the
/// pick's read does; then lets it land, and answers the warning it should
/// show.
fn pick_refused(
    path: &Path,
    meanwhile: impl FnOnce(),
    workbench: &Entity<Workbench>,
    cx: &mut TestAppContext,
    cx_: &mut VisualTestContext,
) {
    set_problems(vec![problem(Severity::Error, path, 1, 1, "bad")], cx);
    dispatch(super::SHOW, cx_);
    confirm(workbench, 0, cx_);
    meanwhile();
    draw(cx_);
    assert!(cx_.has_pending_prompt(), "{}", path.display());
    cx_.simulate_prompt_answer("OK");
    draw(cx_);
    assert_eq!(shown(workbench, cx_), None, "{}", path.display());
}

#[gpui::test]
fn the_file_read_is_the_file_checked(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let settings = dir.path().join("settings.toml");
    fs::write(&settings, "a = 1\n").unwrap();
    init(cx);
    let (workbench, mut cx_) = open(dir.path(), cx);
    let cx_ = &mut cx_;

    // A FIFO swapped in after the pick, before its read.
    let swap = || {
        fs::remove_file(&settings).unwrap();
        mkfifo(&settings);
    };
    pick_refused(&settings, swap, &workbench, cx, cx_);

    // A regular file whose size says 0, with more than kiln reads behind it.
    #[cfg(target_os = "linux")]
    {
        let kallsyms = Path::new("/proc/kallsyms");
        let meta = fs::metadata(kallsyms).unwrap();
        assert_eq!((meta.is_file(), meta.len()), (true, 0));
        assert!(fs::read(kallsyms).unwrap().len() > 1 << 20);
        let link = dir.path().join("kallsyms.toml");
        std::os::unix::fs::symlink(kallsyms, &link).unwrap();
        pick_refused(&link, || {}, &workbench, cx, cx_);
    }
}

#[gpui::test]
fn an_older_read_landing_doesnt_cancel_a_pick(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let settings = dir.path().join("settings.toml");
    let other = dir.path().join("other.txt");
    fs::write(&settings, "a = 1\nb = 2\n").unwrap();
    init(cx);
    let (workbench, mut cx_) = open(dir.path(), cx);
    let cx_ = &mut cx_;
    set_problems(vec![problem(Severity::Error, &settings, 2, 3, "bad")], cx);
    draw(cx_);
    let pane = workbench.read_with(cx_, |workbench, _| workbench.pane().clone());

    // `other` is being read when the pick is made, and lands before it.
    cx_.update(|_, cx| pane.update(cx, |pane, cx| pane.opening(other.clone(), cx)));
    dispatch(super::SHOW, cx_);
    confirm(&workbench, 0, cx_);
    cx_.update(|window, cx| {
        let read = Ok(Some(fe_buffer::Buffer::new("other\n")));
        pane.update(cx, |pane, cx| {
            pane.file_read(other.clone(), read, window, cx)
        });
    });
    assert_eq!(shown(&workbench, cx_), Some(other.clone()));
    draw(cx_);
    assert_eq!(shown(&workbench, cx_), Some(settings.clone()));
    assert_eq!(cursor(&workbench, cx_).as_deref(), Some("Ln 2, Col 3"));
}

#[gpui::test]
fn a_pick_for_a_closed_pane_opens_nothing(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let settings = dir.path().join("settings.toml");
    let keymap = dir.path().join("keymap.toml");
    fs::write(&settings, "a = 1\n").unwrap();
    fs::write(&keymap, "b = 2\n").unwrap();
    init(cx);
    let (workbench, mut cx_) = open(dir.path(), cx);
    let cx_ = &mut cx_;
    let first = workbench.read_with(cx_, |workbench, _| workbench.pane().clone());
    cx_.update(|window, cx| first.update(cx, |pane, cx| pane.open(keymap.clone(), window, cx)));
    draw(cx_);
    dispatch("pane.split_right", cx_);
    set_problems(vec![problem(Severity::Error, &settings, 1, 1, "bad")], cx);
    dispatch(super::SHOW, cx_);

    // The split, which the pick is for, closes before the pick's read lands.
    confirm(&workbench, 0, cx_);
    let close = RunCommand(
        Invocation::of("pane.close")
            .unwrap()
            .context(ContextStack::new().with(ContextFrame::new("workbench"))),
    );
    cx_.update(|window, cx| window.dispatch_action(Box::new(close), cx));
    let active = workbench.read_with(cx_, |workbench, _| workbench.pane().clone());
    assert_eq!(active, first);
    draw(cx_);
    assert_eq!(shown(&workbench, cx_), Some(keymap.clone()));
    let tabs = first.read_with(cx_, |pane, _| pane.tabs().len());
    assert_eq!(tabs, 1);
    let focused = cx_.update(|window, cx| {
        let editor = first.read(cx).active_editor().unwrap().clone();
        editor.focus_handle(cx).is_focused(window)
    });
    assert!(focused);
}
