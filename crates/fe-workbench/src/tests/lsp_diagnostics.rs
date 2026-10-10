//! Diagnostics in the workbench (docs/decisions/lsp-client.md §4.1,
//! "Delivery to the UI"): the fake's publications reach the editor's
//! diagnostic set, and an event of an identity no longer active is
//! rejected when it's delivered, however late.

use std::{
    thread,
    time::{Duration, Instant},
};

use fe_gpui::EditorView;
use fe_lsp::{IncarnationId, LspEvent, SessionId, fake::Writes};
use gpui::{Entity, TestAppContext};
use lsp_types::{
    Diagnostic, Position, PublishDiagnosticsParams, Range, Url, notification::PublishDiagnostics,
};

use super::lsp::Lsp;

/// Has the fake publish, for `uri` at `version`, one diagnostic over the
/// first two bytes, saying `message`.
fn publish(s: &Lsp, uri: &Url, version: i32, message: &str) {
    s.fake
        .notify::<PublishDiagnostics>(PublishDiagnosticsParams {
            uri: uri.clone(),
            diagnostics: vec![Diagnostic {
                range: Range::new(Position::new(0, 0), Position::new(0, 2)),
                message: message.into(),
                ..Default::default()
            }],
            version: Some(version),
        });
}

/// Blocks until `done`, without letting the workbench take any event.
fn wait(what: &str, done: impl Fn() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !done() {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        thread::sleep(Duration::from_millis(2));
    }
}

impl Lsp {
    /// The editor showing `name`.
    fn editor_of(&mut self, name: &str) -> Entity<EditorView> {
        let path = self.path(name);
        self.workbench.read_with(&self.cx, |workbench, cx| {
            (workbench.open_editors(cx).into_iter())
                .find(|editor| editor.read(cx).model().path() == Some(&path))
                .expect("an editor shows it")
        })
    }

    /// The messages of `editor`'s diagnostics, and the text they cover.
    fn shown(&mut self, editor: &Entity<EditorView>) -> Vec<(String, String)> {
        editor.read_with(&self.cx, |view, _| {
            let model = view.model();
            (model.diagnostics().into_iter())
                .map(|entry| {
                    let text = model.text().slice_to_string(entry.range.clone());
                    (text, entry.message.clone())
                })
                .collect()
        })
    }

    fn messages(&mut self, editor: &Entity<EditorView>) -> Vec<String> {
        self.shown(editor).into_iter().map(|(_, m)| m).collect()
    }

    fn delivered(&mut self) -> Vec<(u64, LspEvent, Option<bool>)> {
        self.workbench.read_with(&self.cx, |w, _| w.lsp_delivered())
    }

    /// The last delivery of the `Diagnostics` saying `message`: where it is
    /// among the deliveries, its identity, and whether it was applied.
    fn delivery(&mut self, message: &str) -> (usize, (SessionId, IncarnationId), bool) {
        let delivered = self.delivered();
        (delivered.iter().enumerate().rev())
            .find_map(|(at, (_, event, applied))| match event {
                LspEvent::Diagnostics {
                    session,
                    incarnation,
                    items,
                    ..
                } if items[0].message == message => {
                    Some((at, (*session, *incarnation), applied.expect("decided")))
                }
                _ => None,
            })
            .unwrap_or_else(|| panic!("{message:?} wasn't delivered"))
    }

    /// Asserts the `Diagnostics` saying `message` was delivered and
    /// rejected, with no `Diagnostics` of another identity delivered
    /// before it since `since`.
    fn rejected_first(&mut self, message: &str, since: usize) {
        let (at, identity, applied) = self.delivery(message);
        assert!(!applied, "{message:?} was applied");
        for (_, event, _) in &self.delivered()[since..at] {
            if let LspEvent::Diagnostics {
                session,
                incarnation,
                ..
            } = event
            {
                assert_eq!((*session, *incarnation), identity, "a newer one came first");
            }
        }
    }

    fn diagnostics_sent(&mut self) -> usize {
        self.workbench
            .read_with(&self.cx, |w, _| w.lsp_diagnostics_sent())
    }

    fn restarting_sent(&mut self) -> usize {
        self.workbench
            .read_with(&self.cx, |w, _| w.lsp_restarting_sent())
    }

    fn queued(&mut self) -> usize {
        self.workbench.read_with(&self.cx, |w, _| w.lsp_queued())
    }

    /// Draws a frame without letting background work land: the events
    /// queued for the workbench stay there.
    fn frame(&mut self) {
        self.workbench.update(&mut self.cx, |_, cx| cx.notify());
        self.cx.update(|window, cx| window.draw(cx).clear(cx));
    }

    /// Has the fake publish `message` for `uri` at `version`, and blocks
    /// until the sink has sent it, still queued for the workbench.
    fn publish_queued(&mut self, uri: &Url, version: i32, message: &str) {
        let sent = self.diagnostics_sent();
        publish(self, uri, version, message);
        let workbench = self.workbench.clone();
        let cx = &self.cx;
        wait("the sink to send it", || {
            workbench.read_with(cx, |w, _| w.lsp_diagnostics_sent()) > sent
        });
        assert!(self.queued() > 0);
    }
}

