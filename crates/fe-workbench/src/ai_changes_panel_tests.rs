//! The AI Changes panel: a changeset built directly over open editors, its
//! list, the per-file and header buttons, opening a file at its first
//! pending hunk, the commands and Revert Turn.

use std::{cell::RefCell, path::PathBuf, rc::Rc};

use fe_buffer::{Buffer, HunkStatus, Origin, ProposedChange};
use fe_command::{ContextFrame, ContextStack, Invocation};
use fe_editor::{Changeset, Counts};
use fe_gpui::{EditorView, commands::RunCommand};
use gpui::{Entity, Focusable as _, Modifiers, TestAppContext, VisualTestContext};
use kiln_shell::DockView as _;

use crate::ai_changes_panel::AiChangesPanel;
use crate::{DockPanel, Launch, Workbench, tests::open};

const TWO_FNS: &str = "fn one() {\n    1\n}\n\nfn two() {\n    2\n}\n";

fn proposed() -> String {
    TWO_FNS
        .replace("    1\n", "    10\n")
        .replace("    2\n", "    20\n")
}

fn ai() -> Origin {
    Origin::Ai {
        agent: "claude".into(),
        session: "s".into(),
    }
}

struct Setup<'a> {
    _dir: tempfile::TempDir,
    workbench: Entity<Workbench>,
    cx: &'a mut VisualTestContext,
    /// `a.rs` and `b.rs`, each of `TWO_FNS`, both open, `b.rs` active.
    paths: [PathBuf; 2],
}

fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
}

/// Two open files with a changeset proposing a change to each, shown.
fn setup(cx: &mut TestAppContext) -> Setup<'_> {
    setup_with(cx, |snapshot| {
        ProposedChange::from_text(snapshot, 0..snapshot.len(), &proposed(), ai())
    })
}

/// [`setup`] with each file's proposal made by `make`.
fn setup_with(
    cx: &mut TestAppContext,
    make: impl Fn(&fe_buffer::BufferSnapshot) -> ProposedChange,
) -> Setup<'_> {
    let dir = tempfile::tempdir().unwrap();
    let paths = ["a.rs", "b.rs"].map(|name| dir.path().join(name));
    for path in &paths {
        std::fs::write(path, TWO_FNS).unwrap();
    }
    let launch = Launch {
        root: Some(dir.path().to_owned()),
        file: Some((paths[0].clone(), Some(Buffer::new(TWO_FNS).into()))),
    };
    let (workbench, cx, _) = open(cx, launch);
    cx.update(|window, cx| window.focus(&workbench.focus_handle(cx), cx));
    let second = paths[1].clone();
    workbench.update_in(cx, |workbench, window, cx| {
        let pane = workbench.pane().clone();
        pane.update(cx, |pane, cx| pane.open(second, window, cx));
    });
    cx.run_until_parked();
    let mut changeset = Changeset::new("turn", ai());
    for path in &paths {
        let editor = editor(&workbench, path, cx);
        let proposal = editor.read_with(cx, |editor, _| {
            let buffer = editor.model().buffer();
            let snapshot = buffer.snapshot();
            (buffer.id(), make(&snapshot))
        });
        changeset.add_file(path.clone(), proposal.0, Rc::new(RefCell::new(proposal.1)));
    }
    workbench.update_in(cx, |workbench, window, cx| {
        workbench.set_changeset(changeset, window, cx)
    });
    draw(cx);
    Setup {
        _dir: dir,
        workbench,
        cx,
        paths,
    }
}

fn editor(
    workbench: &Entity<Workbench>,
    path: &std::path::Path,
    cx: &mut VisualTestContext,
) -> Entity<EditorView> {
    workbench.read_with(cx, |workbench, cx| {
        (workbench.open_editors(cx).into_iter())
            .find(|editor| editor.read(cx).model().path() == Some(path))
            .expect("the file is open")
    })
}

impl Setup<'_> {
    fn panel(&mut self) -> Entity<AiChangesPanel> {
        self.workbench.read_with(self.cx, |workbench, cx| {
            workbench.dock().read(cx).ai_changes().cloned().unwrap()
        })
    }

    fn rows(&mut self) -> Vec<(String, usize, usize, Counts)> {
        let panel = self.panel();
        panel.read_with(self.cx, |panel, _| panel.shown_rows())
    }

    fn header(&mut self) -> String {
        let panel = self.panel();
        panel.read_with(self.cx, |panel, _| panel.header_text())
    }

    fn message(&mut self) -> Option<&'static str> {
        let panel = self.panel();
        panel.read_with(self.cx, |panel, _| panel.message_text())
    }

    fn editor(&mut self, ix: usize) -> Entity<EditorView> {
        let path = self.paths[ix].clone();
        editor(&self.workbench, &path, self.cx)
    }

    fn text(&mut self, ix: usize) -> String {
        let editor = self.editor(ix);
        editor.read_with(self.cx, |editor, _| {
            editor.model().buffer().text().to_string()
        })
    }

    /// Clicks the element with debug selector `selector`.
    fn click(&mut self, selector: &'static str) {
        draw(self.cx);
        let bounds = self
            .cx
            .debug_bounds(selector)
            .unwrap_or_else(|| panic!("{selector} was not drawn"));
        self.cx.simulate_click(bounds.center(), Modifiers::none());
        draw(self.cx);
    }

    /// Clicks the row `selector` names, on its second line, clear of its
    /// buttons.
    fn click_row(&mut self, selector: &'static str) {
        draw(self.cx);
        let bounds = self.cx.debug_bounds(selector).expect("the row was drawn");
        let at = bounds.origin + gpui::point(gpui::px(24.), bounds.size.height - gpui::px(8.));
        self.cx.simulate_click(at, Modifiers::none());
        draw(self.cx);
    }

    fn run(&mut self, id: &str, frames: &[&str]) {
        let stack: ContextStack = frames.iter().map(|name| ContextFrame::new(*name)).collect();
        let run = RunCommand(Invocation::of(id).unwrap().context(stack));
        self.cx
            .update(|window, cx| window.dispatch_action(Box::new(run), cx));
        draw(self.cx);
    }
}

fn counts(rows: &[(String, usize, usize, Counts)], ix: usize) -> (usize, usize, usize) {
    let counts = rows[ix].3;
    (counts.pending, counts.accepted, counts.rejected)
}

#[gpui::test]
fn lists_files_with_counts(cx: &mut TestAppContext) {
    let mut s = setup(cx);
    assert_eq!(s.header(), "2 files, 4 hunks pending");
    let rows = s.rows();
    assert_eq!(
        rows.iter().map(|row| row.0.as_str()).collect::<Vec<_>>(),
        ["a.rs", "b.rs"]
    );
    for ix in 0..2 {
        assert_eq!(counts(&rows, ix), (2, 0, 0));
        assert_eq!((rows[ix].1, rows[ix].2), (2, 2), "+/- lines");
    }
    let shown = s.workbench.read_with(s.cx, |workbench, cx| {
        let dock = workbench.dock().read(cx);
        (dock.is_visible(), dock.active())
    });
    assert_eq!(shown, (true, DockPanel::AiChanges));
}

