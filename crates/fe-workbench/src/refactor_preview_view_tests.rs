//! A rename's or a code action's edit to several files, from fe-lsp's fake
//! server, previewed in the AI Changes panel: kept or left out by file and
//! by hunk in a review tab, then applied or cancelled.

use std::{fs, path::PathBuf};

use fe_command::{ContextFrame, ContextStack, Invocation};
use fe_editor::Counts;
use fe_gpui::{EditorView, commands::RunCommand};
use fe_lsp::fake::{InitializeReply, Record, Reply, Script, Scripted, initialize_result};
use gpui::{Entity, Focusable as _, Modifiers, TestAppContext};
use lsp_types::{
    CodeActionOptions, CodeActionProviderCapability, ExecuteCommandOptions, OneOf,
    PositionEncodingKind, RenameOptions, ServerCapabilities,
    request::{
        CodeActionRequest, CodeActionResolveRequest, ExecuteCommand, PrepareRenameRequest, Rename,
        Request as _,
    },
};
use serde_json::{Value, json};

use crate::{ai_changes_panel::AiChangesPanel, tests::lsp::Lsp, workbench::Palette};

/// `src/main.rs`, open: `foo` twice.
const MAIN: &str = "fn foo() {}\nfn main() {\n    foo();\n}\n";
/// `src/lib.rs`, open: two calls.
const LIB: &str = "pub fn a() { foo(); }\npub fn b() { foo(); }\n";
/// `src/other.rs`, closed: two calls.
const OTHER: &str = "fn h() { foo(); }\nfn i() { foo(); }\n";
/// `src/third.rs`, closed: one call.
const THIRD: &str = "fn t() { foo(); }\n";
/// The caret in `main.rs`' call, after its `fo`.
const CARET: usize = 30;

/// A server that prepares renames, and resolves code actions and runs
/// `fix.all`.
fn server() -> Script {
    let mut init = initialize_result(Some(PositionEncodingKind::UTF8));
    init.capabilities = ServerCapabilities {
        rename_provider: Some(OneOf::Right(RenameOptions {
            prepare_provider: Some(true),
            work_done_progress_options: Default::default(),
        })),
        code_action_provider: Some(CodeActionProviderCapability::Options(CodeActionOptions {
            resolve_provider: Some(true),
            ..Default::default()
        })),
        execute_command_provider: Some(ExecuteCommandOptions {
            commands: vec!["fix.all".into()],
            ..Default::default()
        }),
        ..init.capabilities
    };
    Script {
        initialize: InitializeReply::Result(Box::new(init)),
        ..Script::default()
    }
}

/// The range of `foo` at `line`, from `column`.
fn foo(line: u32, column: u32) -> Value {
    json!({
        "start": { "line": line, "character": column },
        "end": { "line": line, "character": column + 3 },
    })
}

fn reply(s: &Lsp, method: &str, result: Value) {
    s.fake
        .set_reply(method, Scripted::now(Reply::Result(result)));
}

/// The project with `lib.rs` and then `main.rs` open, `main.rs` shown with
/// its caret in the call.
fn setup(cx: &mut TestAppContext) -> (Lsp, Entity<EditorView>) {
    let mut s = Lsp::with_scripts(cx, server(), Script::default());
    s.set_trust(true);
    for (name, text) in [
        ("src/main.rs", MAIN),
        ("src/lib.rs", LIB),
        ("src/other.rs", OTHER),
        ("src/third.rs", THIRD),
    ] {
        fs::write(s.path(name), text).unwrap();
    }
    for name in ["src/lib.rs", "src/main.rs"] {
        s.open(name);
        s.fake
            .wait_for_document(&s.uri(name), "didOpen", |r| r.is_some());
    }
    let editor = (s.workbench)
        .read_with(&s.cx, |w, cx| w.active_editor(cx).cloned())
        .expect("main.rs is shown");
    s.cx.update(|window, cx| {
        editor.update(cx, |view, cx| {
            view.model_mut().set_cursor(CARET);
            cx.notify();
        });
        window.focus(&editor.focus_handle(cx), cx);
    });
    s.draw();
    reply(&s, PrepareRenameRequest::METHOD, foo(2, 4));
    (s, editor)
}

/// Runs `command` as the editor's keys would.
fn run(s: &mut Lsp, command: &str) {
    let context = ContextStack::new()
        .with(ContextFrame::new(crate::KEY_CONTEXT))
        .with(ContextFrame::new(fe_editor::commands::CONTEXT));
    let run = RunCommand(Invocation::of(command).unwrap().context(context));
    s.cx.update(|window, cx| window.dispatch_action(Box::new(run), cx));
    s.draw();
}

/// Renames `foo` to `bar`, with the server's `changes`, and waits for the
/// picker to close.
fn rename(s: &mut Lsp, changes: Value) {
    reply(s, Rename::METHOD, json!({ "changes": changes }));
    run(s, "lsp.rename");
    s.until("the picker", |s| {
        s.workbench
            .read_with(&s.cx, |w, _| w.palette == Some(Palette::Rename))
    });
    s.cx.simulate_input("bar");
    s.draw();
    s.cx.simulate_keystrokes("enter");
    s.until("the picker to close", |s| {
        s.workbench.read_with(&s.cx, |w, _| w.palette.is_none())
    });
}

