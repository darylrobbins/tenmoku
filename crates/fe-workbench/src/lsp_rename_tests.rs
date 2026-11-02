//! `lsp.rename`: the fake server's answers to `prepareRename` and
//! `rename`, applied to an open file and a closed one.

use std::{fs, ops::Range, time::Duration};

use fe_command::{ContextFrame, ContextStack, Invocation};
use fe_gpui::{EditorView, commands::RunCommand};
use fe_lsp::fake::{InitializeReply, Reply, Script, Scripted, initialize_result};
use gpui::{Entity, EntityInputHandler as _, Focusable as _, TestAppContext};
use lsp_types::{
    OneOf, PositionEncodingKind, RenameOptions, ServerCapabilities,
    request::{PrepareRenameRequest, Rename, Request as _},
};
use serde_json::{Value, json};

use super::RenamePicker;
use crate::picker::PickerDelegate as _;
use crate::{picker::Picker, tests::lsp::Lsp, workbench::Palette};

/// `src/main.rs` as the tests open it: `foo` twice.
const MAIN: &str = "fn foo() {}\nfn main() {\n    foo();\n}\n";
/// A closed file using it.
const OTHER: &str = "fn h() { foo(); }\n";
/// The caret in the call's `foo`, after its `fo`.
const CARET: usize = 30;

/// A server that prepares renames.
fn renamer() -> Script {
    let mut init = initialize_result(Some(PositionEncodingKind::UTF8));
    init.capabilities = ServerCapabilities {
        rename_provider: Some(OneOf::Right(RenameOptions {
            prepare_provider: Some(true),
            work_done_progress_options: Default::default(),
        })),
        ..init.capabilities
    };
    Script {
        initialize: InitializeReply::Result(Box::new(init)),
        ..Script::default()
    }
}

fn reply(s: &Lsp, method: &str, reply: Reply) {
    s.fake.set_reply(method, Scripted::now(reply));
}

/// The range of `foo` at `line`, from `column`.
fn foo(line: u32, column: u32) -> Value {
    json!({
        "start": { "line": line, "character": column },
        "end": { "line": line, "character": column + 3 },
    })
}

/// The project with `MAIN` open, its caret in the call, and the trusted
/// root's server renaming.
fn setup(cx: &mut TestAppContext) -> (Lsp, Entity<EditorView>) {
    setup_with(Lsp::with_scripts(cx, renamer(), Script::default()))
}

fn setup_with(mut s: Lsp) -> (Lsp, Entity<EditorView>) {
    s.set_trust(true);
    fs::write(s.path("src/main.rs"), MAIN).unwrap();
    fs::write(s.path("src/other.rs"), OTHER).unwrap();
    s.open("src/main.rs");
    s.fake
        .wait_for_document(&s.uri("src/main.rs"), "didOpen", |r| r.is_some());
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
    reply(&s, PrepareRenameRequest::METHOD, Reply::Result(foo(2, 4)));
    (s, editor)
}

/// Runs `lsp.rename` as the editor's context would.
fn rename(s: &mut Lsp) {
    let context = ContextStack::new()
        .with(ContextFrame::new(crate::KEY_CONTEXT))
        .with(ContextFrame::new(fe_editor::commands::CONTEXT));
    let run = RunCommand(Invocation::of("lsp.rename").unwrap().context(context));
    s.cx.update(|window, cx| window.dispatch_action(Box::new(run), cx));
    s.draw();
}

fn text(s: &mut Lsp, editor: &Entity<EditorView>) -> String {
    editor.read_with(&s.cx, |view, _| view.model().text().to_string())
}

fn caret(s: &mut Lsp, editor: &Entity<EditorView>) -> usize {
    editor.read_with(&s.cx, |view, _| view.model().primary_selection().head())
}

fn focused(s: &mut Lsp, editor: &Entity<EditorView>) -> bool {
    s.cx.update(|window, cx| editor.focus_handle(cx).is_focused(window))
}

/// The rename picker, if it shows.
fn picker(s: &mut Lsp) -> Option<Entity<Picker<RenamePicker>>> {
    s.workbench.read_with(&s.cx, |workbench, cx| {
        let layer = workbench.shell().read(cx).modals().read(cx);
        (workbench.palette == Some(Palette::Rename))
            .then(|| layer.modal_view().unwrap().clone().downcast().unwrap())
    })
}

