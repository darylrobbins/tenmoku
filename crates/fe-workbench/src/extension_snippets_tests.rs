//! Extensions' snippets in an editor's completions: offered for the word
//! before the caret in their languages, after the server's items, and
//! expanding through the snippet engine on accept, with or without a
//! server for the language.

use std::fs;

use fe_editor::completion::{CompletionItem, CompletionSession, CompletionTrigger};
use fe_gpui::EditorView;
use fe_lsp::fake::{InitializeReply, Record, Reply, Script, Scripted, initialize_result};
use gpui::{Entity, Focusable as _, TestAppContext};
use lsp_types::{
    CompletionOptions, LSPAny, PositionEncodingKind, ServerCapabilities,
    request::{Completion, Request as _},
};

use crate::tests::lsp::Lsp;
use crate::{ExtensionSnippet, set_extension_snippets};

/// A Rust function, a Go one, and a note for every language.
fn snippets() -> Vec<ExtensionSnippet> {
    let snippet = |name: &str, prefix: &str, body: &str, languages: &[&str]| ExtensionSnippet {
        extension_id: format!("acme.{}", name.to_lowercase()),
        extension_name: name.to_owned(),
        prefix: prefix.to_owned(),
        body: body.to_owned(),
        description: Some(format!("{name}'s {prefix}")),
        languages: languages
            .iter()
            .map(|&language| language.to_owned())
            .collect(),
    };
    vec![
        snippet("Rusty", "fn", "fn ${1:name}(${2}) {\n\t$0\n}", &["rust"]),
        snippet("Gopher", "func", "func ${1:name}() {\n\t$0\n}", &["go"]),
        snippet("Notes", "todo", "TODO(${1:who}): $0", &[]),
    ]
}

