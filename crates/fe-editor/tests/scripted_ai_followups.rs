//! The scripted fake AI's follow-ups from the T501 review: failed deferred
//! writes, the final `\r` of a stream, marks owned by their transactions,
//! and a first chunk equal to the target.

use fe_buffer::{Buffer, Origin, StreamError};
use fe_editor::{
    EditorModel,
    decorations::Decoration,
    scripted_ai::{self, Report, ScriptedAi, Step, StepError},
};

fn model(text: &str) -> (EditorModel, ScriptedAi) {
    let mut model = EditorModel::new(Buffer::new(text), None);
    let ai = ScriptedAi::new(&mut model);
    (model, ai)
}

/// Runs `script`, every step of which must succeed.
fn play(model: &mut EditorModel, ai: &mut ScriptedAi, script: &str) {
    for step in scripted_ai::parse(script).unwrap() {
        if let Err(error) = ai.step(model, &step) {
            panic!("{step:?}: {error}");
        }
    }
}

/// Pushes `chunk`, which a script can't spell if it holds a `\r`.
fn push(model: &mut EditorModel, ai: &mut ScriptedAi, chunk: &str) {
    ai.step(model, &Step::Push(chunk.into())).unwrap();
}

fn text(model: &EditorModel) -> String {
    model.text().to_string()
}

fn touched(model: &EditorModel) -> Vec<Decoration> {
    model.decorations(0..model.text().len())
}

/// The byte ranges the touched rows cover, as (start, end).
fn marked(model: &EditorModel) -> Vec<(usize, usize)> {
    (touched(model).into_iter())
        .map(|d| (d.range.start, d.range.end))
        .collect()
}

#[test]
fn finish_reports_a_conflicting_deferred_write() {
    let (mut model, mut ai) = model("alpha beta\n");
    model.select_range(0..5, false);
    play(&mut model, &mut ai, "stream");
    push(&mut model, &mut ai, "\r");
    play(&mut model, &mut ai, "caret 1:3\ntype x");
    let before = text(&model);
    let result = ai.step(&mut model, &Step::Finish);
    assert_eq!(result, Err(StepError::Stream(StreamError::Conflict)));
    assert_eq!(text(&model), before);
    assert!(ai.stream().is_none());
}

#[test]
fn finish_reports_a_deferred_write_into_an_undone_transaction() {
    let (mut model, mut ai) = model("alpha\n");
    model.select_range(0..5, false);
    play(&mut model, &mut ai, "stream\npush x");
    push(&mut model, &mut ai, "\r");
    play(&mut model, &mut ai, "undo-ai");
    let result = ai.step(&mut model, &Step::Finish);
    assert_eq!(result, Err(StepError::Stream(StreamError::Undone)));
    assert_eq!(text(&model), "alpha\n");
    assert!(ai.stream().is_none());
}

#[test]
fn finish_marks_a_lone_cr_over_a_longer_selection() {
    let (mut model, mut ai) = model("alpha beta\n");
    model.select_range(0..5, false);
    play(&mut model, &mut ai, "stream");
    push(&mut model, &mut ai, "\r");
    play(&mut model, &mut ai, "finish");
    assert_eq!(text(&model), "\n beta\n");
    assert_eq!(marked(&model), [(0, 1)]);
    play(&mut model, &mut ai, "undo-ai");
    assert_eq!(text(&model), "alpha beta\n");
    assert_eq!(marked(&model), []);
    play(&mut model, &mut ai, "redo-ai");
    assert_eq!(text(&model), "\n beta\n");
    assert_eq!(marked(&model), [(0, 1)]);
}

#[test]
fn finish_marks_a_lone_cr_on_an_empty_selection() {
    let (mut model, mut ai) = model("ab\n");
    model.set_cursor(1);
    play(&mut model, &mut ai, "stream");
    push(&mut model, &mut ai, "\r");
    play(&mut model, &mut ai, "finish");
    assert_eq!(text(&model), "a\nb\n");
    assert_eq!(marked(&model), [(1, 2)]);
    play(&mut model, &mut ai, "undo-ai");
    assert_eq!(text(&model), "ab\n");
    assert_eq!(marked(&model), []);
    play(&mut model, &mut ai, "redo-ai");
    assert_eq!(marked(&model), [(1, 2)]);
}

