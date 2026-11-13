//! What `$/progress` shows the host: each open work-done token's title,
//! message and percentage, through [`LspEvent::Progress`] and
//! [`LspManager::progress`](crate::LspManager::progress).

use std::path::Path;
use std::sync::LazyLock;
use std::time::{Duration, Instant};

use lsp_types::notification::Progress;
use lsp_types::{
    NumberOrString, ProgressParams, ProgressParamsValue, WorkDoneProgress, WorkDoneProgressBegin,
    WorkDoneProgressReport,
};

use super::readiness::{begin, end};
use super::{Clocked, ROOT, wait_until};
use crate::fake::{FakeServer, Script};
use crate::readiness::{SETTLE, WORK_MAX};
use crate::{LspEvent, ServerId, ServerSpec, ServerStatus, WorkProgress};

fn token(name: &str) -> NumberOrString {
    NumberOrString::String(name.into())
}

fn begin_with(fake: &FakeServer, name: &str, title: &str, message: &str, percentage: u32) {
    fake.notify::<Progress>(ProgressParams {
        token: token(name),
        value: ProgressParamsValue::WorkDone(WorkDoneProgress::Begin(WorkDoneProgressBegin {
            title: title.into(),
            message: Some(message.into()),
            percentage: Some(percentage),
            ..Default::default()
        })),
    });
}

fn report(fake: &FakeServer, name: &str, message: Option<&str>, percentage: Option<u32>) {
    fake.notify::<Progress>(ProgressParams {
        token: token(name),
        value: ProgressParamsValue::WorkDone(WorkDoneProgress::Report(WorkDoneProgressReport {
            message: message.map(Into::into),
            percentage,
            ..Default::default()
        })),
    });
}

/// The start every expected [`work`] has, and what [`shown`] gives every
/// reported one, for comparing what they show.
static STARTED: LazyLock<Instant> = LazyLock::new(Instant::now);

fn work(name: &str, title: &str, message: Option<&str>, percentage: Option<u32>) -> WorkProgress {
    WorkProgress {
        token: token(name),
        title: title.into(),
        message: message.map(Into::into),
        percentage,
        started: *STARTED,
    }
}

/// `work` as it shows, its starts set to [`STARTED`], once they're seen
/// to be in order: the oldest begun first.
fn shown(mut work: Vec<WorkProgress>) -> Vec<WorkProgress> {
    let starts: Vec<Instant> = work.iter().map(|w| w.started).collect();
    assert!(starts.is_sorted(), "{work:?}");
    for work in &mut work {
        work.started = *STARTED;
    }
    work
}

/// Waits for `s`'s server to report open work that `pick` accepts, and
/// returns it as it came.
fn reported_raw(
    s: &Clocked,
    what: &str,
    pick: impl Fn(&[WorkProgress]) -> bool,
) -> Vec<WorkProgress> {
    let server = s.server;
    s.events.until(what, |event| match event {
        LspEvent::Progress {
            server: from,
            name,
            work,
        } if from == server && pick(&shown(work.clone())) => {
            assert_eq!(name, "fake");
            Some(work)
        }
        _ => None,
    })
}

/// [`reported_raw`], [`shown`].
fn reported(s: &Clocked, what: &str, pick: impl Fn(&[WorkProgress]) -> bool) -> Vec<WorkProgress> {
    shown(reported_raw(s, what, pick))
}

/// What the manager lists for `server`, if anything, as it came.
fn listed_raw(s: &Clocked, server: ServerId) -> Option<Vec<WorkProgress>> {
    let progress = s.manager.progress();
    assert!(progress.len() <= 1, "{progress:?}");
    let listed = progress.into_iter().find(|p| p.server == server)?;
    assert_eq!(listed.name, "fake");
    assert_eq!(listed.root, Path::new(ROOT));
    Some(listed.work)
}

/// [`listed_raw`], [`shown`].
fn listed(s: &Clocked, server: ServerId) -> Option<Vec<WorkProgress>> {
    listed_raw(s, server).map(shown)
}

/// Waits for the server's latest status to be `status`. The events
/// [`reported`] passed over may have held statuses, so they aren't read.
fn status_becomes(s: &Clocked, status: ServerStatus) {
    wait_until(&format!("{status:?}"), || {
        s.manager.status_at("fake", Path::new(ROOT)).as_ref() == Some(&status)
    });
}