#[gpui::test]
fn accept_file_updates_row_and_buffer(cx: &mut TestAppContext) {
    let mut s = setup(cx);
    s.click("ai-changes-accept-a.rs");
    let rows = s.rows();
    assert_eq!(counts(&rows, 0), (0, 2, 0));
    assert_eq!(counts(&rows, 1), (2, 0, 0));
    assert_eq!(s.header(), "2 files, 2 hunks pending");
    assert_eq!(s.text(0), proposed());
    assert_eq!(s.text(1), TWO_FNS);
}

#[gpui::test]
fn reject_file(cx: &mut TestAppContext) {
    let mut s = setup(cx);
    s.click("ai-changes-reject-b.rs");
    let rows = s.rows();
    assert_eq!(counts(&rows, 0), (2, 0, 0));
    assert_eq!(counts(&rows, 1), (0, 0, 2));
    assert_eq!(s.text(1), TWO_FNS);
    assert_eq!(s.text(0), TWO_FNS);
}

#[gpui::test]
fn accept_all_and_reject_all(cx: &mut TestAppContext) {
    let mut s = setup(cx);
    s.click("ai-changes-accept-all");
    assert_eq!(counts(&s.rows(), 0), (0, 2, 0));
    assert_eq!(counts(&s.rows(), 1), (0, 2, 0));
    assert_eq!((s.text(0), s.text(1)), (proposed(), proposed()));
    assert_eq!(s.header(), "2 files, 0 hunks pending");

    s.click("ai-changes-revert");
    s.click("ai-changes-reject-all");
    assert_eq!(counts(&s.rows(), 0), (0, 0, 2));
    assert_eq!(counts(&s.rows(), 1), (0, 0, 2));
    assert_eq!((s.text(0), s.text(1)), (TWO_FNS.into(), TWO_FNS.into()));
}

#[gpui::test]
fn revert_turn_restores_text(cx: &mut TestAppContext) {
    let mut s = setup(cx);
    // Nothing accepted: Revert Turn does nothing.
    s.click("ai-changes-revert");
    assert_eq!(counts(&s.rows(), 0), (2, 0, 0));
    s.click("ai-changes-accept-all");
    assert_eq!(s.text(0), proposed());
    s.click("ai-changes-revert");
    assert_eq!((s.text(0), s.text(1)), (TWO_FNS.into(), TWO_FNS.into()));
    assert_eq!(counts(&s.rows(), 0), (2, 0, 0));
}

#[gpui::test]
fn click_opens_file_at_first_pending_hunk(cx: &mut TestAppContext) {
    let mut s = setup(cx);
    let active = |s: &mut Setup| {
        s.workbench.read_with(s.cx, |workbench, cx| {
            let pane = workbench.pane().read(cx);
            pane.active_editor()
                .and_then(|editor| editor.read(cx).model().path().map(ToOwned::to_owned))
        })
    };
    assert_eq!(active(&mut s), Some(s.paths[1].clone()));
    s.click_row("ai-changes-row-a.rs");
    assert_eq!(active(&mut s), Some(s.paths[0].clone()));
    let editor = s.editor(0);
    let (shown, caret, first) = editor.read_with(s.cx, |editor, _| {
        let proposals = editor.proposals().to_vec();
        let hunks = proposals[0].borrow().hunks(editor.model().snapshot());
        let first = hunks
            .iter()
            .find(|hunk| hunk.status == HunkStatus::Pending)
            .unwrap()
            .range
            .start;
        (
            proposals.len(),
            editor.model().primary_selection().head(),
            first,
        )
    });
    assert_eq!((shown, caret), (1, first));
}

#[gpui::test]
fn hunk_accept_in_editor_updates_counts(cx: &mut TestAppContext) {
    let mut s = setup(cx);
    s.click_row("ai-changes-row-a.rs");
    s.click_row("ai-changes-row-b.rs");
    let editor = s.editor(0);
    let hunk = editor.read_with(s.cx, |editor, _| {
        editor.proposals()[0]
            .borrow()
            .hunks(editor.model().snapshot())[0]
            .id
    });
    editor.update(s.cx, |editor, cx| editor.accept_hunk(hunk, cx).unwrap());
    draw(s.cx);
    let rows = s.rows();
    assert_eq!(counts(&rows, 0), (1, 1, 0));
    assert_eq!(counts(&rows, 1), (2, 0, 0));
    assert_eq!(s.header(), "2 files, 3 hunks pending");
    let other = editor.read_with(s.cx, |editor, _| {
        editor.proposals()[0]
            .borrow()
            .hunks(editor.model().snapshot())[1]
            .id
    });
    editor.update(s.cx, |editor, cx| editor.reject_hunk(other, cx).unwrap());
    draw(s.cx);
    assert_eq!(counts(&s.rows(), 0), (0, 1, 1));
}

#[gpui::test]
fn commands_match_buttons(cx: &mut TestAppContext) {
    let mut s = setup(cx);
    s.click("ai-changes-accept-all");
    let by_button = (s.rows(), s.text(0));
    s.click("ai-changes-revert");
    assert_eq!(counts(&s.rows(), 0), (2, 0, 0));

    s.run("ai.changes_accept_all", &["workbench"]);
    assert_eq!((s.rows(), s.text(0)), by_button);
    s.run("ai.changes_revert", &["workbench"]);
    assert_eq!(counts(&s.rows(), 0), (2, 0, 0));
    assert_eq!(s.text(0), TWO_FNS);
    s.run("ai.changes_reject_all", &["workbench"]);
    assert_eq!(counts(&s.rows(), 1), (0, 0, 2));
    s.run("ai.show_changes", &["workbench"]);
    assert_eq!(
        s.workbench
            .read_with(s.cx, |wb, cx| wb.dock().read(cx).active()),
        DockPanel::AiChanges
    );
}

#[gpui::test]
fn next_and_previous_file_select_while_focused(cx: &mut TestAppContext) {
    let mut s = setup(cx);
    let panel = s.panel();
    let selected = |s: &mut Setup| panel.read_with(s.cx, |panel, _| panel.selected());
    // Outside the panel's context the commands do nothing.
    s.run("ai.changes_next_file", &["workbench", "editor"]);
    assert_eq!(selected(&mut s), None);
    s.run("ai.changes_next_file", &["workbench", "ai_changes"]);
    assert_eq!(selected(&mut s), Some(0));
    s.run("ai.changes_next_file", &["workbench", "ai_changes"]);
    assert_eq!(selected(&mut s), Some(1));
    s.run("ai.changes_previous_file", &["workbench", "ai_changes"]);
    assert_eq!(selected(&mut s), Some(0));
    let context = panel.read_with(s.cx, |panel, _| panel.test_key_context());
    assert!(context.contains("ai_changes"));
}

