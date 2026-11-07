//! The user's snippet files in an editor's completions: offered for the
//! word before the caret in their languages, detail "User snippet", before
//! an extension's that matches as well, expanding through the snippet
//! engine on accept, and following edits to the files with no restart.

use std::fs;

use fe_editor::completion::{CompletionItem, CompletionList, CompletionSession, CompletionTrigger};
use fe_gpui::EditorView;
use fe_lsp::fake::{Reply, Scripted};
use gpui::{Entity, Focusable as _, TestAppContext};
use lsp_types::{
    LSPAny,
    request::{Completion, Request as _},
};

use crate::extension_snippets::{SNIPPET_DOCUMENTATION_LIMIT, SNIPPET_PAYLOAD_LIMIT};
use crate::tests::lsp::Lsp;
use crate::{ExtensionSnippet, set_extension_snippets};

/// Writes `name` in the user's snippets directory, which it creates.
pub(crate) fn write_snippets(s: &Lsp, name: &str, text: &str) {
    let dir = s.config.path().join("snippets");
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join(name), text).unwrap();
}

/// Has the server answer completions with none.
fn reply_with_none(s: &Lsp) {
    let reply: LSPAny = r#"{ "isIncomplete": false, "items": [] }"#.parse().unwrap();
    s.fake
        .set_reply(Completion::METHOD, Scripted::now(Reply::Result(reply)));
}

/// `name`, written with `text`, open and focused with its language
/// loaded, the caret at `caret`.
pub(crate) fn open_at(s: &mut Lsp, name: &str, text: &str, caret: usize) -> Entity<EditorView> {
    fs::write(s.path(name), text).unwrap();
    s.open(name);
    let editor = (s.workbench)
        .read_with(&s.cx, |w, cx| w.active_editor(cx).cloned())
        .expect("the file is shown");
    if !name.ends_with(".txt") {
        s.until("its language", |s| {
            editor.read_with(&s.cx, |view, _| view.syntax().is_some())
        });
    }
    s.cx.update(|window, cx| {
        editor.update(cx, |view, cx| {
            view.model_mut().set_cursor(caret);
            cx.notify();
        });
        window.focus(&editor.focus_handle(cx), cx);
    });
    s.draw();
    editor
}

/// What `editor`'s provider offers at its caret, asked for, in a session
/// there: its items, best first.
fn offered(s: &mut Lsp, editor: &Entity<EditorView>) -> Vec<CompletionItem> {
    let (snapshot, offset, reply) = s.cx.update(|_, cx| {
        let view = editor.read(cx);
        let provider = view.completion_provider().expect("a provider").clone();
        let snapshot = view.model().snapshot().clone();
        let offset = view.model().primary_selection().head();
        let reply = provider.completions(
            snapshot.id(),
            snapshot.version(),
            offset,
            CompletionTrigger::Invoked,
        );
        (snapshot, offset, reply)
    });
    let list = futures::executor::block_on(reply).unwrap_or_default();
    let session = CompletionSession::new(snapshot, offset, list);
    (session.matches().iter())
        .map(|&ix| session.items()[ix].clone())
        .collect()
}

/// The list `editor`'s provider answers at its caret, asked for.
fn asked(s: &mut Lsp, editor: &Entity<EditorView>) -> CompletionList {
    let reply = s.cx.update(|_, cx| {
        let view = editor.read(cx);
        let provider = view.completion_provider().expect("a provider").clone();
        let snapshot = view.model().snapshot();
        let offset = view.model().primary_selection().head();
        provider.completions(
            snapshot.id(),
            snapshot.version(),
            offset,
            CompletionTrigger::Invoked,
        )
    });
    futures::executor::block_on(reply).unwrap_or_default()
}

/// The labels and details `editor` is offered at its caret.
fn labels(s: &mut Lsp, editor: &Entity<EditorView>) -> Vec<(String, String)> {
    (offered(s, editor).into_iter())
        .map(|item| (item.label, item.detail.unwrap_or_default()))
        .collect()
}

fn user(label: &str) -> (String, String) {
    (label.to_owned(), "User snippet".to_owned())
}

const RUST: &str = r##"{
    // Rust's own.
    "Main function": {
        "prefix": "main",
        "body": ["fn main() {", "\t${1:todo!()}", "}$0"],
        "description": "A main function",
    },
    "Test": { "prefix": ["test", "unit"], "body": "#[test]\nfn ${1:name}() {\n\t$0\n}" },
}"##;

