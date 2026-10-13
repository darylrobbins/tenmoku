//! The proposal commands ([`super::proposal_commands`]): their IDs and
//! default keys, the keys needing a proposal shown, next and previous hunk,
//! accept all and reject all, and the host running them.

use std::{cell::RefCell, path::Path, rc::Rc};

use fe_buffer::{Buffer, HunkId, HunkStatus, Origin, ProposedChange, TransactionId};
use fe_command::{Invocation, Value};
use fe_editor::EditorModel;
use fe_keymap::{
    DEFAULT_KEYMAP_PATH, DEFAULT_LINUX, DEFAULT_MACOS, KeySequence, Keymap, KeymapLayer,
};
use gpui::{Entity, Focusable as _, TestAppContext, VisualTestContext};

use crate::{
    commands::{RunCommand, command_catalog, observe_invocations},
    view::ProposalButton,
    *,
};

/// Rows `r0` to `r9`.
const TEXT: &str = "r0\nr1\nr2\nr3\nr4\nr5\nr6\nr7\nr8\nr9\n";
/// [`TEXT`] with rows 1, 4 and 7 replaced.
const PROPOSED: &str = "r0\nx1\nr2\nr3\nx4\nr5\nr6\nx7\nr8\nr9\n";

const IDS: [&str; 6] = [
    "proposal.next_hunk",
    "proposal.previous_hunk",
    "proposal.accept_hunk",
    "proposal.reject_hunk",
    "proposal.accept_all",
    "proposal.reject_all",
];

fn ai() -> Origin {
    Origin::Ai {
        agent: "agent".into(),
        session: "session".into(),
    }
}

/// A platform's resolved default keymap, which has no `platform` lines.
fn keymap(source: &str) -> Keymap {
    let (layer, diagnostics) = KeymapLayer::parse(Path::new(DEFAULT_KEYMAP_PATH), source, None);
    assert_eq!(diagnostics, []);
    Keymap::from_layers(vec![layer.unwrap()])
}

/// A focused editor on [`TEXT`], vim off, the caret at `caret`, with
/// macOS's default keymap.
fn editor(caret: usize, cx: &mut TestAppContext) -> (Entity<EditorView>, &mut VisualTestContext) {
    cx.update(|cx| init_with_keymap(cx, keymap(DEFAULT_MACOS)));
    let (view, cx) = cx.add_window_view(move |_, cx| {
        let mut model = EditorModel::new(Buffer::new(TEXT), None);
        model.set_cursor(caret);
        EditorView::new(model, EditorTheme::default_dark(cx), cx)
    });
    cx.update(|window, cx| {
        window.activate_window();
        window.focus(&view.focus_handle(cx), cx);
    });
    cx.update(|_, cx| set_vim_enabled(cx, false));
    cx.run_until_parked();
    (view, cx)
}

/// The offset of the start of row `row` of [`TEXT`].
fn row(row: usize) -> usize {
    row * 3
}

/// [`PROPOSED`] shown in the view, and its hunks' ids.
fn shown(
    view: &Entity<EditorView>,
    cx: &mut VisualTestContext,
) -> (Rc<RefCell<ProposedChange>>, Vec<HunkId>) {
    let proposal = view.read_with(cx, |view, _| {
        let snapshot = view.model().snapshot();
        let whole = 0..snapshot.text().len();
        let proposal = ProposedChange::from_text(snapshot, whole, PROPOSED, ai());
        Rc::new(RefCell::new(proposal))
    });
    view.update(cx, |view, cx| {
        view.set_proposals(vec![proposal.clone()], cx)
    });
    let ids = view.read_with(cx, |view, _| {
        let hunks = proposal.borrow().hunks(view.model().snapshot());
        hunks.iter().map(|hunk| hunk.id).collect::<Vec<_>>()
    });
    assert_eq!(ids.len(), 3);
    draw(cx);
    (proposal, ids)
}

fn draw(cx: &mut VisualTestContext) {
    cx.update(|window, cx| window.draw(cx).clear(cx));
}

fn status(
    proposal: &Rc<RefCell<ProposedChange>>,
    id: HunkId,
    view: &Entity<EditorView>,
    cx: &mut VisualTestContext,
) -> HunkStatus {
    view.read_with(cx, |view, _| {
        let hunks = proposal.borrow().hunks(view.model().snapshot());
        hunks.iter().find(|hunk| hunk.id == id).unwrap().status
    })
}

