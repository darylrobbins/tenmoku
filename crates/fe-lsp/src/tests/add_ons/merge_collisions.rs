//! Completion and code-action collisions (§5 dedupe, §7 Merging): two
//! entries collapse only when *both* are final and command-free and the
//! two are equivalent. Every case runs twice, the primary answering first
//! and then the add-on, and must give the same list.

use std::time::Duration;

use fe_buffer::Buffer;
use futures::FutureExt;
use lsp_types::{
    CodeActionOptions, CodeActionProviderCapability, CompletionOptions, ExecuteCommandOptions,
    ServerCapabilities, Url,
};
use serde_json::{Value, json};

use super::{Fixture, answer};
use crate::fake::{Record, Reply, Scripted};
use crate::{CodeAction, CompletionItem, CompletionTrigger, ServerId};

const COMPLETION: &str = "textDocument/completion";
const CODE_ACTION: &str = "textDocument/codeAction";
const EXECUTE: &str = "workspace/executeCommand";

/// The word `co` is bytes 4..6.
const TEXT: &str = "a { co }\n";

/// How long the server answering second waits, on the manual clock.
const LATE: Duration = Duration::from_millis(700);

/// The command both servers run.
const COMMAND: &str = "x.run";

/// What each server resolves.
#[derive(Clone, Copy)]
struct Resolves {
    css: bool,
    vars: bool,
}

const FINAL: Resolves = Resolves {
    css: false,
    vars: false,
};

/// Both servers complete and act, resolving as `resolves` says, and run
/// [`COMMAND`].
fn fixture(resolves: Resolves) -> Fixture {
    let give = |resolve: bool| {
        move |capabilities: &mut ServerCapabilities| {
            capabilities.completion_provider = Some(CompletionOptions {
                resolve_provider: Some(resolve),
                ..CompletionOptions::default()
            });
            capabilities.code_action_provider =
                Some(CodeActionProviderCapability::Options(CodeActionOptions {
                    resolve_provider: Some(resolve),
                    ..CodeActionOptions::default()
                }));
            capabilities.execute_command_provider = Some(ExecuteCommandOptions {
                commands: vec![COMMAND.into()],
                ..ExecuteCommandOptions::default()
            });
        }
    };
    Fixture::css_and_vars_with(give(resolves.css), give(resolves.vars))
}

/// Asks with `ask`, the primary answering `method` with `css` and the
/// add-on with `vars`, the one `primary_first` says first and the other
/// only after it has.
fn in_order<T>(
    f: &Fixture,
    method: &str,
    (css, vars): (Value, Value),
    primary_first: bool,
    ask: impl FnOnce(&Fixture) -> futures::future::BoxFuture<'static, T>,
) -> T {
    let (first, second) = match primary_first {
        true => (("css", css), ("vars", vars)),
        false => (("vars", vars), ("css", css)),
    };
    f.fake(first.0)
        .set_reply(method, Scripted::now(Reply::Result(first.1)));
    f.fake(second.0).set_reply(
        method,
        Scripted {
            reply: Reply::Result(second.1),
            delay: LATE,
        },
    );
    let mut asked = ask(f);
    f.fake(first.0).wait_for("the first answer", |records| {
        (records.iter())
            .any(|record| matches!(record, Record::Replied { method: m, .. } if m == method))
    });
    f.clock.wait_for_sleep(LATE);
    assert!((&mut asked).now_or_never().is_none(), "waits for both");
    f.clock.advance(LATE);
    answer(asked)
}

/// Which fake an instance is.
fn named(f: &Fixture, buffer: &Buffer, server: ServerId) -> &'static str {
    match f.placed(buffer, "css") == Some(server) {
        true => "css",
        false => "vars",
    }
}

/// A completion item labelled `x` over the word, with `extra` fields.
fn item(extra: Value) -> Value {
    let mut item = json!({
        "label": "x",
        "kind": 6,
        "textEdit": {
            "range": {
                "start": { "line": 0, "character": 4 },
                "end": { "line": 0, "character": 6 },
            },
            "newText": "x",
        },
    });
    item.as_object_mut()
        .unwrap()
        .extend(extra.as_object().unwrap().clone());
    item
}

