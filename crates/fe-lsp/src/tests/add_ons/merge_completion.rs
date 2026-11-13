//! Merged completion (§5 "Completion merging", §7 Merging): both servers'
//! lists in one, a trigger character asking only the servers that list
//! it, and an incomplete list re-asking every server at the new version.

use std::time::Duration;

use fe_buffer::{Buffer, Origin};
use futures::FutureExt;
use lsp_types::{CompletionOptions, ServerCapabilities};
use serde_json::{Value, json};

use super::{Fixture, answer};
use crate::fake::{Record, Reply, Scripted};
use crate::{CompletionTrigger, Completions, ServerId};

const COMPLETION: &str = "textDocument/completion";

/// `co` being typed in a rule: the word is bytes 4..6.
const TEXT: &str = "a { co }\n";

/// A completion provider with `triggers`, that doesn't resolve.
pub(super) fn completes(capabilities: &mut ServerCapabilities, triggers: &[&str]) {
    capabilities.completion_provider = Some(CompletionOptions {
        trigger_characters: Some(triggers.iter().map(|&c| c.to_owned()).collect()),
        resolve_provider: Some(false),
        ..CompletionOptions::default()
    });
}

/// A list of one item per label, each replacing row 0's bytes 4..`end`.
fn list(incomplete: bool, labels: &[&str], end: u32) -> Reply {
    let items: Vec<Value> = (labels.iter())
        .map(|label| {
            json!({
                "label": label,
                "textEdit": {
                    "range": {
                        "start": { "line": 0, "character": 4 },
                        "end": { "line": 0, "character": end },
                    },
                    "newText": label,
                },
            })
        })
        .collect();
    Reply::Result(json!({ "isIncomplete": incomplete, "items": items }))
}

/// Each item's label, with the server that offered it.
fn offered(list: &Completions) -> Vec<(&str, ServerId)> {
    (list.items.iter())
        .map(|item| (item.label.as_str(), item.server()))
        .collect()
}

#[test]
fn two_lists_merge_and_trigger_char_asks_only_its_server() {
    let mut f = Fixture::css_and_vars_with(|c| completes(c, &["."]), |c| completes(c, &["-"]));
    let mut buffer = Buffer::new(TEXT);
    f.open_on_both(&mut buffer);
    let (css, vars) = (
        f.placed(&buffer, "css").unwrap(),
        f.placed(&buffer, "vars").unwrap(),
    );
    f.fake("css")
        .set_reply(COMPLETION, Scripted::now(list(false, &["color"], 6)));
    f.fake("vars")
        .set_reply(COMPLETION, Scripted::now(list(false, &["--x"], 6)));
    assert_eq!(f.manager.trigger_characters(buffer.id()), [".", "-"]);

    let (id, version) = (buffer.id(), buffer.version());
    let merged = answer(
        f.manager
            .completion(id, version, 6, CompletionTrigger::Invoked),
    )
    .unwrap()
    .expect("a list");
    assert_eq!(offered(&merged), [("color", css), ("--x", vars)]);
    assert!(!merged.is_incomplete);
    assert_eq!(f.asked("css", COMPLETION).len(), 1);
    assert_eq!(f.asked("vars", COMPLETION).len(), 1);

    // `-` is the add-on's trigger alone: the primary isn't asked.
    let triggered =
        answer(
            f.manager
                .completion(id, version, 6, CompletionTrigger::Character("-".into())),
        )
        .unwrap()
        .expect("a list");
    assert_eq!(offered(&triggered), [("--x", vars)]);
    assert_eq!(f.asked("css", COMPLETION).len(), 1);
    let asked = f.asked("vars", COMPLETION);
    assert_eq!(
        asked[1]["context"],
        json!({ "triggerKind": 2, "triggerCharacter": "-" })
    );
}