fn text(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> String {
    view.read_with(cx, |view, _| view.model().text().to_string())
}

fn carets(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> Vec<usize> {
    view.read_with(cx, |view, _| {
        (view.model().selections().iter())
            .map(|selection| selection.range.end)
            .collect()
    })
}

/// The events `view` emits from now on.
fn events(
    view: &Entity<EditorView>,
    cx: &mut VisualTestContext,
) -> Rc<RefCell<Vec<ProposalEvent>>> {
    let events = Rc::new(RefCell::new(Vec::new()));
    let seen = events.clone();
    cx.update(|_, cx| {
        let push = move |_, event: &ProposalEvent, _: &mut gpui::App| {
            seen.borrow_mut().push(event.clone())
        };
        cx.subscribe(view, push).detach();
    });
    events
}

fn keys(keys: &str, cx: &mut VisualTestContext) {
    draw(cx);
    cx.simulate_keystrokes(keys);
    cx.run_until_parked();
}

fn run(command: &str, cx: &mut VisualTestContext) {
    let invocation = Invocation::of(command).unwrap();
    cx.update(|window, cx| window.dispatch_action(Box::new(RunCommand(invocation)), cx));
    cx.run_until_parked();
}

/// Replaces the text in `range` with `new`, as the user typing.
fn edit(
    view: &Entity<EditorView>,
    range: std::ops::Range<usize>,
    new: &str,
    cx: &mut VisualTestContext,
) {
    view.update(cx, |view, cx| {
        let mut buffer = view.model_mut().buffer_mut();
        buffer.edit([(range, new.to_string())], Origin::User);
        drop(buffer);
        cx.notify();
    });
    cx.run_until_parked();
}

#[gpui::test]
fn commands_and_bindings(cx: &mut TestAppContext) {
    let catalog = command_catalog();
    for id in IDS {
        assert_eq!(
            catalog.check(&id.parse().unwrap(), &Value::Null),
            Ok(()),
            "{id}"
        );
    }
    // The command `keys` runs in `context`.
    let bound = |keymap: &Keymap, keys: &str, context: &str| {
        let keys = KeySequence::parse(keys).unwrap();
        (keymap.bindings().iter())
            .find(|binding| {
                let found = binding.context.as_ref().map(ToString::to_string);
                binding.keys == keys && found.as_deref() == Some(context)
            })
            .map(|binding| binding.command.to_string())
    };
    let proposals =
        |names: [(&str, &str); 6]| names.map(|(keys, id)| (keys.to_string(), id.to_string()));
    let mac = proposals([
        ("alt-f5", IDS[0]),
        ("alt-shift-f5", IDS[1]),
        ("cmd-y", IDS[2]),
        ("cmd-n", IDS[3]),
        ("cmd-enter", IDS[4]),
        ("cmd-backspace", IDS[5]),
    ]);
    let linux = proposals([
        ("alt-f5", IDS[0]),
        ("alt-shift-f5", IDS[1]),
        ("ctrl-alt-y", IDS[2]),
        ("ctrl-alt-n", IDS[3]),
        ("ctrl-alt-shift-y", IDS[4]),
        ("ctrl-alt-shift-n", IDS[5]),
    ]);
    for (os, source, expected) in [
        ("macOS", DEFAULT_MACOS, mac),
        ("Linux", DEFAULT_LINUX, linux),
    ] {
        let map = keymap(source);
        for (keys, binding) in expected {
            let found = bound(&map, &keys, "editor && proposal");
            assert_eq!(found, Some(binding), "{keys} on {os}");
        }
        cx.update(|cx| init_with_keymap(cx, map));
        assert_eq!(cx.update(|cx| keymap_report(cx)), [], "{os}");
    }
    let linux = keymap(DEFAULT_LINUX);
    let editor = |keys: &str| bound(&linux, keys, "editor");
    assert_eq!(editor("ctrl-y").as_deref(), Some("editor.redo"));
    assert_eq!(
        editor("ctrl-backspace").as_deref(),
        Some("editor.delete_word_left")
    );
}

#[gpui::test]
fn keys_need_a_proposal(cx: &mut TestAppContext) {
    let (view, cx) = editor(row(4) + 1, cx);
    let seen = Rc::new(RefCell::new(Vec::new()));
    let sink = seen.clone();
    cx.update(|_, cx| {
        observe_invocations(cx, move |invocation| {
            sink.borrow_mut().push(invocation.clone())
        })
    });
    let events = events(&view, cx);

    // Nothing shown.
    for key in ["cmd-y", "cmd-backspace"] {
        keys(key, cx);
    }
    assert_eq!(text(&view, cx), TEXT);
    assert_eq!(*events.borrow(), []);
    assert!(
        seen.borrow()
            .iter()
            .all(|i| !i.command.as_str().starts_with("proposal.")),
        "{:?}",
        seen.borrow()
    );

    let (proposal, ids) = shown(&view, cx);
    keys("cmd-y", cx);
    assert_eq!(
        text(&view, cx),
        PROPOSED.replace("x1", "r1").replace("x7", "r7")
    );
    assert_eq!(status(&proposal, ids[1], &view, cx), HunkStatus::Accepted);
    let events = events.borrow().clone();
    assert!(
        matches!(events[..], [ProposalEvent::Accepted { hunk, .. }] if hunk == ids[1]),
        "{events:?}"
    );
    keys("cmd-z", cx);
    assert_eq!(text(&view, cx), TEXT);

    // Every hunk resolved: the keys do nothing.
    for id in [ids[0], ids[2]] {
        view.update(cx, |view, cx| view.reject_hunk(id, cx).unwrap());
    }
    let before = events.len();
    for key in ["cmd-y", "cmd-backspace"] {
        keys(key, cx);
    }
    assert_eq!(text(&view, cx), TEXT);
    assert_eq!(before, 1);
}

#[gpui::test]
fn next_and_previous_wrap(cx: &mut TestAppContext) {
    let (view, cx) = editor(0, cx);
    let (proposal, ids) = shown(&view, cx);
    // The first goes stale; the last is rejected.
    edit(&view, row(1)..row(1) + 2, "q1", cx);
    view.update(cx, |view, cx| view.reject_hunk(ids[2], cx).unwrap());
    assert_eq!(status(&proposal, ids[0], &view, cx), HunkStatus::Stale);

    let visit = |command: &str, cx: &mut VisualTestContext| {
        run(command, cx);
        carets(&view, cx)
    };
    for (command, expected) in [
        ("proposal.next_hunk", row(1)),
        ("proposal.next_hunk", row(4)),
        ("proposal.next_hunk", row(1)),
        ("proposal.previous_hunk", row(4)),
        ("proposal.previous_hunk", row(1)),
        ("proposal.previous_hunk", row(4)),
    ] {
        assert_eq!(visit(command, cx), [expected], "{command}");
    }

    // None shown: nothing moves.
    view.update(cx, |view, cx| view.reject_hunk(ids[0], cx).unwrap());
    view.update(cx, |view, cx| view.reject_hunk(ids[1], cx).unwrap());
    let seen = Rc::new(RefCell::new(0));
    let sink = seen.clone();
    cx.update(|_, cx| observe_invocations(cx, move |_| *sink.borrow_mut() += 1));
    view.update(cx, |view, cx| {
        assert!(!view.next_hunk(cx));
        assert!(!view.previous_hunk(cx));
    });
    assert_eq!(carets(&view, cx), [row(4)]);
}

#[gpui::test]
fn accept_all_and_reject_all(cx: &mut TestAppContext) {
    let (view, cx) = editor(0, cx);
    let (proposal, ids) = shown(&view, cx);
    edit(&view, row(4)..row(4) + 2, "q4", cx);
    assert_eq!(status(&proposal, ids[1], &view, cx), HunkStatus::Stale);
    let events = events(&view, cx);

    run("proposal.accept_all", cx);
    let applied = "r0\nx1\nr2\nr3\nq4\nr5\nr6\nx7\nr8\nr9\n";
    assert_eq!(text(&view, cx), applied);
    let events_now = events.borrow().clone();
    let transaction = |event: &ProposalEvent| match event {
        ProposalEvent::Accepted { transaction, .. } => *transaction,
        other => panic!("{other:?}"),
    };
    let hunks: Vec<HunkId> = events_now
        .iter()
        .map(|event| match event {
            ProposalEvent::Accepted { hunk, .. } => *hunk,
            other => panic!("{other:?}"),
        })
        .collect();
    assert_eq!(hunks, [ids[0], ids[2]]);
    let one: Vec<TransactionId> = events_now.iter().map(transaction).collect();
    assert_eq!(one[0], one[1], "one transaction");

    run("editor.undo", cx);
    assert_eq!(text(&view, cx), TEXT.replace("r4", "q4"));

    events.borrow_mut().clear();
    run("proposal.reject_all", cx);
    assert_eq!(text(&view, cx), TEXT.replace("r4", "q4"));
    assert_eq!(*events.borrow(), [ProposalEvent::Rejected { hunk: ids[1] }]);
    assert_eq!(status(&proposal, ids[1], &view, cx), HunkStatus::Rejected);
}

#[gpui::test]
fn host_and_failures(cx: &mut TestAppContext) {
    let (view, cx) = editor(row(4), cx);
    let (proposal, ids) = shown(&view, cx);
    let seen = Rc::new(RefCell::new(Vec::new()));
    let sink = seen.clone();
    cx.update(|_, cx| {
        observe_invocations(cx, move |invocation| {
            sink.borrow_mut().push(invocation.command.to_string())
        })
    });

    run("proposal.reject_hunk", cx);
    assert_eq!(status(&proposal, ids[1], &view, cx), HunkStatus::Rejected);
    assert_eq!(text(&view, cx), TEXT);
    run("proposal.next_hunk", cx);
    assert_eq!(
        *seen.borrow(),
        ["proposal.reject_hunk", "proposal.next_hunk"]
    );

    // A stale hunk can't be accepted.
    let at = carets(&view, cx)[0];
    edit(&view, at..at + 2, "q7", cx);
    run("proposal.accept_hunk", cx);
    assert_eq!(text(&view, cx), TEXT.replace("r7", "q7"));
    assert_eq!(status(&proposal, ids[2], &view, cx), HunkStatus::Stale);
    assert_eq!(seen.borrow().len(), 2, "a failed command isn't observed");
}

#[gpui::test]
fn accepting_inside_a_completion_word_refreshes_the_menu(cx: &mut TestAppContext) {
    use super::completion_tests as menu;
    let (view, provider, cx) = menu::editor(cx);
    menu::keys("ctrl-space", cx);
    cx.simulate_input("l");
    cx.run_until_parked();
    let (before, _) = menu::rows(&view, cx);
    assert_eq!(before, ["value", "values"]);

    // `x` goes into the word typed since the request.
    let at = menu::TEXT.find("va\n").unwrap() + 1;
    let proposal = view.read_with(cx, |view, _| {
        let snapshot = view.model().snapshot();
        let edits = [(at..at, "x".to_string())];
        let proposal = ProposedChange::from_edits(snapshot, edits, ai());
        Rc::new(RefCell::new(proposal))
    });
    view.update(cx, |view, cx| {
        view.set_proposals(vec![proposal.clone()], cx)
    });
    let id = view.read_with(cx, |view, _| {
        proposal.borrow().hunks(view.model().snapshot())[0].id
    });
    view.update(cx, |view, cx| view.accept_hunk(id, cx).unwrap());
    draw(cx);
    cx.run_until_parked();

    assert_eq!(text(&view, cx), menu::TEXT.replace("va", "vxal"));
    let (after, _) = menu::rows(&view, cx);
    assert_ne!(after, before, "the rows were filtered for `val`");
    assert!(!provider.calls.borrow().is_empty());
}

#[gpui::test]
fn a_cached_accept_button_does_nothing_once_read_only(cx: &mut TestAppContext) {
    let (view, cx) = editor(row(4), cx);
    let (_, ids) = shown(&view, cx);
    let events = events(&view, cx);
    let (_, bounds) = view.read_with(cx, |view, _| {
        let found = view
            .proposal_buttons
            .iter()
            .find(|b| b.0 == ids[1] && b.1 == ProposalButton::Accept);
        let &(_, button, bounds) = found.unwrap();
        (button, bounds)
    });
    let version = view.read_with(cx, |view, _| view.model().buffer().current().version());
    // Read-only set after the last frame, as `set_read_only` then a click
    // before the next frame would.
    view.update(cx, |view, _| view.read_only = true);
    // The click lands on last frame's box.
    let at = bounds.center();
    view.update(cx, |view, cx| assert!(view.press_proposal_button(at, cx)));
    assert_eq!(text(&view, cx), TEXT);
    let now = view.read_with(cx, |view, _| view.model().buffer().current().version());
    assert_eq!(now, version);
    assert_eq!(*events.borrow(), []);
}