fn panel(s: &mut Lsp) -> Entity<AiChangesPanel> {
    s.workbench.read_with(&s.cx, |workbench, cx| {
        workbench.dock().read(cx).ai_changes().cloned().unwrap()
    })
}

/// Whether the panel shows a preview titled `title`: false before the
/// server's answer has made the panel, so a wait can poll it.
fn previewing(s: &mut Lsp, title: &str) -> bool {
    let panel = s.workbench.read_with(&s.cx, |workbench, cx| {
        workbench.dock().read(cx).ai_changes().cloned()
    });
    panel.is_some_and(|panel| {
        panel
            .read_with(&s.cx, |panel, _| panel.header_text())
            .starts_with(title)
    })
}

fn header(s: &mut Lsp) -> String {
    let panel = panel(s);
    panel.read_with(&s.cx, |panel, _| panel.header_text())
}

fn message_shown(s: &mut Lsp) -> Option<&'static str> {
    let panel = panel(s);
    panel.read_with(&s.cx, |panel, _| panel.message_text())
}

/// Each row's path and counts: pending, accepted, rejected.
fn rows(s: &mut Lsp) -> Vec<(String, usize, usize, usize)> {
    let panel = panel(s);
    let rows = panel.read_with(&s.cx, |panel, _| panel.shown_rows());
    (rows.into_iter())
        .map(
            |(
                path,
                _,
                _,
                Counts {
                    pending,
                    accepted,
                    rejected,
                    ..
                },
            )| { (path, pending, accepted, rejected) },
        )
        .collect()
}

/// The counts of `name`'s row: pending, accepted, rejected.
fn row(s: &mut Lsp, name: &str) -> (usize, usize, usize) {
    let rows = rows(s);
    let (_, pending, accepted, rejected) = (rows.into_iter())
        .find(|(path, ..)| path == name)
        .unwrap_or_else(|| panic!("{name} isn't listed"));
    (pending, accepted, rejected)
}

/// Clicks the element `selector` names.
fn click(s: &mut Lsp, selector: &'static str) {
    s.draw();
    let bounds = (s.cx.debug_bounds(selector)).unwrap_or_else(|| panic!("{selector} wasn't drawn"));
    s.cx.simulate_click(bounds.center(), Modifiers::none());
    s.draw();
}

/// Clicks the row `selector` names, on its second line, clear of its
/// buttons.
fn click_row(s: &mut Lsp, selector: &'static str) {
    s.draw();
    let bounds = (s.cx.debug_bounds(selector)).unwrap_or_else(|| panic!("{selector} wasn't drawn"));
    let at = bounds.origin + gpui::point(gpui::px(24.), bounds.size.height - gpui::px(8.));
    s.cx.simulate_click(at, Modifiers::none());
    s.draw();
}

fn text(s: &mut Lsp, editor: &Entity<EditorView>) -> String {
    editor.read_with(&s.cx, |view, _| view.model().text().to_string())
}

fn disk(s: &Lsp, name: &str) -> String {
    fs::read_to_string(s.path(name)).unwrap()
}

/// Every open editor of `name`, with whether it's read-only.
fn editors_of(s: &mut Lsp, name: &str) -> Vec<(Entity<EditorView>, bool)> {
    let path: PathBuf = s.path(name);
    s.workbench.read_with(&s.cx, |workbench, cx| {
        (workbench.open_editors(cx).into_iter())
            .filter(|editor| editor.read(cx).model().path() == Some(path.as_path()))
            .map(|editor| {
                let read_only = editor.read(cx).is_read_only();
                (editor, read_only)
            })
            .collect()
    })
}

/// Whether `editor` is still open.
fn is_open(s: &mut Lsp, editor: &Entity<EditorView>) -> bool {
    s.workbench
        .read_with(&s.cx, |w, cx| w.open_editors(cx).contains(editor))
}

/// The review tab showing, and its hunks' ids, pending first.
fn review_tab(s: &mut Lsp) -> (Entity<EditorView>, Vec<fe_buffer::HunkId>) {
    let editor = (s.workbench)
        .read_with(&s.cx, |w, cx| w.active_editor(cx).cloned())
        .expect("a review tab shows");
    let hunks = editor.read_with(&s.cx, |view, _| {
        assert!(view.is_read_only(), "a review tab is read-only");
        assert_eq!(view.model().path(), None, "a review tab has no file");
        assert!(!view.model().is_dirty(), "nothing in it is to save");
        let proposals = view.proposals();
        assert_eq!(proposals.len(), 1);
        let hunks = proposals[0].borrow().hunks(view.model().snapshot());
        hunks.into_iter().map(|hunk| hunk.id).collect()
    });
    (editor, hunks)
}

fn caret(s: &mut Lsp, editor: &Entity<EditorView>) -> usize {
    editor.read_with(&s.cx, |view, _| view.model().primary_selection().head())
}

/// The info prompt's message, answered.
fn said(s: &mut Lsp) -> Option<String> {
    let message = s.cx.pending_prompt().map(|(message, _)| message);
    if message.is_some() {
        s.cx.simulate_prompt_answer("OK");
        s.draw();
    }
    message
}