const GLOBAL: &str = r##"{
    "Todo": { "prefix": "todo", "body": "TODO(${1:who}): $0" },
    "Python main": { "prefix": "mainpy", "body": "# $1", "scope": "python" },
}"##;

#[gpui::test]
fn matching_user_snippets_are_offered_and_expand(cx: &mut TestAppContext) {
    let mut s = Lsp::new(cx);
    write_snippets(&s, "rust.json", RUST);
    write_snippets(&s, "global.code-snippets", GLOBAL);
    // An extension's, which the user's `main` comes before.
    s.cx.update(|_, cx| {
        set_extension_snippets(
            cx,
            vec![ExtensionSnippet {
                extension_id: "acme.rusty".to_owned(),
                extension_name: "Rusty".to_owned(),
                prefix: "main".to_owned(),
                body: "fn main() {}".to_owned(),
                description: None,
                languages: vec!["rust".to_owned()],
            }],
        )
    });
    reply_with_none(&s);
    let editor = open_at(&mut s, "src/snip.rs", "\n", 0);
    let uri = s.uri("src/snip.rs");
    s.fake
        .wait_for_document(&uri, "didOpen", |r| r.is_some_and(|r| r.open));

    // `main` typed: the user's, then the extension's; not Python's
    // `mainpy`.
    s.cx.simulate_input("main");
    s.draw();
    let items = offered(&mut s, &editor);
    let shown: Vec<_> = (items.iter())
        .map(|item| (item.label.as_str(), item.detail.as_deref().unwrap()))
        .collect();
    assert_eq!(shown, [("main", "User snippet"), ("main", "Rusty")]);
    let item = &items[0];
    assert!(item.snippet);
    assert_eq!(item.kind.as_deref(), Some("snippet"));
    assert_eq!(item.label_detail.as_deref(), Some(" A main function"));
    assert_eq!(
        item.edit,
        (0..4, "fn main() {\n\t${1:todo!()}\n}$0".to_owned())
    );

    // Accepted, it expands, its placeholder selected, and Tab goes to the
    // end.
    s.cx.simulate_keystrokes("ctrl-space");
    s.until("the menu", |s| {
        editor.read_with(&s.cx, |view, _| view.completion_shown())
    });
    s.cx.simulate_keystrokes("enter");
    s.draw();
    let text = |s: &mut Lsp| editor.read_with(&s.cx, |view, _| view.model().text().to_string());
    let selection =
        |s: &mut Lsp| editor.read_with(&s.cx, |view, _| view.model().primary_selection().range);
    let expanded = text(&mut s);
    assert!(
        expanded.starts_with("fn main() {\n") && expanded.ends_with("todo!()\n}\n"),
        "{expanded:?}"
    );
    let placeholder = expanded.find("todo!()").unwrap();
    assert_eq!(
        selection(&mut s),
        placeholder..placeholder + "todo!()".len()
    );
    s.cx.simulate_keystrokes("tab");
    s.draw();
    let end = expanded.len() - 1;
    assert_eq!(selection(&mut s), end..end);

    // Each of a snippet's prefixes offers it.
    s.cx.simulate_input("\nun");
    s.draw();
    assert_eq!(labels(&mut s, &editor), [user("unit")]);

    // Go sees the global ones alone: `todo`, and nothing for `m`, as
    // `mainpy` is Python's and `main` Rust's; so does plain text.
    for name in ["notes.go", "notes.txt"] {
        let editor = open_at(&mut s, name, "", 0);
        s.cx.simulate_input("t");
        s.draw();
        assert_eq!(labels(&mut s, &editor), [user("todo")], "{name}");
        s.cx.simulate_keystrokes("backspace");
        s.cx.simulate_input("m");
        s.draw();
        assert_eq!(labels(&mut s, &editor), [], "{name}");
    }
    // Rust has its own, and the global one, at no word.
    let editor = open_at(&mut s, "src/other.rs", "", 0);
    assert_eq!(
        labels(&mut s, &editor),
        [
            user("main"),
            user("test"),
            user("todo"),
            user("unit"),
            ("main".to_owned(), "Rusty".to_owned()),
        ]
    );
}

