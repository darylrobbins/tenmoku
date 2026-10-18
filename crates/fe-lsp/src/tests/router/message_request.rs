//! `window/showMessageRequest`: handed to the host, answered with its choice.

use lsp_types::request::ShowMessageRequest;
use lsp_types::{
    MessageActionItem, MessageActionItemProperty, MessageType, ShowMessageRequestParams,
};
use serde_json::{Value, json};

use crate::fake::{FakeServer, Script};
use crate::tests::{Events, manager};
use crate::{LspEvent, MessageReply, ServerSpec};

/// Sends a request with two actions from a thread; the event it reports,
/// and the thread to join for the server's answer.
fn ask(
    fake: &FakeServer,
    events: &Events,
) -> (
    MessageReply,
    std::thread::JoinHandle<Result<Value, crate::fake::FakeError>>,
) {
    let params = ShowMessageRequestParams {
        typ: MessageType::WARNING,
        message: "Reload the workspace?".into(),
        actions: Some(vec![
            MessageActionItem {
                title: "Retry".into(),
                properties: Default::default(),
            },
            MessageActionItem {
                title: "Ignore".into(),
                properties: [(
                    "remember".to_owned(),
                    MessageActionItemProperty::Boolean(true),
                )]
                .into(),
            },
        ]),
    };
    let asking = std::thread::spawn({
        let fake = fake.clone();
        move || fake.request::<ShowMessageRequest>(params)
    });
    let reply = events.until("a ShowMessageRequest", |event| match event {
        LspEvent::ShowMessageRequest {
            typ,
            message,
            actions,
            reply,
            ..
        } => {
            assert_eq!(typ, MessageType::WARNING);
            assert_eq!(message, "Reload the workspace?");
            assert_eq!(actions, ["Retry", "Ignore"]);
            Some(reply)
        }
        _ => None,
    });
    (reply, asking)
}

#[test]
fn show_message_request_answers_the_hosts_choice() {
    let root = tempfile::tempdir().unwrap();
    let fake = FakeServer::new(Script::default());
    let (mut manager, events) = manager(fake.launcher(), None);
    manager
        .start(
            ServerSpec::new("message-request", "fake-server"),
            root.path(),
        )
        .unwrap();
    fake.wait_initialized();

    let (reply, asking) = ask(&fake, &events);
    let clone = reply.clone();
    assert_eq!(clone, reply);
    reply.choose(1);
    assert_eq!(
        asking.join().unwrap(),
        Ok(json!({ "title": "Ignore", "remember": true }))
    );
    // Only the first answer counts.
    clone.dismiss();

    let (reply, asking) = ask(&fake, &events);
    reply.dismiss();
    assert_eq!(asking.join().unwrap(), Ok(Value::Null));

    let (reply, asking) = ask(&fake, &events);
    drop(reply);
    assert_eq!(asking.join().unwrap(), Ok(Value::Null));

    let (reply, asking) = ask(&fake, &events);
    reply.choose(5);
    assert_eq!(asking.join().unwrap(), Ok(Value::Null));

    // A clone answering after the first answer changes nothing.
    let (reply, asking) = ask(&fake, &events);
    let clone = reply.clone();
    reply.choose(0);
    clone.choose(1);
    assert_eq!(asking.join().unwrap(), Ok(json!({ "title": "Retry" })));
}
