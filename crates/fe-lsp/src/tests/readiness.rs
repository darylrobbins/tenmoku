//! Readiness and the empty-answer rule (§3.4), on the injected clock.

use std::time::Duration;

use lsp_types::notification::Progress;
use lsp_types::request::{GotoDefinition, WorkDoneProgressCreate};
use lsp_types::{
    GotoDefinitionParams, GotoDefinitionResponse, NumberOrString, Position, ProgressParams,
    ProgressParamsValue, TextDocumentIdentifier, TextDocumentPositionParams, Url, WorkDoneProgress,
    WorkDoneProgressBegin, WorkDoneProgressCreateParams, WorkDoneProgressEnd,
};
use serde_json::{Value, json};

use super::{Clocked, answer, capture_logs, logs_from, wait_until};
use crate::fake::{Echo, FakeServer, Reply, Script, Scripted};
use crate::readiness::{SETTLE, STATUS_WAIT};
use crate::router::{ServerStatusNotification, ServerStatusParams};
use crate::{ReadinessMode, ServerSpec, ServerStatus};

/// The fake's answer once its indexing is done.
pub(super) fn location() -> Value {
    json!([{
        "uri": "file:///fake/root/a.rs",
        "range": {
            "start": { "line": 1, "character": 2 },
            "end": { "line": 1, "character": 5 },
        },
    }])
}

/// The fake answers `definition` with `value` from now on.
pub(super) fn answer_definition(fake: &FakeServer, value: Value) {
    fake.set_reply(
        "textDocument/definition",
        Scripted::now(Reply::Result(value)),
    );
}

pub(super) fn definition_params() -> GotoDefinitionParams {
    GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier {
                uri: Url::parse("file:///fake/root/a.rs").unwrap(),
            },
            position: Position::new(0, 0),
        },
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
    }
}

/// Asks for a definition: what came back, and whether an empty answer is
/// final. Returning means the fake's notifications before it have been
/// read.
pub(super) fn definition(s: &Clocked) -> (Option<GotoDefinitionResponse>, bool) {
    let reply = answer(
        s.manager
            .request::<GotoDefinition>(s.server, definition_params()),
    )
    .unwrap();
    (reply.result, reply.empty_is_final)
}

/// Asserts an empty answer now isn't final.
fn empty_not_final(s: &Clocked, when: &str) {
    assert_eq!(definition(s), (None, false), "{when}");
}

pub(super) fn begin(fake: &FakeServer, token: &str, title: &str) {
    fake.notify::<Progress>(ProgressParams {
        token: NumberOrString::String(token.into()),
        value: ProgressParamsValue::WorkDone(WorkDoneProgress::Begin(WorkDoneProgressBegin {
            title: title.into(),
            ..Default::default()
        })),
    });
}

pub(super) fn end(fake: &FakeServer, token: &str) {
    fake.notify::<Progress>(ProgressParams {
        token: NumberOrString::String(token.into()),
        value: ProgressParamsValue::WorkDone(WorkDoneProgress::End(WorkDoneProgressEnd {
            message: None,
        })),
    });
}

/// Sends a status, and returns once the client has read it: a request
/// sent before then is answered as of the status before.
fn status(s: &Clocked, quiescent: bool) {
    s.fake
        .notify::<ServerStatusNotification>(ServerStatusParams {
            health: "ok".into(),
            quiescent,
            message: None,
        });
    answer(s.manager.request::<Echo>(s.server, json!({}))).unwrap();
}

fn indexing(title: &str) -> ServerStatus {
    ServerStatus::Indexing {
        title: title.into(),
    }
}

/// The fake's scripted indexing in `progress` mode: progress `phases`
/// (begin and end, after `initialize`), and indexing done at `done`.
/// Empty until then, and never final.
fn scripted_indexing(case: &str, phases: &[(Duration, Duration)], done: Duration) {
    let s = Clocked::new(ServerSpec::new("fake", "fake-server"), Script::default());
    answer_definition(&s.fake, Value::Null);
    let mut steps: Vec<(Duration, Option<(bool, String)>)> = phases
        .iter()
        .enumerate()
        .flat_map(|(n, &(from, to))| {
            let token = format!("phase-{n}");
            [
                (from, Some((true, token.clone()))),
                (to, Some((false, token))),
            ]
        })
        .collect();
    steps.push((done, None));
    steps.sort_by_key(|&(at, _)| at);
    let mut now = Duration::ZERO;
    for (at, step) in steps {
        // Halfway to the step, and at it.
        s.clock.advance((at - now) / 2);
        empty_not_final(&s, &format!("{case}: before {at:?}"));
        s.clock.advance(at - now - (at - now) / 2);
        now = at;
        match step {
            Some((true, token)) => begin(&s.fake, &token, "Indexing"),
            Some((false, token)) => end(&s.fake, &token),
            None => {}
        }
        empty_not_final(&s, &format!("{case}: at {at:?}"));
    }
    // Indexing is done: a non-empty answer is returned.
    answer_definition(&s.fake, location());
    let (result, _) = definition(&s);
    assert_eq!(
        serde_json::to_value(result).unwrap(),
        location(),
        "{case}: after indexing"
    );
    // 60 s of quiet later, an empty answer is still not final.
    s.clock.advance(Duration::from_secs(60));
    answer_definition(&s.fake, Value::Null);
    empty_not_final(&s, &format!("{case}: after 60 s of quiet"));
}

