//! Asking for code actions of given kinds, and automatically.

use serde_json::{Value, json};

use super::{ACTION, Started, titles};
use crate::code_action::CodeActionQuery;

fn ask(s: &Started, query: CodeActionQuery) -> Vec<crate::CodeAction> {
    let beta = super::TEXT.find("beta").unwrap();
    super::answer(s.manager.code_actions_for(
        s.id(),
        s.buffer.version(),
        beta..beta + 4,
        Vec::new(),
        query,
    ))
    .unwrap()
}

fn reply() -> Value {
    json!([
        { "title": "Fix", "kind": "quickfix" },
        { "title": "Organize", "kind": "source.organizeImports" },
        { "title": "Ruff", "kind": "source.organizeImports.ruff" },
        { "title": "Sourcery", "kind": "sourcery" },
        { "title": "Run", "command": "run", "arguments": [] },
    ])
}

#[test]
fn only_sends_and_filters_kinds() {
    let s = Started::new(false);
    s.reply(ACTION, reply());
    let actions = ask(
        &s,
        CodeActionQuery {
            only: Some(vec!["source.organizeImports".into()]),
            automatic: false,
        },
    );
    let sent = s.sent(ACTION);
    assert_eq!(
        sent[0]["context"]["only"],
        json!(["source.organizeImports"])
    );
    assert_eq!(sent[0]["context"]["triggerKind"], json!(1));
    assert_eq!(titles(&actions), ["Organize", "Ruff"]);
    assert!(actions[0].kind_is("source") && !actions[0].kind_is("sourcery"));
}

#[test]
fn automatic_queries_say_so() {
    let s = Started::new(false);
    s.reply(ACTION, reply());
    let actions = ask(
        &s,
        CodeActionQuery {
            only: None,
            automatic: true,
        },
    );
    assert_eq!(s.sent(ACTION)[0]["context"]["triggerKind"], json!(2));
    assert_eq!(
        titles(&actions),
        ["Fix", "Organize", "Ruff", "Sourcery", "Run"]
    );
}
