//! The Problems panel, its status item and `diagnostics.next` and
//! `diagnostics.previous`, with the fake language server of
//! `tests/lsp_diagnostics.rs` publishing for open files.

use std::fs;

use fe_command::{ContextFrame, ContextStack, Invocation};
use fe_gpui::{EditorView, commands::RunCommand, set_vim_enabled};
use gpui::{Entity, Focusable as _, Modifiers, TestAppContext};
use lsp_types::{
    Diagnostic, DiagnosticSeverity, Position, PublishDiagnosticsParams, Range,
    notification::PublishDiagnostics,
};

use super::{ITEM, ProblemsPanel, ShownRow};
use crate::{DockPanel, StatusSide, tests::lsp::Lsp};

/// `src/main.rs`: `x` at offset 20, `y` at 31.
const MAIN: &str = "fn main() {\n    let x = 1;\n    y();\n}\n";
/// `src/lib.rs`: `g` at offset 21.
const LIB: &str = "pub fn f() {}\npub fn g() {}\n";

/// `x`'s and `y`'s offsets in `MAIN`, and `g`'s in `LIB`.
const X: usize = 20;
const Y: usize = 31;
const G: usize = 21;

fn diagnostic(
    line: u32,
    column: u32,
    len: u32,
    severity: DiagnosticSeverity,
    message: &str,
) -> Diagnostic {
    Diagnostic {
        range: Range::new(
            Position::new(line, column),
            Position::new(line, column + len),
        ),
        severity: Some(severity),
        source: Some("rustc".into()),
        message: message.into(),
        ..Default::default()
    }
}

/// Has the fake publish `diagnostics` for `name`, at the version it has.
fn publish(s: &Lsp, name: &str, diagnostics: Vec<Diagnostic>) {
    let uri = s.uri(name);
    let version = s.fake.document(&uri).expect("it's open").version;
    s.fake
        .notify::<PublishDiagnostics>(PublishDiagnosticsParams {
            uri,
            diagnostics,
            version: Some(version),
        });
}

/// `MAIN`'s: a warning on `x` and an error on `y`.
fn main_diagnostics() -> Vec<Diagnostic> {
    vec![
        diagnostic(2, 4, 1, DiagnosticSeverity::ERROR, "cannot find `y`"),
        diagnostic(
            1,
            8,
            1,
            DiagnosticSeverity::WARNING,
            "unused variable: `x`\nprefix it with an underscore",
        ),
    ]
}

/// `LIB`'s: information on `g`.
fn lib_diagnostics() -> Vec<Diagnostic> {
    vec![diagnostic(
        1,
        7,
        1,
        DiagnosticSeverity::INFORMATION,
        "`g` is never used",
    )]
}

/// The rows with every diagnostic published.
fn all_rows() -> Vec<ShownRow> {
    let rustc = Some("rustc".to_owned());
    vec![
        ShownRow::File("src/lib.rs".into(), 1),
        ShownRow::Problem("I", "`g` is never used".into(), rustc.clone(), "2:8".into()),
        ShownRow::File("src/main.rs".into(), 2),
        ShownRow::Problem(
            "W",
            "unused variable: `x`".into(),
            rustc.clone(),
            "2:9".into(),
        ),
        ShownRow::Problem("E", "cannot find `y`".into(), rustc, "3:5".into()),
    ]
}

/// The project with `LIB` then `MAIN` open in the left pane and `MAIN`
/// split into a right one, active; both files published for, and listed.
fn setup(cx: &mut TestAppContext) -> Lsp {
    let mut s = Lsp::new(cx);
    fs::write(s.path("src/main.rs"), MAIN).unwrap();
    fs::write(s.path("src/lib.rs"), LIB).unwrap();
    for name in ["src/lib.rs", "src/main.rs"] {
        s.open(name);
        let uri = s.uri(name);
        s.fake
            .wait_for_document(&uri, "didOpen", |r| r.is_some_and(|r| r.open));
    }
    s.run("pane.split_right");
    assert_eq!(panes(&mut s), 2);
    publish(&s, "src/main.rs", main_diagnostics());
    publish(&s, "src/lib.rs", lib_diagnostics());
    s.until("both files listed", |s| rows(s) == all_rows());
    s
}