/// The picker's input, row count and status.
fn shown(s: &mut Lsp) -> (String, usize, String) {
    let picker = picker(s).expect("the rename picker shows");
    picker.read_with(&s.cx, |picker, cx| {
        (
            picker.input().read(cx).text().to_owned(),
            picker.delegate.match_count(),
            picker.delegate.status_text(),
        )
    })
}

/// Whether the picker's input has its text selected.
fn selected(s: &mut Lsp) -> bool {
    let picker = picker(s).expect("the rename picker shows");
    picker.read_with(&s.cx, |picker, cx| picker.input().read(cx).selected())
}

/// The message of the info prompt showing, answered.
fn said(s: &mut Lsp) -> Option<String> {
    let message = s.cx.pending_prompt().map(|(message, _)| message);
    if message.is_some() {
        s.cx.simulate_prompt_answer("OK");
        s.draw();
    }
    message
}

/// The requests of `method` the fake has had.
fn asked(s: &Lsp, method: &str) -> usize {
    (s.fake.records().iter())
        .filter(|record| {
            matches!(record, fe_lsp::fake::Record::Request { method: m, .. } if m == method)
        })
        .count()
}

#[gpui::test]
fn renames_across_files(cx: &mut TestAppContext) {
    let (mut s, editor) = setup(cx);
    let (main, other) = (s.uri("src/main.rs"), s.uri("src/other.rs"));
    reply(
        &s,
        Rename::METHOD,
        Reply::Result(json!({ "changes": {
            main.as_str(): [
                { "range": foo(0, 3), "newText": "barista" },
                { "range": foo(2, 4), "newText": "barista" },
            ],
            other.as_str(): [{ "range": foo(0, 9), "newText": "barista" }],
        }})),
    );

    rename(&mut s);
    s.until("the picker", |s| picker(s).is_some());
    // The placeholder, all selected, and no row yet.
    assert_eq!(
        shown(&mut s),
        ("foo".into(), 0, "Enter to rename, Escape to cancel".into())
    );
    assert!(selected(&mut s), "the placeholder is selected");
    s.cx.simulate_input("barista");
    s.draw();
    assert_eq!(shown(&mut s).0, "barista");
    assert_eq!(shown(&mut s).1, 1);
    let sent = (s.fake.records().into_iter()).find_map(|record| match record {
        fe_lsp::fake::Record::Request { method, params, .. } if method == Rename::METHOD => {
            Some(params)
        }
        _ => None,
    });
    assert!(sent.is_none(), "nothing is sent before enter");

    s.cx.simulate_keystrokes("enter");
    s.until("the picker to close", |s| picker(s).is_none());
    // Edits to another file wait in the AI Changes panel for Apply.
    assert_eq!(text(&mut s, &editor), MAIN);
    s.draw();
    let apply = (s.cx.debug_bounds("ai-changes-apply")).expect("the preview shows");
    s.cx.simulate_click(apply.center(), gpui::Modifiers::none());
    s.draw();

    let renamed = "fn barista() {}\nfn main() {\n    barista();\n}\n";
    assert_eq!(text(&mut s, &editor), renamed);
    assert_eq!(
        fs::read_to_string(s.path("src/other.rs")).unwrap(),
        "fn h() { barista(); }\n"
    );
    // Focus is back, the caret still in the call's symbol.
    assert!(focused(&mut s, &editor));
    let call = renamed.rfind("barista").unwrap();
    assert_eq!(caret(&mut s, &editor), call + "fo".len());
    assert_eq!(said(&mut s), None);

    // `cmd-z` takes back the open file's part; the closed one stays.
    s.cx.simulate_keystrokes("cmd-z");
    s.draw();
    assert_eq!(text(&mut s, &editor), MAIN);
    assert_eq!(
        fs::read_to_string(s.path("src/other.rs")).unwrap(),
        "fn h() { barista(); }\n"
    );
}

