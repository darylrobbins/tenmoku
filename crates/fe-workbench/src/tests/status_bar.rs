//! The status bar: the active editor's cursor, selections and file facts,
//! following the active pane's tab, the language picker, the toggle, and
//! host items.

use std::{
    fs,
    path::{Path, PathBuf},
    sync::Arc,
};

use fe_buffer::ResolvedSelection;
use fe_command::{ContextFrame, ContextStack, Invocation};
use fe_config::{EditorSettings, Settings, SettingsEvent};
use fe_gpui::{EditorView, commands::RunCommand};
use fe_ui::ROW_HEIGHT;
use gpui::{Bounds, Entity, Focusable as _, Modifiers, Pixels, TestAppContext, VisualTestContext};

use super::{click_row, open_root, panel};
use crate::{StatusItem, StatusSide, Workbench};

/// `files` (name, contents) in a temporary directory, and their paths.
fn project(files: &[(&str, &[u8])]) -> (tempfile::TempDir, Vec<PathBuf>) {
    let dir = tempfile::tempdir().unwrap();
    let paths = files
        .iter()
        .map(|(name, bytes)| {
            let path = dir.path().join(name);
            fs::write(&path, bytes).unwrap();
            path
        })
        .collect();
    (dir, paths)
}

/// A workbench rooted at `dir`, focused as the app focuses it.
fn opened<'a>(
    cx: &'a mut TestAppContext,
    dir: &tempfile::TempDir,
) -> (Entity<Workbench>, &'a mut VisualTestContext) {
    let (workbench, cx) = open_root(cx, dir.path());
    cx.update(|window, cx| window.focus(&workbench.focus_handle(cx), cx));
    draw(cx);
    (workbench, cx)
}

/// Lets background work land, then draws a frame.
fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
}

/// Opens `path` in the active pane, focused, and draws.
fn open(workbench: &Entity<Workbench>, path: &Path, cx: &mut VisualTestContext) {
    let pane = workbench.read_with(cx, |workbench, _| workbench.pane().clone());
    cx.update(|window, cx| pane.update(cx, |pane, cx| pane.open(path.to_owned(), window, cx)));
    draw(cx);
}