/// What a menu shows of an item, and whose it is.
type Shown = (String, &'static str, Option<String>, Option<String>, bool);

/// One completion case, in one order: the fixture, the buffer and the
/// merged items, each as [`Shown`] too.
fn complete(
    resolves: Resolves,
    (css, vars): (Value, Value),
    primary_first: bool,
) -> (Fixture, Buffer, Vec<CompletionItem>, Vec<Shown>) {
    let mut f = fixture(resolves);
    let mut buffer = Buffer::new(TEXT);
    f.open_on_both(&mut buffer);
    let (id, version) = (buffer.id(), buffer.version());
    let replies = (json!([css]), json!([vars]));
    let list = in_order(&f, COMPLETION, replies, primary_first, |f| {
        (f.manager
            .completion(id, version, 6, CompletionTrigger::Invoked))
        .boxed()
    })
    .unwrap()
    .expect("a list");
    let shown = (list.items.iter())
        .map(|item| {
            (
                item.label.clone(),
                named(&f, &buffer, item.server()),
                item.filter_text.clone(),
                item.server_name.clone(),
                item.command().is_some(),
            )
        })
        .collect();
    (f, buffer, list.items, shown)
}

/// `complete` in both orders, which must agree: the primary-first run.
fn complete_both(
    resolves: Resolves,
    css: Value,
    vars: Value,
) -> (Fixture, Buffer, Vec<CompletionItem>, Vec<Shown>) {
    let (.., later) = complete(resolves, (css.clone(), vars.clone()), false);
    let run = complete(resolves, (css, vars), true);
    assert_eq!(run.3, later, "the same list in both orders");
    run
}

fn shown(label: &str, server: &'static str, named: bool) -> Shown {
    (
        label.into(),
        server,
        None,
        named.then(|| server.to_owned()),
        false,
    )
}

#[test]
fn completion_collisions_give_same_list_in_both_orders() {
    let plain = item(json!({}));

    // Two equal final items show once, the primary's copy.
    let (.., list) = complete_both(FINAL, plain.clone(), plain.clone());
    assert_eq!(list, [shown("x", "css", false)], "equal final items");

    // A resolvable item beside an equal final one, either way round: both.
    for resolves in [
        Resolves {
            css: true,
            vars: false,
        },
        Resolves {
            css: false,
            vars: true,
        },
    ] {
        let (.., list) = complete_both(resolves, plain.clone(), plain.clone());
        assert_eq!(
            list,
            [shown("x", "css", true), shown("x", "vars", true)],
            "a resolvable item stays"
        );
    }

    // An item with a command beside an equal command-free one, either way
    // round: both.
    let commanded = item(json!({ "command": { "title": "Go", "command": "other" } }));
    let (.., list) = complete_both(FINAL, commanded.clone(), plain.clone());
    assert!(list[0].4);
    assert_eq!(list.len(), 2, "the primary's command keeps both");
    let (.., list) = complete_both(FINAL, plain.clone(), commanded.clone());
    assert!(list[1].4);
    assert_eq!(list.len(), 2, "the add-on's command keeps both");

    // Same label and edits, neither with `data`, the add-on's server
    // resolving an import the primary's item hasn't: both, and accepting
    // the add-on's applies its import.
    let resolves = Resolves {
        css: false,
        vars: true,
    };
    let (f, buffer, items, list) = complete_both(resolves, plain.clone(), plain.clone());
    assert_eq!(list, [shown("x", "css", true), shown("x", "vars", true)]);
    let import = json!({
        "range": { "start": { "line": 0, "character": 0 }, "end": { "line": 0, "character": 0 } },
        "newText": "@import 'x';\n",
    });
    let mut resolved = item(json!({ "additionalTextEdits": [import] }));
    resolved["label"] = json!("x");
    f.fake("vars").set_reply(
        "completionItem/resolve",
        Scripted::now(Reply::Result(resolved)),
    );
    let vars = f.placed(&buffer, "vars").unwrap();
    let theirs = items.iter().find(|item| item.server() == vars).unwrap();
    let accepted = answer(f.manager.resolve(theirs.clone())).unwrap();
    assert_eq!(
        (accepted.additional_edits.iter())
            .map(|edit| (edit.range.clone(), edit.text.as_str()))
            .collect::<Vec<_>>(),
        [(0..0, "@import 'x';\n")]
    );
    assert!(f.asked("css", "completionItem/resolve").is_empty());

    // Identical command payloads from two owners: both, each run on its
    // own server.
    let run = item(json!({ "command": { "title": "Run", "command": COMMAND, "arguments": [1] } }));
    let (f, _, items, list) = complete_both(FINAL, run.clone(), run.clone());
    assert_eq!(list.len(), 2, "identical commands keep both");
    for item in &items {
        answer(f.manager.run_completion_command(item)).unwrap();
    }
    assert_eq!(f.asked("css", EXECUTE).len(), 1);
    assert_eq!(f.asked("vars", EXECUTE).len(), 1);

    // Same label and edits with another `filterText`: both kept, so typing
    // that matches only the second still finds it.
    let (.., list) = complete_both(
        FINAL,
        item(json!({ "filterText": "xa" })),
        item(json!({ "filterText": "xb" })),
    );
    assert_eq!(
        (list.iter())
            .map(|shown| (shown.1, shown.2.as_deref()))
            .collect::<Vec<_>>(),
        [("css", Some("xa")), ("vars", Some("xb"))]
    );
}

/// A code action titled `Fix`, editing the word, with `extra` fields.
fn action(uri: &Url, new_text: &str, extra: Value) -> Value {
    let mut action = json!({
        "title": "Fix",
        "kind": "refactor",
        "edit": { "changes": { uri.as_str(): [{
            "range": {
                "start": { "line": 0, "character": 4 },
                "end": { "line": 0, "character": 6 },
            },
            "newText": new_text,
        }] } },
    });
    action
        .as_object_mut()
        .unwrap()
        .extend(extra.as_object().unwrap().clone());
    action
}

/// What a menu shows of an action, and whose it is.
type Listed = (String, &'static str, Option<String>);

/// One code-action case, in one order. `actions` builds each server's
/// action from the document's URI.
fn act(
    resolves: Resolves,
    actions: &dyn Fn(&Url) -> (Value, Value),
    primary_first: bool,
) -> (Fixture, Vec<CodeAction>, Vec<Listed>) {
    let mut f = fixture(resolves);
    let mut buffer = Buffer::new(TEXT);
    let uri = f.open_on_both(&mut buffer);
    let (css, vars) = actions(&uri);
    let (id, version) = (buffer.id(), buffer.version());
    let replies = (json!([css]), json!([vars]));
    let found = in_order(&f, CODE_ACTION, replies, primary_first, |f| {
        (f.manager.code_actions(id, version, 4..6, Vec::new())).boxed()
    })
    .unwrap();
    let listed = (found.iter())
        .map(|action| {
            (
                action.title.clone(),
                named(&f, &buffer, action.server()),
                action.server_name.clone(),
            )
        })
        .collect();
    (f, found, listed)
}

/// `act` in both orders, which must agree: the primary-first run.
fn act_both(
    resolves: Resolves,
    actions: impl Fn(&Url) -> (Value, Value),
) -> (Fixture, Vec<CodeAction>, Vec<Listed>) {
    let (.., later) = act(resolves, &actions, false);
    let run = act(resolves, &actions, true);
    assert_eq!(run.2, later, "the same list in both orders");
    run
}

fn listed(server: &'static str, named: bool) -> Listed {
    ("Fix".into(), server, named.then(|| server.to_owned()))
}

#[test]
fn code_action_collisions_give_same_list_in_both_orders() {
    let both = [listed("css", true), listed("vars", true)];

    // Two equal final, command-free actions with edits show once, the
    // primary's copy.
    let (.., list) = act_both(FINAL, |uri| {
        (action(uri, "x", json!({})), action(uri, "x", json!({})))
    });
    assert_eq!(list, [listed("css", false)], "equal final actions");

    // An action with a command beside an equal final one, either way
    // round: both.
    let command = json!({ "command": { "title": "Fix", "command": "other" } });
    let (.., list) = act_both(FINAL, |uri| {
        (
            action(uri, "x", command.clone()),
            action(uri, "x", json!({})),
        )
    });
    assert_eq!(list, both, "the primary's command keeps both");
    let (.., list) = act_both(FINAL, |uri| {
        (
            action(uri, "x", json!({})),
            action(uri, "x", command.clone()),
        )
    });
    assert_eq!(list, both, "the add-on's command keeps both");

    // One from a server that resolves beside an equal final one, either
    // way round: both.
    for resolves in [
        Resolves {
            css: true,
            vars: false,
        },
        Resolves {
            css: false,
            vars: true,
        },
    ] {
        let (.., list) = act_both(resolves, |uri| {
            (action(uri, "x", json!({})), action(uri, "x", json!({})))
        });
        assert_eq!(list, both, "a resolvable action stays");
    }

    // Same title and kind with other edits, or another `disabled`: both,
    // with server names.
    let (.., list) = act_both(FINAL, |uri| {
        (action(uri, "x", json!({})), action(uri, "y", json!({})))
    });
    assert_eq!(list, both, "other edits");
    let disabled = json!({ "disabled": { "reason": "not here" } });
    let (.., list) = act_both(FINAL, |uri| {
        (
            action(uri, "x", json!({})),
            action(uri, "x", disabled.clone()),
        )
    });
    assert_eq!(list, both, "another disabled");

    // Identical commands from two owners: both, each run on its own server.
    let run = json!({ "title": "Fix", "command": { "title": "Fix", "command": COMMAND } });
    let (f, actions, list) = act_both(FINAL, |_| (run.clone(), run.clone()));
    assert_eq!(list, both, "identical commands");
    for action in &actions {
        answer(f.manager.run_code_action_command(action)).unwrap();
    }
    assert_eq!(f.asked("css", EXECUTE).len(), 1);
    assert_eq!(f.asked("vars", EXECUTE).len(), 1);

    // Actions awaiting resolve: both.
    let pending = json!({ "title": "Fix", "kind": "refactor", "data": 1 });
    let resolving = Resolves {
        css: true,
        vars: true,
    };
    let (.., list) = act_both(resolving, |_| (pending.clone(), pending.clone()));
    assert_eq!(list, both, "awaiting resolve");
}