fn panes(s: &mut Lsp) -> usize {
    s.workbench
        .read_with(&s.cx, |w, cx| w.pane_group(cx).panes().len())
}

fn panel(s: &mut Lsp) -> Entity<ProblemsPanel> {
    s.workbench
        .read_with(&s.cx, |w, cx| w.dock().read(cx).problems().clone())
}

fn rows(s: &mut Lsp) -> Vec<ShownRow> {
    let panel = panel(s);
    panel.read_with(&s.cx, |panel, _| panel.shown_rows())
}

fn selected(s: &mut Lsp) -> Option<usize> {
    let panel = panel(s);
    panel.read_with(&s.cx, |panel, _| panel.selected())
}

/// The status item's text, as the last frame drew it.
fn item(s: &mut Lsp) -> Option<String> {
    let bar = s.workbench.read_with(&s.cx, |w, _| w.status_bar().clone());
    let items = bar.read_with(&s.cx, |bar, _| bar.drawn(StatusSide::Left));
    (items.into_iter())
        .find(|(id, _)| id == ITEM)
        .map(|(_, text)| text.to_string())
}

fn active_editor(s: &mut Lsp) -> Entity<EditorView> {
    s.workbench
        .read_with(&s.cx, |w, cx| w.active_editor(cx).cloned())
        .expect("a tab is shown")
}

fn active_path(s: &mut Lsp) -> std::path::PathBuf {
    let editor = active_editor(s);
    editor.read_with(&s.cx, |view, _| view.model().path().unwrap().to_owned())
}

/// The active editor's primary selection.
fn selection(s: &mut Lsp) -> std::ops::Range<usize> {
    let editor = active_editor(s);
    editor.read_with(&s.cx, |view, _| view.model().primary_selection().range)
}

fn focused(s: &mut Lsp, editor: &Entity<EditorView>) -> bool {
    s.cx.update(|window, cx| editor.focus_handle(cx).is_focused(window))
}

fn panel_focused(s: &mut Lsp) -> bool {
    let panel = panel(s);
    s.cx.update(|window, cx| panel.focus_handle(cx).is_focused(window))
}

/// The dock's visibility and active panel.
fn dock(s: &mut Lsp) -> (bool, DockPanel) {
    s.workbench.read_with(&s.cx, |w, cx| {
        let dock = w.dock().read(cx);
        (dock.is_visible(), dock.active())
    })
}

/// Runs `id` as a key would where `frames` hold, under the workbench's.
fn run_in(s: &mut Lsp, id: &str, frames: &[&str]) {
    let context = (frames.iter()).fold(
        ContextStack::new().with(ContextFrame::new(crate::KEY_CONTEXT)),
        |stack, frame| stack.with(ContextFrame::new(*frame)),
    );
    let run = RunCommand(Invocation::of(id).unwrap().context(context));
    s.cx.update(|window, cx| window.dispatch_action(Box::new(run), cx));
    s.draw();
}

/// Runs a panel command, as a key in the panel would.
fn in_panel(s: &mut Lsp, id: &str) {
    run_in(s, id, &[super::KEY_CONTEXT]);
}

/// Runs a command as a key in the editor would.
fn in_editor(s: &mut Lsp, id: &str) {
    run_in(s, id, &[fe_editor::commands::CONTEXT]);
}

#[gpui::test]
fn lists(cx: &mut TestAppContext) {
    let mut s = setup(cx);
    // main.rs once, though two splits show it; files by path, problems by
    // position, each message's first line.
    assert_eq!(rows(&mut s), all_rows());
    assert_eq!(item(&mut s).as_deref(), Some("1"));

    // A file's problems cleared: its group goes.
    publish(&s, "src/main.rs", Vec::new());
    s.until("main.rs cleared", |s| rows(s).len() == 2);
    assert_eq!(rows(&mut s), all_rows()[..2]);
    assert_eq!(item(&mut s), None);

    // Published again, it's back; then every problem cleared.
    publish(&s, "src/main.rs", main_diagnostics());
    s.until("main.rs listed again", |s| rows(s) == all_rows());
    assert_eq!(item(&mut s).as_deref(), Some("1"));
    publish(&s, "src/lib.rs", Vec::new());
    publish(&s, "src/main.rs", Vec::new());
    s.until("all cleared", |s| rows(s).is_empty());
    let panel = panel(&mut s);
    let message = panel.read_with(&s.cx, |panel, _| panel.message_text());
    assert_eq!(message, Some("No problems in the open files"));
    assert_eq!(item(&mut s), None);
}