#[test]
fn incomplete_server_reasks_both_at_new_version() {
    let mut f = Fixture::css_and_vars_with(|c| completes(c, &[]), |c| completes(c, &[]));
    let mut buffer = Buffer::new(TEXT);
    let uri = f.open_on_both(&mut buffer);
    let vars = f.placed(&buffer, "vars").unwrap();
    f.fake("css")
        .set_reply(COMPLETION, Scripted::now(list(true, &["color"], 6)));
    f.fake("vars")
        .set_reply(COMPLETION, Scripted::now(list(false, &["--x"], 6)));
    let first =
        answer(
            f.manager
                .completion(buffer.id(), buffer.version(), 6, CompletionTrigger::Invoked),
        )
        .unwrap()
        .expect("a list");
    assert!(first.is_incomplete, "the primary's list is incomplete");

    // Typing on: `col`, the word now 4..7.
    buffer.edit([(6..6, "l")], Origin::User).expect("an edit");
    f.manager.changed(buffer.id());
    f.fake("css")
        .set_reply(COMPLETION, Scripted::now(list(false, &["color"], 7)));
    f.fake("vars")
        .set_reply(COMPLETION, Scripted::now(list(false, &["--x"], 7)));
    let again = answer(f.manager.completion(
        buffer.id(),
        buffer.version(),
        7,
        CompletionTrigger::Incomplete,
    ))
    .unwrap()
    .expect("a list");

    // Both were asked again, each with the new text synced first.
    for name in ["css", "vars"] {
        let asked = f.asked(name, COMPLETION);
        assert_eq!(asked.len(), 2, "{name} asked again");
        assert_eq!(asked[1]["position"], json!({ "line": 0, "character": 7 }));
        assert_eq!(asked[1]["context"]["triggerKind"], json!(3));
        let records = f.fake(name).records();
        let changed = (records.iter())
            .rposition(|record| record.is_notification("textDocument/didChange"))
            .expect("the edit was synced");
        let requested = (records.iter())
            .rposition(|record| record.method() == COMPLETION)
            .unwrap();
        assert!(changed < requested, "{name} had the new text when asked");
    }
    f.wait_synced("vars", &uri, &buffer);
    assert!(!again.is_incomplete, "the whole list was replaced");

    // Accepting the complete server's item applies its edit at the new
    // version's range: no stray text.
    let item = (again.items.iter())
        .find(|item| item.server() == vars)
        .unwrap();
    assert_eq!(item.edit.range, 4..7);
    buffer
        .edit(
            [(item.edit.range.clone(), item.edit.text.as_str())],
            Origin::User,
        )
        .unwrap();
    assert_eq!(buffer.text().to_string(), "a { --x }\n");
}

#[test]
fn superseded_request_is_cancelled_on_both_servers() {
    let mut f = Fixture::css_and_vars_with(|c| completes(c, &[]), |c| completes(c, &[]));
    let mut buffer = Buffer::new(TEXT);
    f.open_on_both(&mut buffer);
    for name in ["css", "vars"] {
        f.fake(name).set_reply(
            COMPLETION,
            Scripted {
                reply: list(true, &[name], 6),
                delay: Duration::from_secs(1),
            },
        );
    }
    let older = f
        .manager
        .completion(buffer.id(), buffer.version(), 6, CompletionTrigger::Invoked)
        .boxed();
    for name in ["css", "vars"] {
        f.fake(name).wait_for("the request", |records| {
            (records.iter()).any(|record| record.method() == COMPLETION)
        });
    }
    // A newer request supersedes it: dropping it cancels it everywhere.
    drop(older);
    for name in ["css", "vars"] {
        f.fake(name).wait_for("the cancel", |records| {
            (records.iter()).any(|record| record.is_notification("$/cancelRequest"))
        });
        let id = (f.fake(name).records().into_iter())
            .find_map(|record| match record {
                Record::Request { id, method, .. } if method == COMPLETION => Some(id),
                _ => None,
            })
            .unwrap();
        let cancelled = (f.fake(name).records().into_iter()).any(|record| {
            matches!(record, Record::Notification { method, params }
                if method == "$/cancelRequest" && params["id"] == json!(id))
        });
        assert!(cancelled, "{name}'s request was cancelled");
    }
}
