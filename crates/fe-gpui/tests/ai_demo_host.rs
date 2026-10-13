//! `ai_demo`'s host glue, headless: what it does with the view's events
//! and the buffer's undos and redos, and what its scripted suggestion steps
//! report.

use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

use fe_buffer::{Buffer, Origin};
use fe_command::{ContextFrame, ContextStack, Invocation};
use fe_editor::{
    EditorModel,
    scripted_ai::{self, Report, ScriptedAi, Step, StepError},
};
use fe_gpui::{EditorTheme, EditorView, GhostTextEvent, commands::RunCommand};
use gpui::{AnyWindowHandle, Context, Focusable as _, TestAppContext, WindowHandle};

#[path = "../examples/ai_demo/host.rs"]
mod host;

use host::Host;

struct Demo {
    window: WindowHandle<EditorView>,
    ai: Rc<RefCell<ScriptedAi>>,
    host: Host,
}

/// A focused window on `text`.
fn window(cx: &mut TestAppContext, text: &str) -> WindowHandle<EditorView> {
    cx.update(fe_gpui::init);
    let text = text.to_owned();
    let window = cx.add_window(move |window, cx| {
        let view = EditorView::new(
            EditorModel::new(Buffer::new(&text), None),
            EditorTheme::default_dark(cx),
            cx,
        );
        window.focus(&view.focus_handle(cx), cx);
        view
    });
    cx.run_until_parked();
    window
}

/// A window on `text` with a driver, wired as `ai_demo` wires it.
fn demo(cx: &mut TestAppContext, text: &str) -> Demo {
    let window = window(cx, text);
    let (ai, host) = cx.update(|cx| {
        let view = window.root(cx).unwrap();
        let ai = view.update(cx, |view, _| ScriptedAi::new(view.model_mut()));
        let ai = Rc::new(RefCell::new(ai));
        let host = host::install(&view, &ai, cx);
        (ai, host)
    });
    cx.run_until_parked();
    Demo { window, ai, host }
}

/// Runs editor command `id` in `window`, as its key would.
fn command(cx: &mut TestAppContext, window: WindowHandle<EditorView>, id: &str) {
    let frame = ContextFrame::new(fe_gpui::KEY_CONTEXT);
    let invocation = Invocation::of(id).unwrap();
    let command = RunCommand(invocation.context(ContextStack::new().with(frame)));
    AnyWindowHandle::from(window)
        .update(cx, |_, window, cx| {
            window.dispatch_action(Box::new(command), cx);
        })
        .unwrap();
    cx.run_until_parked();
}

/// How many times `window`'s view is notified while `f` runs and what it
/// set off settles.
fn notifies(
    cx: &mut TestAppContext,
    window: WindowHandle<EditorView>,
    f: impl FnOnce(&mut TestAppContext),
) -> usize {
    let count = Rc::new(Cell::new(0));
    let counted = count.clone();
    let observing = cx.update(|cx| {
        let view = window.root(cx).unwrap();
        cx.observe(&view, move |_, _| counted.set(counted.get() + 1))
    });
    f(cx);
    cx.run_until_parked();
    drop(observing);
    count.get()
}

impl Demo {
    /// Runs each of `script`'s steps on the driver, as `ai_demo` runs a
    /// step that isn't the user's, each of which must succeed.
    fn play(&self, cx: &mut TestAppContext, script: &str) {
        for step in scripted_ai::parse(script).unwrap() {
            self.update(cx, |view, ai, cx| {
                if let Err(error) = ai.step(view.model_mut(), &step) {
                    panic!("{step:?}: {error}");
                }
                match step {
                    Step::Suggest(_) => view.set_ghost_text(ai.ghost_text().cloned(), cx),
                    Step::Propose { .. } => view.set_proposals(ai.proposals().to_vec(), cx),
                    _ => {}
                }
                cx.notify();
            });
        }
        cx.run_until_parked();
    }

    fn update<R>(
        &self,
        cx: &mut TestAppContext,
        f: impl FnOnce(&mut EditorView, &mut ScriptedAi, &mut Context<EditorView>) -> R,
    ) -> R {
        let ai = self.ai.clone();
        (self.window)
            .update(cx, |view, _, cx| f(view, &mut ai.borrow_mut(), cx))
            .unwrap()
    }

    /// A suggestion step, run in the window as a key would.
    fn ghost(&self, cx: &mut TestAppContext, step: &Step) -> Result<Report, StepError> {
        let result = host::run_ghost_step(self.window, &self.host, step, &mut cx.to_async());
        cx.run_until_parked();
        result.unwrap()
    }