/// Has the server answer completions with `items`.
fn reply_with(s: &Lsp, items: &str) {
    let reply: LSPAny = format!(r#"{{ "isIncomplete": false, "items": [{items}] }}"#)
        .parse()
        .unwrap();
    s.fake
        .set_reply(Completion::METHOD, Scripted::now(Reply::Result(reply)));
}

/// `name`, written with `text`, open and focused with its language
/// loaded, the caret at `caret`.
fn open_at(s: &mut Lsp, name: &str, text: &str, caret: usize) -> Entity<EditorView> {
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

/// The completion requests the server got.
fn asked(s: &Lsp) -> usize {
    (s.fake.records().iter())
        .filter(|record| {
            matches!(record, Record::Request { method, .. } if method == Completion::METHOD)
        })
        .count()
}

#[gpui::test]
fn matching_snippets_are_offered_as_snippet_items(cx: &mut TestAppContext) {
    let mut s = Lsp::new(cx);
    s.cx.update(|_, cx| set_extension_snippets(cx, snippets()));
    reply_with(&s, "");
    let before = "fn main() {\n    ";
    let editor = open_at(
        &mut s,
        "src/snip.rs",
        "fn main() {\n    \n}\n",
        before.len(),
    );
    let uri = s.uri("src/snip.rs");
    s.fake
        .wait_for_document(&uri, "didOpen", |r| r.is_some_and(|r| r.open));

    // `fn` typed matches Rust's `fn`, and Go's `func`, which isn't offered
    // in Rust.
    s.cx.simulate_input("fn");
    s.draw();
    let items = offered(&mut s, &editor);
    let labels: Vec<_> = items.iter().map(|item| item.label.as_str()).collect();
    assert_eq!(labels, ["fn"]);
    let item = &items[0];
    assert!(item.snippet);
    assert_eq!(item.kind.as_deref(), Some("snippet"));
    assert_eq!(item.detail.as_deref(), Some("Rusty"));
    assert_eq!(item.label_detail.as_deref(), Some(" Rusty's fn"));
    // It replaces the word before the caret.
    let caret = before.len() + "fn".len();
    assert_eq!(
        item.edit,
        (caret - 2..caret, "fn ${1:name}(${2}) {\n\t$0\n}".to_owned())
    );
    assert!(item.data.is_none() && item.command.is_none());

    // A word it doesn't match offers none.
    s.cx.update(|_, cx| {
        editor.update(cx, |view, cx| {
            view.model_mut().set_cursor("fn main".len());
            cx.notify();
        })
    });
    s.draw();
    let items = offered(&mut s, &editor);
    assert!(items.is_empty(), "{items:?}");

    // `todo`, for no language, is offered in each: Rust, Go and plain
    // text; `fn` and `func` in their own alone.
    for (name, language, offered_there) in [
        ("src/notes.rs", Some("rust"), &["fn", "todo"][..]),
        ("notes.go", Some("go"), &["func", "todo"]),
        ("notes.txt", None, &["todo"]),
    ] {
        let editor = open_at(&mut s, name, "", 0);
        assert_eq!(
            editor.read_with(&s.cx, |view, _| view.syntax().map(|s| s.language().name())),
            language
        );
        s.cx.simulate_input("t");
        s.draw();
        let labels: Vec<_> = (offered(&mut s, &editor).into_iter())
            .map(|item| item.label)
            .collect();
        assert_eq!(labels, ["todo"], "{name}");
        // Every one of the language's, and every language's, at no word.
        s.cx.simulate_keystrokes("backspace");
        s.draw();
        let labels: Vec<_> = (offered(&mut s, &editor).into_iter())
            .map(|item| item.label)
            .collect();
        assert_eq!(labels, offered_there, "{name}");
    }
}

#[gpui::test]
fn accepting_one_expands_with_tabstops(cx: &mut TestAppContext) {
    let mut s = Lsp::new(cx);
    s.cx.update(|_, cx| set_extension_snippets(cx, snippets()));
    reply_with(&s, "");
    let before = "mod m {\n    ";
    let editor = open_at(&mut s, "src/snip.rs", "mod m {\n    \n}\n", before.len());
    let uri = s.uri("src/snip.rs");
    s.fake
        .wait_for_document(&uri, "didOpen", |r| r.is_some_and(|r| r.open));

    // `fn` typed on the indented row, then completed: the body goes in
    // indented to the row, its tab as the row's indent unit, and its
    // first placeholder selected.
    s.cx.simulate_input("fn");
    s.draw();
    s.cx.simulate_keystrokes("ctrl-space");
    s.until("the menu", |s| {
        editor.read_with(&s.cx, |view, _| view.completion_shown())
    });
    s.cx.simulate_keystrokes("enter");
    s.draw();
    let text = |s: &mut Lsp| editor.read_with(&s.cx, |view, _| view.model().text().to_string());
    let selection =
        |s: &mut Lsp| editor.read_with(&s.cx, |view, _| view.model().primary_selection().range);
    let expanded = "mod m {\n    fn name() {\n        \n    }\n}\n";
    assert_eq!(text(&mut s), expanded);
    let name = expanded.find("name").unwrap();
    assert_eq!(selection(&mut s), name..name + "name".len());

    // Tab walks its stops: the parameters, then the body's line.
    s.cx.simulate_input("go");
    s.cx.simulate_keystrokes("tab");
    s.draw();
    let expanded = "mod m {\n    fn go() {\n        \n    }\n}\n";
    assert_eq!(text(&mut s), expanded);
    let params = expanded.find("()").unwrap() + 1;
    assert_eq!(selection(&mut s), params..params);
    s.cx.simulate_keystrokes("tab");
    s.draw();
    let body = expanded.find("() {\n").unwrap() + "() {\n        ".len();
    assert_eq!(selection(&mut s), body..body);
    s.cx.simulate_input("x");
    s.draw();
    assert_eq!(
        text(&mut s),
        "mod m {\n    fn go() {\n        x\n    }\n}\n"
    );
}

#[gpui::test]
fn they_work_without_a_language_server(cx: &mut TestAppContext) {
    let mut s = Lsp::new(cx);
    s.cx.update(|_, cx| set_extension_snippets(cx, snippets()));

    // With a server, they come after all its items, even `function`, which
    // `fn` matches less well than the snippet `fn`.
    reply_with(
        &s,
        r#"{ "label": "fn_server", "kind": 3 }, { "label": "function", "kind": 3 }"#,
    );
    let text = "fn main() {\n    fn\n}\n";
    let rust = open_at(&mut s, "src/snip.rs", text, "fn main() {\n    fn".len());
    let uri = s.uri("src/snip.rs");
    s.fake
        .wait_for_document(&uri, "didOpen", |r| r.is_some_and(|r| r.open));
    let labels: Vec<_> = (offered(&mut s, &rust).into_iter())
        .map(|item| item.label)
        .collect();
    assert_eq!(labels, ["fn_server", "function", "fn"]);
    let asked_rust = asked(&s);
    assert_eq!(asked_rust, 1);

    // Go has no server.
    let text = "package main\n\nfu";
    let editor = open_at(&mut s, "main.go", text, text.len());
    assert_eq!(
        editor.read_with(&s.cx, |view, _| view.syntax().map(|s| s.language().name())),
        Some("go")
    );

    // Offered: Go's `func`, not Rust's `fn`; nothing asked.
    let labels: Vec<_> = (offered(&mut s, &editor).into_iter())
        .map(|item| item.label)
        .collect();
    assert_eq!(labels, ["func"]);
    assert_eq!(asked(&s), asked_rust);

    // Accepted, it expands, and Tab walks it.
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
        expanded.starts_with("package main\n\nfunc name() {\n") && expanded.ends_with("\n}"),
        "{expanded:?}"
    );
    let name = expanded.find("name").unwrap();
    assert_eq!(selection(&mut s), name..name + "name".len());
    s.cx.simulate_keystrokes("tab");
    s.draw();
    let body = expanded.find("() {\n").unwrap() + "() {\n".len();
    let body = body + expanded[body..].find('\n').unwrap();
    assert_eq!(selection(&mut s), body..body);
    assert_eq!(asked(&s), asked_rust);
}

#[gpui::test]
fn a_prefix_starting_with_punctuation_replaces_all_typed_of_it(cx: &mut TestAppContext) {
    let mut s = Lsp::new(cx);
    let region = ExtensionSnippet {
        extension_id: "acme.regions".to_owned(),
        extension_name: "Regions".to_owned(),
        prefix: "#region".to_owned(),
        body: "#region $1\n$0\n#endregion".to_owned(),
        description: None,
        languages: Vec::new(),
    };
    s.cx.update(|_, cx| set_extension_snippets(cx, vec![region]));
    let text = "package main\n\n// x #Reg";
    let editor = open_at(&mut s, "main.go", text, text.len());

    // The word is `Reg`, but `#Reg` starts the prefix, case aside.
    let items = offered(&mut s, &editor);
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].edit.0, text.len() - "#Reg".len()..text.len());
}