fn asked(s: &Lsp, method: &str) -> usize {
    (s.fake.records().iter())
        .filter(|record| matches!(record, Record::Request { method: m, .. } if m == method))
        .count()
}

/// `main.rs` and `other.rs`'s renames: three edits in two files.
fn two_files(s: &Lsp) -> Value {
    json!({
        s.uri("src/main.rs").as_str(): [
            { "range": foo(0, 3), "newText": "bar" },
            { "range": foo(2, 4), "newText": "bar" },
        ],
        s.uri("src/other.rs").as_str(): [{ "range": foo(0, 9), "newText": "bar" }],
    })
}

#[gpui::test]
fn a_rename_across_files_waits_in_the_panel(cx: &mut TestAppContext) {
    let (mut s, editor) = setup(cx);
    let changes = two_files(&s);
    rename(&mut s, changes);
    s.until("the preview", |s| header(s).starts_with("Rename"));

    assert_eq!(header(&mut s), "Rename foo to bar: 2 files, 3 edits");
    assert_eq!(message_shown(&mut s), None);
    assert_eq!(
        rows(&mut s),
        [
            ("src/main.rs".into(), 2, 0, 0),
            ("src/other.rs".into(), 1, 0, 0)
        ]
    );
    // Nothing changed, nor will without Apply.
    assert_eq!(text(&mut s, &editor), MAIN);
    assert_eq!(disk(&s, "src/other.rs"), OTHER);
    assert_eq!(caret(&mut s, &editor), CARET);
    assert_eq!(said(&mut s), None);
    s.draw();
    assert_eq!(text(&mut s, &editor), MAIN);
    assert_eq!(disk(&s, "src/other.rs"), OTHER);
}

#[gpui::test]
fn a_rename_in_its_own_file_applies_at_once(cx: &mut TestAppContext) {
    let (mut s, editor) = setup(cx);
    let main = s.uri("src/main.rs");
    let changes = json!({ main.as_str(): [
        { "range": foo(0, 3), "newText": "bar" },
        { "range": foo(2, 4), "newText": "bar" },
    ]});
    rename(&mut s, changes);
    s.until("the rename", |s| text(s, &editor) != MAIN);

    let renamed = "fn bar() {}\nfn main() {\n    bar();\n}\n";
    assert_eq!(text(&mut s, &editor), renamed);
    assert_eq!(caret(&mut s, &editor), CARET);
    let previewed = s.workbench.read_with(&s.cx, |w, _| w.ai_changes.is_some());
    assert!(!previewed, "nothing is previewed");
}

#[gpui::test]
fn apply_makes_the_kept_edits_one_undo_per_buffer(cx: &mut TestAppContext) {
    let (mut s, editor) = setup(cx);
    let lib = editors_of(&mut s, "src/lib.rs")[0].0.clone();
    let changes = json!({
        s.uri("src/main.rs").as_str(): [
            { "range": foo(0, 3), "newText": "bar" },
            { "range": foo(2, 4), "newText": "bar" },
        ],
        s.uri("src/lib.rs").as_str(): [
            { "range": foo(0, 13), "newText": "bar" },
            { "range": foo(1, 13), "newText": "bar" },
        ],
        s.uri("src/other.rs").as_str(): [
            { "range": foo(0, 9), "newText": "bar" },
            { "range": foo(1, 9), "newText": "bar" },
        ],
        s.uri("src/third.rs").as_str(): [{ "range": foo(0, 9), "newText": "bar" }],
    });
    rename(&mut s, changes);
    s.until("the preview", |s| header(s).starts_with("Rename"));
    assert_eq!(header(&mut s), "Rename foo to bar: 4 files, 7 edits");

    // A whole file left out from the panel.
    click(&mut s, "ai-changes-reject-src/third.rs");
    assert_eq!(row(&mut s, "src/third.rs"), (0, 0, 1));
    assert_eq!(header(&mut s), "Rename foo to bar: 3 files, 6 edits");

    // A hunk of a closed file left out in its review tab.
    click_row(&mut s, "ai-changes-row-src/other.rs");
    let (review, hunks) = review_tab(&mut s);
    let first = review.clone();
    assert_eq!(text(&mut s, &review), OTHER);
    assert_eq!(hunks.len(), 2);
    review.update(&mut s.cx, |view, cx| {
        view.reject_hunk(hunks[1], cx).unwrap()
    });
    s.draw();
    assert_eq!(row(&mut s, "src/other.rs"), (1, 0, 1));

    // And one of an open file, in a copy: its buffer isn't touched.
    click_row(&mut s, "ai-changes-row-src/lib.rs");
    let (review, hunks) = review_tab(&mut s);
    assert_ne!(review, lib);
    review.update(&mut s.cx, |view, cx| {
        view.reject_hunk(hunks[0], cx).unwrap()
    });
    s.draw();
    assert_eq!(row(&mut s, "src/lib.rs"), (1, 0, 1));
    assert_eq!(header(&mut s), "Rename foo to bar: 3 files, 4 edits");
    assert_eq!(text(&mut s, &lib), LIB);
    assert_eq!(text(&mut s, &editor), MAIN);
    assert_eq!(caret(&mut s, &editor), CARET);

    click(&mut s, "ai-changes-apply");
    assert_eq!(said(&mut s), None);
    let renamed = "fn bar() {}\nfn main() {\n    bar();\n}\n";
    assert_eq!(text(&mut s, &editor), renamed);
    assert_eq!(
        text(&mut s, &lib),
        "pub fn a() { foo(); }\npub fn b() { bar(); }\n"
    );
    assert_eq!(
        disk(&s, "src/other.rs"),
        "fn h() { bar(); }\nfn i() { foo(); }\n"
    );
    assert_eq!(disk(&s, "src/third.rs"), THIRD);
    // The review tabs are gone, and so is the preview.
    assert!(!is_open(&mut s, &first));
    assert!(!is_open(&mut s, &review));
    assert_eq!(editors_of(&mut s, "src/lib.rs").len(), 1);
    assert_eq!(message_shown(&mut s), Some("No pending changes"));
    // The caret moved once it was applied, in the editor focused.
    let call = renamed.rfind("bar").unwrap();
    assert_eq!(caret(&mut s, &editor), call + "fo".len());
    assert!(s.cx.update(|window, cx| editor.focus_handle(cx).is_focused(window)));

    // One undo takes each open file's part back whole.
    s.cx.simulate_keystrokes("cmd-z");
    s.draw();
    assert_eq!(text(&mut s, &editor), MAIN);
    let pane = s.workbench.read_with(&s.cx, |w, _| w.pane().clone());
    s.cx.update(|window, cx| {
        let at = (pane.read(cx).tabs().iter()).position(|tab| tab.editor() == Some(&lib));
        pane.update(cx, |pane, cx| pane.activate(at.unwrap(), window, cx));
        window.focus(&lib.focus_handle(cx), cx);
    });
    s.draw();
    s.cx.simulate_keystrokes("cmd-z");
    s.draw();
    assert_eq!(text(&mut s, &lib), LIB);
}

