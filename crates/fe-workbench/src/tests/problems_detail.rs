//! The Problems panel's detail pane (`docs/design/design-system.md#problems`,
//! T1587): for the selected problem, `code · source`, the message, the
//! problem's row and one either side with its range underlined, and the
//! language server's preferred quick fix as a celadon-soft line with a +
//! glyph and its title, applied with Apply fix (⌥↵ in the panel). Keys go
//! through the default keymap; the server is fe-lsp's fake (the LSP tests'
//! [`Lsp`]).

use std::fs;

use fe_command::{ContextFrame, ContextStack, Invocation};
use fe_editor::diagnostics::{Diagnostic, Problem, Severity};
use fe_gpui::{EditorView, commands::RunCommand};
use fe_lsp::fake::{InitializeReply, Record, Reply, Script, Scripted, initialize_result};
use fe_ui::ActiveTheme as _;
use gpui::{Bounds, Entity, Focusable as _, Pixels, TestAppContext, VisualTestContext};
use lsp_types::{
    CodeActionProviderCapability, ExecuteCommandOptions, Position, PublishDiagnosticsParams, Range,
    ServerCapabilities,
    notification::PublishDiagnostics,
    request::{CodeActionRequest, ExecuteCommand, Request},
};
use serde_json::{Value, json};

use super::{lsp::Lsp, open_root};
use crate::dock::problems_panel::{
    APPLY_SELECTOR, ColumnUnit, DETAIL_SELECTOR, DrawnDetail, FIX_SELECTOR, ProblemsPanel,
    SourceProblem,
};

/// `a.rs`: its problem is on `beta`, row 1.
const A: &str = "fn alpha() {}\nlet beta = 1;\nfn gamma() {}\nfn delta() {}\n";
/// `notes.txt`, which no editor shows: a task's problem is on its row 2.
const NOTES: &str = "one\ntwo\nthree\nfour\n";
const HREF: &str = "https://doc.rust-lang.org/error_codes/E0308.html";

fn draw(cx: &mut VisualTestContext) {
    for _ in 0..2 {
        cx.run_until_parked();
        cx.update(|window, cx| window.draw(cx).clear(cx));
    }
    cx.run_until_parked();
}

fn panel_of(
    workbench: &Entity<crate::Workbench>,
    cx: &mut VisualTestContext,
) -> Entity<ProblemsPanel> {
    workbench.read_with(cx, |workbench, cx| {
        workbench.dock().read(cx).problems().clone()
    })
}

fn inside(inner: Bounds<Pixels>, outer: Bounds<Pixels>) -> bool {
    inner.left() >= outer.left()
        && inner.right() <= outer.right()
        && inner.top() >= outer.top()
        && inner.bottom() <= outer.bottom()
}

/// One snippet row as drawn: its one-based number, and its text before,
/// in and after the underlined range.
fn row(number: u32, before: &str, marked: &str, after: &str) -> (u32, String, String, String) {
    (number - 1, before.into(), marked.into(), after.into())
}