fn active_editor(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Entity<EditorView> {
    workbench
        .read_with(cx, |workbench, cx| workbench.active_editor(cx).cloned())
        .expect("a file is shown")
}

fn item_bounds(id: &str, cx: &mut VisualTestContext) -> Option<Bounds<Pixels>> {
    // `debug_bounds` takes a `&'static str`.
    cx.debug_bounds(format!("status-item:{id}").leak())
}

/// The texts of the items the last frame drew on `side`, left to right.
fn drawn(
    workbench: &Entity<Workbench>,
    side: StatusSide,
    cx: &mut VisualTestContext,
) -> Vec<String> {
    let bar = workbench.read_with(cx, |workbench, _| workbench.status_bar().clone());
    let items = bar.read_with(cx, |bar, _| bar.drawn(side));
    // The bar's last render, if the last frame showed it; less the bell,
    // which `notification_inbox_view`'s tests cover.
    items
        .into_iter()
        .filter(|(id, _)| id != StatusItem::NOTIFICATIONS)
        .filter(|(id, _)| item_bounds(id, cx).is_some())
        .map(|(_, text)| text.to_string())
        .collect()
}

/// The text of the built-in item `id`, as the last frame drew it.
fn item(workbench: &Entity<Workbench>, id: &str, cx: &mut VisualTestContext) -> Option<String> {
    let bar = workbench.read_with(cx, |workbench, _| workbench.status_bar().clone());
    let items = bar.read_with(cx, |bar, _| bar.drawn(StatusSide::Right));
    items
        .into_iter()
        .find(|(item, _)| item == id)
        .filter(|_| item_bounds(id, cx).is_some())
        .map(|(_, text)| text.to_string())
}

fn cursor(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Option<String> {
    item(workbench, StatusItem::CURSOR, cx)
}

fn language(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Option<String> {
    item(workbench, StatusItem::LANGUAGE, cx)
}

fn renders(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> usize {
    let bar = workbench.read_with(cx, |workbench, _| workbench.status_bar().clone());
    bar.read_with(cx, |bar, _| bar.renders())
}

/// Runs workbench command `id` as an unbound key would.
fn dispatch(id: &str, cx: &mut VisualTestContext) {
    let run = RunCommand(
        Invocation::of(id)
            .unwrap()
            .context(ContextStack::new().with(ContextFrame::new("workbench"))),
    );
    cx.update(|window, cx| window.dispatch_action(Box::new(run), cx));
    draw(cx);
}

fn editor_focused(editor: &Entity<EditorView>, cx: &mut VisualTestContext) -> bool {
    cx.update(|window, cx| editor.focus_handle(cx).is_focused(window))
}

#[gpui::test]
fn caret_moves_update_line_and_column(cx: &mut TestAppContext) {
    let (dir, paths) = project(&[("a.rs", "fn main() {}\n    let é = 1;\n".as_bytes())]);
    let (workbench, cx) = opened(cx, &dir);
    open(&workbench, &paths[0], cx);
    assert_eq!(cursor(&workbench, cx).as_deref(), Some("Ln 1, Col 1"));

    cx.simulate_keystrokes("down end");
    draw(cx);
    // 14 chars, though `é` makes it 15 bytes.
    assert_eq!(cursor(&workbench, cx).as_deref(), Some("Ln 2, Col 15"));

    cx.simulate_keystrokes("shift-left shift-left shift-left");
    draw(cx);
    assert_eq!(
        cursor(&workbench, cx).as_deref(),
        Some("Ln 2, Col 12 (3 selected)")
    );
}

#[gpui::test]
fn multiple_selections_are_counted(cx: &mut TestAppContext) {
    let (dir, paths) = project(&[("a.txt", b"one\ntwo\nthree\n")]);
    let (workbench, cx) = opened(cx, &dir);
    open(&workbench, &paths[0], cx);
    let editor = active_editor(&workbench, cx);
    let set = |offsets: &[usize], cx: &mut VisualTestContext| {
        editor.update(cx, |editor, cx| {
            let carets = offsets.iter().map(|&offset| ResolvedSelection {
                range: offset..offset,
                reversed: false,
            });
            editor.model_mut().set_selections(carets);
            cx.notify();
        });
        draw(cx);
    };
    set(&[0, 4, 8], cx);
    assert_eq!(
        cursor(&workbench, cx).as_deref(),
        Some("Ln 3, Col 1 (3 selections)")
    );
    set(&[8], cx);
    assert_eq!(cursor(&workbench, cx).as_deref(), Some("Ln 3, Col 1"));
}

#[gpui::test]
fn follows_the_active_editor(cx: &mut TestAppContext) {
    let (dir, paths) = project(&[("a.rs", b"fn a() {}\n"), ("b.md", b"# b\n")]);
    let (workbench, cx) = opened(cx, &dir);
    open(&workbench, &paths[0], cx);
    open(&workbench, &paths[1], cx);
    assert_eq!(language(&workbench, cx).as_deref(), Some("Markdown"));
    // The frame the switch draws shows it: no second frame is needed.
    cx.simulate_keystrokes("cmd-shift-[");
    cx.run_until_parked();
    assert_eq!(language(&workbench, cx).as_deref(), Some("Rust"));

    // a.rs splits into a new right pane, and b.md opens there.
    cx.simulate_keystrokes("cmd-\\");
    draw(cx);
    assert_eq!(language(&workbench, cx).as_deref(), Some("Rust"));
    click_row(&panel(&workbench, cx), "b.md", cx);
    draw(cx);
    assert_eq!(language(&workbench, cx).as_deref(), Some("Markdown"));
    let panes = workbench.read_with(cx, |workbench, cx| workbench.pane_group(cx).panes().len());
    assert_eq!(panes, 2);

    cx.simulate_keystrokes("cmd-k cmd-left");
    draw(cx);
    assert_eq!(language(&workbench, cx).as_deref(), Some("Rust"));
    let left_editor = active_editor(&workbench, cx);
    // Sampled with no extra frame drawn after the switch's.
    cx.simulate_keystrokes("cmd-k cmd-right");
    cx.run_until_parked();
    assert_eq!(language(&workbench, cx).as_deref(), Some("Markdown"));

    // Edits to the editor switched away from don't re-render the bar.
    let before = renders(&workbench, cx);
    left_editor.update(cx, |editor, cx| {
        editor.model_mut().insert("x");
        cx.notify();
    });
    draw(cx);
    let text = left_editor.read_with(cx, |editor, _| editor.model().text().to_string());
    assert!(text.starts_with('x'), "{text:?}");
    assert_eq!(renders(&workbench, cx), before);
    assert_eq!(language(&workbench, cx).as_deref(), Some("Markdown"));
}

#[gpui::test]
fn file_facts(cx: &mut TestAppContext) {
    let (dir, paths) = project(&[
        ("lf.rs", b"fn a() {}\n"),
        ("crlf.txt", b"a\r\nb\r\n"),
        ("bom.txt", b"\xEF\xBB\xBFhello\n"),
        ("data.xyz", b"x\n"),
    ]);
    let (workbench, cx) = opened(cx, &dir);
    open(&workbench, &paths[0], cx);
    let facts = |workbench: &Entity<Workbench>, cx: &mut VisualTestContext| {
        [
            StatusItem::INDENT,
            StatusItem::ENCODING,
            StatusItem::LINE_ENDING,
            StatusItem::LANGUAGE,
        ]
        .map(|id| item(workbench, id, cx).unwrap_or_default())
    };
    assert_eq!(facts(&workbench, cx), ["Spaces: 4", "UTF-8", "LF", "Rust"]);
    // In VS Code's order, the cursor first.
    assert_eq!(
        drawn(&workbench, StatusSide::Right, cx),
        ["Ln 1, Col 1", "Spaces: 4", "UTF-8", "LF", "Rust"]
    );
    assert_eq!(
        drawn(&workbench, StatusSide::Left, cx),
        Vec::<String>::new()
    );

    open(&workbench, &paths[1], cx);
    assert_eq!(
        item(&workbench, StatusItem::LINE_ENDING, cx).as_deref(),
        Some("CRLF")
    );
    open(&workbench, &paths[2], cx);
    assert_eq!(
        item(&workbench, StatusItem::ENCODING, cx).as_deref(),
        Some("UTF-8 with BOM")
    );
    open(&workbench, &paths[3], cx);
    assert_eq!(language(&workbench, cx).as_deref(), Some("Plain Text"));

    let settings = |tab_size| {
        Arc::new(Settings {
            editor: EditorSettings {
                tab_size,
                ..EditorSettings::default()
            },
            ..Settings::default()
        })
    };
    let event = SettingsEvent {
        settings: settings(2),
        previous: settings(4),
        diagnostics: Vec::new(),
    };
    workbench.update(cx, |workbench, cx| workbench.reload_settings(&event, cx));
    draw(cx);
    assert_eq!(
        item(&workbench, StatusItem::INDENT, cx).as_deref(),
        Some("Spaces: 2")
    );
}

#[gpui::test]
fn language_picker_sets_the_language(cx: &mut TestAppContext) {
    let (dir, paths) = project(&[("a.rs", b"fn a() {}\n")]);
    let (workbench, cx) = opened(cx, &dir);
    open(&workbench, &paths[0], cx);
    let editor = active_editor(&workbench, cx);
    let picker = workbench.read_with(cx, |workbench, _| workbench.language_picker().clone());
    let open = |workbench: &Entity<Workbench>, cx: &mut VisualTestContext| {
        workbench.read_with(cx, |workbench, _| workbench.language_picker_open())
    };
    let syntax = |cx: &mut VisualTestContext| {
        editor.read_with(cx, |editor, _| {
            editor.syntax().map(|syntax| syntax.language().name())
        })
    };

    let language_item = item_bounds(StatusItem::LANGUAGE, cx).expect("drawn");
    cx.simulate_click(language_item.center(), Modifiers::none());
    draw(cx);
    assert!(open(&workbench, cx));
    let labels = picker.read_with(cx, |picker, _| picker.delegate.labels());
    assert_eq!(labels[0], ("Plain Text".into(), false));
    let current: Vec<_> = labels.iter().filter(|(_, current)| *current).collect();
    assert_eq!(current, [&("Rust".into(), true)]);
    let names: Vec<_> = labels[1..]
        .iter()
        .map(|(label, _)| label.to_lowercase())
        .collect();
    assert!(names.is_sorted(), "{names:?}");

    cx.simulate_input("pyth");
    draw(cx);
    let labels = picker.read_with(cx, |picker, _| picker.delegate.labels());
    assert_eq!(labels, [("Python".into(), false)]);
    cx.simulate_keystrokes("enter");
    draw(cx);
    assert_eq!(syntax(cx), Some("python"));
    assert_eq!(language(&workbench, cx).as_deref(), Some("Python"));
    assert!(!open(&workbench, cx));
    assert!(editor_focused(&editor, cx));

    // Plain Text is first, so selected.
    dispatch("language.select", cx);
    assert!(open(&workbench, cx));
    cx.simulate_keystrokes("enter");
    draw(cx);
    assert_eq!(syntax(cx), None);
    assert_eq!(language(&workbench, cx).as_deref(), Some("Plain Text"));
    assert!(editor_focused(&editor, cx));

    // Asked again while it shows, it stays, afresh.
    dispatch("language.select", cx);
    cx.simulate_input("rust");
    dispatch("language.select", cx);
    assert!(open(&workbench, cx));
    let input = picker.read_with(cx, |picker, cx| picker.input().read(cx).text().to_owned());
    assert_eq!(input, "");
    assert!(cx.update(|window, cx| picker.focus_handle(cx).is_focused(window)));
    cx.simulate_input("rust");
    cx.simulate_keystrokes("escape");
    draw(cx);
    assert!(!open(&workbench, cx));
    assert_eq!(syntax(cx), None);
    assert_eq!(language(&workbench, cx).as_deref(), Some("Plain Text"));
    assert!(editor_focused(&editor, cx));
}

#[gpui::test]
fn toggle_hides_the_bar(cx: &mut TestAppContext) {
    let (dir, paths) = project(&[("a.rs", b"fn a() {}\n")]);
    let (workbench, cx) = opened(cx, &dir);
    open(&workbench, &paths[0], cx);
    let pane = workbench.read_with(cx, |workbench, _| workbench.pane().clone());
    let bottom = |cx: &mut VisualTestContext| pane.read_with(cx, |pane, _| pane.bounds().bottom());
    let shown = bottom(cx);
    let window = cx.update(|window, _| window.viewport_size());
    assert_eq!(shown, window.height - ROW_HEIGHT);
    let bar = cx.debug_bounds("status-bar").expect("drawn");
    assert_eq!(bar.size.height, ROW_HEIGHT);
    assert_eq!(bar.size.width, window.width);
    assert_eq!(bar.bottom(), window.height);
    assert_eq!(drawn(&workbench, StatusSide::Right, cx).len(), 5);

    let visible = |cx: &mut VisualTestContext| {
        workbench.read_with(cx, |workbench, _| workbench.status_bar_visible())
    };
    let hidden = |workbench: &Entity<Workbench>, cx: &mut VisualTestContext| {
        assert!(!workbench.read_with(cx, |workbench, _| workbench.status_bar_visible()));
        assert_eq!(cx.debug_bounds("status-bar"), None);
        assert_eq!(
            drawn(workbench, StatusSide::Right, cx),
            Vec::<String>::new()
        );
        assert_eq!(
            pane.read_with(cx, |pane, _| pane.bounds().bottom()),
            shown + ROW_HEIGHT
        );
    };
    dispatch("view.toggle_status_bar", cx);
    hidden(&workbench, cx);
    dispatch("view.toggle_status_bar", cx);
    assert!(visible(cx));
    assert_eq!(bottom(cx), shown);
    assert_eq!(drawn(&workbench, StatusSide::Right, cx).len(), 5);

    workbench.update(cx, |workbench, cx| {
        workbench.set_status_bar_visible(false, cx)
    });
    draw(cx);
    hidden(&workbench, cx);
}

#[gpui::test]
fn host_items(cx: &mut TestAppContext) {
    let (dir, paths) = project(&[("a.rs", b"fn a() {}\n")]);
    let (workbench, cx) = opened(cx, &dir);
    open(&workbench, &paths[0], cx);
    let bar = workbench.read_with(cx, |workbench, _| workbench.status_bar().clone());
    let dock = workbench.read_with(cx, |workbench, _| workbench.dock().clone());

    // No context: a click stamps the focused element's.
    let dock_item = StatusItem {
        id: "host.dock".into(),
        side: StatusSide::Left,
        priority: 0,
        text: "Dock".into(),
        command: Some(Invocation::of("dock.toggle").unwrap()),
    };
    bar.update(cx, |bar, cx| bar.set_item(dock_item, cx));
    draw(cx);
    assert_eq!(drawn(&workbench, StatusSide::Left, cx), ["Dock"]);
    let dock_bounds = item_bounds("host.dock", cx).expect("drawn");
    let window = cx.update(|window, _| window.viewport_size());
    assert!(dock_bounds.left() < window.width / 2., "{dock_bounds:?}");
    assert!(dock.read_with(cx, |dock, _| dock.is_visible()));
    cx.simulate_click(dock_bounds.center(), Modifiers::none());
    draw(cx);
    assert!(!dock.read_with(cx, |dock, _| dock.is_visible()));

    let host_language = StatusItem {
        id: StatusItem::LANGUAGE.into(),
        side: StatusSide::Right,
        priority: 500,
        text: "Host Rust".into(),
        command: None,
    };
    bar.update(cx, |bar, cx| bar.set_item(host_language, cx));
    draw(cx);
    assert_eq!(
        drawn(&workbench, StatusSide::Right, cx),
        ["Ln 1, Col 1", "Spaces: 4", "UTF-8", "LF", "Host Rust"]
    );
    bar.update(cx, |bar, cx| bar.remove_item(StatusItem::LANGUAGE, cx));
    draw(cx);
    assert_eq!(language(&workbench, cx).as_deref(), Some("Rust"));
    bar.update(cx, |bar, cx| bar.remove_item("host.dock", cx));
    draw(cx);
    assert_eq!(
        drawn(&workbench, StatusSide::Left, cx),
        Vec::<String>::new()
    );

    // With no tabs, only the host's items draw.
    let host = StatusItem {
        id: "host.status".into(),
        side: StatusSide::Right,
        priority: 0,
        text: "Ready".into(),
        command: None,
    };
    bar.update(cx, |bar, cx| bar.set_item(host, cx));
    cx.simulate_keystrokes("cmd-w");
    draw(cx);
    let tabs = workbench.read_with(cx, |workbench, cx| workbench.pane().read(cx).tabs().len());
    assert_eq!(tabs, 0);
    assert_eq!(drawn(&workbench, StatusSide::Right, cx), ["Ready"]);
    assert_eq!(
        drawn(&workbench, StatusSide::Left, cx),
        Vec::<String>::new()
    );
}