#[gpui::test]
fn an_edited_buffer_refuses_apply_and_keeps_the_preview(cx: &mut TestAppContext) {
    let (mut s, editor) = setup(cx);
    let changes = two_files(&s);
    rename(&mut s, changes);
    s.until("the preview", |s| header(s).starts_with("Rename"));

    s.cx.update(|_, cx| {
        editor.update(cx, |view, cx| {
            view.model_mut().set_cursor(0);
            view.model_mut().insert("// ");
            cx.notify();
        })
    });
    s.draw();
    let edited = text(&mut s, &editor);
    click(&mut s, "ai-changes-apply");
    let stale = format!("{} has changed since", s.path("src/main.rs").display());
    assert_eq!(said(&mut s), Some(stale));
    assert_eq!(text(&mut s, &editor), edited);
    assert_eq!(disk(&s, "src/other.rs"), OTHER);
    assert_eq!(header(&mut s), "Rename foo to bar: 2 files, 3 edits");
    assert_eq!(rows(&mut s).len(), 2);
}

#[gpui::test]
#[cfg(unix)]
fn a_failure_after_part_was_made_ends_the_preview(cx: &mut TestAppContext) {
    use std::os::unix::fs::PermissionsExt as _;
    let (mut s, editor) = setup(cx);
    // A closed file in a folder that can't be written to.
    fs::create_dir(s.path("src/locked")).unwrap();
    fs::write(s.path("src/locked/l.rs"), THIRD).unwrap();
    let locked = s.path("src/locked");
    fs::set_permissions(&locked, fs::Permissions::from_mode(0o555)).unwrap();
    let changes = json!({
        s.uri("src/main.rs").as_str(): [{ "range": foo(0, 3), "newText": "bar" }],
        s.uri("src/locked/l.rs").as_str(): [{ "range": foo(0, 9), "newText": "bar" }],
    });
    rename(&mut s, changes);
    s.until("the preview", |s| header(s).starts_with("Rename"));

    click(&mut s, "ai-changes-apply");
    let message = said(&mut s).expect("the failure is said");
    fs::set_permissions(&locked, fs::Permissions::from_mode(0o755)).unwrap();
    assert!(message.contains("can't write"), "{message}");
    assert!(message.ends_with("Part of the edit was made."), "{message}");
    // The open file's part was made: the preview can't be applied again.
    assert_eq!(text(&mut s, &editor), MAIN.replacen("foo", "bar", 1));
    assert_eq!(disk(&s, "src/locked/l.rs"), THIRD);
    assert_eq!(message_shown(&mut s), Some("No pending changes"));
}

