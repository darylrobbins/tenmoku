//! The AI Changes panel: a changeset built directly over open editors, its
//! list, the per-file and header buttons, opening a file at its first
//! pending hunk, the commands and Revert Turn.

use std::{cell::RefCell, path::PathBuf, rc::Rc};

use fe_buffer::{Buffer, HunkStatus, Origin, ProposedChange};
use fe_command::{ContextFrame, ContextStack, Invocation};
use fe_editor::{Changeset, Counts};
use fe_gpui::{EditorView, commands::RunCommand};
use gpui::{Entity, Focusable as _, Modifiers, TestAppContext, VisualTestContext};

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
            let proposal =
                ProposedChange::from_text(&snapshot, 0..snapshot.len(), &proposed(), ai());
            (buffer.id(), proposal)
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

#[gpui::test]
fn an_empty_new_file_is_unsaved_too(cx: &mut TestAppContext) {
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
    let (text, dirty) = editor(&workbench, &created, cx).read_with(cx, |editor, _| {
        let buffer = editor.model().buffer();
        (buffer.text().to_string(), buffer.is_dirty())
    });
    assert_eq!((text.as_str(), dirty), ("", true));
}