#[gpui::test]
fn open(cx: &mut TestAppContext) {
    let mut s = setup(cx);
    s.run("problems.show");
    assert_eq!(dock(&mut s), (true, DockPanel::Problems));
    assert!(panel_focused(&mut s));

    // The selection moves over problem rows only, stopping at either end.
    assert_eq!(selected(&mut s), None);
    in_panel(&mut s, "problems.select_next");
    assert_eq!(selected(&mut s), Some(1));
    in_panel(&mut s, "problems.select_next");
    assert_eq!(selected(&mut s), Some(3));
    in_panel(&mut s, "problems.select_next");
    in_panel(&mut s, "problems.select_next");
    assert_eq!(selected(&mut s), Some(4));
    in_panel(&mut s, "problems.select_previous");
    in_panel(&mut s, "problems.select_previous");
    assert_eq!(selected(&mut s), Some(1));
    in_panel(&mut s, "problems.select_previous");
    assert_eq!(selected(&mut s), Some(1));

    // lib.rs shows only in the left pane: it's made active, with lib.rs's
    // tab, `g` selected and the editor focused.
    let right = s.workbench.read_with(&s.cx, |w, _| w.pane().clone());
    in_panel(&mut s, "problems.open");
    let left = s.workbench.read_with(&s.cx, |w, _| w.pane().clone());
    assert_ne!(left, right);
    assert_eq!(active_path(&mut s), s.path("src/lib.rs"));
    assert_eq!(selection(&mut s), G..G + 1);
    let editor = active_editor(&mut s);
    assert!(focused(&mut s, &editor));

    // A click on `y`'s row opens main.rs in the active pane, which shows
    // it too.
    s.run("problems.show");
    let panel = panel(&mut s);
    let bounds = panel.read_with(&s.cx, |panel, _| panel.row_bounds(4).expect("drawn"));
    s.cx.simulate_click(bounds.center(), Modifiers::none());
    s.draw();
    assert_eq!(selected(&mut s), Some(4));
    assert_eq!(s.workbench.read_with(&s.cx, |w, _| w.pane().clone()), left);
    assert_eq!(active_path(&mut s), s.path("src/main.rs"));
    assert_eq!(selection(&mut s), Y..Y + 1);
    let editor = active_editor(&mut s);
    assert!(focused(&mut s, &editor));

    // The selection follows its problem as the list changes.
    publish(&s, "src/lib.rs", Vec::new());
    s.until("lib.rs cleared", |s| rows(s).len() == 3);
    assert_eq!(selected(&mut s), Some(2));
}

#[gpui::test]
fn next_previous(cx: &mut TestAppContext) {
    let mut s = setup(cx);
    let editor = active_editor(&mut s);
    assert_eq!(selection(&mut s), 0..0);
    let mut carets = Vec::new();
    for _ in 0..3 {
        in_editor(&mut s, "diagnostics.next");
        carets.push(selection(&mut s));
    }
    assert_eq!(carets, [X..X, Y..Y, X..X], "forward, wrapping");
    carets.clear();
    for _ in 0..3 {
        in_editor(&mut s, "diagnostics.previous");
        carets.push(selection(&mut s));
    }
    assert_eq!(carets, [Y..Y, X..X, Y..Y], "back, wrapping");
    assert_eq!(active_editor(&mut s), editor);

    // From a selection's caret, its head, whichever way it runs.
    let from = |s: &mut Lsp, range: std::ops::Range<usize>, reversed, id| {
        let set = fe_buffer::ResolvedSelection { range, reversed };
        s.cx.update(|_, cx| {
            editor.update(cx, |view, cx| {
                view.model_mut().set_selections([set]);
                cx.notify();
            })
        });
        in_editor(s, id);
        selection(s)
    };
    assert_eq!(from(&mut s, 0..X + 1, false, "diagnostics.next"), Y..Y);
    assert_eq!(from(&mut s, 0..X + 1, false, "diagnostics.previous"), X..X);
    assert_eq!(from(&mut s, X + 1..Y + 2, true, "diagnostics.next"), Y..Y);
    assert_eq!(
        from(&mut s, X + 1..Y + 2, true, "diagnostics.previous"),
        X..X
    );

    // A file without problems: nothing moves.
    s.open("src/other.rs");
    in_editor(&mut s, "diagnostics.next");
    assert_eq!(selection(&mut s), 0..0);
}