#[gpui::test]
fn revert_prompts_when_files_edited(cx: &mut TestAppContext) {
    let mut s = setup(cx);
    s.click("ai-changes-accept-all");
    // Edit b.rs, the active tab, after the turn accepted its changes.
    let editor = s.editor(1);
    s.cx.update(|window, cx| window.focus(&editor.focus_handle(cx), cx));
    draw(s.cx);
    s.cx.simulate_input("x");
    draw(s.cx);
    let edited = s.text(1);
    assert_ne!(edited, proposed());

    s.click("ai-changes-revert");
    let (question, detail) = s.cx.pending_prompt().expect("asks first");
    assert_eq!(question, "Revert the turn's changes?");
    assert!(
        detail.contains("b.rs") && !detail.contains("a.rs"),
        "{detail}"
    );
    s.cx.simulate_prompt_answer("Cancel");
    draw(s.cx);
    // The untouched file went back; the edited one wasn't overwritten.
    assert_eq!(s.text(1), edited);
    assert_eq!(s.text(0), TWO_FNS);

    s.click("ai-changes-revert");
    assert!(s.cx.pending_prompt().is_some());
    s.cx.simulate_prompt_answer("Revert");
    draw(s.cx);
    assert_eq!(s.text(1), TWO_FNS);
}

#[gpui::test]
fn resolved_changeset_message(cx: &mut TestAppContext) {
    let mut s = setup(cx);
    assert_eq!(s.message(), None);
    s.click("ai-changes-reject-all");
    assert_eq!(s.message(), Some("No pending changes"));
    assert_eq!(s.header(), "2 files, 0 hunks pending");
}

#[gpui::test]
fn a_new_file_is_made_as_a_buffer_in_a_tab(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let a = dir.path().join("a.rs");
    std::fs::write(&a, TWO_FNS).unwrap();
    let launch = Launch {
        root: Some(dir.path().to_owned()),
        file: Some((a.clone(), Some(Buffer::new(TWO_FNS).into()))),
    };
    let (workbench, cx, _) = open(cx, launch);
    let created = dir.path().join("new.rs");
    let mut changeset = Changeset::new("turn", ai());
    changeset.add_new_file(created.clone(), "fn made() {}\n");
    workbench.update_in(cx, |workbench, window, cx| {
        workbench.set_changeset(changeset, window, cx)
    });
    draw(cx);
    let tabs = |cx: &mut VisualTestContext| {
        workbench.read_with(cx, |workbench, cx| workbench.open_editors(cx).len())
    };
    assert_eq!(tabs(cx), 1);
    workbench.update_in(cx, |workbench, window, cx| {
        workbench.decide_ai_file(&created, true, window, cx)
    });
    draw(cx);
    assert_eq!(tabs(cx), 2);
    let text = editor(&workbench, &created, cx)
        .read_with(cx, |editor, _| editor.model().buffer().text().to_string());
    assert_eq!(text, "fn made() {}\n");
}

/// Hunk `ix` of editor `editor`'s first proposal.
fn hunk_id(
    editor: &Entity<EditorView>,
    ix: usize,
    cx: &mut VisualTestContext,
) -> fe_buffer::HunkId {
    editor.read_with(cx, |editor, _| {
        editor.proposals()[0]
            .borrow()
            .hunks(editor.model().snapshot())[ix]
            .id
    })
}

#[gpui::test]
fn proposals_show_in_open_editors_without_a_click(cx: &mut TestAppContext) {
    let mut s = setup(cx);
    for ix in 0..2 {
        let editor = s.editor(ix);
        assert_eq!(editor.read_with(s.cx, |e, _| e.proposals().len()), 1);
    }
}

#[gpui::test]
fn editor_accepts_are_reverted_with_the_turn(cx: &mut TestAppContext) {
    let mut s = setup(cx);
    let editor = s.editor(0);
    let first = hunk_id(&editor, 0, s.cx);
    editor.update(s.cx, |editor, cx| editor.accept_hunk(first, cx).unwrap());
    draw(s.cx);
    assert_ne!(s.text(0), TWO_FNS);
    s.click("ai-changes-revert");
    assert_eq!(s.text(0), TWO_FNS);
    assert_eq!(counts(&s.rows(), 0), (2, 0, 0));

    // Mixed: one hunk in the editor, the rest from the panel.
    let first = hunk_id(&editor, 0, s.cx);
    editor.update(s.cx, |editor, cx| editor.accept_hunk(first, cx).unwrap());
    draw(s.cx);
    s.click("ai-changes-accept-a.rs");
    assert_eq!(s.text(0), proposed());
    s.click("ai-changes-revert");
    assert_eq!(s.text(0), TWO_FNS);
}

#[gpui::test]
fn a_reopened_file_is_followed_to_its_new_buffer(cx: &mut TestAppContext) {
    let mut s = setup(cx);
    // b.rs is the active tab: close it, then click its row.
    s.workbench.update_in(s.cx, |workbench, window, cx| {
        let pane = workbench.pane().clone();
        pane.update(cx, |pane, cx| pane.close_active(window, cx));
    });
    draw(s.cx);
    s.click_row("ai-changes-row-b.rs");
    s.cx.run_until_parked();
    draw(s.cx);
    let editor = s.editor(1);
    assert_eq!(editor.read_with(s.cx, |e, _| e.proposals().len()), 1);
    s.click("ai-changes-accept-b.rs");
    assert_eq!(s.text(1), proposed());
    s.click("ai-changes-revert");
    assert_eq!(s.text(1), TWO_FNS);
}

#[gpui::test]
fn hunks_made_stale_are_not_called_resolved(cx: &mut TestAppContext) {
    let mut s = setup(cx);
    let editor = s.editor(0);
    editor.update(s.cx, |editor, cx| {
        let len = editor.model().buffer().len();
        editor
            .model_mut()
            .buffer_mut()
            .edit([(0..len, "unrelated\n")], Origin::User);
        cx.notify();
    });
    draw(s.cx);
    let rows = s.rows();
    assert_eq!(rows[0].3.stale, 2, "{rows:?}");
    s.click("ai-changes-reject-b.rs");
    assert_eq!(s.message(), None);
}

#[gpui::test]
fn a_confirmation_belongs_to_its_changeset(cx: &mut TestAppContext) {
    let mut s = setup(cx);
    s.click("ai-changes-accept-all");
    let editor = s.editor(1);
    s.cx.update(|window, cx| window.focus(&editor.focus_handle(cx), cx));
    draw(s.cx);
    s.cx.simulate_input("x");
    draw(s.cx);
    let edited = s.text(1);
    s.click("ai-changes-revert");
    assert!(s.cx.pending_prompt().is_some());
    // Another turn replaces the changeset while the question is open.
    s.workbench.update_in(s.cx, |workbench, window, cx| {
        workbench.set_changeset(Changeset::new("other", ai()), window, cx)
    });
    s.cx.simulate_prompt_answer("Revert");
    draw(s.cx);
    assert_eq!(s.text(1), edited);
}

