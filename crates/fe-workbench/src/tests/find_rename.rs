//! The find bar's replace row: Rename symbol in place of Replace when the
//! current match is a whole symbol the fake language server renames.

use std::fs;

use fe_gpui::EditorView;
use fe_lsp::fake::{InitializeReply, Reply, Script, Scripted, initialize_result};
use gpui::{Entity, Focusable as _, Modifiers, TestAppContext};
use lsp_types::{
    OneOf, PositionEncodingKind, RenameOptions, ServerCapabilities,
    request::{PrepareRenameRequest, Rename, Request as _},
};
use serde_json::{Value, json};

use crate::{find_bar::FindOp, tests::lsp::Lsp};

/// `foo` twice, the caret in the second.
const MAIN: &str = "fn foo() {}\nfn main() {\n    foo();\n}\n";
const CARET: usize = 30;

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

fn foo(line: u32, column: u32) -> Value {
    json!({
        "start": { "line": line, "character": column },
        "end": { "line": line, "character": column + 3 },
    })
}

fn asked(s: &Lsp, method: &str) -> usize {
    (s.fake.records().iter())
        .filter(|record| {
            matches!(record, fe_lsp::fake::Record::Request { method: m, .. } if m == method)
        })
        .count()
}

/// `MAIN` open with the server renaming (answering for the second `foo`),
/// the replace row shown and `query` typed in the find field, `replace`
/// in the replace field.
fn setup(cx: &mut TestAppContext, query: &str, replace: &str) -> (Lsp, Entity<EditorView>) {
    let mut s = Lsp::with_scripts(cx, renamer(), Script::default());
    s.set_trust(true);
    fs::write(s.path("src/main.rs"), MAIN).unwrap();
    s.open("src/main.rs");
    s.fake
        .wait_for_document(&s.uri("src/main.rs"), "didOpen", |r| r.is_some());
    let editor = (s.workbench)
        .read_with(&s.cx, |w, cx| w.active_editor(cx).cloned())
        .expect("main.rs is shown");
    s.cx.update(|_, cx| {
        editor.update(cx, |view, cx| {
            view.model_mut().set_cursor(CARET);
            cx.notify();
        })
    });
    reply(&s, PrepareRenameRequest::METHOD, Reply::Result(foo(2, 4)));
    find(&mut s, query, replace);
    (s, editor)
}

fn find(s: &mut Lsp, query: &str, replace: &str) {
    let pane = s.workbench.read_with(&s.cx, |w, _| w.pane().clone());
    s.cx.update(|window, cx| {
        pane.update(cx, |pane, cx| {
            pane.run_find(FindOp::ShowReplace, window, cx)
        })
    });
    s.draw();
    let input = pane.read_with(&s.cx, |pane, _| pane.find_bar().unwrap().input().clone());
    s.cx.update(|window, cx| window.focus(&input.focus_handle(cx), cx));
    s.cx.simulate_input(query);
    s.draw();
    // The match in the call, which the fake server renames.
    let editor = (s.workbench)
        .read_with(&s.cx, |w, cx| w.active_editor(cx).cloned())
        .unwrap();
    let call = MAIN.rfind(query).unwrap();
    s.cx.update(|_, cx| {
        editor.update(cx, |view, cx| {
            view.select_and_reveal(call..call + query.len(), cx)
        })
    });
    s.draw();
    let input = pane.read_with(&s.cx, |pane, _| {
        pane.find_bar().unwrap().replace_input().clone()
    });
    s.cx.update(|_, cx| input.update(cx, |input, cx| input.set_text(replace.to_owned(), cx)));
    s.draw();
}

fn shown(s: &mut Lsp, id: &'static str) -> bool {
    s.cx.debug_bounds(id).is_some()
}

fn click(s: &mut Lsp, id: &'static str) {
    let bounds =
        s.cx.debug_bounds(id)
            .unwrap_or_else(|| panic!("{id} shows"));
    s.cx.simulate_click(bounds.center(), Modifiers::none());
    s.draw();
}

fn text(s: &mut Lsp, editor: &Entity<EditorView>) -> String {
    editor.read_with(&s.cx, |view, _| view.model().text().to_string())
}

#[gpui::test]
fn a_symbol_match_shows_rename_symbol(cx: &mut TestAppContext) {
    let (mut s, _) = setup(cx, "foo", "bar");
    s.until("Rename symbol", |s| shown(s, "find-rename-symbol"));
    assert!(!shown(&mut s, "find-replace"));
    assert!(shown(&mut s, "find-replace-all"));
    // Asked once for each match the bar was on.
    assert!(asked(&s, PrepareRenameRequest::METHOD) >= 1);
}

#[gpui::test]
fn a_non_symbol_or_partial_word_match_shows_replace(cx: &mut TestAppContext) {
    // Part of a word: nothing is asked.
    let (mut s, _) = setup(cx, "fo", "bar");
    s.draw();
    assert!(shown(&mut s, "find-replace"));
    assert!(!shown(&mut s, "find-rename-symbol"));
    assert_eq!(asked(&s, PrepareRenameRequest::METHOD), 0);
}