#[gpui::test]
fn editing_the_file_reloads_without_restart(cx: &mut TestAppContext) {
    let mut s = Lsp::new(cx);
    reply_with_none(&s);
    // None yet: no snippets directory.
    let editor = open_at(&mut s, "src/snip.rs", "", 0);
    let uri = s.uri("src/snip.rs");
    s.fake
        .wait_for_document(&uri, "didOpen", |r| r.is_some_and(|r| r.open));
    assert_eq!(labels(&mut s, &editor), []);

    write_snippets(&s, "rust.json", RUST);
    assert_eq!(
        labels(&mut s, &editor),
        [user("main"), user("test"), user("unit")]
    );

    // Rewritten: the next completion offers what it says now.
    write_snippets(
        &s,
        "rust.json",
        r#"{ "Entry": { "prefix": "entry", "body": "fn entry() {}" } }"#,
    );
    assert_eq!(labels(&mut s, &editor), [user("entry")]);

    // A bad edit drops that snippet alone.
    write_snippets(
        &s,
        "rust.json",
        r#"{ "Entry": { "prefix": "entry", "body": "fn ${1:entry() {" },
             "Exit": { "prefix": "exit", "body": "return;" } }"#,
    );
    assert_eq!(labels(&mut s, &editor), [user("exit")]);

    // Another file added counts too.
    write_snippets(&s, "global.code-snippets", GLOBAL);
    assert_eq!(labels(&mut s, &editor), [user("exit"), user("todo")]);

    // Deleted, its snippets go.
    fs::remove_file(s.config.path().join("snippets/rust.json")).unwrap();
    assert_eq!(labels(&mut s, &editor), [user("todo")]);
    fs::remove_dir_all(s.config.path().join("snippets")).unwrap();
    assert_eq!(labels(&mut s, &editor), []);
}

#[gpui::test]
fn a_large_snippet_with_many_prefixes_is_offered_within_a_bound(cx: &mut TestAppContext) {
    let mut s = Lsp::new(cx);
    reply_with_none(&s);
    // One 512 KiB body under a thousand prefixes, for Rust.
    let body = "x".repeat(512 << 10);
    let prefixes: Vec<String> = (0..1000).map(|n| format!("\"p{n}\"")).collect();
    let file = format!(
        r#"{{ "Big": {{ "prefix": [{}], "body": "{body}" }} }}"#,
        prefixes.join(", ")
    );
    write_snippets(&s, "rust.json", &file);

    // Read once: every prefix's offer shares the one text.
    let offers = s.cx.update(|_, cx| crate::user_snippets::user_offers(cx));
    assert_eq!(offers.len(), 1000);
    assert!(
        (offers.iter()).all(|offer| std::rc::Rc::ptr_eq(&offer.text, &offers[0].text)),
        "each prefix shares the snippet's text"
    );

    // Go is offered none of it.
    let go = open_at(&mut s, "main.go", "", 0);
    assert_eq!(labels(&mut s, &go), []);

    // Rust, at no word, matches all thousand: the items carry no more than
    // the limit, and the list is incomplete, as some were left out.
    let editor = open_at(&mut s, "src/big.rs", "", 0);
    let uri = s.uri("src/big.rs");
    s.fake
        .wait_for_document(&uri, "didOpen", |r| r.is_some_and(|r| r.open));
    let list = asked(&mut s, &editor);
    assert!(list.is_incomplete);
    let items = list.items;
    assert!(!items.is_empty() && items.len() < 1000, "{}", items.len());
    assert!(!items.iter().any(|item| item.label == "p999"));
    let carried: usize = (items.iter()).map(|item| item.edit.1.len()).sum();
    assert!(carried <= SNIPPET_PAYLOAD_LIMIT, "{carried}");
    let documented: usize = (items.iter())
        .map(|item| item.documentation.as_ref().map_or(0, String::len))
        .sum();
    assert!(
        documented <= SNIPPET_DOCUMENTATION_LIMIT + 64 * items.len(),
        "{documented}"
    );

    // The menu open at no word, typing a prefix left out narrows to it, as
    // the list was incomplete, and Enter accepts it.
    s.cx.simulate_keystrokes("ctrl-space");
    s.until("the menu", |s| {
        editor.read_with(&s.cx, |view, _| view.completion_shown())
    });
    s.cx.simulate_input("p999");
    s.draw();
    s.until("the menu", |s| {
        editor.read_with(&s.cx, |view, _| view.completion_shown())
    });
    s.cx.simulate_keystrokes("enter");
    s.draw();
    let text = editor.read_with(&s.cx, |view, _| view.model().text().to_string());
    assert!(
        text == body,
        "{} bytes: {:?}…",
        text.len(),
        &text[..text.len().min(20)]
    );
}

