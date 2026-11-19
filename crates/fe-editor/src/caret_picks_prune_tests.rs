//! `History::caret_picks` and `redo_selections` hold entries only for the
//! transactions undo and redo can still reach.

use std::ops::Range;

use crate::completion::{CompletionItem, CompletionList, CompletionSession};
use crate::tests::{parse, render};
use crate::{EditorModel, TransactionId};

fn accept(model: &mut EditorModel, n: usize) -> TransactionId {
    if n > 0 {
        model.insert(" fo");
    }
    let caret = model.primary_selection().head();
    let range: Range<usize> = caret - 2..caret;
    let mut foo = CompletionItem {
        label: "foo".into(),
        edit: (range, "foo".into()),
        ..CompletionItem::default()
    };
    foo.additional_edits = vec![(0..0, format!("use m{n};\n"))];
    let list = CompletionList {
        items: vec![foo],
        is_incomplete: false,
    };
    CompletionSession::new(model.snapshot().clone(), caret, list)
        .accept(model)
        .unwrap()
        .unwrap()
}

fn picks(model: &EditorModel) -> usize {
    model.history.borrow().caret_picks.len()
}

fn reachable(model: &EditorModel) -> usize {
    model.buffer().history_times(|_| true).len()
}

#[test]
fn bounded_past_a_small_history_limit() {
    let mut model = parse("fo|\n");
    model.buffer.borrow_mut().set_history_limit(600);
    for n in 0..200 {
        accept(&mut model, n);
        assert!(picks(&model) <= reachable(&model), "{n}");
    }
    assert!(reachable(&model) < 200, "the limit trimmed history");
    assert!(picks(&model) < 200);
}

#[test]
fn a_discarded_redo_loses_its_entry() {
    let mut model = parse("fo|\n");
    let first = accept(&mut model, 0);
    let second = accept(&mut model, 1);
    model.undo().unwrap();
    assert!(model.history.borrow().caret_picks.contains_key(&second));
    // A new edit discards the redo; the next accept prunes.
    model.insert(" fo");
    accept_at_end(&mut model);
    let history = model.history.borrow();
    assert!(history.caret_picks.contains_key(&first));
    assert!(!history.caret_picks.contains_key(&second));
}

fn accept_at_end(model: &mut EditorModel) {
    accept(model, 2);
}

#[test]
fn compaction_drops_removed_entries_and_keeps_live_ones() {
    let mut model = parse("fo|\n");
    let mut ids = Vec::new();
    let mut versions = Vec::new();
    let mut shown = Vec::new();
    for n in 0..5 {
        ids.push(accept(&mut model, n));
        versions.push(model.snapshot().version());
        shown.push(render(&model));
    }
    // Drops the first two transactions (batches at or below the floor).
    model.buffer.borrow_mut().compact_log(versions[1]);
    let live: Vec<_> = model
        .buffer()
        .history_times(|_| true)
        .iter()
        .map(|time| time.transaction)
        .collect();
    assert!(ids[..2].iter().all(|id| !live.contains(id)));
    assert!(ids[2..].iter().all(|id| live.contains(id)));
    let last = accept(&mut model, 5);
    {
        let history = model.history.borrow();
        for id in &ids[..2] {
            assert!(!history.caret_picks.contains_key(id));
        }
        for id in ids[2..].iter().chain([&last]) {
            assert!(history.caret_picks.contains_key(id));
        }
    }
    // Undo the new completion, its typing, then a retained transaction:
    // it puts its caret where it did, and redo restores its state.
    for _ in 0..3 {
        model.undo().unwrap();
    }
    let end = model.text().len() - 1;
    assert_eq!(model.selections(), [crate::caret(end)]);
    model.redo().unwrap();
    assert_eq!(render(&model), shown[4]);
}

#[test]
fn undo_and_redo_place_carets_as_before() {
    let mut model = parse("fo|\n");
    let mut shown = Vec::new();
    for n in 0..6 {
        accept(&mut model, n);
        shown.push(render(&model));
    }
    // Force a prune that keeps everything, then walk back and forth.
    for n in (0..6).rev() {
        model.undo().unwrap();
        let end = model.text().len() - 1;
        assert_eq!(model.selections(), [crate::caret(end)], "{n}");
        if n > 0 {
            model.undo().unwrap();
        }
    }
    model.redo().unwrap();
    assert_eq!(render(&model), shown[0]);
}

#[test]
fn siblings_prune_but_spare_live_and_open_transactions() {
    let mut model = parse("fo|\n");
    let ghost = accept(&mut model, 0);
    model.undo().unwrap();
    model.insert("x"); // discards the redo: `ghost` is unreachable
    let live = accept(&mut model, 1);
    let open = model
        .buffer
        .borrow_mut()
        .start_transaction(model.origin.clone());
    let version = model.snapshot().version();
    {
        let mut history = model.history.borrow_mut();
        for id in [ghost, live, open] {
            history.typing_starts.insert(id, (version, vec![]));
            history.change_start_carets.insert(id);
        }
    }
    // The revision moved on since the last prune: this one prunes.
    model.history.borrow_mut().picks_pruned_at = None;
    model.pick_caret(
        live,
        crate::CaretPick {
            undo: None,
            redo: None,
        },
    );
    let history = model.history.borrow();
    assert!(!history.typing_starts.contains_key(&ghost));
    assert!(!history.change_start_carets.contains(&ghost));
    for id in [live, open] {
        assert!(history.typing_starts.contains_key(&id));
        assert!(history.change_start_carets.contains(&id));
    }
}