#[gpui::test]
fn a_range_the_server_does_not_rename_shows_replace(cx: &mut TestAppContext) {
    let mut s = Lsp::with_scripts(cx, renamer(), Script::default());
    s.set_trust(true);
    fs::write(s.path("src/main.rs"), MAIN).unwrap();
    s.open("src/main.rs");
    s.fake
        .wait_for_document(&s.uri("src/main.rs"), "didOpen", |r| r.is_some());
    // The server renames the first `foo` only.
    reply(&s, PrepareRenameRequest::METHOD, Reply::Result(foo(0, 3)));
    let editor = (s.workbench)
        .read_with(&s.cx, |w, cx| w.active_editor(cx).cloned())
        .unwrap();
    s.cx.update(|_, cx| {
        editor.update(cx, |view, cx| {
            view.model_mut().set_cursor(CARET);
            cx.notify();
        })
    });
    find(&mut s, "foo", "bar");
    s.until("the question", |s| {
        asked(s, PrepareRenameRequest::METHOD) >= 1
    });
    s.draw();
    assert!(shown(&mut s, "find-replace"));
    assert!(!shown(&mut s, "find-rename-symbol"));
}

#[gpui::test]
fn rename_applies_the_replace_text_through_the_server(cx: &mut TestAppContext) {
    let (mut s, editor) = setup(cx, "foo", "barista");
    let main = s.uri("src/main.rs");
    reply(
        &s,
        Rename::METHOD,
        Reply::Result(json!({ "changes": { main.as_str(): [
            { "range": foo(0, 3), "newText": "barista" },
            { "range": foo(2, 4), "newText": "barista" },
        ]}})),
    );
    s.until("Rename symbol", |s| shown(s, "find-rename-symbol"));
    click(&mut s, "find-rename-symbol");
    s.until("the rename", |s| {
        text(s, &editor) == "fn barista() {}\nfn main() {\n    barista();\n}\n"
    });
    let sent = (s.fake.records().into_iter()).find_map(|record| match record {
        fe_lsp::fake::Record::Request { method, params, .. } if method == Rename::METHOD => {
            Some(params)
        }
        _ => None,
    });
    assert_eq!(sent.unwrap()["newName"], "barista");
}

#[gpui::test]
fn an_empty_replace_field_does_nothing(cx: &mut TestAppContext) {
    let (mut s, editor) = setup(cx, "foo", "");
    s.until("Rename symbol", |s| shown(s, "find-rename-symbol"));
    click(&mut s, "find-rename-symbol");
    s.draw();
    assert_eq!(asked(&s, Rename::METHOD), 0);
    assert_eq!(text(&mut s, &editor), MAIN);
}

#[gpui::test]
fn no_server_shows_replace(cx: &mut TestAppContext) {
    let mut s = Lsp::untrusted(cx);
    fs::write(s.path("src/main.rs"), MAIN).unwrap();
    s.open("src/main.rs");
    find(&mut s, "foo", "bar");
    s.draw();
    assert!(shown(&mut s, "find-replace"));
    assert!(!shown(&mut s, "find-rename-symbol"));
}

#[gpui::test]
fn replace_all_stays_textual(cx: &mut TestAppContext) {
    let (mut s, editor) = setup(cx, "foo", "bar");
    s.until("Rename symbol", |s| shown(s, "find-rename-symbol"));
    click(&mut s, "find-replace-all");
    assert_eq!(
        text(&mut s, &editor),
        "fn bar() {}\nfn main() {\n    bar();\n}\n"
    );
    assert_eq!(asked(&s, Rename::METHOD), 0);
}

#[gpui::test]
fn a_match_beside_multibyte_text_does_not_panic(cx: &mut TestAppContext) {
    let mut s = Lsp::untrusted(cx);
    fs::write(s.path("src/main.rs"), "éé foo éé\n").unwrap();
    s.open("src/main.rs");
    find(&mut s, "foo", "bar");
    s.draw();
    assert!(shown(&mut s, "find-replace"));
}

#[gpui::test]
fn a_server_starting_later_enables_rename_symbol(cx: &mut TestAppContext) {
    let mut s = Lsp::with_scripts(cx, renamer(), Script::default());
    fs::write(s.path("src/main.rs"), MAIN).unwrap();
    s.open("src/main.rs");
    reply(&s, PrepareRenameRequest::METHOD, Reply::Result(foo(2, 4)));
    // Untrusted: no server runs, so Replace shows.
    find(&mut s, "foo", "bar");
    s.draw();
    assert!(shown(&mut s, "find-replace"));
    s.set_trust(true);
    s.until("Rename symbol", |s| shown(s, "find-rename-symbol"));
}
