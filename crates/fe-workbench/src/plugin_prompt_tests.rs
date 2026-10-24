//! The plugin grant prompt: its text, the full-access prompt, and what the
//! answers come back as.

use fe_command::capability::{Capability, Grants};
use gpui::{Task, TestAppContext, VisualTestContext};

use crate::plugin_prompt::{
    PendingPluginGrant, PluginAnswer, PluginAsk, plugin_prompt_text, show_plugin_prompt,
};

fn grants(capabilities: &[&str]) -> Grants {
    capabilities
        .iter()
        .map(|capability| Capability::parse(capability).unwrap())
        .collect()
}

fn install(requested: &[&str]) -> PendingPluginGrant {
    PendingPluginGrant {
        id: "acme.tool".into(),
        name: "Acme Tool".into(),
        version: (1, 2, 3),
        license: "MIT".into(),
        dir: "/ext/acme-tool".into(),
        requested: grants(requested),
        ask: PluginAsk::Install,
    }
}

fn ask(vcx: &mut VisualTestContext, pending: &PendingPluginGrant) -> Task<PluginAnswer> {
    let answer = vcx.update(|window, cx| show_plugin_prompt(window, pending, cx));
    vcx.run_until_parked();
    answer
}

/// Answers the prompt up with `button`.
fn answer(vcx: &mut VisualTestContext, button: &str) {
    vcx.simulate_prompt_answer(button);
    vcx.run_until_parked();
}

#[test]
fn prompt_text_lists_every_capability_and_warning() {
    let pending = install(&[
        "buffer.read",
        "status",
        "net:api.example.com",
        "process.spawn:python3",
    ]);
    let (question, detail) = plugin_prompt_text(&pending);
    assert_eq!(question, "Allow the plugin Acme Tool to load?");
    for expected in [
        "acme.tool",
        "1.2.3",
        "MIT",
        "/ext/acme-tool",
        "buffer.read: read the text of your open files",
        "status: show items in the status bar",
        "net:api.example.com: connect to api.example.com",
        "process.spawn:python3: run the program python3, outside the sandbox",
        "It can read your code and send it to api.example.com.",
        "It runs python3 outside the sandbox, with your permissions.",
        "python3 is near-wildcard: it can run any code",
        "Don't Allow keeps it from loading",
    ] {
        assert!(detail.contains(expected), "{expected} in {detail}");
    }
    assert!(!detail.contains("full access"), "{detail}");

    // A benign plugin has no warnings to show; a wildcard says it asks again.
    let (_, detail) = plugin_prompt_text(&install(&["buffer.read", "commands"]));
    assert!(!detail.contains("Warnings"), "{detail}");
    let (_, detail) = plugin_prompt_text(&install(&["net:*"]));
    assert!(detail.contains("can send your code anywhere"), "{detail}");
    assert!(detail.contains("full access"), "{detail}");
}

#[test]
fn an_escalation_says_what_was_added_and_that_the_old_version_keeps_running() {
    let pending = PendingPluginGrant {
        version: (2, 0, 0),
        ask: PluginAsk::Escalation {
            added: grants(&["net:api.example.com"]),
            keep_running: Some((1, 4, 0)),
        },
        ..install(&["buffer.read", "net:api.example.com"])
    };
    let (question, detail) = plugin_prompt_text(&pending);
    assert_eq!(question, "Allow Acme Tool 2.0.0 more access?");
    let adds = detail.find("It adds:").expect("what it adds");
    let keeps = detail.find("It keeps:").expect("what it keeps");
    assert!(adds < keeps, "{detail}");
    assert!(
        detail[adds..keeps].contains("net:api.example.com: connect to api.example.com"),
        "{detail}"
    );
    assert!(!detail[adds..keeps].contains("buffer.read"), "{detail}");
    assert!(detail[keeps..].contains("buffer.read"), "{detail}");
    // What was granted before combines with what's added.
    assert!(
        detail.contains("It can read your code and send it to api.example.com."),
        "{detail}"
    );
    assert!(
        detail.contains("The installed version, 1.4.0, keeps running"),
        "{detail}"
    );
    assert!(!detail.contains("Don't Allow keeps it from loading"));
}

#[gpui::test]
async fn a_wildcard_needs_a_second_acceptance(cx: &mut TestAppContext) {
    let vcx = cx.add_empty_window();
    let pending = install(&["buffer.read", "net:*"]);

    // Allow only leads to the full-access prompt, whose own Allow is a
    // separate answer.
    let answered = ask(vcx, &pending);
    answer(vcx, "Allow");
    let (question, detail) = vcx.pending_prompt().expect("the full-access prompt");
    assert_eq!(question, "Give Acme Tool full access?");
    assert!(detail.contains("net:*: connect to any host"), "{detail}");
    assert!(detail.contains("can send your code anywhere"), "{detail}");
    answer(vcx, "Allow Full Access");
    assert_eq!(
        answered.await,
        PluginAnswer::Allowed {
            accept_wildcards: true
        }
    );
    assert!(!vcx.has_pending_prompt());

    // Allow then Don't Allow grants nothing.
    let answered = ask(vcx, &pending);
    answer(vcx, "Allow");
    answer(vcx, "Don't Allow");
    assert_eq!(answered.await, PluginAnswer::Declined);

    // Without a wildcard, Allow alone grants, and accepts none.
    let answered = ask(vcx, &install(&["buffer.read", "net:api.example.com"]));
    answer(vcx, "Allow");
    assert!(!vcx.has_pending_prompt());
    assert_eq!(
        answered.await,
        PluginAnswer::Allowed {
            accept_wildcards: false
        }
    );

    // An escalation asks only about what it adds: a wildcard granted
    // before isn't accepted again.
    let mut escalation = install(&["net:*", "process.spawn:rg"]);
    escalation.ask = PluginAsk::Escalation {
        added: grants(&["process.spawn:rg"]),
        keep_running: Some((1, 0, 0)),
    };
    let answered = ask(vcx, &escalation);
    answer(vcx, "Allow");
    assert!(!vcx.has_pending_prompt());
    assert_eq!(
        answered.await,
        PluginAnswer::Allowed {
            accept_wildcards: false
        }
    );
    escalation.ask = PluginAsk::Escalation {
        added: grants(&["process.spawn:*"]),
        keep_running: Some((1, 0, 0)),
    };
    let answered = ask(vcx, &escalation);
    answer(vcx, "Allow");
    assert!(vcx.has_pending_prompt());
    answer(vcx, "Allow Full Access");
    assert_eq!(
        answered.await,
        PluginAnswer::Allowed {
            accept_wildcards: true
        }
    );
}

#[gpui::test]
async fn dont_allow_declines_and_closing_leaves_it_pending(cx: &mut TestAppContext) {
    let first = cx.add_empty_window();
    let answered = ask(first, &install(&["buffer.read"]));
    answer(first, "Don't Allow");
    assert_eq!(answered.await, PluginAnswer::Declined);

    // The window closes with the prompt up: the answer the platform still
    // takes decides nothing.
    let answered = ask(first, &install(&["buffer.read"]));
    first.update(|window, _| window.remove_window());
    first.run_until_parked();
    answer(first, "Allow");
    assert_eq!(answered.await, PluginAnswer::Pending);

    // And so with the full-access prompt up.
    let second = cx.add_empty_window();
    let answered = ask(second, &install(&["process.spawn:*"]));
    answer(second, "Allow");
    second.update(|window, _| window.remove_window());
    second.run_until_parked();
    answer(second, "Allow Full Access");
    assert_eq!(answered.await, PluginAnswer::Pending);
}