#[gpui::test]
fn an_edit_renaming_a_file_is_decided_whole(cx: &mut TestAppContext) {
    let (mut s, editor) = setup(cx);
    let main = s.uri("src/main.rs");
    reply(
        &s,
        Rename::METHOD,
        json!({ "documentChanges": [
            { "textDocument": { "uri": main.as_str(), "version": null }, "edits": [
                { "range": foo(0, 3), "newText": "bar" },
                { "range": foo(2, 4), "newText": "bar" },
            ]},
            { "kind": "rename", "oldUri": s.uri("src/other.rs").as_str(),
              "newUri": s.uri("src/renamed.rs").as_str() },
        ]}),
    );
    run(&mut s, "lsp.rename");
    s.until("the picker", |s| {
        s.workbench
            .read_with(&s.cx, |w, _| w.palette == Some(Palette::Rename))
    });
    s.cx.simulate_input("bar");
    s.draw();
    s.cx.simulate_keystrokes("enter");
    s.until("the preview", |s| previewing(s, "Rename"));

    assert_eq!(header(&mut s), "Rename foo to bar: 2 files, 3 edits");
    let panel = panel(&mut s);
    let listed = panel.read_with(&s.cx, |panel, _| panel.listed_operations());
    assert_eq!(
        listed,
        [None, Some("Rename to src/renamed.rs".to_owned())],
        "the file's row, then the operation's"
    );
    // Neither the file nor the operation can be decided alone.
    s.draw();
    assert!(s.cx.debug_bounds("ai-changes-reject-src/main.rs").is_none());
    assert!(
        s.cx.debug_bounds("ai-changes-reject-src/other.rs")
            .is_none()
    );
    assert_eq!(text(&mut s, &editor), MAIN);
    assert_eq!(disk(&s, "src/other.rs"), OTHER);

    click(&mut s, "ai-changes-apply");
    assert_eq!(said(&mut s), None);
    assert_eq!(text(&mut s, &editor), MAIN.replace("foo", "bar"));
    assert!(!s.path("src/other.rs").exists());
    assert_eq!(disk(&s, "src/renamed.rs"), OTHER);
}

/// A code action whose resolved edit renames `foo` in `main.rs` and
/// `other.rs`, then runs `fix.all`.
fn code_action(s: &mut Lsp) {
    let command = json!({ "title": "Fix all", "command": "fix.all" });
    reply(
        s,
        CodeActionRequest::METHOD,
        json!([{ "title": "Fix across files", "kind": "quickfix", "data": 1, "command": command }]),
    );
    let edit = json!({ "changes": {
        s.uri("src/main.rs").as_str(): [{ "range": foo(0, 3), "newText": "fixed" }],
        s.uri("src/other.rs").as_str(): [{ "range": foo(0, 9), "newText": "fixed" }],
    }});
    reply(
        s,
        CodeActionResolveRequest::METHOD,
        json!({ "title": "Fix across files", "kind": "quickfix", "edit": edit, "command": command }),
    );
    reply(s, ExecuteCommand::METHOD, Value::Null);
    run(s, "lsp.code_actions");
    s.until("the picker", |s| {
        s.workbench
            .read_with(&s.cx, |w, _| w.palette == Some(Palette::CodeActions))
    });
    s.cx.simulate_keystrokes("enter");
    s.until("the preview", |s| previewing(s, "Fix"));
}

#[gpui::test]
fn a_code_action_runs_its_command_after_apply(cx: &mut TestAppContext) {
    let (mut s, editor) = setup(cx);
    code_action(&mut s);
    assert_eq!(header(&mut s), "Fix across files: 2 files, 2 edits");
    assert_eq!(asked(&s, CodeActionResolveRequest::METHOD), 1);
    assert_eq!(asked(&s, ExecuteCommand::METHOD), 0);
    assert_eq!(text(&mut s, &editor), MAIN);
    assert_eq!(disk(&s, "src/other.rs"), OTHER);

    click(&mut s, "ai-changes-apply");
    s.until("the command", |s| asked(s, ExecuteCommand::METHOD) == 1);
    assert_eq!(text(&mut s, &editor), MAIN.replacen("foo", "fixed", 1));
    assert_eq!(disk(&s, "src/other.rs"), OTHER.replacen("foo", "fixed", 1));
    assert_eq!(said(&mut s), None);
}

#[gpui::test]
fn cancel_changes_nothing_and_runs_no_command(cx: &mut TestAppContext) {
    let (mut s, editor) = setup(cx);
    code_action(&mut s);
    click_row(&mut s, "ai-changes-row-src/other.rs");
    let (review, _) = review_tab(&mut s);

    click(&mut s, "ai-changes-cancel");
    assert_eq!(message_shown(&mut s), Some("No pending changes"));
    assert!(!is_open(&mut s, &review));
    s.draw();
    assert_eq!(asked(&s, ExecuteCommand::METHOD), 0);
    assert_eq!(text(&mut s, &editor), MAIN);
    assert_eq!(disk(&s, "src/other.rs"), OTHER);
    assert_eq!(said(&mut s), None);
}

/// Renames `foo` to `bar` with the server's `documentChanges`, and waits
/// for the preview.
fn rename_changes(s: &mut Lsp, changes: Value) {
    reply(s, Rename::METHOD, json!({ "documentChanges": changes }));
    run(s, "lsp.rename");
    s.until("the picker", |s| {
        s.workbench
            .read_with(&s.cx, |w, _| w.palette == Some(Palette::Rename))
    });
    s.cx.simulate_input("bar");
    s.draw();
    s.cx.simulate_keystrokes("enter");
    s.until("the preview", |s| previewing(s, "Rename"));
}

/// `name`'s text edits, against the server's open document.
fn document(s: &Lsp, name: &str, edits: Value) -> Value {
    json!({ "textDocument": { "uri": s.uri(name).as_str(), "version": null }, "edits": edits })
}

