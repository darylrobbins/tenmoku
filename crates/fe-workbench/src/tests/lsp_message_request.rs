//! A language server's question (`window/showMessageRequest`) shown as a
//! toast with a button per answer, which stays until one is clicked or it's
//! closed.

use std::{collections::HashMap, thread, time::Duration};

use fe_lsp::fake::FakeError;
use gpui::{Modifiers, TestAppContext};
use lsp_types::{
    MessageActionItem, MessageType, ShowMessageRequestParams, request::ShowMessageRequest,
};
use serde_json::{Value, json};

use crate::{
    tests::lsp::Lsp,
    workbench::toast::{LIFETIME, Level},
};

type Answer = thread::JoinHandle<Result<Value, FakeError>>;

/// The fake server asks, from a thread, as it blocks for the answer.
fn ask(s: &Lsp, typ: MessageType, actions: &[&str]) -> Answer {
    let fake = s.fake.clone();
    let params = ShowMessageRequestParams {
        typ,
        message: "Reload the workspace?".into(),
        actions: Some(
            (actions.iter())
                .map(|title| MessageActionItem {
                    title: (*title).into(),
                    properties: HashMap::new(),
                })
                .collect(),
        ),
    };
    thread::spawn(move || fake.request::<ShowMessageRequest>(params))
}

/// What each toast shown says, top first: level, message, detail and the
/// titles of its buttons.
fn shown(s: &mut Lsp) -> Vec<(Level, String, Option<String>, Vec<String>)> {
    let toasts = s.workbench.read_with(&s.cx, |w, _| w.toasts().clone());
    toasts.read_with(&s.cx, |layer, _| {
        (layer.queue().shown().iter())
            .map(|entry| {
                let toast = &entry.toast;
                let titles = (toast.choices.as_ref()).map_or(vec![], |c| c.titles().to_vec());
                (
                    toast.level,
                    toast.message.clone(),
                    toast.detail.clone(),
                    titles,
                )
            })
            .collect()
    })
}

fn count(s: &mut Lsp) -> usize {
    shown(s).len()
}

#[gpui::test]
fn a_server_question_shows_as_a_toast_and_answers_the_choice(cx: &mut TestAppContext) {
    let mut s = Lsp::new(cx);
    s.open("src/main.rs");
    s.fake.wait_initialized();

    let reload = ask(&s, MessageType::WARNING, &["Reload", "Ignore"]);
    s.until("the question's toast", |s| count(s) == 1);
    assert_eq!(
        shown(&mut s),
        [(
            Level::Warning,
            "rust-analyzer".to_owned(),
            Some("Reload the workspace?".to_owned()),
            vec!["Reload".to_owned(), "Ignore".to_owned()],
        )]
    );
    let button =
        s.cx.debug_bounds("toast-choice-0-0")
            .expect("Reload button");
    assert!(
        s.cx.debug_bounds("toast-choice-0-1").is_some(),
        "Ignore button"
    );
    s.cx.simulate_click(button.center(), Modifiers::none());
    assert_eq!(reload.join().unwrap().unwrap(), json!({"title": "Reload"}));
    s.until("the toast to go", |s| count(s) == 0);

    // Two identical questions are two toasts, which outlast any lifetime.
    let first = ask(&s, MessageType::INFO, &["Yes"]);
    s.until("the first toast", |s| count(s) == 1);
    let second = ask(&s, MessageType::INFO, &["Yes"]);
    s.until("both toasts", |s| count(s) == 2);
    s.cx.executor()
        .advance_clock(LIFETIME + Duration::from_secs(1));
    s.draw();
    assert_eq!(count(&mut s), 2);

    // Closing one answers it null; the other is dismissed with the rest.
    let close =
        s.cx.debug_bounds("icon-button:toast-close-1")
            .expect("close button");
    s.cx.simulate_click(close.center(), Modifiers::none());
    assert_eq!(first.join().unwrap().unwrap(), Value::Null);
    s.until("one toast left", |s| count(s) == 1);
    s.run("toast.dismiss_all");
    assert_eq!(second.join().unwrap().unwrap(), Value::Null);
    s.until("no toasts", |s| count(s) == 0);
}