#[gpui::test]
fn typing_at_the_caret_before_the_answer_keeps_them(cx: &mut TestAppContext) {
    let mut s = Lsp::new(cx);
    s.cx.update(|_, cx| set_extension_snippets(cx, snippets()));
    reply_with(&s, "");
    let caret = "fn main() {\n    f".len();
    let editor = open_at(&mut s, "src/snip.rs", "fn main() {\n    f\n}\n", caret);
    let uri = s.uri("src/snip.rs");
    s.fake
        .wait_for_document(&uri, "didOpen", |r| r.is_some_and(|r| r.open));

    // Asked at `f`, then `n` typed before the answer is polled.
    let (base, reply) = s.cx.update(|_, cx| {
        let view = editor.read(cx);
        let provider = view.completion_provider().expect("a provider").clone();
        let base = view.model().snapshot().clone();
        let reply =
            provider.completions(base.id(), base.version(), caret, CompletionTrigger::Invoked);
        (base, reply)
    });
    s.cx.simulate_input("n");
    s.draw();
    let list = futures::executor::block_on(reply).expect("the snippet");
    let labels: Vec<_> = list.items.iter().map(|item| item.label.as_str()).collect();
    assert_eq!(labels, ["fn"]);
    assert_eq!(list.items[0].edit.0, caret - 1..caret);

    // Still `fn` in a session over the typing, and accepted it expands.
    s.cx.update(|_, cx| {
        editor.update(cx, |view, cx| {
            let mut session = CompletionSession::new(base, caret, list);
            assert!(session.update(view.model()));
            assert_eq!(session.word(), "fn");
            assert_eq!(session.matches().len(), 1);
            session.accept(view.model_mut()).unwrap().expect("accepted");
            cx.notify();
        })
    });
    s.draw();
    let text = editor.read_with(&s.cx, |view, _| view.model().text().to_string());
    assert_eq!(text, "fn main() {\n    fn name() {\n        \n    }\n}\n");
    let selection = editor.read_with(&s.cx, |view, _| view.model().primary_selection().range);
    let name = text.rfind("name").unwrap();
    assert_eq!(selection, name..name + "name".len());

    // An edit before the caret since the request loses them.
    let caret = "fn main() {\n    fn name() {\n        ".len();
    let reply = s.cx.update(|_, cx| {
        editor.update(cx, |view, cx| {
            view.model_mut().end_snippet();
            view.model_mut().set_cursor(caret);
            cx.notify();
        });
        let view = editor.read(cx);
        let provider = view.completion_provider().expect("a provider").clone();
        let version = view.model().snapshot().version();
        let id = view.model().snapshot().id();
        provider.completions(id, version, caret, CompletionTrigger::Invoked)
    });
    s.cx.update(|_, cx| {
        editor.update(cx, |view, cx| {
            view.model_mut().set_cursor(0);
            cx.notify();
        })
    });
    s.cx.simulate_input("x");
    s.draw();
    let list = futures::executor::block_on(reply);
    assert!(
        list.as_ref().is_none_or(|list| list.items.is_empty()),
        "{list:?}"
    );
}