#[gpui::test]
fn nothing_and_failures(cx: &mut TestAppContext) {
    let (mut s, editor) = setup(cx);
    let main = s.uri("src/main.rs");
    reply(
        &s,
        Rename::METHOD,
        Reply::Result(json!({ "changes": {
            main.as_str(): [{ "range": foo(0, 3), "newText": "bar" }],
        }})),
    );

    // Nothing there.
    reply(&s, PrepareRenameRequest::METHOD, Reply::Result(Value::Null));
    rename(&mut s);
    s.until("the prompt", |s| s.cx.pending_prompt().is_some());
    assert_eq!(said(&mut s).as_deref(), Some("Nothing to rename here"));
    assert!(picker(&mut s).is_none());

    // The server's refusal, in its words.
    reply(
        &s,
        PrepareRenameRequest::METHOD,
        Reply::Error {
            code: -32602,
            message: "cannot rename a keyword".into(),
        },
    );
    rename(&mut s);
    s.until("the prompt", |s| s.cx.pending_prompt().is_some());
    assert_eq!(said(&mut s).as_deref(), Some("cannot rename a keyword"));
    assert!(picker(&mut s).is_none());

    // Typing in the buffer before confirming: refused, nothing changed.
    reply(&s, PrepareRenameRequest::METHOD, Reply::Result(foo(2, 4)));
    rename(&mut s);
    s.until("the picker", |s| picker(s).is_some());
    s.cx.simulate_input("bar");
    s.cx.update(|_, cx| {
        editor.update(cx, |view, cx| {
            view.model_mut().set_cursor(0);
            view.model_mut().insert("// ");
            cx.notify();
        })
    });
    s.draw();
    let changed = text(&mut s, &editor);
    s.cx.simulate_keystrokes("enter");
    s.until("the prompt", |s| s.cx.pending_prompt().is_some());
    assert!(said(&mut s).is_some());
    assert_eq!(text(&mut s, &editor), changed);
    assert_eq!(asked(&s, Rename::METHOD), 0);

    // Escape sends no rename.
    rename(&mut s);
    s.until("the picker", |s| picker(s).is_some());
    s.cx.simulate_input("bar");
    s.cx.simulate_keystrokes("escape");
    s.draw();
    assert!(picker(&mut s).is_none());
    assert!(focused(&mut s, &editor));
    assert_eq!(asked(&s, Rename::METHOD), 0);
    assert_eq!(said(&mut s), None);
}

#[gpui::test]
fn a_changed_source_discards_edits_to_other_files(cx: &mut TestAppContext) {
    let (mut s, editor) = setup_with(Lsp::with_fake_clock(cx, renamer(), Script::default()));
    let other = s.uri("src/other.rs");
    s.fake.set_reply(
        Rename::METHOD,
        Scripted {
            reply: Reply::Result(json!({ "changes": {
                other.as_str(): [{ "range": foo(0, 9), "newText": "bar" }],
            }})),
            delay: Duration::from_secs(1),
        },
    );

    rename(&mut s);
    s.until("the picker", |s| picker(s).is_some());
    s.cx.simulate_input("bar");
    s.draw();
    s.cx.simulate_keystrokes("enter");
    s.until("the request", |s| asked(s, Rename::METHOD) == 1);
    s.clock.wait_for_sleep(Duration::from_secs(1));
    // The source changes while the server works; its edits skip it.
    s.cx.update(|_, cx| {
        editor.update(cx, |view, cx| {
            view.model_mut().set_cursor(0);
            view.model_mut().insert("// ");
            cx.notify();
        })
    });
    s.clock.advance(Duration::from_secs(1));
    s.until("the prompt", |s| s.cx.pending_prompt().is_some());
    assert!(said(&mut s).is_some());
    s.until("the picker to close", |s| picker(s).is_none());
    assert_eq!(
        fs::read_to_string(s.path("src/other.rs")).unwrap(),
        OTHER,
        "nothing is applied"
    );
}

#[gpui::test]
fn the_selected_name_takes_platform_replacements(cx: &mut TestAppContext) {
    let (mut s, _) = setup(cx);
    let edit = |s: &mut Lsp, range: Option<Range<usize>>, text: &str| {
        let input = picker(s)
            .unwrap()
            .read_with(&s.cx, |p, _| p.input().clone());
        s.cx.update(|window, cx| {
            input.update(cx, |input, cx| {
                input.replace_text_in_range(range, text, window, cx)
            })
        });
        s.draw();
    };

    rename(&mut s);
    s.until("the picker", |s| picker(s).is_some());
    // `foo` replaced by `fo` is an edit, not a deletion.
    edit(&mut s, Some(0..3), "fo");
    assert_eq!((shown(&mut s).0.as_str(), shown(&mut s).1), ("fo", 1));
    assert!(!selected(&mut s));

    // Deleting the selected name leaves nothing to rename to.
    let input = picker(&mut s)
        .unwrap()
        .read_with(&s.cx, |p, _| p.input().clone());
    s.cx.update(|_, cx| input.update(cx, |input, cx| input.select_all(cx)));
    edit(&mut s, Some(0..2), "");
    assert_eq!((shown(&mut s).0.as_str(), shown(&mut s).1), ("", 0));
}