/// Lets readiness's 500 ms pass and sees the server `Ready`.
fn settle(s: &Clocked) {
    s.clock.wait_for_sleep(SETTLE);
    s.clock.advance(SETTLE);
    status_becomes(s, ServerStatus::Ready);
}

#[test]
fn open_tokens_report_title_message_and_percentage() {
    let s = Clocked::new(ServerSpec::new("fake", "fake-server"), Script::default());
    assert_eq!(listed(&s, s.server), None);

    let before = Instant::now();
    begin_with(&s.fake, "index", "Indexing", "0/10", 0);
    let first = vec![work("index", "Indexing", Some("0/10"), Some(0))];
    let begun = reported_raw(&s, "the begin", |w| w.len() == 1);
    let started = begun[0].started;
    assert!(before <= started && started <= Instant::now());
    assert_eq!(shown(begun), first);
    assert_eq!(listed(&s, s.server), Some(first));

    // A report updates what it says; what it leaves out stays.
    report(&s.fake, "index", Some("3/10"), Some(30));
    let want = vec![work("index", "Indexing", Some("3/10"), Some(30))];
    assert_eq!(reported(&s, "the report", |w| w == want), want);
    report(&s.fake, "index", None, Some(40));
    let want = vec![work("index", "Indexing", Some("3/10"), Some(40))];
    assert_eq!(reported(&s, "the percentage", |w| w == want), want);
    report(&s.fake, "index", Some("4/10"), None);
    let want = vec![work("index", "Indexing", Some("4/10"), Some(40))];
    assert_eq!(reported(&s, "the message", |w| w == want), want);
    assert_eq!(want[0].fraction(), Some(0.4));

    // A second token, with neither; listed after the first, which began
    // first, even as the first is reported again.
    begin(&s.fake, "check", "Checking");
    report(&s.fake, "index", Some("5/10"), Some(50));
    let want = vec![
        work("index", "Indexing", Some("5/10"), Some(50)),
        work("check", "Checking", None, None),
    ];
    let both = reported_raw(&s, "both", |w| w == want);
    // Its start is its begin's, however often it reports.
    assert_eq!(both[0].started, started);
    assert!(both[1].started >= started);
    assert_eq!(shown(both), want);
    assert_eq!(listed(&s, s.server), Some(want.clone()));
    assert_eq!(want[1].fraction(), None);

    // A percentage past 100 reads as 100.
    report(&s.fake, "check", None, Some(250));
    let over = reported(&s, "the overshoot", |w| {
        w.len() == 2 && w[1].percentage.is_some()
    });
    assert_eq!(over[1].percentage, Some(100));
    assert_eq!(over[1].fraction(), Some(1.0));

    // A report for a token never begun shows nothing: it has no title.
    report(&s.fake, "stray", Some("?"), Some(10));
    begin(&s.fake, "last", "Last");
    let after = reported(&s, "the last", |w| w.len() == 3);
    assert_eq!(after[2], work("last", "Last", None, None));

    // What the status shows is as before: the latest begun counted title.
    status_becomes(
        &s,
        ServerStatus::Indexing {
            title: "Last".into(),
        },
    );
}

