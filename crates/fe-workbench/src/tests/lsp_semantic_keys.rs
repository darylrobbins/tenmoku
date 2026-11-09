//! A server's `property` token over a key the grammar paints Lapis, as
//! rust-analyzer sends for `Book { title: … }`, leaves the key's colour;
//! the same token over member access still paints `property`.

use super::*;
use crate::{Pane, pane_group::Direction};

fn script() -> Script {
    let mut script = super::script();
    let init = match &mut script.initialize {
        InitializeReply::Result(init) => init,
        _ => unreachable!(),
    };
    let Some(SemanticTokensServerCapabilities::SemanticTokensOptions(options)) =
        &mut init.capabilities.semantic_tokens_provider
    else {
        unreachable!()
    };
    // Index 3.
    options.legend.token_types.push(SemanticTokenType::PROPERTY);
    script
}

const LINE: &str = "fn f(b: Book) -> Book { Book { title: b.title } }";

#[gpui::test]
fn a_property_token_on_a_struct_key_leaves_it_lapis(cx: &mut TestAppContext) {
    let mut s = Lsp::with_fake_clock(cx, script(), Script::default());
    std::fs::write(s.path("src/main.rs"), format!("{LINE}\n")).unwrap();
    let at = |nth: usize| LINE.match_indices("title").nth(nth).unwrap().0 as u32;
    // `property` on the key, then on the access.
    let data = [0, at(0), 5, 3, 0, 0, at(1) - at(0), 5, 3, 0];
    s.fake.set_reply(
        FULL,
        Scripted::now(Reply::Result(json!({ "resultId": "1", "data": data }))),
    );
    s.set_trust(true);
    s.open("src/main.rs");
    let main = s.uri("src/main.rs");
    s.fake
        .wait_for_document(&main, "didOpen", |r| r.is_some_and(|r| r.open));
    let editor = (s.workbench)
        .read_with(&s.cx, |w, cx| w.active_editor(cx).cloned())
        .expect("main.rs is shown");
    s.cx.update(|window, cx| window.focus(&editor.focus_handle(cx), cx));
    s.draw();
    s.until("the parse", |s| {
        s.draw();
        editor.read_with(&s.cx, |view, _| {
            view.syntax()
                .is_some_and(|syntax| syntax.tree().is_some() && !syntax.needs_parse())
        })
    });
    s.until("the first ask", |s| {
        s.clock.advance(Duration::from_millis(20));
        s.cx.executor().advance_clock(REST);
        !asked(s).is_empty()
    });
    s.until("the tokens", |s| !painted(s, &editor).is_empty());
    let access = at(1) as usize;
    assert_eq!(
        painted(&s, &editor),
        vec![(access..access + 5, "property".to_owned())]
    );
}

/// Where the two `title`s of [`LINE`] start: the key, then the access.
fn titles() -> (usize, usize) {
    let at = |nth: usize| LINE.match_indices("title").nth(nth).unwrap().0;
    (at(0), at(1))
}

/// `[key, access]` as `property` tokens, encoded.
fn key_and_access() -> [u32; 10] {
    let (key, access) = titles();
    [0, key as u32, 5, 3, 0, 0, (access - key) as u32, 5, 3, 0]
}

fn property(start: usize) -> (std::ops::Range<usize>, String) {
    (start..start + 5, "property".to_owned())
}

/// main.rs shown with [`LINE`], its first tokens landed with the key left
/// to the grammar, and src/other.rs on disk to open over it. Deltas answer
/// that nothing changed.
fn opened(cx: &mut TestAppContext) -> (Lsp, Entity<EditorView>) {
    let mut s = Lsp::with_fake_clock(cx, script(), Script::default());
    std::fs::write(s.path("src/main.rs"), format!("{LINE}\n")).unwrap();
    std::fs::write(s.path("src/other.rs"), "fn other() {}\n").unwrap();
    s.fake.set_reply(
        FULL,
        Scripted::now(Reply::Result(
            json!({ "resultId": "1", "data": key_and_access() }),
        )),
    );
    s.fake.set_reply(
        DELTA,
        Scripted::now(Reply::Result(json!({ "resultId": "2", "edits": [] }))),
    );
    s.set_trust(true);
    s.open("src/main.rs");
    let editor = (s.workbench)
        .read_with(&s.cx, |w, cx| w.active_editor(cx).cloned())
        .expect("main.rs is shown");
    let (_, access) = titles();
    s.until("the first tokens", |s| {
        s.clock.advance(Duration::from_millis(20));
        s.cx.executor().advance_clock(REST);
        painted(s, &editor) == vec![property(access)]
    });
    (s, editor)
}

/// Adds a row at the end through `editor`, which moves no token.
fn append_row(s: &mut Lsp, editor: &Entity<EditorView>) {
    s.cx.update(|_, cx| {
        editor.update(cx, |view, cx| {
            let end = view.model().snapshot().text().len();
            view.model_mut().set_cursor(end);
            view.model_mut().insert("\n");
            cx.notify();
        })
    });
    s.draw();
}

fn deltas(s: &Lsp) -> usize {
    (s.fake.records().iter())
        .filter(|r| matches!(r, Record::Request { method, .. } if method == DELTA))
        .count()
}

