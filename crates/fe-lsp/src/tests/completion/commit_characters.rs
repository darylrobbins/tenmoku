//! An item's commit characters: its own, else the server's `allCommitCharacters`.

use std::collections::HashMap;
use std::path::Path;

use fe_buffer::Buffer;
use lsp_types::request::{Completion, Request};
use lsp_types::{CompletionOptions, PositionEncodingKind};
use serde_json::json;

use super::super::{Started, answer, manager};
use crate::fake::{FakeServer, InitializeReply, Reply, Script, Scripted, initialize_result};
use crate::{CompletionTrigger, ServerSpec};

/// A server whose completion options carry `all`, started on the fake.
fn started(all: Option<Vec<String>>) -> Started {
    let mut init = initialize_result(Some(PositionEncodingKind::UTF8));
    init.capabilities.completion_provider = Some(CompletionOptions {
        all_commit_characters: all,
        ..CompletionOptions::default()
    });
    let fake = FakeServer::new(Script {
        initialize: InitializeReply::Result(Box::new(init)),
        replies: HashMap::new(),
        ..Script::default()
    });
    let (mut manager, events) = manager(fake.launcher(), None);
    let server = manager
        .start(
            ServerSpec::new("fake", "fake-server"),
            Path::new(super::super::ROOT),
        )
        .unwrap();
    fake.wait_initialized();
    Started {
        fake,
        manager,
        server,
        events,
    }
}

fn commit_characters(s: &mut Started) -> Vec<Vec<String>> {
    let mut buffer = Buffer::new("va");
    s.open_synced(&mut buffer, "a.rs");
    s.fake.set_reply(
        Completion::METHOD,
        Scripted::now(Reply::Result(json!([
            { "label": "own", "commitCharacters": ["(", ";"] },
            { "label": "plain" },
        ]))),
    );
    let items =
        answer(
            s.manager
                .completion(buffer.id(), buffer.version(), 2, CompletionTrigger::Invoked),
        )
        .unwrap()
        .unwrap()
        .items;
    items
        .into_iter()
        .map(|item| item.commit_characters)
        .collect()
}

#[test]
fn commit_characters_come_from_the_item_else_the_server() {
    let mut s = started(Some(vec![".".into()]));
    assert_eq!(commit_characters(&mut s), [vec!["(", ";"], vec!["."]]);

    let mut s = started(None);
    assert_eq!(
        commit_characters(&mut s),
        [vec!["(", ";"], Vec::<&str>::new()]
    );
}