#[gpui::test]
fn shows_code_source_message_and_a_three_line_snippet(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_owned();
    fs::write(root.join("a.rs"), A).unwrap();
    fs::write(root.join("notes.txt"), NOTES).unwrap();
    let (workbench, cx) = open_root(cx, &root);
    cx.update(|window, _| window.activate_window());
    let pane = workbench.read_with(cx, |workbench, _| workbench.pane().clone());
    let a = root.join("a.rs");
    cx.update(|window, cx| pane.update(cx, |pane, cx| pane.open(a.clone(), window, cx)));
    draw(cx);
    let beta = A.find("beta").unwrap();
    let diagnostic = Diagnostic {
        source: Some("rustc".into()),
        code: Some("E0308".into()),
        code_href: Some(HREF.into()),
        ..Diagnostic::new(Severity::Error, "mismatched types\nexpected `&str`")
    };
    workbench.update(cx, |workbench, cx| {
        workbench.set_diagnostics_for_audit(&a, vec![(beta..beta + 4, diagnostic)], cx)
    });
    // A task's problem in a file no editor shows: no code, no range.
    let panel = panel_of(&workbench, cx);
    let task = SourceProblem {
        path: root.join("notes.txt"),
        problem: Problem {
            range: 0..0,
            row: 2,
            column: 0,
            severity: Severity::Warning,
            message: "three is odd".into(),
            source: Some("make".into()),
            code: None,
            related: Vec::new(),
            code_href: None,
        },
        unit: ColumnUnit::Bytes,
        file_column: 0,
    };
    panel.update(cx, |panel, _| panel.set_source(7, vec![task]));
    workbench.update(cx, |workbench, cx| workbench.sync_problems(cx));
    // Shown, as `problems.show` shows it: focused.
    cx.update(|window, cx| window.focus(&workbench.focus_handle(cx), cx));
    let run = RunCommand(
        Invocation::of("problems.show")
            .unwrap()
            .context(ContextStack::new().with(ContextFrame::new("workbench"))),
    );
    cx.update(|window, cx| window.dispatch_action(Box::new(run), cx));
    draw(cx);
    let detail = |cx: &mut VisualTestContext| panel.read_with(cx, |panel, _| panel.drawn_detail());
    assert_eq!(detail(cx), None, "nothing's selected");
    assert_eq!(cx.debug_bounds(DETAIL_SELECTOR), None);

    // The first problem, a.rs's: its code links to its page.
    cx.simulate_keystrokes("down");
    draw(cx);
    let shown = detail(cx).expect("the pane shows");
    assert_eq!(shown.code.as_deref(), Some("E0308"));
    assert_eq!(shown.source.as_deref(), Some("rustc"));
    assert!(shown.linked);
    // The whole message, though its row shows the first line.
    assert_eq!(shown.message, "mismatched types\nexpected `&str`");
    assert_eq!(
        shown.snippet,
        [
            row(1, "fn alpha() {}", "", ""),
            row(2, "let ", "beta", " = 1;"),
            row(3, "fn gamma() {}", "", ""),
        ]
    );
    assert_eq!(shown.fix, None, "no server");
    assert!(!shown.apply);
    assert_eq!(cx.debug_bounds(APPLY_SELECTOR), None);
    // Below the list, in the panel.
    let pane_bounds = cx.debug_bounds(DETAIL_SELECTOR).expect("the pane is drawn");
    let panel_bounds = cx
        .debug_bounds("sidebar-panel")
        .expect("the panel is drawn");
    assert!(
        inside(pane_bounds, panel_bounds),
        "{pane_bounds:?} in {panel_bounds:?}"
    );
    assert_eq!(pane_bounds.bottom(), panel_bounds.bottom());

    // The task's problem: its file read from disk, nothing underlined.
    cx.simulate_keystrokes("down");
    draw(cx);
    let shown = detail(cx).expect("the pane shows");
    assert_eq!((shown.code, shown.source.as_deref()), (None, Some("make")));
    assert!(!shown.linked);
    assert_eq!(shown.message, "three is odd");
    assert_eq!(
        shown.snippet,
        [
            row(2, "two", "", ""),
            row(3, "three", "", ""),
            row(4, "four", "", ""),
        ]
    );
}

/// `main.rs`: a diagnostic over `helper`.
const MAIN: &str = "fn main() {\n    helper();\n}\n";
const MESSAGE: &str = "unresolved name";

/// `helper` on `main.rs`' row 1.
fn helper() -> Range {
    Range::new(Position::new(1, 4), Position::new(1, 10))
}

fn diagnostic() -> Value {
    json!({ "range": helper(), "message": MESSAGE, "code": "E1" })
}

/// An edit of `main.rs` renaming `helper` to `fixed`.
fn rename(s: &Lsp) -> Value {
    json!({ "changes": { s.uri("src/main.rs").as_str(): [{
        "range": {"start": {"line": 1, "character": 4}, "end": {"line": 1, "character": 10}},
        "newText": "fixed",
    }]}})
}

/// An edit of `main.rs` adding a `use` on its first row.
fn import(s: &Lsp) -> Value {
    json!({ "changes": { s.uri("src/main.rs").as_str(): [{
        "range": {"start": {"line": 0, "character": 0}, "end": {"line": 0, "character": 0}},
        "newText": "use helpers::helper;\n",
    }]}})
}

