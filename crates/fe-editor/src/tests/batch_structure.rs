use std::time::{Duration, Instant};

use super::parse;
use crate::*;

const TEXT: &str = "top\ndef a():\n    one\n    two\nend";
const FOLD: [fe_display::Fold; 1] = [fe_display::Fold { start: 1, end: 3 }];

/// A model of `TEXT` and a split of it with rows 1..=3 folded.
fn folded() -> (EditorModel, EditorModel) {
    let a = parse(TEXT);
    let mut b = a.split();
    b.fold_rows(1..=3);
    (a, b)
}

/// Catches `b` up by syncing, or by dropping a borrow of the buffer.
fn catch_up(b: &mut EditorModel, through_buffer: bool) {
    if through_buffer {
        drop(b.buffer_mut());
    } else {
        b.sync_display();
    }
}

#[test]
fn insert_then_delete_through_one_borrow_keeps_the_fold() {
    for through_buffer in [false, true] {
        let (mut a, mut b) = folded();
        {
            let mut guard = a.buffer_mut();
            let buffer: &mut Buffer = &mut guard;
            buffer.edit([(6..6, "X")], Origin::User);
            buffer.edit([(6..7, "")], Origin::User);
        }
        catch_up(&mut b, through_buffer);
        assert_eq!(b.text().to_string(), TEXT);
        assert_eq!(
            b.display().folds(),
            FOLD,
            "through_buffer: {through_buffer}"
        );
    }
}

#[test]
fn transient_header_split_through_one_borrow_unfolds() {
    for through_buffer in [false, true] {
        let (mut a, mut b) = folded();
        {
            let mut guard = a.buffer_mut();
            let buffer: &mut Buffer = &mut guard;
            buffer.edit([(6..6, "\n")], Origin::User);
            buffer.edit([(6..7, "")], Origin::User);
        }
        catch_up(&mut b, through_buffer);
        assert_eq!(b.display().folds(), [], "through_buffer: {through_buffer}");
    }
}

#[test]
fn grouped_overlapping_replacements_keep_the_fold_through_undo_and_redo() {
    for through_buffer in [false, true] {
        let (mut a, mut b) = folded();
        {
            let mut buffer = a.buffer_mut();
            buffer.start_transaction(Origin::User);
            buffer.edit([(6..7, "X")], Origin::User);
            buffer.edit([(6..7, "Y")], Origin::User);
            buffer.end_transaction();
        }
        catch_up(&mut b, through_buffer);
        assert_eq!(
            b.display().folds(),
            FOLD,
            "edit, through_buffer: {through_buffer}"
        );
        a.undo().unwrap();
        catch_up(&mut b, through_buffer);
        assert_eq!(b.text().to_string(), TEXT);
        assert_eq!(
            b.display().folds(),
            FOLD,
            "undo, through_buffer: {through_buffer}"
        );
        a.redo().unwrap();
        catch_up(&mut b, through_buffer);
        assert_eq!(b.text().to_string(), "top\ndeY a():\n    one\n    two\nend");
        assert_eq!(
            b.display().folds(),
            FOLD,
            "redo, through_buffer: {through_buffer}"
        );
    }
}

#[test]
fn grouped_insertions_through_one_borrow_keep_the_fold_through_undo_and_redo() {
    for through_buffer in [false, true] {
        let (mut a, mut b) = folded();
        {
            let mut guard = a.buffer_mut();
            let buffer: &mut Buffer = &mut guard;
            buffer.start_transaction(Origin::User);
            buffer.edit([(6..6, "X")], Origin::User);
            buffer.edit([(7..7, "Y")], Origin::User);
            buffer.end_transaction();
        }
        catch_up(&mut b, through_buffer);
        assert_eq!(
            b.display().folds(),
            FOLD,
            "edit, through_buffer: {through_buffer}"
        );
        a.undo().unwrap();
        catch_up(&mut b, through_buffer);
        assert_eq!(b.text().to_string(), TEXT);
        assert_eq!(
            b.display().folds(),
            FOLD,
            "undo, through_buffer: {through_buffer}"
        );
        a.redo().unwrap();
        catch_up(&mut b, through_buffer);
        assert_eq!(
            b.text().to_string(),
            "top\ndeXYf a():\n    one\n    two\nend"
        );
        assert_eq!(
            b.display().folds(),
            FOLD,
            "redo, through_buffer: {through_buffer}"
        );
    }
}

/// Records `batches` one-char batches through one `buffer_mut` borrow, syncs a folded
/// split, and returns how long that took. Checks the folds and text length on every run.
fn time_recording(batches: usize) -> Duration {
    let (mut a, mut b) = folded();
    let started = Instant::now();
    {
        let mut guard = a.buffer_mut();
        let buffer: &mut Buffer = &mut guard;
        for _ in 0..batches {
            let len = buffer.len();
            buffer.edit([(len..len, "!")], Origin::User);
        }
    }
    b.sync_display();
    let took = started.elapsed();
    assert_eq!(b.display().folds(), FOLD);
    assert_eq!(b.text().len(), TEXT.len() + batches);
    took
}

#[test]
fn many_batches_recorded_together_take_linear_work() {
    // A single timing per size is at the mercy of machine load: a burst landing on one size's
    // run skews the ratio either way. So the sizes alternate for several rounds, which exposes
    // both to the same load, and each size is judged by its fastest run, the one least disturbed.
    // Linear work gives about 10x between the sizes and quadratic about 100x, so 50x separates
    // them with headroom for the noise that remains.
    const SMALL: usize = 1_000;
    const LARGE: usize = 10_000;
    const ROUNDS: usize = 5;
    let mut small = Vec::new();
    let mut large = Vec::new();
    for _ in 0..ROUNDS {
        small.push(time_recording(SMALL));
        large.push(time_recording(LARGE));
    }
    let fastest_small = small.iter().min().copied().unwrap();
    let fastest_large = large.iter().min().copied().unwrap();
    assert!(
        fastest_large < fastest_small * 50,
        "fastest {LARGE} batches took {fastest_large:?}, over 50x the fastest {SMALL} \
         batches at {fastest_small:?}; {SMALL}: {small:?}, {LARGE}: {large:?}"
    );
}