#[gpui::test]
fn a_body_of_tabstops_alone_is_offered_and_starts_a_session(cx: &mut TestAppContext) {
    let mut s = Lsp::new(cx);
    let stops = ExtensionSnippet {
        extension_id: "acme.stops".to_owned(),
        extension_name: "Stops".to_owned(),
        prefix: "stops".to_owned(),
        body: "${1:}$0".to_owned(),
        description: None,
        languages: Vec::new(),
    };
    let empty = ExtensionSnippet {
        prefix: "empty".to_owned(),
        body: String::new(),
        ..stops.clone()
    };
    s.cx.update(|_, cx| set_extension_snippets(cx, vec![stops.clone(), empty]));
    assert_eq!(s.cx.update(|_, cx| crate::extension_snippets(cx)), [stops]);
    let text = "package main\n\nst";
    let editor = open_at(&mut s, "main.go", text, text.len());
    let items = offered(&mut s, &editor);
    let labels: Vec<_> = items.iter().map(|item| item.label.as_str()).collect();
    assert_eq!(labels, ["stops"]);

    s.cx.simulate_keystrokes("ctrl-space");
    s.until("the menu", |s| {
        editor.read_with(&s.cx, |view, _| view.completion_shown())
    });
    s.cx.simulate_keystrokes("enter");
    s.draw();
    let (text, in_snippet) = editor.read_with(&s.cx, |view, _| {
        (view.model().text().to_string(), view.model().in_snippet())
    });
    assert_eq!(text, "package main\n\n");
    assert!(in_snippet);
}

#[test]
fn a_body_that_doesnt_parse_is_named() {
    for body in [
        "fn ${1:name}(${2}) {\n\t$0\n}",
        "$1$0",
        "${1:a ${2:b ${3}}}",
        "${1|one,two\\,three|}",
        "${TM_FILENAME/(.*)\\..*/${1:/upcase}/g}",
        "${name:default} $name $ cost \\${1 } {}",
        "$",
    ] {
        assert_eq!(crate::snippet_body_error(body), None, "{body:?}");
    }
    let nested = |n: usize| "${1:".repeat(n) + &"}".repeat(n);
    assert_eq!(crate::snippet_body_error(&nested(64)), None);
    for body in [
        "fn ${1:name",
        "${}",
        "${ 1}",
        "${1x}",
        "${1|a,b}",
        "${1|a,b|",
        "${1/a/b",
        "${name/a/b/",
        "$99999999999",
        &nested(65),
    ] {
        assert!(crate::snippet_body_error(body).is_some(), "{body:?}");
    }
}

#[test]
fn only_a_word_character_trigger_offers_snippets() {
    use crate::extension_snippets::offers_snippets;
    let typed = |trigger: &str| CompletionTrigger::Character(trigger.to_owned());
    assert!(offers_snippets(&CompletionTrigger::Invoked));
    assert!(offers_snippets(&CompletionTrigger::Incomplete));
    for word in ["_", "a", "Z", "7", "é"] {
        assert!(offers_snippets(&typed(word)), "{word:?}");
    }
    for punctuation in [".", ":", ">", "::", "->", "(", " ", ""] {
        assert!(!offers_snippets(&typed(punctuation)), "{punctuation:?}");
    }
}

/// A Rust workbench whose server completes after `.` and after `_`, with
/// a complete list, and extensions' `.log` and `_log` with the rest.
fn triggered(cx: &mut TestAppContext) -> Lsp {
    let mut init = initialize_result(Some(PositionEncodingKind::UTF8));
    init.capabilities = ServerCapabilities {
        completion_provider: Some(CompletionOptions {
            trigger_characters: Some(vec![".".to_owned(), "_".to_owned()]),
            ..CompletionOptions::default()
        }),
        ..init.capabilities
    };
    let script = Script {
        initialize: InitializeReply::Result(Box::new(init)),
        ..Script::default()
    };
    let mut s = Lsp::with_scripts(cx, script, Script::default());
    s.set_trust(true);
    let log = |prefix: &str| ExtensionSnippet {
        extension_id: "acme.logs".to_owned(),
        extension_name: "Logs".to_owned(),
        prefix: prefix.to_owned(),
        body: format!("{prefix}(\"${{1:what}}\");$0"),
        description: None,
        languages: vec!["rust".to_owned()],
    };
    let mut all = snippets();
    all.extend([log(".log"), log("_log")]);
    s.cx.update(|_, cx| set_extension_snippets(cx, all));
    s
}