#[gpui::test]
fn a_new_file_is_unsaved_and_reverting_closes_its_tab(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let a = dir.path().join("a.rs");
    std::fs::write(&a, TWO_FNS).unwrap();
    let launch = Launch {
        root: Some(dir.path().to_owned()),
        file: Some((a.clone(), Some(Buffer::new(TWO_FNS).into()))),
    };
    let (workbench, cx, _) = open(cx, launch);
    let created = dir.path().join("new.rs");
    let mut changeset = Changeset::new("turn", ai());
    changeset.add_new_file(created.clone(), "fn made() {}\n");
    workbench.update_in(cx, |workbench, window, cx| {
        workbench.set_changeset(changeset, window, cx)
    });
    draw(cx);
    let tabs = |cx: &mut VisualTestContext| {
        workbench.read_with(cx, |workbench, cx| workbench.open_editors(cx).len())
    };
    for round in 0..2 {
        workbench.update_in(cx, |workbench, window, cx| {
            workbench.decide_ai_file(&created, true, window, cx)
        });
        draw(cx);
        assert_eq!(tabs(cx), 2, "round {round}");
        let dirty = editor(&workbench, &created, cx)
            .read_with(cx, |editor, _| editor.model().buffer().is_dirty());
        assert!(dirty, "a file not on disk is unsaved");
        workbench.update_in(cx, |workbench, window, cx| {
            workbench.run_ai_changes_op(crate::ai_changes_panel::AiChangesOp::Revert, window, cx)
        });
        draw(cx);
        assert_eq!(tabs(cx), 1, "round {round}");
    }
}

#[gpui::test]
fn an_edit_between_editor_accepts_survives_revert(cx: &mut TestAppContext) {
    let mut s = setup(cx);
    let editor = s.editor(1);
    let first = hunk_id(&editor, 0, s.cx);
    editor.update(s.cx, |editor, cx| editor.accept_hunk(first, cx).unwrap());
    draw(s.cx);
    s.cx.update(|window, cx| window.focus(&editor.focus_handle(cx), cx));
    draw(s.cx);
    s.cx.simulate_input("x");
    draw(s.cx);
    let second = hunk_id(&editor, 1, s.cx);
    editor.update(s.cx, |editor, cx| editor.accept_hunk(second, cx).unwrap());
    draw(s.cx);
    s.click("ai-changes-revert");
    // The turn's hunks are undone; the text typed between them is kept.
    assert!(s.cx.pending_prompt().is_none());
    assert_eq!(s.text(1), format!("x{TWO_FNS}"));
}

#[gpui::test]
fn an_earlier_turns_accept_is_not_this_turns(cx: &mut TestAppContext) {
    let mut s = setup(cx);
    let editor = s.editor(1);
    // A proposal of an earlier turn, still shown by the editor.
    let prior = editor.read_with(s.cx, |editor, _| {
        let snapshot = editor.model().buffer().snapshot();
        Rc::new(RefCell::new(ProposedChange::from_edits(
            &snapshot,
            [(0..0, "// earlier\n")],
            ai(),
        )))
    });
    editor.update(s.cx, |editor, cx| {
        let mut proposals = editor.proposals().to_vec();
        proposals.push(prior.clone());
        editor.set_proposals(proposals, cx);
    });
    draw(s.cx);
    let own = hunk_id(&editor, 0, s.cx);
    editor.update(s.cx, |editor, cx| editor.accept_hunk(own, cx).unwrap());
    let earlier = editor.read_with(s.cx, |editor, _| {
        prior.borrow().hunks(editor.model().snapshot())[0].id
    });
    editor.update(s.cx, |editor, cx| editor.accept_hunk(earlier, cx).unwrap());
    draw(s.cx);
    s.click("ai-changes-revert");
    // This turn's hunks are undone; the earlier turn's text stays.
    assert_eq!(s.text(1), format!("// earlier\n{TWO_FNS}"));
}

#[gpui::test]
fn editor_accept_all_with_an_earlier_turn_keeps_its_edits_on_revert(cx: &mut TestAppContext) {
    let mut s = setup(cx);
    let editor = s.editor(1);
    let prior = editor.read_with(s.cx, |editor, _| {
        let snapshot = editor.model().buffer().snapshot();
        Rc::new(RefCell::new(ProposedChange::from_edits(
            &snapshot,
            [(0..0, "// earlier\n")],
            ai(),
        )))
    });
    editor.update(s.cx, |editor, cx| {
        // The earlier turn's proposal comes first, as it was attached first.
        let mut proposals = vec![prior.clone()];
        proposals.extend(editor.proposals().iter().cloned());
        editor.set_proposals(proposals, cx);
    });
    draw(s.cx);
    editor.update(s.cx, |editor, cx| editor.accept_all(cx));
    draw(s.cx);
    assert_eq!(s.text(1), format!("// earlier\n{}", proposed()));
    s.click("ai-changes-revert");
    assert_eq!(s.text(1), format!("// earlier\n{TWO_FNS}"));
}

/// A workbench where an accepted changeset made the empty `empty.rs`.
fn with_empty_new_file(
    cx: &mut TestAppContext,
) -> (
    tempfile::TempDir,
    PathBuf,
    Entity<Workbench>,
    &mut VisualTestContext,
) {
    let dir = tempfile::tempdir().unwrap();
    let a = dir.path().join("a.rs");
    std::fs::write(&a, TWO_FNS).unwrap();
    let launch = Launch {
        root: Some(dir.path().to_owned()),
        file: Some((a.clone(), Some(Buffer::new(TWO_FNS).into()))),
    };
    let (workbench, cx, _) = open(cx, launch);
    let created = dir.path().join("empty.rs");
    let mut changeset = Changeset::new("turn", ai());
    changeset.add_new_file(created.clone(), "");
    workbench.update_in(cx, |workbench, window, cx| {
        workbench.set_changeset(changeset, window, cx)
    });
    workbench.update_in(cx, |workbench, window, cx| {
        workbench.decide_ai_file(&created, true, window, cx)
    });
    draw(cx);
    (dir, created, workbench, cx)
}

fn text_and_dirty(
    workbench: &Entity<Workbench>,
    path: &std::path::Path,
    cx: &mut VisualTestContext,
) -> (String, bool) {
    editor(workbench, path, cx).read_with(cx, |editor, _| {
        let buffer = editor.model().buffer();
        (buffer.text().to_string(), buffer.is_dirty())
    })
}

#[gpui::test]
fn an_empty_new_file_is_unsaved_too(cx: &mut TestAppContext) {
    let (_dir, created, workbench, cx) = with_empty_new_file(cx);
    let (text, dirty) = text_and_dirty(&workbench, &created, cx);
    assert_eq!((text.as_str(), dirty), ("", true));
}