#[gpui::test]
fn compact_snippets_that_expand_far_are_expanded_only_when_offered(cx: &mut TestAppContext) {
    let mut s = Lsp::new(cx);
    reply_with_none(&s);
    // A thousand Rust snippets, each a chain of mirrors a few hundred bytes
    // long that the engine expands to megabytes.
    let chain: String = std::iter::once("${1:x}".to_owned())
        .chain((2..=22).map(|n| format!("${{{n}:${}${}}}", n - 1, n - 1)))
        .collect();
    let entries: Vec<String> = (0..1000)
        .map(|n| format!(r#""S{n}": {{ "prefix": "s{n}", "body": "{chain}" }}"#))
        .collect();
    write_snippets(&s, "rust.json", &format!("{{ {} }}", entries.join(", ")));

    // Read, they keep their bodies alone, not what those expand to.
    let set =
        s.cx.update(|_, cx| crate::user_snippets::user_snippet_set(cx));
    assert_eq!(set.snippets.len(), 1000);
    let kept: usize = (set.texts.iter()).map(|text| text.body.len()).sum();
    assert_eq!(kept, 1000 * chain.len());

    // Go is offered none, expanding none.
    let go = open_at(&mut s, "main.go", "", 0);
    assert_eq!(labels(&mut s, &go), []);

    // Rust, at no word, is offered all; their documentation stays within
    // its limit.
    let editor = open_at(&mut s, "src/chain.rs", "", 0);
    let uri = s.uri("src/chain.rs");
    s.fake
        .wait_for_document(&uri, "didOpen", |r| r.is_some_and(|r| r.open));
    let list = asked(&mut s, &editor);
    assert!(!list.is_incomplete);
    assert_eq!(list.items.len(), 1000);
    let documented: usize = (list.items.iter())
        .map(|item| item.documentation.as_ref().map_or(0, String::len))
        .sum();
    assert!(
        documented <= SNIPPET_DOCUMENTATION_LIMIT + 64 * 1000,
        "{documented}"
    );

    // A short one is still documented.
    write_snippets(&s, "rust.json", RUST);
    s.cx.simulate_input("main");
    s.draw();
    let items = offered(&mut s, &editor);
    let documentation = items[0].documentation.as_deref().unwrap_or_default();
    assert!(documentation.contains("fn main() {"), "{documentation:?}");
}

#[gpui::test]
fn each_read_is_one_consistent_set(cx: &mut TestAppContext) {
    let mut s = Lsp::new(cx);
    let set = |s: &mut Lsp| {
        s.cx.update(|_, cx| crate::user_snippets::user_snippet_set(cx))
    };
    let bodies = |set: &crate::user_snippets::UserSnippetSet| -> Vec<(String, String, String)> {
        (set.snippets.iter().zip(set.texts.iter()))
            .map(|(snippet, text)| {
                assert_eq!(snippet.body, text.body, "{}", snippet.name);
                (
                    snippet.name.clone(),
                    snippet.body.clone(),
                    text.body.clone(),
                )
            })
            .collect()
    };
    write_snippets(&s, "rust.json", RUST);
    write_snippets(&s, "global.code-snippets", GLOBAL);
    let first = set(&mut s);
    assert_eq!(bodies(&first).len(), 4);

    // Replaced, with the snippets reordered and fewer: a set read before
    // still pairs each snippet with its own text, as does one read after.
    write_snippets(
        &s,
        "rust.json",
        r#"{ "Test": { "prefix": "x", "body": "only $1" } }"#,
    );
    let second = set(&mut s);
    assert_eq!(bodies(&first).len(), 4);
    let names: Vec<_> = bodies(&second).into_iter().map(|(name, ..)| name).collect();
    assert_eq!(names, ["Todo", "Python main", "Test"]);
    // Each offer's text is its own snippet's.
    for offer in second.offers.iter() {
        let ix = (second.texts.iter())
            .position(|text| std::rc::Rc::ptr_eq(text, &offer.text))
            .expect("an offer's text is one of the set's");
        assert!(second.snippets[ix].prefixes.contains(&offer.prefix));
    }

    // Removed, a new read has none of it.
    fs::remove_dir_all(s.config.path().join("snippets")).unwrap();
    let third = set(&mut s);
    assert!(third.snippets.is_empty() && third.texts.is_empty() && third.offers.is_empty());
    assert_eq!(bodies(&second).len(), 3);
}