#[gpui::test]
fn a_review_copy_saves_nothing_over_its_file(cx: &mut TestAppContext) {
    let (mut s, _) = setup(cx);
    let changes = two_files(&s);
    rename(&mut s, changes);
    s.until("the preview", |s| header(s).starts_with("Rename"));
    click_row(&mut s, "ai-changes-row-src/other.rs");
    let (review, _) = review_tab(&mut s);
    click(&mut s, "ai-changes-accept-src/other.rs");
    // The tab shows what was accepted, and still has nothing to save.
    assert_eq!(text(&mut s, &review), OTHER.replacen("foo", "bar", 1));
    assert!(!review.read_with(&s.cx, |view, _| view.model().is_dirty()));
    let title = review.read_with(&s.cx, |view, cx| {
        use kiln_shell::PaneItem as _;
        view.tab_title(cx).to_string()
    });
    assert_eq!(title, "other.rs (preview)");

    // Neither Save nor Save As, by key or command, writes it anywhere.
    s.cx.update(|window, cx| window.focus(&review.focus_handle(cx), cx));
    s.cx.simulate_keystrokes("cmd-s");
    s.draw();
    assert!(!s.cx.did_prompt_for_new_path(), "no save panel");
    s.cx.update(|window, cx| window.focus(&review.focus_handle(cx), cx));
    s.run("workbench.save_as");
    assert!(!s.cx.did_prompt_for_new_path(), "no save panel");
    // Not even to the file it's a copy of, answered as a panel would be.
    let original = s.path("src/other.rs");
    s.workbench.update_in(&mut s.cx, |workbench, window, cx| {
        workbench.save_editor_as(&review, original, window, cx)
    });
    s.draw();
    let copy = s.path("src/copy.rs");
    let written = review.update(&mut s.cx, |view, cx| {
        view.write_copy(copy.clone(), true, cx)
    });
    s.draw();
    assert!(s.cx.foreground_executor().block_test(written).is_err());
    assert!(!copy.exists());
    while said(&mut s).is_some() {}
    assert_eq!(disk(&s, "src/other.rs"), OTHER);
    click(&mut s, "ai-changes-cancel");
    assert_eq!(disk(&s, "src/other.rs"), OTHER);
}

#[gpui::test]
fn a_target_saved_under_another_name_refuses_apply_first(cx: &mut TestAppContext) {
    let (mut s, _) = setup(cx);
    let lib = editors_of(&mut s, "src/lib.rs")[0].0.clone();
    let changes = json!([
        { "kind": "create", "uri": s.uri("src/new.rs").as_str() },
        document(&s, "src/lib.rs", json!([{ "range": foo(0, 13), "newText": "bar" }])),
    ]);
    rename_changes(&mut s, changes);
    // lib.rs saved as another file meanwhile, its text as it was.
    let moved = s.path("src/lib2.rs");
    s.workbench.update_in(&mut s.cx, |workbench, window, cx| {
        workbench.save_editor_as(&lib, moved.clone(), window, cx)
    });
    s.until("the save", |s| {
        lib.read_with(&s.cx, |view, _| {
            view.model().path() == Some(moved.as_path())
        })
    });

    click(&mut s, "ai-changes-apply");
    let refused = format!(
        "{} has been saved under another name since",
        s.path("src/lib.rs").display()
    );
    assert_eq!(said(&mut s), Some(refused));
    assert!(!s.path("src/new.rs").exists(), "nothing was made");
    assert!(header(&mut s).starts_with("Rename foo to bar"));
}

#[gpui::test]
fn a_closed_target_opened_since_refuses_apply_first(cx: &mut TestAppContext) {
    let (mut s, _) = setup(cx);
    let changes = json!([
        { "kind": "create", "uri": s.uri("src/new.rs").as_str() },
        document(&s, "src/other.rs", json!([{ "range": foo(0, 9), "newText": "bar" }])),
    ]);
    rename_changes(&mut s, changes);
    // Opened and edited, its file as it was.
    s.open("src/other.rs");
    let other = editors_of(&mut s, "src/other.rs")[0].0.clone();
    s.cx.update(|_, cx| other.update(cx, |view, _| view.model_mut().insert("// ")));
    s.draw();

    click(&mut s, "ai-changes-apply");
    let refused = format!("{} has been opened since", s.path("src/other.rs").display());
    assert_eq!(said(&mut s), Some(refused));
    assert!(!s.path("src/new.rs").exists(), "nothing was made");
    assert_eq!(disk(&s, "src/other.rs"), OTHER);
    assert!(header(&mut s).starts_with("Rename foo to bar"));
}