#[gpui::test]
fn empty_new_file_undo_inserts_no_newline(cx: &mut TestAppContext) {
    let (_dir, created, workbench, cx) = with_empty_new_file(cx);
    let editor = editor(&workbench, &created, cx);
    editor.update(cx, |editor, _| {
        let _ = editor.model_mut().undo();
        let _ = editor.model_mut().undo();
    });
    let (text, dirty) = text_and_dirty(&workbench, &created, cx);
    assert_eq!((text.as_str(), dirty), ("", true));
}

#[gpui::test]
fn empty_new_file_close_is_protected(cx: &mut TestAppContext) {
    let (_dir, created, workbench, cx) = with_empty_new_file(cx);
    let editor = editor(&workbench, &created, cx);
    editor.update(cx, |editor, _| {
        let _ = editor.model_mut().undo();
    });
    cx.update(|window, cx| window.focus(&editor.focus_handle(cx), cx));
    let run = RunCommand(
        Invocation::of("workbench.close_tab")
            .unwrap()
            .context(ContextStack::new().with(ContextFrame::new("workbench"))),
    );
    cx.update(|window, cx| window.dispatch_action(Box::new(run), cx));
    cx.run_until_parked();
    let question = cx.pending_prompt().map(|(question, _)| question);
    assert_eq!(
        question.as_deref(),
        Some("Discard unsaved changes to empty.rs?")
    );
}

#[gpui::test]
fn empty_new_file_saves_accepted_text(cx: &mut TestAppContext) {
    let (_dir, created, workbench, cx) = with_empty_new_file(cx);
    let editor = editor(&workbench, &created, cx);
    cx.update(|window, cx| window.focus(&editor.focus_handle(cx), cx));
    cx.simulate_keystrokes("cmd-s");
    cx.run_until_parked();
    assert_eq!(std::fs::read(&created).unwrap(), b"");
    let (text, dirty) = text_and_dirty(&workbench, &created, cx);
    assert_eq!((text.as_str(), dirty), ("", false));
}

#[gpui::test]
fn accept_all_then_revert_turn(cx: &mut TestAppContext) {
    // Each file: a hunk that changes nothing, then a real one.
    let at = TWO_FNS.find("    1\n").unwrap();
    let mut s = setup_with(cx, |snapshot| {
        ProposedChange::from_edits_keeping_noops(
            snapshot,
            [(0..0, ""), (at..at + 6, "    10\n")],
            ai(),
        )
    });
    assert_eq!(counts(&s.rows(), 0), (2, 0, 0));
    let editor = s.editor(0);
    editor.update(s.cx, |editor, cx| assert_eq!(editor.accept_all(cx), 2));
    draw(s.cx);
    // The no-op counts as accepted in the panel.
    assert_eq!(counts(&s.rows(), 0), (0, 2, 0));
    assert_eq!(counts(&s.rows(), 1), (2, 0, 0));
    s.click("ai-changes-revert");
    assert_eq!(s.text(0), TWO_FNS);
    assert_eq!(counts(&s.rows(), 0), (2, 0, 0));
}

#[gpui::test]
fn revert_confirm_reasks_after_edit_while_pending(cx: &mut TestAppContext) {
    let mut s = setup(cx);
    s.click("ai-changes-accept-all");
    let editor = s.editor(1);
    s.cx.update(|window, cx| window.focus(&editor.focus_handle(cx), cx));
    draw(s.cx);
    s.cx.simulate_input("x");
    draw(s.cx);
    s.click("ai-changes-revert");
    assert!(s.cx.pending_prompt().is_some());
    // The file is edited again while the question is open.
    s.cx.update(|window, cx| window.focus(&editor.focus_handle(cx), cx));
    s.cx.simulate_input("y");
    draw(s.cx);
    let edited = s.text(1);
    s.cx.simulate_prompt_answer("Revert");
    draw(s.cx);
    assert_eq!(s.text(1), edited, "the answer was to the earlier text");
    let (_, detail) = s.cx.pending_prompt().expect("asks again");
    assert!(detail.contains("b.rs"), "{detail}");
    s.cx.simulate_prompt_answer("Revert");
    draw(s.cx);
    assert_eq!(s.text(1), TWO_FNS);
}

/// A workbench with `a.rs` open and a changeset of one new file, `new.rs`.
fn with_new_file(
    cx: &mut TestAppContext,
) -> (
    tempfile::TempDir,
    Entity<Workbench>,
    &mut VisualTestContext,
    PathBuf,
) {
    with_new_file_named(cx, "new.rs", "fn made() {}\n")
}

/// [`with_new_file`] for the file `name` proposed with `text`.
fn with_new_file_named<'a>(
    cx: &'a mut TestAppContext,
    name: &str,
    text: &str,
) -> (
    tempfile::TempDir,
    Entity<Workbench>,
    &'a mut VisualTestContext,
    PathBuf,
) {
    let dir = tempfile::tempdir().unwrap();
    let a = dir.path().join("a.rs");
    std::fs::write(&a, TWO_FNS).unwrap();
    let launch = Launch {
        root: Some(dir.path().to_owned()),
        file: Some((a, Some(Buffer::new(TWO_FNS).into()))),
    };
    let (workbench, cx, _) = open(cx, launch);
    cx.update(|window, cx| window.focus(&workbench.focus_handle(cx), cx));
    let created = dir.path().join(name);
    let mut changeset = Changeset::new("turn", ai());
    changeset.add_new_file(created.clone(), text);
    workbench.update_in(cx, |workbench, window, cx| {
        workbench.set_changeset(changeset, window, cx)
    });
    draw(cx);
    (dir, workbench, cx, created)
}