/// A server offering code actions and running `fix.after`, `main.rs` open
/// with its diagnostic, the server answering code actions with `actions`
/// (given the harness, for the edits' URIs), and the Problems panel shown,
/// focused, nothing selected.
fn setup(cx: &mut TestAppContext, actions: impl Fn(&Lsp) -> Value) -> (Lsp, Entity<EditorView>) {
    let mut init = initialize_result(Some(lsp_types::PositionEncodingKind::UTF8));
    init.capabilities = ServerCapabilities {
        code_action_provider: Some(CodeActionProviderCapability::Simple(true)),
        execute_command_provider: Some(ExecuteCommandOptions {
            commands: vec!["fix.after".into()],
            ..Default::default()
        }),
        ..init.capabilities
    };
    let script = Script {
        initialize: InitializeReply::Result(Box::new(init)),
        ..Script::default()
    };
    let mut s = Lsp::with_scripts(cx, script, Script::default());
    s.set_trust(true);
    fs::write(s.path("src/main.rs"), MAIN).unwrap();
    s.open("src/main.rs");
    let uri = s.uri("src/main.rs");
    s.fake
        .wait_for_document(&uri, "didOpen", |r| r.is_some())
        .unwrap();
    let editor = (s.workbench)
        .read_with(&s.cx, |w, cx| w.active_editor(cx).cloned())
        .expect("main.rs is shown");
    s.fake
        .notify::<PublishDiagnostics>(PublishDiagnosticsParams {
            uri,
            diagnostics: vec![serde_json::from_value(diagnostic()).unwrap()],
            version: Some(0),
        });
    s.until("the diagnostic", |s| {
        editor.read_with(&s.cx, |view, _| view.model().diagnostics().len() == 1)
    });
    let reply = actions(&s);
    s.fake.set_reply(
        CodeActionRequest::METHOD,
        Scripted::now(Reply::Result(reply)),
    );
    s.fake.set_reply(
        ExecuteCommand::METHOD,
        Scripted::now(Reply::Result(Value::Null)),
    );
    s.run("problems.show");
    let panel = panel(&mut s);
    assert!(
        s.cx.update(|window, cx| panel.focus_handle(cx).is_focused(window)),
        "the panel has focus"
    );
    (s, editor)
}

fn panel(s: &mut Lsp) -> Entity<ProblemsPanel> {
    let workbench = s.workbench.clone();
    panel_of(&workbench, &mut s.cx)
}

fn asked(s: &Lsp, method: &str) -> Vec<Value> {
    (s.fake.records().into_iter())
        .filter_map(|record| match record {
            Record::Request {
                method: m, params, ..
            } if m == method => Some(params),
            _ => None,
        })
        .collect()
}

fn text(s: &mut Lsp, editor: &Entity<EditorView>) -> String {
    editor.read_with(&s.cx, |view, _| view.model().text().to_string())
}

fn drawn(s: &mut Lsp) -> DrawnDetail {
    let panel = panel(s);
    panel
        .read_with(&s.cx, |panel, _| panel.drawn_detail())
        .expect("the pane shows")
}

/// Selects the problem, and waits for the server to have been asked for
/// its fixes and its answer drawn.
fn select(s: &mut Lsp) {
    s.cx.simulate_keystrokes("down");
    s.until("the fixes asked for", |s| {
        !asked(s, CodeActionRequest::METHOD).is_empty()
    });
    s.draw();
}

/// The server's order: a refactor, an import that's a quick fix, then the
/// preferred fix for the diagnostic, renaming `helper` and then running
/// `fix.after`.
fn offered(s: &Lsp) -> Value {
    json!([
        { "title": "Extract function", "kind": "refactor.extract" },
        { "title": "Import helper", "kind": "quickfix", "edit": import(s) },
        { "title": "Rename to fixed", "kind": "quickfix", "isPreferred": true,
          "diagnostics": [diagnostic()], "edit": rename(s),
          "command": { "title": "after", "command": "fix.after" } },
    ])
}