#[test]
fn undoing_an_accepted_hunk_restores_the_streams_marks() {
    let (mut model, mut ai) = model("one\ntwo\n");
    model.set_cursor(0);
    play(&mut model, &mut ai, "stream\npush new\\n\nfinish");
    assert_eq!(text(&model), "new\none\ntwo\n");
    assert_eq!(marked(&model), [(0, 4)]);
    play(&mut model, &mut ai, "propose 1-1 other\\n\naccept-hunk 1");
    assert_eq!(text(&model), "other\none\ntwo\n");
    assert_eq!(marked(&model), [(0, 6)]);
    play(&mut model, &mut ai, "undo");
    assert_eq!(text(&model), "new\none\ntwo\n");
    assert_eq!(marked(&model), [(0, 4)]);
    play(&mut model, &mut ai, "redo");
    assert_eq!(text(&model), "other\none\ntwo\n");
    assert_eq!(marked(&model), [(0, 6)]);
}

#[test]
fn a_first_chunk_equal_to_the_target_marks_the_region() {
    // A chunk that makes no transaction, as a held-back `\r` makes none,
    // still establishes and marks the region.
    let (mut first, mut ai) = model("old\nrest\n");
    first.select_range(0..3, false);
    play(&mut first, &mut ai, "stream");
    let report = ai.step(&mut first, &Step::Push("\r".into())).unwrap();
    assert!(matches!(report, Report::Pushed { region, .. } if region == (0..3)));
    assert!(ai.stream().unwrap().transaction().is_none());
    assert_eq!(marked(&first), [(0, 3)]);

    // The text itself, over itself.
    let (mut other, mut ai) = model("old\nrest\n");
    other.select_range(0..3, false);
    play(&mut other, &mut ai, "stream\npush old");
    assert_eq!(text(&other), "old\nrest\n");
    assert_eq!(marked(&other), [(0, 3)]);
}

#[test]
fn an_accepted_hunk_around_the_stream_restores_only_the_streams_rows() {
    let (mut model, mut ai) = model("one\ntwo\n");
    model.set_cursor(4);
    play(&mut model, &mut ai, "stream\npush new\\n\nfinish");
    assert_eq!(text(&model), "one\nnew\ntwo\n");
    assert_eq!(marked(&model), [(4, 8)]);
    play(&mut model, &mut ai, "propose 1-3 X\\n\naccept-hunk 1");
    assert_eq!(text(&model), "X\n");
    assert_eq!(marked(&model), [(0, 2)]);
    play(&mut model, &mut ai, "undo");
    assert_eq!(text(&model), "one\nnew\ntwo\n");
    assert_eq!(marked(&model), [(4, 8)]);
    play(&mut model, &mut ai, "redo");
    assert_eq!(marked(&model), [(0, 2)]);
}

#[test]
fn a_hunk_accepted_over_an_open_stream_covers_its_mark() {
    let (mut model, mut ai) = model("one\ntwo\n");
    model.set_cursor(0);
    play(&mut model, &mut ai, "stream\npush new\\n");
    play(&mut model, &mut ai, "propose 1-1 other\\n\naccept-hunk 1");
    assert_eq!(marked(&model), [(0, 6)]);
    play(&mut model, &mut ai, "finish");
    assert_eq!(marked(&model), [(0, 6)]);
    play(&mut model, &mut ai, "undo");
    assert_eq!(text(&model), "new\none\ntwo\n");
    assert_eq!(marked(&model), [(0, 4)]);
    play(&mut model, &mut ai, "redo");
    assert_eq!(marked(&model), [(0, 6)]);
}

#[test]
fn text_pushed_after_a_hunk_covers_the_stream_is_marked() {
    let (mut model, mut ai) = model("one\ntwo\n");
    model.set_cursor(0);
    play(&mut model, &mut ai, "stream\npush new\\n");
    play(&mut model, &mut ai, "propose 1-1 other\\n\naccept-hunk 1");
    play(&mut model, &mut ai, "push extra\\n");
    assert_eq!(text(&model), "other\nextra\none\ntwo\n");
    assert_eq!(marked(&model), [(0, 6), (6, 12)]);
    play(&mut model, &mut ai, "finish\nundo");
    assert_eq!(text(&model), "new\nextra\none\ntwo\n");
    assert_eq!(marked(&model), [(0, 4), (4, 10)]);
}

#[test]
fn a_cover_is_restored_by_the_batch_that_undoes_it() {
    let (mut model, mut ai) = model("one\ntwo\n");
    model.set_cursor(4);
    play(&mut model, &mut ai, "stream\npush new\\n\nfinish");
    assert_eq!(marked(&model), [(4, 8)]);
    // A second stream replaces everything, in two batches.
    model.select_range(0..12, false);
    play(&mut model, &mut ai, "stream\npush X\\n\npush Y\\n\nfinish");
    assert_eq!(text(&model), "X\nY\n");
    assert_eq!(marked(&model), [(0, 4)]);
    play(&mut model, &mut ai, "undo-ai");
    assert_eq!(text(&model), "one\nnew\ntwo\n");
    assert_eq!(marked(&model), [(4, 8)]);
}