fn active_path(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Option<PathBuf> {
    workbench.read_with(cx, |workbench, cx| {
        let pane = workbench.pane().read(cx);
        (pane.active_editor()).and_then(|editor| editor.read(cx).model().path().map(Into::into))
    })
}

fn click_new_row(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) {
    click_row_named(workbench, "ai-changes-row-new.rs", cx)
}

fn click_row_named(
    workbench: &Entity<Workbench>,
    selector: &'static str,
    cx: &mut VisualTestContext,
) {
    draw(cx);
    let bounds = cx.debug_bounds(selector).expect("drawn");
    let at = bounds.origin + gpui::point(gpui::px(24.), bounds.size.height - gpui::px(8.));
    cx.simulate_click(at, Modifiers::none());
    draw(cx);
    let _ = workbench;
}

#[gpui::test]
fn new_file_row_opens_when_pending(cx: &mut TestAppContext) {
    let (_dir, workbench, cx, created) = with_new_file(cx);
    assert_ne!(active_path(&workbench, cx), Some(created.clone()));
    click_new_row(&workbench, cx);
    assert_eq!(active_path(&workbench, cx), Some(created.clone()));
    let text = editor(&workbench, &created, cx)
        .read_with(cx, |editor, _| editor.model().buffer().text().to_string());
    assert_eq!(text, "fn made() {}\n");
    let read_only = editor(&workbench, &created, cx).read_with(cx, |e, _| e.is_read_only());
    assert!(read_only, "the review copy takes no edits");
    // Looking decided nothing: the file is still pending and can be rejected.
    let pending = |workbench: &Entity<Workbench>, cx: &mut VisualTestContext| {
        let panel = workbench.read_with(cx, |workbench, cx| {
            workbench.dock().read(cx).ai_changes().cloned().unwrap()
        });
        panel.read_with(cx, |panel, _| panel.shown_rows()[0].3.pending)
    };
    assert_eq!(pending(&workbench, cx), 1);
    workbench.update_in(cx, |workbench, window, cx| {
        workbench.decide_ai_file(&created, false, window, cx)
    });
    draw(cx);
    let tabs = workbench.read_with(cx, |workbench, cx| workbench.open_editors(cx).len());
    assert_eq!(tabs, 1, "the review tab went with the decision");
}

#[gpui::test]
fn pending_new_file_row_enter_then_accept_keeps_one_tab(cx: &mut TestAppContext) {
    let (_dir, workbench, cx, created) = with_new_file(cx);
    click_new_row(&workbench, cx);
    workbench.update_in(cx, |workbench, window, cx| {
        workbench.decide_ai_file(&created, true, window, cx)
    });
    draw(cx);
    let tabs = workbench.read_with(cx, |workbench, cx| workbench.open_editors(cx).len());
    assert_eq!(tabs, 2);
    assert_eq!(active_path(&workbench, cx), Some(created));
}

#[gpui::test]
fn new_file_row_opens_when_accepted(cx: &mut TestAppContext) {
    let (_dir, workbench, cx, created) = with_new_file(cx);
    workbench.update_in(cx, |workbench, window, cx| {
        workbench.decide_ai_file(&created, true, window, cx)
    });
    draw(cx);
    // Show a.rs instead, then Enter on the new file's row.
    workbench.update_in(cx, |workbench, window, cx| {
        let pane = workbench.pane().clone();
        pane.update(cx, |pane, cx| pane.activate(0, window, cx));
    });
    draw(cx);
    assert_ne!(active_path(&workbench, cx), Some(created.clone()));
    let panel = workbench.read_with(cx, |workbench, cx| {
        workbench.dock().read(cx).ai_changes().cloned().unwrap()
    });
    cx.update(|window, cx| window.focus(&panel.focus_handle(cx), cx));
    panel.update(cx, |panel, cx| panel.move_selection(true, cx));
    cx.simulate_keystrokes("enter");
    draw(cx);
    assert_eq!(active_path(&workbench, cx), Some(created.clone()));
    let tabs = workbench.read_with(cx, |workbench, cx| workbench.open_editors(cx).len());
    assert_eq!(tabs, 2, "no second tab for it");
}

const SVG: &str =
    "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"4\" height=\"4\">\n<rect/>\n</svg>\n";

/// A workbench with `a.rs` open and a changeset proposing a change to
/// `pic.svg`, which the active pane shows as an image.
fn with_svg(
    cx: &mut TestAppContext,
) -> (
    tempfile::TempDir,
    Entity<Workbench>,
    &mut VisualTestContext,
    PathBuf,
) {
    let dir = tempfile::tempdir().unwrap();
    let a = dir.path().join("a.rs");
    let svg = dir.path().join("pic.svg");
    std::fs::write(&a, TWO_FNS).unwrap();
    std::fs::write(&svg, SVG).unwrap();
    let launch = Launch {
        root: Some(dir.path().to_owned()),
        file: Some((a, Some(Buffer::new(TWO_FNS).into()))),
    };
    let (workbench, cx, _) = open(cx, launch);
    cx.update(|window, cx| window.focus(&workbench.focus_handle(cx), cx));
    workbench.update_in(cx, |workbench, window, cx| {
        let pane = workbench.pane().clone();
        pane.update(cx, |pane, cx| pane.open_as_text(svg.clone(), window, cx));
    });
    cx.run_until_parked();
    let (id, proposal) = editor(&workbench, &svg, cx).read_with(cx, |editor, _| {
        let buffer = editor.model().buffer();
        let snapshot = buffer.snapshot();
        let new = SVG.replace("<rect/>", "<circle/>");
        (
            buffer.id(),
            ProposedChange::from_text(&snapshot, 0..snapshot.len(), &new, ai()),
        )
    });
    let mut changeset = Changeset::new("turn", ai());
    changeset.add_file(svg.clone(), id, Rc::new(RefCell::new(proposal)));
    workbench.update_in(cx, |workbench, window, cx| {
        workbench.set_changeset(changeset, window, cx);
        let pane = workbench.pane().clone();
        pane.update(cx, |pane, cx| pane.open_as_image(&svg, window, cx));
    });
    draw(cx);
    (dir, workbench, cx, svg)
}

fn shows_viewer(
    pane: &Entity<crate::Pane>,
    path: &std::path::Path,
    cx: &mut VisualTestContext,
) -> bool {
    pane.read_with(cx, |pane, _| {
        (pane.tabs().iter())
            .any(|tab| tab.path.as_deref() == Some(path) && tab.image_viewer().is_some())
    })
}

fn click_svg_row(cx: &mut VisualTestContext) {
    draw(cx);
    let bounds = cx.debug_bounds("ai-changes-row-pic.svg").expect("drawn");
    let at = bounds.origin + gpui::point(gpui::px(24.), bounds.size.height - gpui::px(8.));
    cx.simulate_click(at, Modifiers::none());
    cx.run_until_parked();
    draw(cx);
}

#[gpui::test]
fn svg_row_opens_text_proposal(cx: &mut TestAppContext) {
    let (_dir, workbench, cx, svg) = with_svg(cx);
    let pane = workbench.read_with(cx, |workbench, _| workbench.pane().clone());
    assert!(shows_viewer(&pane, &svg, cx));
    click_svg_row(cx);
    assert!(!shows_viewer(&pane, &svg, cx), "the viewer was replaced");
    assert_eq!(active_path(&workbench, cx), Some(svg.clone()));
    let shown = editor(&workbench, &svg, cx).read_with(cx, |editor, _| editor.proposals().len());
    assert_eq!(shown, 1);
}

#[gpui::test]
fn svg_row_reopens_in_another_pane(cx: &mut TestAppContext) {
    let (_dir, workbench, cx, svg) = with_svg(cx);
    click_svg_row(cx);
    // Split: the new pane shares the text; make its tab a viewer again.
    cx.simulate_keystrokes("cmd-\\");
    draw(cx);
    let panes: Vec<_> = workbench.read_with(cx, |workbench, cx| {
        workbench
            .pane_group(cx)
            .panes()
            .into_iter()
            .cloned()
            .collect()
    });
    let [left, right] = <[_; 2]>::try_from(panes).expect("two panes");
    workbench.update_in(cx, |workbench, window, cx| {
        assert_eq!(workbench.pane(), &right);
        right.update(cx, |pane, cx| pane.open_as_image(&svg, window, cx));
    });
    draw(cx);
    assert!(shows_viewer(&right, &svg, cx));
    workbench.update_in(cx, |workbench, window, cx| {
        if workbench.pane() != &right {
            workbench.focus_next_pane(true, window, cx);
        }
    });
    draw(cx);
    assert_eq!(workbench.read_with(cx, |w, _| w.pane().clone()), right);
    click_svg_row(cx);
    let right_editor = right.read_with(cx, |pane, _| pane.active_editor().cloned());
    let right_editor = right_editor.expect("text in the active pane");
    assert_eq!(right_editor.read_with(cx, |e, _| e.proposals().len()), 1);
    let shared = left.read_with(cx, |pane, cx| {
        let left = pane.active_editor().unwrap().read(cx).model();
        right_editor.read(cx).model().shares_buffer_with(left)
    });
    assert!(shared);
    // Close every tab of the file; the row opens it again, followed.
    for pane in [&left, &right] {
        workbench.update_in(cx, |_, window, cx| {
            pane.update(cx, |pane, cx| {
                while pane.position(&svg).is_some() {
                    pane.close_active(window, cx);
                }
            })
        });
    }
    draw(cx);
    click_svg_row(cx);
    let reopened = editor(&workbench, &svg, cx).read_with(cx, |editor, _| editor.proposals().len());
    assert_eq!(reopened, 1);
}

#[gpui::test]
fn svg_row_shows_a_background_viewer_tab(cx: &mut TestAppContext) {
    let (_dir, workbench, cx, svg) = with_svg(cx);
    // Another tab is in front of the SVG's viewer.
    workbench.update_in(cx, |workbench, window, cx| {
        let pane = workbench.pane().clone();
        pane.update(cx, |pane, cx| pane.activate(0, window, cx));
    });
    draw(cx);
    assert_ne!(active_path(&workbench, cx), Some(svg.clone()));
    click_svg_row(cx);
    assert_eq!(active_path(&workbench, cx), Some(svg));
}

#[gpui::test]
fn svg_row_read_is_dropped_when_the_user_moves_on(cx: &mut TestAppContext) {
    let (_dir, workbench, cx, svg) = with_svg(cx);
    workbench.update_in(cx, |workbench, window, cx| {
        let pane = workbench.pane().clone();
        pane.update(cx, |pane, cx| pane.activate(0, window, cx));
        workbench.open_ai_file(svg.clone(), window, cx);
        // Before the read lands, another tab is shown.
        let pane = workbench.pane().clone();
        pane.update(cx, |pane, cx| pane.activate(0, window, cx));
    });
    cx.run_until_parked();
    draw(cx);
    assert_ne!(
        active_path(&workbench, cx),
        Some(svg.clone()),
        "stays on a.rs"
    );
}

/// `with_svg` with the SVG's viewer tab closed: no tab shows the file.
fn with_svg_closed(
    cx: &mut TestAppContext,
) -> (
    tempfile::TempDir,
    Entity<Workbench>,
    &mut VisualTestContext,
    PathBuf,
) {
    let (dir, workbench, cx, svg) = with_svg(cx);
    workbench.update_in(cx, |workbench, window, cx| {
        let pane = workbench.pane().clone();
        pane.update(cx, |pane, cx| pane.close_active(window, cx));
    });
    draw(cx);
    let pane = workbench.read_with(cx, |workbench, _| workbench.pane().clone());
    assert!(pane.read_with(cx, |pane, _| pane.position(&svg).is_none()));
    (dir, workbench, cx, svg)
}

#[gpui::test]
fn svg_reopen_does_not_steal_focus_after_navigation(cx: &mut TestAppContext) {
    let (_dir, workbench, cx, svg) = with_svg_closed(cx);
    workbench.update_in(cx, |workbench, window, cx| {
        workbench.open_ai_file(svg.clone(), window, cx);
        // Before the read lands, the user shows another tab.
        let pane = workbench.pane().clone();
        pane.update(cx, |pane, cx| pane.activate(0, window, cx));
    });
    cx.run_until_parked();
    draw(cx);
    assert_ne!(active_path(&workbench, cx), Some(svg.clone()), "stays put");
    assert!(!workbench.read_with(cx, |workbench, _| workbench.ai_opening_pending()));
}

#[gpui::test]
fn failed_text_read_retires_observer(cx: &mut TestAppContext) {
    let (_dir, workbench, cx, svg) = with_svg_closed(cx);
    // A directory in its place: the read fails.
    std::fs::remove_file(&svg).unwrap();
    std::fs::create_dir(&svg).unwrap();
    workbench.update_in(cx, |workbench, window, cx| {
        workbench.open_ai_file(svg.clone(), window, cx);
    });
    assert!(workbench.read_with(cx, |workbench, _| workbench.ai_opening_pending()));
    cx.run_until_parked();
    draw(cx);
    assert!(
        !workbench.read_with(cx, |workbench, _| workbench.ai_opening_pending()),
        "the failed read retired the observer"
    );
    let pane = workbench.read_with(cx, |workbench, _| workbench.pane().clone());
    assert!(pane.read_with(cx, |pane, _| pane.position(&svg).is_none()));
    // Dropped for another read of the file: also retired, nothing shown.
    std::fs::remove_dir(&svg).unwrap();
    std::fs::write(&svg, SVG).unwrap();
    workbench.update_in(cx, |workbench, window, cx| {
        workbench.open_ai_file(svg.clone(), window, cx);
        let pane = workbench.pane().clone();
        pane.update(cx, |pane, cx| pane.open_as_text(svg.clone(), window, cx));
    });
    cx.run_until_parked();
    draw(cx);
    assert!(!workbench.read_with(cx, |workbench, _| workbench.ai_opening_pending()));
}

/// Types `x` in the active editor.
fn type_x(cx: &mut VisualTestContext) {
    cx.simulate_input("x");
    draw(cx);
}

/// The text of every editor of `path`, one per view.
fn texts(
    workbench: &Entity<Workbench>,
    path: &std::path::Path,
    cx: &mut VisualTestContext,
) -> Vec<String> {
    workbench.read_with(cx, |workbench, cx| {
        (workbench.open_editors(cx).into_iter())
            .filter(|editor| editor.read(cx).model().path() == Some(path))
            .map(|editor| editor.read(cx).model().buffer().text().to_string())
            .collect()
    })
}

#[gpui::test]
fn preview_read_only_in_split_view(cx: &mut TestAppContext) {
    let (_dir, workbench, cx, created) = with_new_file(cx);
    click_new_row(&workbench, cx);
    cx.simulate_keystrokes("cmd-\\");
    draw(cx);
    let views = texts(&workbench, &created, cx);
    assert_eq!(views.len(), 2, "a view in each pane");
    type_x(cx);
    assert_eq!(texts(&workbench, &created, cx), vec!["fn made() {}\n"; 2]);
}

#[gpui::test]
fn edited_preview_protected_on_accept_and_reject(cx: &mut TestAppContext) {
    for accept in [true, false] {
        let (_dir, workbench, cx, created) = with_new_file(cx);
        click_new_row(&workbench, cx);
        cx.simulate_keystrokes("cmd-\\");
        draw(cx);
        type_x(cx);
        workbench.update_in(cx, |workbench, window, cx| {
            workbench.decide_ai_file(&created, accept, window, cx)
        });
        draw(cx);
        let views = texts(&workbench, &created, cx);
        match accept {
            true => assert!(
                views.iter().all(|text| text == "fn made() {}\n"),
                "{views:?}"
            ),
            false => assert!(views.is_empty(), "{views:?}"),
        }
    }
}

#[gpui::test]
fn accepted_new_file_row_reopens_saved_contents(cx: &mut TestAppContext) {
    let (_dir, workbench, cx, created) = with_new_file(cx);
    workbench.update_in(cx, |workbench, window, cx| {
        workbench.decide_ai_file(&created, true, window, cx);
    });
    draw(cx);
    // Saved, so closing it closes the tab.
    let tab = editor(&workbench, &created, cx);
    let saving = tab.update_in(cx, |editor, window, cx| editor.save(window, cx));
    saving.detach();
    cx.run_until_parked();
    workbench.update_in(cx, |workbench, window, cx| {
        let pane = workbench.pane().clone();
        pane.update(cx, |pane, cx| pane.close_active(window, cx));
    });
    draw(cx);
    assert!(texts(&workbench, &created, cx).is_empty(), "closed");
    // Saved with edits meanwhile.
    std::fs::write(&created, "fn edited() {}\n").unwrap();
    click_new_row(&workbench, cx);
    cx.run_until_parked();
    draw(cx);
    assert_eq!(active_path(&workbench, cx), Some(created.clone()));
    assert_eq!(texts(&workbench, &created, cx), vec!["fn edited() {}\n"]);
    let rows = workbench.read_with(cx, |workbench, cx| {
        let panel = workbench.dock().read(cx).ai_changes().cloned().unwrap();
        panel.read(cx).shown_rows()
    });
    assert_eq!(rows.len(), 1, "still in the changeset");
    // The reopened buffer is the file's: Revert Turn asks about its
    // edits, then removes it.
    draw(cx);
    let bounds = cx.debug_bounds("ai-changes-revert").expect("drawn");
    cx.simulate_click(bounds.center(), Modifiers::none());
    draw(cx);
    let (question, detail) = cx.pending_prompt().expect("edited, so it asks");
    assert_eq!(question, "Revert the turn's changes?");
    assert!(detail.contains("new.rs"), "{detail}");
    cx.simulate_prompt_answer("Revert");
    draw(cx);
    assert!(texts(&workbench, &created, cx).is_empty(), "reverted");
}

#[gpui::test]
fn accepted_new_svg_row_reopens_as_text(cx: &mut TestAppContext) {
    let (_dir, workbench, cx, svg) = with_new_file_named(cx, "pic.svg", SVG);
    workbench.update_in(cx, |workbench, window, cx| {
        workbench.decide_ai_file(&svg, true, window, cx);
    });
    draw(cx);
    let tab = editor(&workbench, &svg, cx);
    let saving = tab.update_in(cx, |editor, window, cx| editor.save(window, cx));
    saving.detach();
    cx.run_until_parked();
    workbench.update_in(cx, |workbench, window, cx| {
        let pane = workbench.pane().clone();
        pane.update(cx, |pane, cx| pane.close_active(window, cx));
    });
    draw(cx);
    assert!(texts(&workbench, &svg, cx).is_empty(), "closed");
    click_row_named(&workbench, "ai-changes-row-pic.svg", cx);
    cx.run_until_parked();
    draw(cx);
    assert_eq!(active_path(&workbench, cx), Some(svg.clone()));
    assert_eq!(texts(&workbench, &svg, cx), vec![SVG.to_string()]);
}

#[gpui::test]
fn replacing_the_turn_closes_its_new_file_previews(cx: &mut TestAppContext) {
    let (_dir, workbench, cx, created) = with_new_file(cx);
    click_new_row(&workbench, cx);
    let text = |cx: &mut VisualTestContext| {
        editor(&workbench, &created, cx)
            .read_with(cx, |editor, _| editor.model().buffer().text().to_string())
    };
    assert_eq!(text(cx), "fn made() {}\n");
    let mut changeset = Changeset::new("next", ai());
    changeset.add_new_file(created.clone(), "fn other() {}\n");
    workbench.update_in(cx, |workbench, window, cx| {
        workbench.set_changeset(changeset, window, cx)
    });
    draw(cx);
    click_new_row(&workbench, cx);
    assert_eq!(text(cx), "fn other() {}\n");
    workbench.update_in(cx, |workbench, window, cx| {
        workbench.decide_ai_file(&created, true, window, cx)
    });
    draw(cx);
    let tabs = workbench.read_with(cx, |workbench, cx| workbench.open_editors(cx).len());
    assert_eq!(tabs, 2);
    assert_eq!(text(cx), "fn other() {}\n");
}

/// Whether the right dock's rail has a button for AI Changes.
fn on_rail(s: &mut Setup<'_>) -> bool {
    s.workbench.read_with(s.cx, |workbench, cx| {
        let state = workbench.dock().read(cx).state();
        state.on_rail(DockPanel::AiChanges.id(), cx)
            || state.joined().contains(&DockPanel::AiChanges.id())
    })
}

#[gpui::test]
fn ai_changes_shows_in_the_sidebar_on_changeset(cx: &mut TestAppContext) {
    let mut s = setup(cx);
    let panel = s.panel();
    let (visible, active, position, has_problems) = s.workbench.read_with(s.cx, |wb, cx| {
        let dock = wb.dock().read(cx);
        (
            dock.is_visible(),
            dock.active(),
            dock.state().position(),
            // Problems is the bottom panel's (T1548).
            !dock.state().panel_ids().contains(&DockPanel::Problems.id()),
        )
    });
    assert_eq!(
        (visible, active, position, has_problems),
        (
            true,
            DockPanel::AiChanges,
            // `workbench.rail`'s default side.
            kiln_shell::DockPosition::Left,
            true
        )
    );
    let focused =
        s.cx.update(|window, cx| panel.focus_handle(cx).contains_focused(window, cx));
    assert!(focused);
}

#[gpui::test]
fn ai_changes_has_no_rail_entry(cx: &mut TestAppContext) {
    let mut s = setup(cx);
    assert!(!on_rail(&mut s), "shown");
    s.workbench.update_in(s.cx, |wb, _, cx| {
        wb.dock().update(cx, |dock, cx| {
            dock.dock_mut().set_visible(false);
            cx.notify();
        })
    });
    draw(s.cx);
    assert!(!on_rail(&mut s), "hidden");
    s.run("ai.show_changes", &["workbench"]);
    s.run("ai.show_changes", &["workbench"]);
    assert!(!on_rail(&mut s), "shown again");
}