#[gpui::test]
fn status(cx: &mut TestAppContext) {
    let mut s = setup(cx);
    assert_eq!(item(&mut s).as_deref(), Some("1"));
    assert_ne!(dock(&mut s).1, DockPanel::Problems);

    // A click runs `problems.show`: the dock shows the panel, focused.
    let bounds =
        s.cx.debug_bounds("status-item:problems")
            .expect("the item is drawn");
    s.cx.simulate_click(bounds.center(), Modifiers::none());
    s.draw();
    assert_eq!(dock(&mut s), (true, DockPanel::Problems));
    assert!(panel_focused(&mut s));
    let ran = s.workbench.read_with(&s.cx, |w, _| {
        (w.commands_run.iter()).any(|run| run.command.as_str() == super::SHOW)
    });
    assert!(ran);

    // Hidden, the item still shows it.
    s.run("dock.toggle");
    assert!(!dock(&mut s).0);
    let bounds =
        s.cx.debug_bounds("status-item:problems")
            .expect("the item is drawn");
    s.cx.simulate_click(bounds.center(), Modifiers::none());
    s.draw();
    assert_eq!(dock(&mut s), (true, DockPanel::Problems));
}

#[gpui::test]
fn vim(cx: &mut TestAppContext) {
    let mut s = setup(cx);
    let panel = panel(&mut s);
    let flagged = |s: &mut Lsp| {
        panel.read_with(&s.cx, |panel, cx| {
            panel.test_key_context(cx).contains("vim")
        })
    };
    assert!(!flagged(&mut s));
    s.cx.update(|_, cx| set_vim_enabled(cx, true));
    s.draw();
    assert!(flagged(&mut s));
    // The element sets it.
    s.run("problems.show");
    let stack = s.cx.update(|window, _| window.context_stack());
    let context = stack.last().expect("the panel's context");
    assert!(context.contains(super::KEY_CONTEXT) && context.contains("vim"));

    // `diagnostics.next` from normal mode moves the caret and stays in
    // normal mode.
    let editor = active_editor(&mut s);
    s.cx.update(|window, cx| window.focus(&editor.focus_handle(cx), cx));
    s.draw();
    let mode = |s: &mut Lsp, editor: &Entity<EditorView>| {
        editor.read_with(&s.cx, |view, _| view.vim_mode())
    };
    assert_eq!(mode(&mut s, &editor), Some("normal"));
    in_editor(&mut s, "diagnostics.next");
    assert_eq!(selection(&mut s).start, X);
    assert_eq!(mode(&mut s, &editor), Some("normal"));
    in_editor(&mut s, "diagnostics.next");
    assert_eq!(selection(&mut s).start, Y);
    assert_eq!(mode(&mut s, &editor), Some("normal"));

    // Left in insert mode, escape rebound so it no longer leaves it, then a
    // problem opened from the panel: focus is in the editor, in normal
    // mode, the caret at the problem's start.
    s.cx.simulate_keystrokes("i");
    s.draw();
    assert_eq!(mode(&mut s, &editor), Some("insert"));
    s.cx.update(|_, cx| fe_gpui::set_user_keymap(cx, escape_rebound()));
    s.cx.simulate_keystrokes("escape");
    s.draw();
    assert_eq!(mode(&mut s, &editor), Some("insert"), "escape is rebound");
    s.run("problems.show");
    assert!(panel_focused(&mut s));
    in_panel(&mut s, "problems.select_next");
    in_panel(&mut s, "problems.select_next");
    in_panel(&mut s, "problems.open");
    assert_eq!(active_editor(&mut s), editor);
    assert!(focused(&mut s, &editor));
    assert_eq!(mode(&mut s, &editor), Some("normal"));
    assert_eq!(selection(&mut s).start, X);

    // lib.rs, a tab not drawn, left in insert mode: opened from the panel,
    // it's drawn, then in normal mode with its caret at `g`.
    let left = s
        .workbench
        .read_with(&s.cx, |w, cx| w.pane_group(cx).panes()[0].clone());
    let workbench = s.workbench.clone();
    let show = |s: &mut Lsp, tab| {
        s.cx.update(|window, cx| {
            workbench.update(cx, |w, cx| w.activate(left.clone(), window, cx));
            left.update(cx, |pane, cx| pane.activate(tab, window, cx));
        });
    };
    show(&mut s, 0);
    s.draw();
    let lib = active_editor(&mut s);
    assert_eq!(active_path(&mut s), s.path("src/lib.rs"));
    s.cx.update(|window, cx| window.focus(&lib.focus_handle(cx), cx));
    s.cx.simulate_keystrokes("i");
    s.draw();
    assert_eq!(mode(&mut s, &lib), Some("insert"));
    show(&mut s, 1);
    s.draw();
    assert_ne!(active_editor(&mut s), lib);
    s.run("problems.show");
    in_panel(&mut s, "problems.select_previous");
    in_panel(&mut s, "problems.select_previous");
    assert_eq!(selected(&mut s), Some(1));
    in_panel(&mut s, "problems.open");
    assert_eq!(active_editor(&mut s), lib);
    assert!(focused(&mut s, &lib));
    assert_eq!(mode(&mut s, &lib), Some("normal"));
    assert_eq!(selection(&mut s), G..G);
}