    /// Accepts the latest proposal's first hunk through the view, as its
    /// button does, after a notify, as of a click focusing the view: the
    /// view's observers then run before the accept's event goes out.
    fn accept_hunk(&self, cx: &mut TestAppContext) {
        self.update(cx, |view, ai, cx| {
            let proposal = ai.proposals().last().unwrap().clone();
            let id = proposal.borrow().hunks(view.model().snapshot())[0].id;
            cx.notify();
            view.accept_hunk(id, cx).unwrap();
        });
        cx.run_until_parked();
    }

    /// Undoes (`redo`: redoes) the AI's last edit as the host would, on the
    /// view's model, not through the driver.
    fn undo_ai(&self, cx: &mut TestAppContext, redo: bool) {
        self.update(cx, |view, _, cx| {
            let mut buffer = view.model_mut().buffer_mut();
            let undone = if redo {
                buffer.redo(Origin::is_ai)
            } else {
                buffer.undo(Origin::is_ai)
            };
            assert!(undone.unwrap().is_some());
            drop(buffer);
            cx.notify();
        });
        cx.run_until_parked();
    }

    fn text(&self, cx: &mut TestAppContext) -> String {
        self.update(cx, |view, _, _| view.model().text().to_string())
    }

    /// The first and last row of each touched-row mark.
    fn marks(&self, cx: &mut TestAppContext) -> Vec<(u32, u32)> {
        self.update(cx, |view, _, _| {
            let model = view.model();
            (model.decorations(0..model.text().len()).iter())
                .map(|mark| mark.covered_rows(model.text()).into_inner())
                .collect()
        })
    }

    fn shows_suggestion(&self, cx: &mut TestAppContext) -> bool {
        self.update(cx, |view, _, _| view.ghost_text().is_some())
    }
}

#[gpui::test]
fn full_accept_then_undo_is_not_reoffered(cx: &mut TestAppContext) {
    // Each takes all that's offered.
    let full = [
        ("foo bar", Step::Accept),
        ("foo", Step::AcceptWord),
        ("foo bar", Step::AcceptLine),
    ];
    for (offered, step) in full {
        let demo = demo(cx, "x\n");
        demo.play(cx, &format!("caret 1:2\nsuggest {offered}"));
        assert!(matches!(demo.ghost(cx, &step), Ok(Report::Accepted(_))));
        assert_eq!(demo.text(cx), format!("x{offered}\n"));
        assert!(demo.ai.borrow().ghost_text().is_none(), "{step:?}");
        // The undo puts the caret back where it was offered, where it
        // would be live again.
        let shown_again = demo.update(cx, |view, ai, cx| {
            ai.step(view.model_mut(), &Step::Undo).unwrap();
            host::show_again(view, ai, cx)
        });
        assert!(!shown_again, "{step:?}");
        assert_eq!(demo.text(cx), "x\n");
        assert!(!demo.shows_suggestion(cx));
    }

    // A word of it taken, the rest is still offered, by both.
    let demo = demo(cx, "x\n");
    demo.play(cx, "caret 1:2\nsuggest foo bar");
    assert!(matches!(
        demo.ghost(cx, &Step::AcceptWord),
        Ok(Report::Accepted(_))
    ));
    assert_eq!(demo.text(cx), "xfoo\n");
    assert!(demo.ai.borrow().ghost_text().is_some());
    assert!(demo.shows_suggestion(cx));
    // Then the rest taken, it's done with.
    assert!(matches!(
        demo.ghost(cx, &Step::Accept),
        Ok(Report::Accepted(_))
    ));
    assert_eq!(demo.text(cx), "xfoo bar\n");
    assert!(demo.ai.borrow().ghost_text().is_none());
}

