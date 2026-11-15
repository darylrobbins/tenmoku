//! Ask an agent and Explain on the Problems detail pane: the chat opened
//! with the problem and three rows either side attached, the first leaving
//! the prompt to send, the second sending it. The chat's provider is the
//! scripted one: no network.

use std::{fs, rc::Rc, sync::Arc};

use fe_chat::{ChatEvent, ChatProvider, ContextItem, ContextKind, Rows, ScriptedProvider};
use fe_command::{ContextFrame, ContextStack, Invocation};
use fe_editor::diagnostics::{Diagnostic, Severity};
use fe_gpui::commands::RunCommand;
use gpui::{Entity, Focusable as _, TestAppContext, VisualTestContext};
use kiln::core::secrets::MemoryStore;
use kiln_l10n::tr;

use super::open_root;
use crate::{
    Workbench,
    chat_panel::{ChatPanel, KeySource},
    dock::problems_panel::{ASK_SELECTOR, EXPLAIN_SELECTOR, ProblemsPanel},
};

const MESSAGE: &str = "cannot find value `nine`";

/// Ten rows, `nine` on row 5 (index 4).
fn source() -> String {
    "row0\nrow1\nrow2\nrow3\nlet nine = 9;\nrow5\nrow6\nrow7\nrow8\nrow9\n".to_owned()
}

fn draw(cx: &mut VisualTestContext) {
    for _ in 0..2 {
        cx.run_until_parked();
        cx.update(|window, cx| window.draw(cx).clear(cx));
    }
    cx.run_until_parked();
}

fn reply() -> Vec<ChatEvent> {
    use fe_chat::{BlockKind, StopReason};
    vec![
        ChatEvent::Started {
            model: "claude-opus-5-5".to_owned(),
            message_id: "msg".to_owned(),
        },
        ChatEvent::BlockStarted {
            index: 0,
            kind: BlockKind::Text,
        },
        ChatEvent::TextDelta {
            index: 0,
            text: "ok".to_owned(),
        },
        ChatEvent::BlockFinished {
            index: 0,
            block: serde_json::json!({"type": "text", "text": "ok"}),
        },
        ChatEvent::Finished {
            stop_reason: StopReason::EndTurn,
            stop_details: None,
        },
    ]
}

struct Fixture {
    workbench: Entity<Workbench>,
    panel: Entity<ProblemsPanel>,
    provider: Arc<ScriptedProvider>,
    _dir: tempfile::TempDir,
}

/// A trusted (or not) folder with `a.txt` open and its problem selected in
/// the Problems panel, a chat with a key (or none) over a provider that
/// answers once. With `chat_open` the chat's made, so its key is read.
fn setup(
    cx: &mut TestAppContext,
    key: bool,
    trusted: bool,
    chat_open: bool,
) -> (Fixture, &mut VisualTestContext) {
    let dir = tempfile::tempdir().unwrap();
    let root = dunce::canonicalize(dir.path()).unwrap();
    fs::write(root.join("a.txt"), source()).unwrap();
    let (workbench, cx) = open_root(cx, &root);
    let provider = Arc::new(ScriptedProvider::new([reply()]));
    let shared = provider.clone();
    cx.update(|window, cx| {
        window.activate_window();
        ChatPanel::set_provider_factory(
            cx,
            Rc::new(move |_, _| shared.clone() as Arc<dyn ChatProvider>),
        );
        cx.set_global(KeySource {
            env: key.then(|| "key".to_owned()),
            store: Arc::new(MemoryStore::new()),
        });
    });
    workbench.update(cx, |workbench, cx| workbench.set_root_trust(trusted, cx));
    let a = root.join("a.txt");
    let pane = workbench.read_with(cx, |workbench, _| workbench.pane().clone());
    cx.update(|window, cx| pane.update(cx, |pane, cx| pane.open(a.clone(), window, cx)));
    draw(cx);
    let at = source().find("nine").unwrap();
    let diagnostic = Diagnostic {
        source: Some("rustc".into()),
        code: Some("E0425".into()),
        ..Diagnostic::new(Severity::Error, MESSAGE)
    };
    workbench.update(cx, |workbench, cx| {
        workbench.set_diagnostics_for_audit(&a, vec![(at..at + 4, diagnostic)], cx)
    });
    if chat_open {
        cx.update(|window, cx| workbench.update(cx, |w, cx| w.open_chat(window, cx)));
        draw(cx);
    }
    cx.update(|window, cx| window.focus(&workbench.focus_handle(cx), cx));
    let run = RunCommand(
        Invocation::of("problems.show")
            .unwrap()
            .context(ContextStack::new().with(ContextFrame::new("workbench"))),
    );
    cx.update(|window, cx| window.dispatch_action(Box::new(run), cx));
    draw(cx);
    cx.simulate_keystrokes("down");
    draw(cx);
    let panel = workbench.read_with(cx, |workbench, cx| {
        workbench.dock().read(cx).problems().clone()
    });
    let fixture = Fixture {
        workbench,
        panel,
        provider,
        _dir: dir,
    };
    (fixture, cx)
}