/// The default keymap, with escape in vim's insert mode moving right
/// instead of leaving it.
fn escape_rebound() -> fe_keymap::Keymap {
    use fe_keymap::{DEFAULT_KEYMAP_PATH, DEFAULT_MACOS, Keymap, KeymapLayer};
    let user = "\
[[bindings]]
keys = \"escape\"
command = \"editor.move_right\"
context = \"editor && vim.mode == insert\"
";
    let path = std::path::Path::new(DEFAULT_KEYMAP_PATH);
    let (default, _) = KeymapLayer::parse(path, DEFAULT_MACOS, None);
    let (user, diagnostics) = KeymapLayer::parse(std::path::Path::new("keymap.toml"), user, None);
    assert_eq!(diagnostics, []);
    Keymap::from_layers([default.unwrap(), user.unwrap()])
}

#[gpui::test]
fn selection_kept(cx: &mut TestAppContext) {
    let mut s = setup(cx);
    // Two problems alike but for their column, the second selected.
    let twin = |column| diagnostic(1, column, 1, DiagnosticSeverity::WARNING, "twin");
    publish(&s, "src/main.rs", vec![twin(4), twin(8)]);
    // Not `len() == 5`: setup's rows are already five, so that held before
    // the twins arrived, and they then replaced the row selected below.
    s.until("the twins", |s| {
        rows(s).get(3)
            == Some(&ShownRow::Problem(
                "W",
                "twin".into(),
                Some("rustc".into()),
                "2:5".into(),
            ))
    });
    s.run("problems.show");
    for _ in 0..3 {
        in_panel(&mut s, "problems.select_next");
    }
    assert_eq!(selected(&mut s), Some(4));

    // Another file's problems change: the same problem stays selected.
    publish(&s, "src/lib.rs", Vec::new());
    s.until("lib.rs cleared", |s| rows(s).len() == 3);
    assert_eq!(selected(&mut s), Some(2));
    in_panel(&mut s, "problems.open");
    assert_eq!(selection(&mut s), X..X + 1);

    // Moved (here, by a line inserted above), the nearest alike is.
    publish(
        &s,
        "src/main.rs",
        vec![twin(4), twin(8)]
            .into_iter()
            .map(|mut d| {
                d.range.start.line = 2;
                d.range.end.line = 2;
                d
            })
            .collect(),
    );
    s.until("the twins moved", |s| {
        rows(s)[2] == ShownRow::Problem("W", "twin".into(), Some("rustc".into()), "3:9".into())
    });
    assert_eq!(selected(&mut s), Some(2));
}
