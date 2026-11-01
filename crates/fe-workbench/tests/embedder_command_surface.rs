//! The command surface `docs/embedder-audit.md` names for a host: what
//! [`fe_workbench::command_list`] lists is one entry per ID and context
//! (`editor.save` is listed for the editor and again for a Markdown
//! preview shown in its place), each titled, one title per ID; and every
//! command the audit tells a host to invoke by ID checks against
//! [`fe_workbench::command_catalog`], with the args it gives, with no app.

use std::collections::{BTreeMap, BTreeSet};

use fe_command::{CommandId, Value};
use serde_json::json;

/// The commands the audit names for a host, with the args it shows.
fn named_for_hosts() -> Vec<(&'static str, Value)> {
    vec![
        ("editor.insert", json!({ "text": "x" })),
        ("editor.undo", Value::Null),
        ("editor.redo", Value::Null),
        ("ghost_text.accept", Value::Null),
        ("ghost_text.accept_word", Value::Null),
        ("ghost_text.accept_line", Value::Null),
        ("ghost_text.dismiss", Value::Null),
        ("proposal.accept_hunk", Value::Null),
        ("proposal.reject_hunk", Value::Null),
        ("proposal.accept_all", Value::Null),
        ("proposal.reject_all", Value::Null),
        ("proposal.next_hunk", Value::Null),
        ("proposal.previous_hunk", Value::Null),
        ("chat.open", Value::Null),
        ("chat.send", Value::Null),
        ("workbench.close_tab", Value::Null),
        ("command_palette.toggle", Value::Null),
    ]
}

#[test]
fn listed_commands_are_unique_and_titled() {
    let mut contexts = BTreeSet::new();
    let mut titles = BTreeMap::new();
    for meta in fe_workbench::command_list() {
        let id = meta.id.to_string();
        assert!(!meta.title.trim().is_empty(), "{id} has no title");
        let context = meta.context.as_ref().map(|context| format!("{context:?}"));
        assert!(
            contexts.insert((id.clone(), context.clone())),
            "{id} is listed twice in {context:?}"
        );
        let title = titles
            .entry(id.clone())
            .or_insert_with(|| meta.title.clone());
        assert_eq!(*title, meta.title, "{id} has two titles");
    }
    assert!(titles.len() > 100, "{} commands listed", titles.len());
}

#[test]
fn commands_named_for_hosts_are_in_the_catalog() {
    let catalog = fe_workbench::command_catalog();
    let unchecked: Vec<String> = (named_for_hosts().into_iter())
        .filter_map(|(id, args)| {
            let command = CommandId::parse(id).expect("a valid command ID");
            (catalog.check(&command, &args).err()).map(|error| format!("{id}: {error:?}"))
        })
        .collect();
    assert_eq!(unchecked, Vec::<String>::new());
}