#[test]
fn ended_tokens_disappear() {
    let mut s = Clocked::new(ServerSpec::new("fake", "fake-server"), Script::default());
    let server = s.server;
    begin(&s.fake, "a", "A");
    begin(&s.fake, "b", "B");
    reported(&s, "both", |w| w.len() == 2);

    end(&s.fake, "a");
    let left = vec![work("b", "B", None, None)];
    assert_eq!(reported(&s, "a's end", |w| w.len() == 1), left);
    assert_eq!(listed(&s, s.server), Some(left));

    // A reused token stays until each of its begins has ended, and keeps
    // its place.
    begin(&s.fake, "b", "B again");
    begin(&s.fake, "c", "C");
    let want = vec![work("b", "B again", None, None), work("c", "C", None, None)];
    assert_eq!(reported(&s, "b again", |w| w == want), want);
    // One of b's begins ending changes nothing shown; the next begin
    // finds b still there.
    end(&s.fake, "b");
    begin(&s.fake, "d", "D");
    let want = vec![
        work("b", "B again", None, None),
        work("c", "C", None, None),
        work("d", "D", None, None),
    ];
    assert_eq!(reported(&s, "d", |w| w.len() == 3), want);
    end(&s.fake, "b");
    end(&s.fake, "c");
    end(&s.fake, "d");
    assert_eq!(reported(&s, "none open", <[_]>::is_empty), Vec::new());
    assert_eq!(listed(&s, s.server), None);
    settle(&s);

    // A session that ends with work open takes it along: an empty report
    // comes before its status says so.
    begin(&s.fake, "late", "Late");
    reported(&s, "late", |w| w.len() == 1);
    assert!(listed(&s, s.server).is_some());
    s.fake.crash();
    let mut cleared = false;
    let status = s.events.until("Restarting", |event| match event {
        LspEvent::Progress {
            server: from, work, ..
        } if from == server => {
            assert!(work.is_empty(), "{work:?}");
            cleared = true;
            None
        }
        LspEvent::Status {
            server: from,
            status: status @ ServerStatus::Restarting(_),
            ..
        } if from == server => Some(status),
        _ => None,
    });
    assert_eq!(status, ServerStatus::Restarting(1));
    assert!(cleared, "no empty report before Restarting");
    assert_eq!(listed(&s, s.server), None);

    // Its next session starts with nothing open, and a stop clears too.
    s.clock.wait_for_sleep(Duration::from_secs(1));
    s.clock.advance(Duration::from_secs(1));
    s.expect(ServerStatus::Starting);
    s.fake.wait_initialized_at(2);
    begin(&s.fake, "again", "Again");
    reported(&s, "again", |w| w.len() == 1);
    s.manager.stop(server);
    let status = s.events.until("Stopped", |event| match event {
        LspEvent::Progress {
            server: from, work, ..
        } if from == server => {
            assert!(work.is_empty(), "{work:?}");
            None
        }
        LspEvent::Status {
            server: from,
            status: status @ ServerStatus::Stopped(_),
            ..
        } if from == server => Some(status),
        _ => None,
    });
    assert_eq!(status, ServerStatus::Stopped(None));
    assert!(s.manager.progress().is_empty());
}

#[test]
fn the_64_token_cap_still_holds() {
    assert_eq!(WORK_MAX, 64);
    let s = Clocked::new(ServerSpec::new("fake", "fake-server"), Script::default());
    let names: Vec<String> = (0..70).map(|n| format!("t{n:02}")).collect();
    for name in &names {
        begin(&s.fake, name, name);
    }
    // The 64 oldest begun are listed; the others aren't, yet.
    let oldest: Vec<NumberOrString> = names[..64].iter().map(|n| token(n)).collect();
    let tokens = |w: &[WorkProgress]| w.iter().map(|w| w.token.clone()).collect::<Vec<_>>();
    reported(&s, "the 64 oldest", |w| tokens(w) == oldest);
    let listed_now = listed(&s, s.server).unwrap();
    assert_eq!(tokens(&listed_now), oldest);

    // Readiness still counts all 70: the latest begun is the status.
    status_becomes(
        &s,
        ServerStatus::Indexing {
            title: "t69".into(),
        },
    );
    // Every begin has been read by now: t64 to t69 are hidden.
    let hidden_from = Instant::now();
    assert_eq!(tokens(&listed(&s, s.server).unwrap()), oldest);

    // One ending lets the next oldest in, with the start of its begin,
    // which came while it was hidden: after t63's, before the begins were all read.
    end(&s.fake, "t00");
    let next: Vec<NumberOrString> = names[1..65].iter().map(|n| token(n)).collect();
    let admitted = reported_raw(&s, "t64 in", |w| tokens(w) == next);
    let (t63, t64) = (&admitted[62], &admitted[63]);
    assert_eq!(t64.token, token("t64"));
    assert!(t63.started <= t64.started && t64.started < hidden_from);

    // All ending empties it, and readiness settles as before.
    for name in &names[1..] {
        end(&s.fake, name);
    }
    reported(&s, "none open", <[_]>::is_empty);
    settle(&s);
    assert_eq!(listed(&s, s.server), None);
}