#[gpui::test]
fn stale_events_are_rejected(cx: &mut TestAppContext) {
    let mut s = Lsp::new(cx);
    s.open("src/main.rs");
    let main = s.uri("src/main.rs");
    s.fake
        .wait_for_document(&main, "didOpen", |r| r.is_some_and(|r| r.open));
    let editor = s.editor_of("src/main.rs");

    // The fake's publication shows.
    publish(&s, &main, 0, "first");
    s.until("the publication", |s| s.messages(&editor) == ["first"]);
    assert_eq!(s.shown(&editor), [("fn".into(), "first".into())]);
    assert!(s.delivery("first").2);

    // Closed and reopened on the manager (plain text, then Rust again),
    // with one for the old identity still queued: rejected as it's drained,
    // before the new identity has published anything.
    let since = s.delivered().len();
    s.publish_queued(&main, 0, "stale across the reopen");
    s.cx.update(|_, cx| editor.update(cx, |view, cx| view.set_language(None, cx)));
    s.frame();
    s.cx.update(|_, cx| editor.update(cx, |view, cx| view.set_language(Some("rust"), cx)));
    s.frame();
    s.fake.wait_for_document(&main, "the reopen", |r| {
        r.is_some_and(|r| r.open && r.version == 1)
    });
    assert!(s.queued() > 0);
    s.draw();
    assert_eq!(s.queued(), 0);
    s.rejected_first("stale across the reopen", since);
    s.until("the first cleared", |s| s.messages(&editor).is_empty());

    // The same across a restart after a crash.
    publish(&s, &main, 1, "second");
    s.until("the second", |s| s.messages(&editor) == ["second"]);
    let since = s.delivered().len();
    s.publish_queued(&main, 1, "stale across the restart");
    let restarting = s.restarting_sent();
    s.fake.crash();
    // The stderr drain arms its own 1 s `STDERR_GRACE` sleep first; once
    // `Restarting` is sent only the backoff can still be armed.
    let workbench = s.workbench.clone();
    let cx = &s.cx;
    wait("the restart status", || {
        workbench.read_with(cx, |w, _| w.lsp_restarting_sent()) > restarting
    });
    s.clock.wait_for_sleep(Duration::from_secs(1));
    s.clock.advance(Duration::from_secs(1));
    s.fake.wait_initialized_at(2);
    s.fake
        .wait_for_document(&main, "the restart's didOpen", |r| {
            r.is_some_and(|r| r.open && r.launch == 2)
        });
    assert!(s.queued() > 0);
    s.draw();
    assert_eq!(s.queued(), 0);
    s.rejected_first("stale across the restart", since);
    s.until("the second cleared", |s| s.messages(&editor).is_empty());

    // A Diagnostics delivered after its DiagnosticsCleared: "second",
    // whose identity the crash ended.
    let (at, identity, applied) = s.delivery("second");
    assert!(applied);
    let delivered = s.delivered();
    let cleared = delivered[at..].iter().any(|(_, event, _)| {
        matches!(event, LspEvent::DiagnosticsCleared { session, incarnation, .. }
            if (*session, *incarnation) == identity)
    });
    assert!(cleared, "its DiagnosticsCleared was delivered");
    let (generation, event, _) = delivered[at].clone();
    s.workbench
        .update(&mut s.cx, |w, cx| w.lsp_deliver(generation, event, cx));
    assert!(!s.delivery("second").2);
    assert!(s.messages(&editor).is_empty());

    // The actor's open of lib.rs held (its thread blocked writing main.rs's
    // edit) while the tab closes: once it resumes, nothing of lib.rs is
    // opened, and its publication is dropped.
    s.fake.set_writes(Writes::Stepped);
    s.cx.update(|_, cx| {
        editor.update(cx, |view, cx| {
            view.model_mut().insert("x");
            cx.notify();
        })
    });
    s.frame();
    let fake = s.fake.clone();
    wait("the edit's write", || fake.write_waiting());
    s.open("src/lib.rs");
    assert!(s.active_server().is_some(), "lib.rs went to the manager");
    let lib = s.uri("src/lib.rs");
    let path = s.path("src/lib.rs");
    let workbench = s.workbench.clone();
    s.cx.update(|window, cx| {
        let pane = workbench.read(cx).pane().clone();
        pane.update(cx, |pane, cx| {
            let ix = pane.position(&path).unwrap();
            pane.activate(ix, window, cx);
            pane.close_active(window, cx);
        });
    });
    s.draw();
    s.fake.set_writes(Writes::Open);
    s.fake.wait_for_document(&main, "the edit", |r| {
        r.is_some_and(|r| r.launch == 2 && r.version == 1)
    });
    let sent = s.diagnostics_sent();
    publish(&s, &lib, 0, "stale open");
    publish(&s, &main, 1, "marker");
    s.until("the marker", |s| s.messages(&editor) == ["marker"]);
    assert_eq!(
        s.diagnostics_sent(),
        sent + 1,
        "lib.rs's publication was decoded"
    );
    assert_eq!(s.fake.document(&lib), None, "lib.rs was opened");
}