#[gpui::test]
fn hand_undo_redo_updates_touched_rows(cx: &mut TestAppContext) {
    let demo = demo(cx, "one\ntwo\nthree\nfour\n");
    // Row 4 streamed, row 2's hunk accepted through the view.
    demo.play(
        cx,
        "caret 4:1\nstream\npush 4\\n\nfinish\npropose 2-2 TWO\\n",
    );
    demo.accept_hunk(cx);
    assert_eq!(demo.text(cx), "one\nTWO\nthree\n4\nfour\n");
    assert_eq!(demo.marks(cx), [(1, 1), (3, 3)]);

    // The accept undone by key: its mark goes, with no step run.
    command(cx, demo.window, "editor.undo");
    assert_eq!(demo.text(cx), "one\ntwo\nthree\n4\nfour\n");
    assert_eq!(demo.marks(cx), [(3, 3)]);
    // The stream undone by the host on the view's model: its mark goes.
    demo.undo_ai(cx, false);
    assert_eq!(demo.text(cx), "one\ntwo\nthree\nfour\n");
    assert_eq!(demo.marks(cx), []);

    // Redone, each is marked where it lands again, its anchors having
    // collapsed.
    demo.undo_ai(cx, true);
    assert_eq!(demo.marks(cx), [(3, 3)]);
    command(cx, demo.window, "editor.redo");
    assert_eq!(demo.text(cx), "one\nTWO\nthree\n4\nfour\n");
    assert_eq!(demo.marks(cx), [(1, 1), (3, 3)]);

    // The host's servicing notifies the view once more, to repaint the
    // marks, and that notify, the buffer unchanged, services nothing.
    let bare = window(cx, "one\n");
    bare.update(cx, |view, _, _| view.model_mut().insert("1"))
        .unwrap();
    let unwired = notifies(cx, bare, |cx| command(cx, bare, "editor.undo"));
    let text = bare.update(cx, |view, _, _| view.model().text().to_string());
    assert_eq!(text.unwrap(), "one\n");
    let wired = notifies(cx, demo.window, |cx| {
        command(cx, demo.window, "editor.undo")
    });
    assert_eq!(wired, unwired + 1);
    assert_eq!(demo.marks(cx), [(3, 3)]);
    let plain = notifies(cx, demo.window, |cx| {
        demo.update(cx, |_, _, cx| cx.notify());
    });
    assert_eq!(plain, 1);
}

#[gpui::test]
fn hand_accepted_hunk_still_marked(cx: &mut TestAppContext) {
    let demo = demo(cx, "one\ntwo\nthree\n");
    demo.play(cx, "propose 3-3 THREE\\n");
    demo.accept_hunk(cx);
    assert_eq!(demo.text(cx), "one\ntwo\nTHREE\n");
    assert_eq!(demo.marks(cx), [(2, 2)]);
}

#[gpui::test]
fn ghost_steps_report_outcome(cx: &mut TestAppContext) {
    let demo = demo(cx, "x\n");
    let accepts = Rc::new(RefCell::new(Vec::new()));
    let seen = accepts.clone();
    let _subscription = cx.update(|cx| {
        let view = demo.window.root(cx).unwrap();
        cx.subscribe(&view, move |_, event: &GhostTextEvent, _| {
            if let GhostTextEvent::Accepted { transaction, .. } = event {
                seen.borrow_mut().push(*transaction);
            }
        })
    });
    let ghost_steps = [
        Step::Accept,
        Step::AcceptWord,
        Step::AcceptLine,
        Step::Dismiss,
    ];
    // Nothing offered: nothing to take or turn down.
    for step in &ghost_steps {
        assert_eq!(demo.ghost(cx, step), Err(StepError::NoSuggestion));
    }

    demo.play(cx, "caret 1:2");
    for step in &ghost_steps[..3] {
        demo.play(cx, "suggest foo");
        let report = demo.ghost(cx, step);
        let transaction = *accepts.borrow().last().unwrap();
        assert_eq!(report, Ok(Report::Accepted(transaction)), "{step:?}");
    }
    assert_eq!(accepts.borrow().len(), 3);
    assert_eq!(demo.text(cx), "xfoofoofoo\n");

    demo.play(cx, "suggest bar");
    assert_eq!(demo.ghost(cx, &Step::Dismiss), Ok(Report::Dismissed));
    assert!(!demo.shows_suggestion(cx));
    assert_eq!(demo.ghost(cx, &Step::Dismiss), Err(StepError::NoSuggestion));

    // Gone stale with no frame drawn since, the caret moved off it, the
    // view still holds it: an accept only drops it.
    demo.play(cx, "suggest baz");
    demo.update(cx, |view, _, _| view.model_mut().set_cursor(0));
    assert!(demo.shows_suggestion(cx));
    assert_eq!(demo.ghost(cx, &Step::Accept), Err(StepError::NoSuggestion));
    assert!(!demo.shows_suggestion(cx));
    assert_eq!(accepts.borrow().len(), 3);
    assert_eq!(demo.text(cx), "xfoofoofoo\n");
}