fn click(selector: &'static str, cx: &mut VisualTestContext) {
    let bounds = cx.debug_bounds(selector).expect("the button is drawn");
    cx.simulate_click(bounds.center(), Default::default());
    draw(cx);
}

fn chat(f: &Fixture, cx: &mut VisualTestContext) -> Option<Entity<ChatPanel>> {
    f.workbench.read_with(cx, |workbench, cx| {
        workbench.dock().read(cx).chat().cloned()
    })
}

fn composer(panel: &Entity<ChatPanel>, cx: &mut VisualTestContext) -> String {
    let view = panel.read_with(cx, |panel, _| panel.view().clone());
    view.read_with(cx, |view, cx| view.composer_text(cx))
}

fn attached(panel: &Entity<ChatPanel>, cx: &mut VisualTestContext) -> Vec<ContextItem> {
    let view = panel.read_with(cx, |panel, _| panel.view().clone());
    view.read_with(cx, |view, _| view.attachments())
}

fn origin() -> String {
    "E0425 · rustc".to_owned()
}

#[gpui::test]
fn ask_prefills_the_chat_without_sending(cx: &mut TestAppContext) {
    let (f, cx) = setup(cx, true, true, false);
    click(ASK_SELECTOR, cx);
    let panel = chat(&f, cx).expect("the chat opened");
    assert_eq!(
        composer(&panel, cx),
        tr!(
            "workbench-problems-ask-prompt",
            message = MESSAGE,
            origin = origin()
        )
        .to_string()
    );
    assert_eq!(attached(&panel, cx).len(), 1);
    assert!(f.provider.requests().is_empty(), "the user sends it");
}

#[gpui::test]
fn explain_sends(cx: &mut TestAppContext) {
    let (f, cx) = setup(cx, true, true, false);
    click(EXPLAIN_SELECTOR, cx);
    let requests = f.provider.requests();
    assert_eq!(requests.len(), 1);
    let sent = format!("{:?}", requests[0].messages.last().unwrap().content);
    let prompt = tr!(
        "workbench-problems-explain-prompt",
        message = MESSAGE,
        origin = origin()
    )
    .to_string();
    assert!(sent.contains(&prompt), "{sent}");
    let panel = chat(&f, cx).expect("the chat opened");
    assert_eq!(composer(&panel, cx), "");
}

#[gpui::test]
fn both_are_disabled_with_a_reason_when_chat_is_unavailable(cx: &mut TestAppContext) {
    // An untrusted folder.
    let (f, cx) = setup(cx, true, false, false);
    let blocked = f.panel.read_with(cx, |panel, cx| panel.agent_blocked(cx));
    assert_eq!(
        blocked.as_deref(),
        Some(tr!("workbench-problems-ask-untrusted").to_string().as_str())
    );
    click(ASK_SELECTOR, cx);
    click(EXPLAIN_SELECTOR, cx);
    assert!(f.provider.requests().is_empty());
    if let Some(panel) = chat(&f, cx) {
        assert_eq!(composer(&panel, cx), "");
        assert!(attached(&panel, cx).is_empty());
    }
}

#[gpui::test]
fn no_key_disables_them_too(cx: &mut TestAppContext) {
    let (f, cx) = setup(cx, false, true, true);
    let blocked = f.panel.read_with(cx, |panel, cx| panel.agent_blocked(cx));
    assert_eq!(
        blocked.as_deref(),
        Some(tr!("workbench-problems-ask-no-key").to_string().as_str())
    );
    click(EXPLAIN_SELECTOR, cx);
    let panel = chat(&f, cx).expect("made");
    assert_eq!(composer(&panel, cx), "");
}

#[gpui::test]
fn the_prompt_carries_message_code_source_and_the_surrounding_rows(cx: &mut TestAppContext) {
    let (f, cx) = setup(cx, true, true, false);
    click(ASK_SELECTOR, cx);
    let panel = chat(&f, cx).expect("the chat opened");
    let prompt = composer(&panel, cx);
    for part in [MESSAGE, "E0425", "rustc"] {
        assert!(prompt.contains(part), "{prompt}");
    }
    let items = attached(&panel, cx);
    let [item] = &items[..] else {
        panic!("one item: {items:?}")
    };
    // Row 5 (index 4) and three either side, as the chat counts: 2 to 8.
    assert!(matches!(
        &item.kind,
        ContextKind::File { path, rows: Some(Rows { first: 2, last: 8 }) } if path == "a.txt"
    ));
    assert_eq!(
        item.text,
        "row1\nrow2\nrow3\nlet nine = 9;\nrow5\nrow6\nrow7\n"
    );
}