/// Tokens that land before the grammar has parsed their version paint
/// every `property`; once it has, without another edit or ask, the key's
/// is taken off.
#[gpui::test]
fn a_key_painted_before_the_parse_is_lapis_once_parsed(cx: &mut TestAppContext) {
    let (mut s, editor) = opened(cx);
    let (key, access) = titles();

    // main.rs behind another tab isn't drawn, so its grammar doesn't catch
    // up with an edit there, and the next tokens land before its parse.
    s.open("src/other.rs");
    append_row(&mut s, &editor);
    s.until("the tokens before the parse", |s| {
        s.cx.executor().advance_clock(REST);
        painted(s, &editor).len() == 2
    });
    assert_eq!(painted(&s, &editor), vec![property(key), property(access)]);
    let asks = (asked(&s).len(), deltas(&s));

    // Shown again, it's parsed, and the key goes back to the grammar's.
    s.open("src/main.rs");
    s.until("the key checked", |s| painted(s, &editor).len() == 1);
    assert_eq!(painted(&s, &editor), vec![property(access)]);
    assert_eq!((asked(&s).len(), deltas(&s)), asks, "no new ask");
}

/// A refresh that times out drops the tokens a delta builds on, but not
/// those waiting for the parse: the key still leaves `property` once
/// parsed, with no answer since.
#[gpui::test]
fn a_timed_out_refresh_keeps_the_key_check_waiting(cx: &mut TestAppContext) {
    let (mut s, editor) = opened(cx);
    let (key, access) = titles();
    s.open("src/other.rs");
    append_row(&mut s, &editor);
    s.until("the tokens before the parse", |s| {
        s.cx.executor().advance_clock(REST);
        painted(s, &editor).len() == 2
    });

    // Every ask from here answers after `lsp.timeouts.hover` (3 s).
    let late = |id: &str| Scripted {
        reply: Reply::Result(json!({ "resultId": id, "data": key_and_access() })),
        delay: Duration::from_secs(10),
    };
    s.fake.set_reply(FULL, late("3"));
    s.fake.set_reply(DELTA, late("3"));
    let before = asked(&s).len();
    let main = s.uri("src/main.rs");
    let fake = s.fake.clone();
    let refresh = std::thread::spawn(move || fake.request::<SemanticTokensRefresh>(()));
    // main.rs's refresh ask times out, and its retry, from scratch, is sent.
    let retried = |s: &Lsp| {
        asked(s)[before..].iter().any(|(method, params)| {
            method == FULL && params["textDocument"]["uri"].as_str() == Some(main.as_str())
        })
    };
    s.until("the retry after a timeout", |s| {
        s.clock.advance(Duration::from_millis(500));
        s.cx.executor().advance_clock(REST);
        retried(s)
    });
    refresh.join().unwrap().ok();
    assert_eq!(painted(&s, &editor), vec![property(key), property(access)]);

    s.open("src/main.rs");
    s.until("the key checked", |s| painted(s, &editor).len() == 1);
    assert_eq!(painted(&s, &editor), vec![property(access)]);
}

/// With main.rs in two panes, one hidden behind another tab and so behind
/// on its parse, the shown one's grammar decides. Each way round, so
/// whichever the tokens list first is the stale one once.
#[gpui::test]
fn a_hidden_split_behind_on_its_parse_doesnt_keep_a_key_painted(cx: &mut TestAppContext) {
    let (mut s, left_editor) = opened(cx);
    let (key, access) = titles();
    let left = s.workbench.read_with(&s.cx, |w, _| w.pane().clone());
    let workbench = s.workbench.clone();
    workbench.update_in(&mut s.cx, |workbench, window, cx| {
        let new = (workbench.workspace().clone()).update(cx, |workspace, cx| {
            workspace.split(Direction::Right, window, cx)
        });
        workbench.activate(new, window, cx);
    });
    s.draw();
    let right = s.workbench.read_with(&s.cx, |w, _| w.pane().clone());
    assert_ne!(left, right, "a split");
    let right_editor = right
        .read_with(&s.cx, |pane, _| pane.tabs()[0].editor().cloned())
        .expect("main.rs is copied into the split");
    let other = s.path("src/other.rs");
    let open_other = |s: &mut Lsp, pane: &Entity<Pane>| {
        let path = other.clone();
        s.cx.update(|window, cx| pane.update(cx, |pane, cx| pane.open(path, window, cx)));
        s.draw();
    };

    // Each round's delta puts a `variable` token on a name before the key,
    // so its landing shows: `f`, then `b`.
    let variable = |name: usize| (name..name + 1, "variable".to_owned());
    let mut result = 2;
    for (hidden, shown_editor, name) in [(&right, &left_editor, 3), (&left, &right_editor, 5)] {
        // The hidden pane's main.rs goes behind other.rs; the other pane's
        // is shown.
        if hidden == &left {
            right.update_in(&mut s.cx, |pane, window, cx| pane.activate(0, window, cx));
        }
        open_other(&mut s, hidden);
        result += 1;
        let deleted = if result == 3 { 5 } else { 10 };
        s.fake.set_reply(
            DELTA,
            Scripted::now(Reply::Result(json!({
                "resultId": result.to_string(),
                "edits": [{
                    "start": 0,
                    "deleteCount": deleted,
                    "data": [0, name, 1, 2, 0, 0, key - name, 5, 3, 0],
                }],
            }))),
        );
        append_row(&mut s, shown_editor);
        s.until("the delta's tokens", |s| {
            s.cx.executor().advance_clock(REST);
            painted(s, shown_editor).first() == Some(&variable(name))
        });
        assert_eq!(
            painted(&s, shown_editor),
            vec![variable(name), property(access)],
            "{name}"
        );
    }
}