#[gpui::test]
fn the_accept_commands_work_in_a_review_tab_and_typing_doesnt(cx: &mut TestAppContext) {
    let (mut s, editor) = setup(cx);
    let changes = two_files(&s);
    rename(&mut s, changes);
    s.until("the preview", |s| header(s).starts_with("Rename"));
    click_row(&mut s, "ai-changes-row-src/main.rs");
    let (review, _) = review_tab(&mut s);
    // The caret is on the first pending hunk.
    s.cx.update(|window, cx| window.focus(&review.focus_handle(cx), cx));
    run(&mut s, "proposal.accept_hunk");
    assert_eq!(row(&mut s, "src/main.rs"), (1, 1, 0));
    s.cx.simulate_input("typed");
    s.draw();
    assert!(!text(&mut s, &review).contains("typed"), "it's read-only");
    run(&mut s, "proposal.accept_all");
    assert_eq!(row(&mut s, "src/main.rs"), (0, 2, 0));
    assert_eq!(text(&mut s, &editor), MAIN, "only the copy has them");
}

#[gpui::test]
fn a_closed_file_changed_since_refuses_apply(cx: &mut TestAppContext) {
    let (mut s, editor) = setup(cx);
    let changes = two_files(&s);
    rename(&mut s, changes);
    s.until("the preview", |s| header(s).starts_with("Rename"));
    // Edited outside, where the server's positions now land elsewhere.
    let outside = format!("// moved\n{OTHER}");
    fs::write(s.path("src/other.rs"), &outside).unwrap();

    click(&mut s, "ai-changes-apply");
    let stale = format!("{} has changed since", s.path("src/other.rs").display());
    assert_eq!(said(&mut s), Some(stale));
    assert_eq!(disk(&s, "src/other.rs"), outside);
    assert_eq!(text(&mut s, &editor), MAIN);
    assert_eq!(header(&mut s), "Rename foo to bar: 2 files, 3 edits");
}

#[gpui::test]
fn a_stale_buffer_after_an_operation_refuses_apply_first(cx: &mut TestAppContext) {
    let (mut s, editor) = setup(cx);
    let changes = json!([
        { "kind": "rename", "oldUri": s.uri("src/other.rs").as_str(),
          "newUri": s.uri("src/renamed.rs").as_str() },
        document(&s, "src/main.rs", json!([{ "range": foo(0, 3), "newText": "bar" }])),
    ]);
    rename_changes(&mut s, changes);
    s.cx.update(|_, cx| {
        editor.update(cx, |view, cx| {
            view.model_mut().set_cursor(0);
            view.model_mut().insert("// ");
            cx.notify();
        })
    });
    s.draw();

    click(&mut s, "ai-changes-apply");
    let stale = format!("{} has changed since", s.path("src/main.rs").display());
    assert_eq!(said(&mut s), Some(stale));
    // The rename before it wasn't made, and the preview stays.
    assert_eq!(disk(&s, "src/other.rs"), OTHER);
    assert!(!s.path("src/renamed.rs").exists());
    assert_eq!(header(&mut s), "Rename foo to bar: 2 files, 2 edits");
}

#[gpui::test]
#[cfg(unix)]
fn a_write_through_a_symlink_then_a_failure_ends_the_preview(cx: &mut TestAppContext) {
    use std::os::unix::fs::PermissionsExt as _;
    let (mut s, _) = setup(cx);
    fs::write(s.path("src/real.rs"), THIRD).unwrap();
    std::os::unix::fs::symlink(s.path("src/real.rs"), s.path("src/link.rs")).unwrap();
    fs::create_dir(s.path("src/locked")).unwrap();
    fs::write(s.path("src/locked/l.rs"), THIRD).unwrap();
    let locked = s.path("src/locked");
    fs::set_permissions(&locked, fs::Permissions::from_mode(0o555)).unwrap();
    // Written in this order: through the link, then the one that fails.
    let changes = json!({
        s.uri("src/link.rs").as_str(): [{ "range": foo(0, 9), "newText": "bar" }],
        s.uri("src/locked/l.rs").as_str(): [{ "range": foo(0, 9), "newText": "bar" }],
    });
    rename(&mut s, changes);
    s.until("the preview", |s| header(s).starts_with("Rename"));

    click(&mut s, "ai-changes-apply");
    let message = said(&mut s).expect("the failure is said");
    fs::set_permissions(&locked, fs::Permissions::from_mode(0o755)).unwrap();
    assert!(message.ends_with("Part of the edit was made."), "{message}");
    assert_eq!(disk(&s, "src/real.rs"), THIRD.replacen("foo", "bar", 1));
    assert_eq!(message_shown(&mut s), Some("No pending changes"));
}