#[gpui::test]
fn the_preferred_quickfix_shows_as_a_celadon_line_with_a_plus_glyph(cx: &mut TestAppContext) {
    let (mut s, _editor) = setup(cx, offered);
    select(&mut s);
    s.until("the fix", |s| drawn(s).fix.is_some());
    // The quick fixes were asked for, automatically, with the diagnostic,
    // over its range.
    let sent = asked(&s, CodeActionRequest::METHOD);
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0]["context"]["only"], json!(["quickfix"]));
    assert_eq!(sent[0]["context"]["triggerKind"], json!(2));
    assert_eq!(sent[0]["range"], serde_json::to_value(helper()).unwrap());
    let diagnostics = sent[0]["context"]["diagnostics"].as_array().unwrap();
    assert_eq!(diagnostics.len(), 1);
    assert_eq!(diagnostics[0]["message"], MESSAGE);
    assert_eq!(diagnostics[0]["code"], "E1");

    let shown = drawn(&mut s);
    assert_eq!(shown.code.as_deref(), Some("E1"));
    assert!(!shown.linked, "no page for the code");
    assert_eq!(
        shown.snippet,
        [
            row(1, "fn main() {", "", ""),
            row(2, "    ", "helper", "();"),
            row(3, "}", "", ""),
        ]
    );
    let fix = shown.fix.expect("the fix shows");
    // The preferred one, not the first quick fix: the row as fixed.
    assert_eq!(fix.lines, ["    fixed();"]);
    assert_eq!(fix.title, "Rename to fixed");
    let celadon_soft = s.cx.update(|_, cx| cx.theme().colors.status.success_soft);
    assert_eq!(fix.ground, celadon_soft);
    assert_eq!(fix.glyph, fe_ui::Icon::Plus);
    assert!(shown.apply);
    // Drawn in the pane, the button below it.
    let pane =
        s.cx.debug_bounds(DETAIL_SELECTOR)
            .expect("the pane is drawn");
    let line = s.cx.debug_bounds(FIX_SELECTOR).expect("the fix is drawn");
    let button =
        s.cx.debug_bounds(APPLY_SELECTOR)
            .expect("Apply fix is drawn");
    assert!(inside(line, pane) && inside(button, pane));
    assert!(button.top() >= line.bottom());
    // Nothing's applied by showing it.
    assert!(asked(&s, ExecuteCommand::METHOD).is_empty());
}

#[gpui::test]
fn apply_fix_applies_the_fixs_edit(cx: &mut TestAppContext) {
    let (mut s, editor) = setup(cx, offered);
    select(&mut s);
    s.until("the fix", |s| drawn(s).fix.is_some());
    s.cx.simulate_keystrokes("alt-enter");
    s.until("the fix applied", |s| {
        text(s, &editor) == "fn main() {\n    fixed();\n}\n"
    });
    // Then its command ran, as the code-action picker runs it.
    s.until("the command", |s| {
        !asked(s, ExecuteCommand::METHOD).is_empty()
    });
    let ran = asked(&s, ExecuteCommand::METHOD);
    assert_eq!(ran.len(), 1);
    assert_eq!(ran[0]["command"], "fix.after");
    assert!(!s.cx.has_pending_prompt());
}

#[gpui::test]
fn no_fix_means_no_button_and_alt_enter_does_nothing(cx: &mut TestAppContext) {
    // A refactor and a disabled quick fix: nothing to apply.
    let (mut s, editor) = setup(cx, |s| {
        json!([
            { "title": "Extract function", "kind": "refactor.extract", "edit": rename(s) },
            { "title": "Rename to fixed", "kind": "quickfix", "isPreferred": true,
              "disabled": { "reason": "not now" }, "edit": rename(s) },
        ])
    });
    select(&mut s);
    s.draw();
    let shown = drawn(&mut s);
    assert_eq!(shown.message, MESSAGE);
    assert_eq!(shown.fix, None);
    assert!(!shown.apply);
    assert_eq!(s.cx.debug_bounds(FIX_SELECTOR), None);
    assert_eq!(s.cx.debug_bounds(APPLY_SELECTOR), None);

    s.cx.simulate_keystrokes("alt-enter");
    s.draw();
    assert_eq!(text(&mut s, &editor), MAIN);
    assert!(asked(&s, ExecuteCommand::METHOD).is_empty());
    assert!(!s.cx.has_pending_prompt());
    // The panel keeps focus and the selection.
    let panel = panel(&mut s);
    assert!(s.cx.update(|window, cx| panel.focus_handle(cx).is_focused(window)));
    assert_eq!(panel.read_with(&s.cx, |panel, _| panel.selected()), Some(1));
}
