//! First-that-answers (§5, §7 Routing): every server advertising the
//! feature is asked at once, and the first non-empty answer *in server
//! order* is taken, whichever came first. Rename goes to the server whose
//! preparation accepted it, alone.

use std::path::Path;
use std::time::Duration;

use fe_buffer::Buffer;
use futures::FutureExt;
use lsp_types::{OneOf, RenameOptions, ServerCapabilities, Url};
use serde_json::{Value, json};

use super::{Fixture, ROOT, answer};
use crate::fake::{Reply, Scripted};
use crate::rename::RenameTarget;
use crate::workspace_edit::EditTarget;
use crate::{DocumentHighlight, HighlightKind, LspError};

const HIGHLIGHT: &str = "textDocument/documentHighlight";
const PREPARE: &str = "textDocument/prepareRename";
const RENAME: &str = "textDocument/rename";

/// Two rows: `a` at bytes 0..1, `b` at 5..6.
const TEXT: &str = "a {}\nb {}\n";

fn highlights(capabilities: &mut ServerCapabilities) {
    capabilities.document_highlight_provider = Some(OneOf::Left(true));
}

fn renames(capabilities: &mut ServerCapabilities) {
    capabilities.rename_provider = Some(OneOf::Right(RenameOptions {
        prepare_provider: Some(true),
        work_done_progress_options: Default::default(),
    }));
}

/// The first character of row `row`.
fn first_of(row: u32) -> Value {
    json!({
        "start": { "line": row, "character": 0 },
        "end": { "line": row, "character": 1 },
    })
}

/// A reply giving one highlight, of row `row`'s first character.
fn highlight(row: u32) -> Reply {
    Reply::Result(json!([{ "range": first_of(row), "kind": 1 }]))
}

#[test]
fn primary_non_empty_answer_wins_even_when_add_on_is_first() {
    let mut f = Fixture::css_and_vars_with(highlights, highlights);
    let mut buffer = Buffer::new(TEXT);
    f.open_on_both(&mut buffer);
    // The add-on answers at once; the primary only once the clock moves.
    let late = Duration::from_millis(500);
    f.fake("css").set_reply(
        HIGHLIGHT,
        Scripted {
            reply: highlight(0),
            delay: late,
        },
    );
    f.fake("vars")
        .set_reply(HIGHLIGHT, Scripted::now(highlight(1)));

    let mut asked = f
        .manager
        .document_highlights(buffer.id(), buffer.version(), 0)
        .boxed();
    // Both are asked at once, and the add-on has answered: still no
    // answer, as the primary comes first.
    f.clock.wait_for_sleep(late);
    f.fake("vars").wait_for("the add-on asked", |records| {
        (records.iter()).any(|record| record.method() == HIGHLIGHT)
    });
    f.barrier(f.placed(&buffer, "vars").unwrap());
    assert!((&mut asked).now_or_never().is_none());

    f.clock.advance(late);
    assert_eq!(
        answer(asked),
        Ok(Some(vec![DocumentHighlight::new(
            0..1,
            HighlightKind::Text
        )])),
        "the primary's highlight"
    );
    assert_eq!(f.asked("css", HIGHLIGHT).len(), 1);
    assert_eq!(f.asked("vars", HIGHLIGHT).len(), 1);
}

#[test]
fn add_on_answer_used_when_primary_is_empty() {
    let mut f = Fixture::css_and_vars_with(highlights, highlights);
    let mut buffer = Buffer::new(TEXT);
    f.open_on_both(&mut buffer);
    f.fake("css")
        .set_reply(HIGHLIGHT, Scripted::now(Reply::Result(json!([]))));
    f.fake("vars")
        .set_reply(HIGHLIGHT, Scripted::now(highlight(1)));

    let found = answer(
        f.manager
            .document_highlights(buffer.id(), buffer.version(), 5),
    );
    assert_eq!(
        found,
        Ok(Some(vec![DocumentHighlight::new(
            5..6,
            HighlightKind::Text
        )])),
        "the add-on's highlight"
    );
    assert_eq!(f.asked("css", HIGHLIGHT).len(), 1);
    assert_eq!(f.asked("vars", HIGHLIGHT).len(), 1);
}