#[test]
fn progress_mode_never_trusts_quiet() {
    let ms = Duration::from_millis;
    let secs = Duration::from_secs;
    scripted_indexing("first progress 700 ms late", &[(ms(700), secs(3))], secs(3));
    scripted_indexing(
        "first progress 15 s late",
        &[(secs(15), secs(18))],
        secs(18),
    );
    scripted_indexing(
        "a 900 ms gap",
        &[(ms(100), secs(1)), (ms(1900), secs(3))],
        secs(3),
    );
    scripted_indexing(
        "a 15 s gap",
        &[(ms(100), secs(1)), (secs(16), secs(18))],
        secs(18),
    );
    scripted_indexing("no progress at all", &[], secs(5));

    // Ready latches 500 ms after the last counted token ends: a created
    // token is open until it begins, tokens are matched per occurrence,
    // and those `ready_ignores_progress` names don't count.
    let s = Clocked::new(
        ServerSpec {
            ready_ignores_progress: vec!["flycheck/".into()],
            ..ServerSpec::new("fake", "fake-server")
        },
        Script::default(),
    );
    answer_definition(&s.fake, Value::Null);
    s.fake
        .request::<WorkDoneProgressCreate>(WorkDoneProgressCreateParams {
            token: NumberOrString::String("fetch".into()),
        })
        .unwrap();
    s.clock.advance(Duration::from_secs(1));
    empty_not_final(&s, "created");
    s.events.assert_quiet();
    begin(&s.fake, "fetch", "Fetching");
    s.expect(indexing("Fetching"));
    begin(&s.fake, "flycheck/0", "cargo check");
    begin(&s.fake, "fetch", "Fetching again");
    s.expect(indexing("Fetching again"));
    s.clock.advance(Duration::from_secs(1));
    end(&s.fake, "fetch");
    // One occurrence of `fetch` is still open.
    empty_not_final(&s, "fetching");
    assert!(!s.clock.sleeping().contains(&SETTLE));
    s.clock.advance(Duration::from_secs(1));
    end(&s.fake, "fetch");
    // Only flycheck is open, and it doesn't count.
    s.expect(ServerStatus::Starting);
    s.clock.wait_for_sleep(SETTLE);
    s.clock.advance(SETTLE - Duration::from_millis(1));
    s.events.assert_quiet();
    empty_not_final(&s, "settling");
    s.clock.advance(Duration::from_millis(1));
    s.expect(ServerStatus::Ready);
    // Latched: more progress doesn't take it back.
    begin(&s.fake, "reindex", "Indexing");
    empty_not_final(&s, "ready, indexing again");
    s.clock.advance(Duration::from_secs(60));
    end(&s.fake, "reindex");
    empty_not_final(&s, "ready, 60 s later");
    s.events.assert_quiet();

    // A token is its kind and its value: numeric 1 and "1" are two.
    let s = Clocked::new(ServerSpec::new("fake", "fake-server"), Script::default());
    answer_definition(&s.fake, Value::Null);
    let create = |token: NumberOrString| {
        s.fake
            .request::<WorkDoneProgressCreate>(WorkDoneProgressCreateParams { token })
            .unwrap();
    };
    create(NumberOrString::Number(1));
    create(NumberOrString::String("1".into()));
    let numeric = |value| {
        s.fake.notify::<Progress>(ProgressParams {
            token: NumberOrString::Number(1),
            value: ProgressParamsValue::WorkDone(value),
        })
    };
    numeric(WorkDoneProgress::Begin(WorkDoneProgressBegin {
        title: "numeric".into(),
        ..Default::default()
    }));
    s.expect(indexing("numeric"));
    numeric(WorkDoneProgress::End(WorkDoneProgressEnd { message: None }));
    s.expect(ServerStatus::Starting);
    // "1" is still open.
    s.clock.advance(Duration::from_secs(1));
    empty_not_final(&s, "\"1\" open");
    s.events.assert_quiet();
    end(&s.fake, "1");
    s.settle();

    // Progress read after the 500 ms passed, but before the timer fired,
    // finds the server already Ready.
    for late in ["begin", "create"] {
        let s = Clocked::new(ServerSpec::new("fake", "fake-server"), Script::default());
        answer_definition(&s.fake, Value::Null);
        s.clock.wait_for_sleep(SETTLE);
        s.clock.advance(SETTLE);
        match late {
            "begin" => begin(&s.fake, "late", "Late"),
            _ => s
                .fake
                .request::<WorkDoneProgressCreate>(WorkDoneProgressCreateParams {
                    token: NumberOrString::String("late".into()),
                })
                .map(drop)
                .unwrap(),
        }
        s.expect(ServerStatus::Ready);
        empty_not_final(&s, late);
        s.events.assert_quiet();
    }
}