#[gpui::test]
fn a_code_action_whose_source_closed_says_why_its_command_failed(cx: &mut TestAppContext) {
    let (mut s, editor) = setup(cx);
    let lib = editors_of(&mut s, "src/lib.rs")[0].0.clone();
    let command = json!({ "title": "Fix all", "command": "fix.all" });
    reply(
        &s,
        CodeActionRequest::METHOD,
        json!([{ "title": "Fix elsewhere", "kind": "quickfix", "data": 1, "command": command }]),
    );
    let edit = json!({ "changes": {
        s.uri("src/lib.rs").as_str(): [{ "range": foo(0, 13), "newText": "fixed" }],
        s.uri("src/other.rs").as_str(): [{ "range": foo(0, 9), "newText": "fixed" }],
    }});
    reply(
        &s,
        CodeActionResolveRequest::METHOD,
        json!({ "title": "Fix elsewhere", "kind": "quickfix", "edit": edit, "command": command }),
    );
    reply(&s, ExecuteCommand::METHOD, Value::Null);
    run(&mut s, "lsp.code_actions");
    s.until("the picker", |s| {
        s.workbench
            .read_with(&s.cx, |w, _| w.palette == Some(Palette::CodeActions))
    });
    s.cx.simulate_keystrokes("enter");
    s.until("the preview", |s| previewing(s, "Fix"));
    // The file it was asked in closes meanwhile.
    let main = editor.read_with(&s.cx, |view, _| view.model().buffer().id());
    let pane = s.workbench.read_with(&s.cx, |w, _| w.pane().clone());
    s.cx.update(|window, cx| pane.update(cx, |pane, cx| pane.discard_buffers(&[main], window, cx)));
    s.draw();

    click(&mut s, "ai-changes-apply");
    assert_eq!(text(&mut s, &lib), LIB.replacen("foo", "fixed", 1));
    assert_eq!(disk(&s, "src/other.rs"), OTHER.replacen("foo", "fixed", 1));
    // Its command is asked for, through the server it came from: it can't
    // run, and that's said rather than skipped.
    s.until("the reason", |s| s.cx.pending_prompt().is_some());
    assert!(said(&mut s).is_some());
}

#[gpui::test]
fn a_buffer_edited_around_an_operation_takes_one_undo(cx: &mut TestAppContext) {
    let (mut s, editor) = setup(cx);
    let changes = json!([
        document(&s, "src/main.rs", json!([{ "range": foo(0, 3), "newText": "bar" }])),
        { "kind": "create", "uri": s.uri("src/new.rs").as_str() },
        document(&s, "src/main.rs", json!([{ "range": foo(2, 4), "newText": "bar" }])),
    ]);
    rename_changes(&mut s, changes);
    click(&mut s, "ai-changes-apply");
    assert_eq!(said(&mut s), None);
    assert_eq!(text(&mut s, &editor), MAIN.replace("foo", "bar"));
    assert!(s.path("src/new.rs").exists());

    s.cx.update(|window, cx| window.focus(&editor.focus_handle(cx), cx));
    s.cx.simulate_keystrokes("cmd-z");
    s.draw();
    assert_eq!(text(&mut s, &editor), MAIN);
}

#[gpui::test]
fn an_operation_only_preview_lists_it(cx: &mut TestAppContext) {
    let (mut s, _) = setup(cx);
    let changes = json!([
        { "kind": "rename", "oldUri": s.uri("src/other.rs").as_str(),
          "newUri": s.uri("src/renamed.rs").as_str() },
    ]);
    rename_changes(&mut s, changes);
    assert_eq!(header(&mut s), "Rename foo to bar: 1 file, 1 edit");
    assert_eq!(message_shown(&mut s), None);
    s.draw();
    assert!(
        (s.cx.debug_bounds("ai-changes-operation-0-src/other.rs")).is_some(),
        "the operation's row is drawn"
    );
    click(&mut s, "ai-changes-apply");
    assert_eq!(said(&mut s), None);
    assert_eq!(disk(&s, "src/renamed.rs"), OTHER);
}

#[gpui::test]
fn a_hunk_accepted_in_its_review_tab_is_counted_and_kept(cx: &mut TestAppContext) {
    let (mut s, editor) = setup(cx);
    let changes = two_files(&s);
    rename(&mut s, changes);
    s.until("the preview", |s| header(s).starts_with("Rename"));
    click_row(&mut s, "ai-changes-row-src/main.rs");
    let (review, hunks) = review_tab(&mut s);
    assert!(review.read_with(&s.cx, |view, _| view.accepts_when_read_only()));
    // As its Accept button does.
    review.update(&mut s.cx, |view, cx| {
        view.accept_hunk(hunks[0], cx).unwrap()
    });
    s.draw();
    assert_eq!(row(&mut s, "src/main.rs"), (1, 1, 0));
    assert!(!review.read_with(&s.cx, |view, _| view.model().is_dirty()));
    assert_eq!(text(&mut s, &editor), MAIN, "only the copy has it");

    click(&mut s, "ai-changes-apply");
    assert_eq!(said(&mut s), None);
    assert_eq!(text(&mut s, &editor), MAIN.replace("foo", "bar"));
}

#[gpui::test]
fn a_split_review_tab_shows_and_decides_its_hunks(cx: &mut TestAppContext) {
    let (mut s, _) = setup(cx);
    let changes = two_files(&s);
    rename(&mut s, changes);
    s.until("the preview", |s| header(s).starts_with("Rename"));
    click_row(&mut s, "ai-changes-row-src/main.rs");
    let (review, _) = review_tab(&mut s);
    s.cx.update(|window, cx| window.focus(&review.focus_handle(cx), cx));
    s.run("pane.split_right");
    let (split, hunks) = review_tab(&mut s);
    assert_ne!(split, review);
    assert_eq!(hunks.len(), 2);

    split.update(&mut s.cx, |view, cx| {
        view.reject_hunk(hunks[1], cx).unwrap()
    });
    s.draw();
    assert_eq!(row(&mut s, "src/main.rs"), (1, 0, 1));
    assert_eq!(header(&mut s), "Rename foo to bar: 2 files, 2 edits");
}