#[test]
fn undoing_a_whole_buffer_deletion_restores_the_streams_mark() {
    let (mut model, mut ai) = model("one\ntwo\n");
    model.set_cursor(4);
    play(&mut model, &mut ai, "stream\npush new\\n\nfinish");
    assert_eq!(marked(&model), [(4, 8)]);
    model.select_range(0..12, false);
    model.backspace();
    assert_eq!(text(&model), "");
    play(&mut model, &mut ai, "undo");
    assert_eq!(text(&model), "one\nnew\ntwo\n");
    assert_eq!(marked(&model), [(4, 8)]);
}

#[test]
fn a_deferred_cr_over_a_covered_stream_marks_only_the_cr() {
    let (mut model, mut ai) = model("one\ntwo\n");
    model.set_cursor(0);
    play(&mut model, &mut ai, "stream\npush new\\n");
    play(&mut model, &mut ai, "propose 1-1 other\\n\naccept-hunk 1");
    push(&mut model, &mut ai, "\r");
    play(&mut model, &mut ai, "finish");
    assert_eq!(text(&model), "other\n\none\ntwo\n");
    assert_eq!(marked(&model), [(0, 6), (6, 7)]);
}

#[test]
fn redoing_a_split_stream_restores_each_marks_own_span() {
    let (mut model, mut ai) = model("one\ntwo\n");
    model.set_cursor(0);
    play(&mut model, &mut ai, "stream\npush new\\n");
    play(&mut model, &mut ai, "propose 1-1 other\\n\naccept-hunk 1");
    play(&mut model, &mut ai, "push extra\\n\nfinish");
    play(&mut model, &mut ai, "undo");
    assert_eq!(marked(&model), [(0, 4), (4, 10)]);
    play(&mut model, &mut ai, "undo-ai");
    assert_eq!(text(&model), "one\ntwo\n");
    assert_eq!(marked(&model), []);
    play(&mut model, &mut ai, "redo-ai");
    assert_eq!(text(&model), "new\nextra\none\ntwo\n");
    assert_eq!(marked(&model), [(0, 4), (4, 10)]);
    // Redoing the accepted replacement covers the original part again.
    play(&mut model, &mut ai, "redo");
    assert_eq!(text(&model), "other\nextra\none\ntwo\n");
    assert_eq!(marked(&model), [(0, 6), (6, 12)]);
}

#[test]
fn only_the_covering_batchs_undo_restores_the_mark() {
    let (mut model, mut ai) = model("one\ntwo\n");
    model.set_cursor(4);
    play(&mut model, &mut ai, "stream\npush new\\n\nfinish");
    // One user transaction: replace everything, then the same place again.
    let transaction = model.buffer_mut().edit([(0..12, "X\n")], Origin::User);
    let transaction = transaction.unwrap();
    model
        .buffer_mut()
        .edit_in(transaction, [(0..2, "Y\n")])
        .unwrap();
    assert_eq!(text(&model), "Y\n");
    play(&mut model, &mut ai, "undo");
    assert_eq!(text(&model), "one\nnew\ntwo\n");
    assert_eq!(marked(&model), [(4, 8)]);
}

#[test]
fn a_nested_deletion_keeps_the_first_covers_bounds() {
    let (mut model, mut ai) = model("one\ntwo\n");
    model.set_cursor(0);
    play(&mut model, &mut ai, "stream\npush new\\n");
    play(&mut model, &mut ai, "propose 1-1 other\\n\naccept-hunk 1");
    model.select_range(0..model.text().len(), false);
    model.backspace();
    play(&mut model, &mut ai, "undo\nundo");
    assert_eq!(text(&model), "new\none\ntwo\n");
    assert_eq!(marked(&model), [(0, 4)]);
}

#[test]
fn a_push_after_undoing_the_cover_keeps_the_tail_marks_split() {
    let (mut model, mut ai) = model("one\ntwo\n");
    model.set_cursor(0);
    play(&mut model, &mut ai, "stream\npush new\\n");
    play(&mut model, &mut ai, "propose 1-1 other\\n\naccept-hunk 1");
    play(&mut model, &mut ai, "push extra\\n\nundo");
    assert_eq!(marked(&model), [(0, 4), (4, 10)]);
    play(&mut model, &mut ai, "push more\\n");
    assert_eq!(text(&model), "new\nextra\nmore\none\ntwo\n");
    assert_eq!(marked(&model), [(0, 4), (4, 15)]);
    play(&mut model, &mut ai, "redo");
    assert_eq!(text(&model), "other\nextra\nmore\none\ntwo\n");
    assert_eq!(marked(&model), [(0, 6), (6, 17)]);
}