fn status_spec(name: &str) -> ServerSpec {
    ServerSpec {
        readiness: ReadinessMode::ServerStatus,
        ..ServerSpec::new(name, "fake-server")
    }
}

#[test]
fn server_status_mode() {
    // Progress never makes it ready, even past when the progress rule
    // would have latched.
    let s = Clocked::new(status_spec("status"), Script::default());
    answer_definition(&s.fake, Value::Null);
    begin(&s.fake, "a", "Indexing");
    s.expect(indexing("Indexing"));
    s.clock.advance(Duration::from_secs(1));
    end(&s.fake, "a");
    s.expect(ServerStatus::Starting);
    s.clock.advance(Duration::from_secs(4));
    empty_not_final(&s, "quiet progress, no status");
    s.events.assert_quiet();
    status(&s, false);
    empty_not_final(&s, "quiescent false");
    s.events.assert_quiet();

    // Ready at the first quiescent status; empty answers are final.
    status(&s, true);
    s.expect(ServerStatus::Ready);
    assert_eq!(definition(&s), (None, true));

    // Quiescent false again: not final, while Ready stays latched.
    status(&s, false);
    empty_not_final(&s, "re-indexing");
    s.events.assert_quiet();
    status(&s, true);
    assert_eq!(definition(&s), (None, true));
    s.clock.advance(Duration::from_secs(60));
    assert_eq!(definition(&s), (None, true));

    // No status 10 s after initialize: it routes by the progress rule,
    // logs the fallback, and its empty answers are never final.
    capture_logs("fe-lsp silent");
    let s = Clocked::new(status_spec("silent"), Script::default());
    answer_definition(&s.fake, Value::Null);
    begin(&s.fake, "b", "Indexing");
    s.expect(indexing("Indexing"));
    s.clock.wait_for_sleep(STATUS_WAIT);
    s.clock.advance(STATUS_WAIT - Duration::from_millis(1));
    empty_not_final(&s, "waiting for a status");
    s.clock.advance(Duration::from_millis(1));
    wait_until("the fallback logged", || {
        logs_from("fe-lsp silent")
            .iter()
            .any(|line| line.contains("no experimental/serverStatus within 10 s"))
    });
    // By the progress rule: ready 500 ms after its progress ends.
    s.events.assert_quiet();
    end(&s.fake, "b");
    s.expect(ServerStatus::Starting);
    s.settle();
    empty_not_final(&s, "after the fallback");
    status(&s, true);
    empty_not_final(&s, "a late quiescent status");

    // A first status read after the 10 s passed, but before the timer
    // fired: the fallback happened all the same.
    capture_logs("fe-lsp late");
    let s = Clocked::new(status_spec("late"), Script::default());
    answer_definition(&s.fake, Value::Null);
    s.clock.wait_for_sleep(STATUS_WAIT);
    s.clock.advance(STATUS_WAIT);
    status(&s, true);
    empty_not_final(&s, "a status after the deadline");
    wait_until("the fallback logged", || {
        logs_from("fe-lsp late")
            .iter()
            .any(|line| line.contains("no experimental/serverStatus within 10 s"))
    });
}

#[test]
fn first_request_after_ready() {
    let s = Clocked::new(ServerSpec::new("fake", "fake-server"), Script::default());
    answer_definition(&s.fake, location());
    s.settle();
    let (result, _) = definition(&s);
    assert_eq!(serde_json::to_value(result).unwrap(), location());
}