/// Both servers rename and prepare: the primary finds nothing to rename at
/// the start of `TEXT`, and the add-on accepts `a`. Returns the fixture,
/// the buffer, its URI and the prepared target.
fn prepared_by_add_on() -> (Fixture, Buffer, Url, RenameTarget) {
    let mut f = Fixture::css_and_vars_with(renames, renames);
    let mut buffer = Buffer::new(TEXT);
    let uri = f.open_on_both(&mut buffer);
    f.fake("css")
        .set_reply(PREPARE, Scripted::now(Reply::Result(Value::Null)));
    f.fake("vars")
        .set_reply(PREPARE, Scripted::now(Reply::Result(first_of(0))));
    let edit = |text: &str| {
        Scripted::now(Reply::Result(json!({
            "changes": { uri.as_str(): [{ "range": first_of(0), "newText": text }] },
        })))
    };
    f.fake("css").set_reply(RENAME, edit("from-css"));
    f.fake("vars").set_reply(RENAME, edit("from-vars"));

    let target = answer(f.manager.prepare_rename(buffer.id(), buffer.version(), 0))
        .unwrap()
        .expect("the add-on accepts");
    assert_eq!(
        (target.range.clone(), target.placeholder.as_str()),
        (0..1, "a")
    );
    let vars = f.placed(&buffer, "vars").unwrap();
    assert_eq!(target.owner.map(|owner| owner.server), Some(vars));
    // Both were asked to prepare.
    assert_eq!(f.asked("css", PREPARE).len(), 1);
    assert_eq!(f.asked("vars", PREPARE).len(), 1);
    (f, buffer, uri, target)
}

#[test]
fn rename_goes_to_accepting_owner_only() {
    let (f, buffer, _uri, target) = prepared_by_add_on();
    let renamed =
        answer(
            f.manager
                .rename_prepared(buffer.id(), buffer.version(), &target, 0, "z".into()),
        )
        .unwrap();
    let [edits] = renamed.as_slice() else {
        panic!("one file's edits: {renamed:?}");
    };
    match &edits.target {
        EditTarget::Open {
            buffer: edited,
            edits,
            ..
        } => {
            assert_eq!(*edited, buffer.id());
            assert_eq!(edits, &[(0..1, "from-vars".to_owned())]);
        }
        other => panic!("not the open buffer's edits: {other:?}"),
    }
    assert_eq!(f.asked("vars", RENAME).len(), 1);
    assert!(f.asked("css", RENAME).is_empty());
}

#[test]
fn rename_to_restarted_owner_is_unavailable_and_sends_nothing() {
    let (mut f, buffer, uri, target) = prepared_by_add_on();
    let old = target.owner.unwrap().server;
    let new = (f.manager.restart("vars", Path::new(ROOT)).unwrap()).expect("it restarted");
    assert_ne!(new, old);
    let text = buffer.text().to_string();
    f.fake("vars")
        .wait_for_document(&uri, "the new instance to open it", |r| {
            r.is_some_and(|r| r.open && r.launch == 2 && r.text == text)
        });

    let renamed =
        answer(
            f.manager
                .rename_prepared(buffer.id(), buffer.version(), &target, 0, "z".into()),
        );
    match renamed {
        Err(LspError::Unavailable(why)) => assert_eq!(why, "the language server restarted"),
        other => panic!("not unavailable: {other:?}"),
    }
    // Neither the new instance nor the primary was asked.
    f.barrier(new);
    f.barrier(f.placed(&buffer, "css").unwrap());
    for name in ["css", "vars"] {
        assert!(f.asked(name, RENAME).is_empty(), "{name} got a rename");
    }
}