/// What `editor`'s provider answers to `trigger` typed before `caret`:
/// the labels, as the server's and the snippets' come.
fn answered(s: &mut Lsp, editor: &Entity<EditorView>, caret: usize, trigger: &str) -> Vec<String> {
    let reply = s.cx.update(|_, cx| {
        let view = editor.read(cx);
        let provider = view.completion_provider().expect("a provider").clone();
        let snapshot = view.model().snapshot();
        provider.completions(
            snapshot.id(),
            snapshot.version(),
            caret,
            CompletionTrigger::Character(trigger.to_owned()),
        )
    });
    let list = futures::executor::block_on(reply).unwrap_or_default();
    list.items.into_iter().map(|item| item.label).collect()
}

#[gpui::test]
fn a_word_character_trigger_offers_the_snippets_its_word_matches(cx: &mut TestAppContext) {
    let mut s = triggered(cx);
    reply_with(
        &s,
        r#"{ "label": "_len", "kind": 2 }, { "label": "_lock", "kind": 2 }"#,
    );
    let before = "fn main() {\n    let a = ";
    let editor = open_at(
        &mut s,
        "src/snip.rs",
        "fn main() {\n    let a = \n}\n",
        before.len(),
    );
    let uri = s.uri("src/snip.rs");
    s.fake
        .wait_for_document(&uri, "didOpen", |r| r.is_some_and(|r| r.open));

    // `_` asks the server, and the snippets its word, `_`, matches come
    // after the server's items: `_log`, not `.log`, `fn` or `todo`.
    s.cx.simulate_input("_");
    s.until("the menu", |s| {
        editor.read_with(&s.cx, |view, _| view.completion_shown())
    });
    assert_eq!(asked(&s), 1);
    let caret = before.len() + 1;
    assert_eq!(
        answered(&mut s, &editor, caret, "_"),
        ["_len", "_lock", "_log"]
    );

    // Typing `lo` narrows the complete list, asking nothing more (the
    // second request was the one above): the server's `_lock`, then
    // `_log`, which, accepted, replaces `_lo`.
    s.cx.simulate_input("lo");
    s.draw();
    assert!(editor.read_with(&s.cx, |view, _| view.completion_shown()));
    assert_eq!(asked(&s), 2);
    s.cx.simulate_keystrokes("down enter");
    s.draw();
    let text = editor.read_with(&s.cx, |view, _| view.model().text().to_string());
    assert_eq!(text, "fn main() {\n    let a = _log(\"what\");\n}\n");
    let selection = editor.read_with(&s.cx, |view, _| view.model().primary_selection().range);
    let what = text.find("what").unwrap();
    assert_eq!(selection, what..what + "what".len());
}

#[gpui::test]
fn a_punctuation_trigger_offers_no_snippets(cx: &mut TestAppContext) {
    let mut s = triggered(cx);
    reply_with(
        &s,
        r#"{ "label": "len", "kind": 2 }, { "label": "lock", "kind": 2 }"#,
    );
    let before = "fn main() {\n    x";
    let editor = open_at(
        &mut s,
        "src/snip.rs",
        "fn main() {\n    x\n}\n",
        before.len(),
    );
    let uri = s.uri("src/snip.rs");
    s.fake
        .wait_for_document(&uri, "didOpen", |r| r.is_some_and(|r| r.open));

    // `.` asks the server alone: its items, no snippet, not even `.log`.
    s.cx.simulate_input(".");
    s.until("the menu", |s| {
        editor.read_with(&s.cx, |view, _| view.completion_shown())
    });
    let caret = before.len() + 1;
    assert_eq!(answered(&mut s, &editor, caret, "."), ["len", "lock"]);
    for punctuation in [":", ">"] {
        assert_eq!(
            answered(&mut s, &editor, caret, punctuation),
            ["len", "lock"]
        );
    }

    // Typing `lo` narrows to `lock`; asked for there, `.log` and `_log`
    // are offered after the server's items, `.log` replacing `.lo`.
    s.cx.simulate_input("lo");
    s.draw();
    s.cx.simulate_keystrokes("escape ctrl-space");
    s.until("the menu", |s| {
        editor.read_with(&s.cx, |view, _| view.completion_shown())
    });
    let items = offered(&mut s, &editor);
    let labels: Vec<_> = items.iter().map(|item| item.label.as_str()).collect();
    assert_eq!(labels, ["lock", ".log", "_log"]);
    let caret = caret + "lo".len();
    assert_eq!(items[1].edit.0, caret - ".lo".len()..caret);
    assert_eq!(items[2].edit.0, caret - "lo".len()..caret);
}
