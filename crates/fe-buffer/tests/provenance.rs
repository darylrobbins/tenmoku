//! Who wrote which bytes: spans follow edits, undo and redo; accepted hunks
//! are their proposal's agent's; loaded text has no author.

use std::ops::Range;

use fe_buffer::{
    Author, Backup, BackupId, BackupMeta, Buffer, EditStream, Origin, ProposedChange, Text,
};
use proptest::prelude::*;

fn ai(agent: &str) -> Origin {
    Origin::Ai {
        agent: agent.into(),
        session: "session".into(),
    }
}

fn ai_author(agent: &str) -> Author {
    Author::Ai {
        agent: agent.into(),
    }
}

fn spans(buffer: &Buffer) -> Vec<(Range<usize>, Author)> {
    buffer.provenance().spans()
}

fn undo_any(buffer: &mut Buffer) {
    buffer.undo(|_| true).unwrap().expect("something to undo");
}

fn redo_any(buffer: &mut Buffer) {
    buffer.redo(|_| true).unwrap().expect("something to redo");
}

#[test]
fn typed_text_is_the_users() {
    let mut buffer = Buffer::new("disk\n");
    assert_eq!(spans(&buffer), []);
    buffer.edit([(0..0, "ab")], Origin::User);
    assert_eq!(spans(&buffer), [(0..2, Author::User)]);
}

#[test]
fn an_ai_edit_is_the_agents() {
    let mut buffer = Buffer::new("one two");
    buffer.edit([(4..7, "2")], ai("claude"));
    assert_eq!(spans(&buffer), [(4..5, ai_author("claude"))]);
}

#[test]
fn each_origin_authors_its_own_text() {
    let mut buffer = Buffer::new("");
    buffer.edit([(0..0, "l")], Origin::Lsp);
    buffer.edit([(1..1, "f")], Origin::Format);
    buffer.edit([(2..2, "h")], Origin::Host("plugin".into()));
    assert_eq!(
        spans(&buffer),
        [
            (0..1, Author::Tool("lsp")),
            (1..2, Author::Tool("format")),
            (2..3, Author::Host("plugin".into())),
        ]
    );
}

#[test]
fn spans_after_an_edit_shift_and_replaced_spans_go() {
    let mut buffer = Buffer::new("....");
    buffer.edit([(2..2, "ab")], Origin::User); // ..ab..
    buffer.edit([(0..1, "xyz")], ai("a")); // xyz.ab..
    assert_eq!(
        spans(&buffer),
        [(0..3, ai_author("a")), (4..6, Author::User)]
    );
    // Across the user's span and some disk text: the span's other part stays.
    buffer.edit([(5..7, "")], Origin::Format); // xyz.a.
    assert_eq!(buffer.text().to_string(), "xyz.a.");
    assert_eq!(
        spans(&buffer),
        [(0..3, ai_author("a")), (4..5, Author::User)]
    );
}

#[test]
fn a_batch_authors_each_of_its_edits() {
    let mut buffer = Buffer::new("a-b-c");
    buffer.edit([(1..2, "1"), (3..4, "22"), (5..5, "3")], ai("a"));
    assert_eq!(buffer.text().to_string(), "a1b22c3");
    assert_eq!(
        spans(&buffer),
        [
            (1..2, ai_author("a")),
            (3..5, ai_author("a")),
            (6..7, ai_author("a")),
        ]
    );
}

#[test]
fn text_typed_inside_a_span_splits_it() {
    let mut buffer = Buffer::new("");
    buffer.edit([(0..0, "abcd")], ai("a"));
    buffer.edit([(2..2, "U")], Origin::User);
    assert_eq!(
        spans(&buffer),
        [
            (0..2, ai_author("a")),
            (2..3, Author::User),
            (3..5, ai_author("a")),
        ]
    );
    // And deleting what split it joins the two parts up.
    buffer.edit([(2..3, "")], ai("a"));
    assert_eq!(spans(&buffer), [(0..4, ai_author("a"))]);
}

#[test]
fn an_accepted_hunk_is_the_proposals_agent_though_applied_as_user() {
    let mut buffer = Buffer::new("one two three");
    let mut proposal = ProposedChange::from_edits(
        buffer.current(),
        [(4..7, "TWO"), (8..13, "3")],
        ai("claude"),
    );
    let [first, second] = [
        proposal.hunks(buffer.current())[0].id,
        proposal.hunks(buffer.current())[1].id,
    ];

    proposal
        .accept_as(&mut buffer, first, Origin::User)
        .unwrap();
    assert_eq!(spans(&buffer), [(4..7, ai_author("claude"))]);
    // The undo stack still follows the origin it was applied under.
    assert_eq!(buffer.history_bytes(&ai("claude")), 0);
    assert!(buffer.history_bytes(&Origin::User) > 0);

    buffer
        .undo(|origin| *origin == Origin::User)
        .unwrap()
        .unwrap();
    assert_eq!(buffer.text().to_string(), "one two three");
    assert_eq!(spans(&buffer), []);
    buffer
        .redo(|origin| *origin == Origin::User)
        .unwrap()
        .unwrap();
    assert_eq!(spans(&buffer), [(4..7, ai_author("claude"))]);

    proposal
        .accept_as(&mut buffer, second, Origin::User)
        .unwrap();
    assert_eq!(buffer.text().to_string(), "one TWO 3");
    assert_eq!(
        spans(&buffer),
        [(4..7, ai_author("claude")), (8..9, ai_author("claude"))]
    );
}

#[test]
fn accept_all_as_user_is_the_proposals_agent_too() {
    let mut buffer = Buffer::new("one two");
    let mut proposal = ProposedChange::from_edits(
        buffer.current(),
        [(0..3, "ONE"), (4..7, "TWO")],
        ai("claude"),
    );
    proposal.accept_all_as(&mut buffer, Origin::User);
    assert_eq!(
        spans(&buffer),
        [(0..3, ai_author("claude")), (4..7, ai_author("claude"))]
    );
    buffer.edit([(3..3, "!")], Origin::User);
    undo_any(&mut buffer);
    buffer
        .undo(|origin| *origin == Origin::User)
        .unwrap()
        .unwrap();
    assert_eq!(spans(&buffer), []);
}

#[test]
fn with_author_outlasts_its_call_only_in_the_text_it_wrote() {
    let mut buffer = Buffer::new("");
    let id = buffer.with_author(ai("a"), |buffer| {
        buffer.with_author(ai("b"), |buffer| buffer.edit([(0..0, "b")], Origin::User));
        buffer.edit([(1..1, "a")], Origin::User)
    });
    assert!(id.is_some());
    buffer.edit([(2..2, "u")], Origin::User);
    assert_eq!(
        spans(&buffer),
        [
            (0..1, ai_author("b")),
            (1..2, ai_author("a")),
            (2..3, Author::User),
        ]
    );
}

#[test]
fn undo_of_an_ai_edit_restores_the_users_spans() {
    let mut buffer = Buffer::new("disk ");
    buffer.edit([(5..5, "typed")], Origin::User);
    buffer.edit([(3..7, "AI")], ai("a"));
    assert_eq!(buffer.text().to_string(), "disAIped");
    assert_eq!(
        spans(&buffer),
        [(3..5, ai_author("a")), (5..8, Author::User)]
    );

    buffer.undo(|origin| origin.is_ai()).unwrap().unwrap();
    assert_eq!(buffer.text().to_string(), "disk typed");
    // What the AI replaced of the disk text has no author again.
    assert_eq!(spans(&buffer), [(5..10, Author::User)]);
}

#[test]
fn undo_restores_spans_moved_by_later_edits() {
    let mut buffer = Buffer::new("");
    buffer.edit([(0..0, "user")], Origin::User);
    buffer.edit([(0..4, "ai")], ai("a"));
    buffer.edit([(0..0, "..")], Origin::User);
    buffer.edit([(4..4, "..")], Origin::Format);
    buffer.undo(|origin| origin.is_ai()).unwrap().unwrap();
    assert_eq!(buffer.text().to_string(), "..user..");
    assert_eq!(
        spans(&buffer),
        [(0..6, Author::User), (6..8, Author::Tool("format")),]
    );
}

#[test]
fn a_refused_undo_leaves_the_spans() {
    let mut buffer = Buffer::new("");
    buffer.edit([(0..0, "user")], Origin::User);
    buffer.edit([(0..4, "ai")], ai("a"));
    buffer.edit([(1..1, "x")], Origin::User);
    let before = spans(&buffer);
    assert!(buffer.undo(|origin| origin.is_ai()).is_err());
    assert_eq!(spans(&buffer), before);
}

#[test]
fn undoing_a_batch_whose_restore_points_met_keeps_each_authors_text() {
    // Two deletions an AI made, whose text was the user's and an agent's.
    let mut buffer = Buffer::new("");
    buffer.edit([(0..0, "u")], Origin::User);
    buffer.edit([(1..1, "--")], Origin::Format);
    buffer.edit([(3..3, "b")], ai("b"));
    buffer.edit([(0..1, ""), (3..4, "")], ai("a"));
    assert_eq!(buffer.text().to_string(), "--");
    // Someone deletes what lay between, so both points of the undo meet.
    buffer.edit([(0..2, "")], Origin::Lsp);
    buffer.undo(|origin| *origin == ai("a")).unwrap().unwrap();
    assert_eq!(buffer.text().to_string(), "ub");
    assert_eq!(
        spans(&buffer),
        [(0..1, Author::User), (1..2, ai_author("b"))]
    );
}

#[test]
fn redo_reapplies_the_ai_author() {
    let mut buffer = Buffer::new("");
    buffer.edit([(0..0, "user")], Origin::User);
    buffer.edit([(0..4, "ai")], ai("a"));
    undo_any(&mut buffer);
    assert_eq!(spans(&buffer), [(0..4, Author::User)]);
    redo_any(&mut buffer);
    assert_eq!(spans(&buffer), [(0..2, ai_author("a"))]);
    undo_any(&mut buffer);
    assert_eq!(spans(&buffer), [(0..4, Author::User)]);
}

#[test]
fn a_stream_is_the_agents_and_undoing_it_restores_the_target() {
    let mut buffer = Buffer::new("");
    buffer.edit([(0..0, "target")], Origin::User);
    let mut stream = EditStream::begin(&buffer, 0..6, ai("a"));
    stream.push(&mut buffer, "one ").unwrap();
    buffer.edit([(0..0, "typed ")], Origin::User);
    stream.push(&mut buffer, "two").unwrap();
    stream.finish(&mut buffer);
    assert_eq!(buffer.text().to_string(), "typed one two");
    assert_eq!(
        spans(&buffer),
        [(0..6, Author::User), (6..13, ai_author("a"))]
    );
    buffer.undo(|origin| origin.is_ai()).unwrap().unwrap();
    assert_eq!(buffer.text().to_string(), "typed target");
    assert_eq!(spans(&buffer), [(0..12, Author::User)]);
    buffer.redo(|origin| origin.is_ai()).unwrap().unwrap();
    assert_eq!(
        spans(&buffer),
        [(0..6, Author::User), (6..13, ai_author("a"))]
    );
}

#[test]
fn compacting_the_log_keeps_what_undo_gives_back() {
    let mut buffer = Buffer::new("");
    buffer.edit([(0..0, "user")], Origin::User);
    let floor = buffer.version();
    buffer.edit([(0..4, "ai")], ai("a"));
    buffer.compact_log(floor);
    buffer.undo(|origin| origin.is_ai()).unwrap().unwrap();
    assert_eq!(spans(&buffer), [(0..4, Author::User)]);
}

#[test]
fn a_transaction_undoes_and_redoes_each_batchs_spans() {
    let mut buffer = Buffer::new("");
    buffer.edit([(0..0, "abcd")], Origin::User);
    buffer.start_transaction(ai("a"));
    buffer.edit([(0..1, "X")], ai("a"));
    buffer.edit([(3..4, "YY")], ai("a"));
    buffer.end_transaction();
    let after = spans(&buffer);
    undo_any(&mut buffer);
    assert_eq!(spans(&buffer), [(0..4, Author::User)]);
    redo_any(&mut buffer);
    assert_eq!(spans(&buffer), after);
}

#[test]
fn row_authors_prefers_ai_then_user() {
    let mut buffer = Buffer::new("disk\ndisk\ndisk\ndisk\ndisk\ndisk\n");
    // Row 1: a tool only. Row 2: a tool and the user. Row 3: the user and two
    // agents, the later one winning. Row 4: disk. Row 5: an agent.
    buffer.edit([(5..5, "t")], Origin::Format); // row 1
    buffer.edit([(11..11, "t")], Origin::Lsp); // row 2
    buffer.edit([(12..12, "u")], Origin::User);
    buffer.edit([(18..18, "u")], Origin::User); // row 3
    buffer.edit([(19..19, "a")], ai("a"));
    buffer.edit([(21..21, "b")], ai("b"));
    buffer.edit([(31..31, "c")], ai("a")); // row 5
    let text = buffer.text().to_string();
    assert_eq!(text, "disk\ntdisk\ntudisk\nuadbisk\ndisk\ncdisk\n");

    let rows = buffer.provenance().row_authors(buffer.text(), 0..100);
    assert_eq!(
        rows,
        [
            (0..1, None),
            (1..2, Some(Author::Tool("format"))),
            (2..3, Some(Author::User)),
            (3..4, Some(ai_author("b"))),
            (4..5, None),
            (5..6, Some(ai_author("a"))),
            (6..7, None),
        ]
    );
    // A window of them.
    assert_eq!(
        buffer.provenance().row_authors(buffer.text(), 2..3),
        [(2..3, Some(Author::User))]
    );
    assert_eq!(buffer.provenance().row_authors(buffer.text(), 9..12), []);
}

#[test]
fn row_authors_joins_rows_by_one_author() {
    let mut buffer = Buffer::new("disk\n");
    buffer.edit([(5..5, "a\nb\nc\n")], ai("a"));
    assert_eq!(
        buffer.provenance().row_authors(buffer.text(), 0..5),
        [(0..1, None), (1..4, Some(ai_author("a"))), (4..5, None),]
    );
}

#[test]
fn a_line_break_claims_only_an_empty_row() {
    // The user's enter at the end of a disk line leaves that line unclaimed.
    let mut buffer = Buffer::new("disk");
    buffer.edit([(4..4, "\n\nx")], Origin::User);
    assert_eq!(
        buffer.provenance().row_authors(buffer.text(), 0..3),
        [(0..1, None), (1..3, Some(Author::User))]
    );
}

#[test]
fn reload_clears_authors() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("file.txt");
    std::fs::write(&path, "typed on disk\n").unwrap();
    let read =
        |path: &std::path::Path| Buffer::read_file(std::fs::File::open(path).unwrap()).unwrap();

    let mut buffer = Buffer::new("");
    buffer.edit([(0..0, "typed in buffer\n")], Origin::User);
    buffer.edit([(0..0, "ai\n")], ai("a"));
    assert!(!spans(&buffer).is_empty());
    let (file, stamp) = read(&path);
    buffer.reload(file, Some(stamp), Origin::User);
    assert_eq!(buffer.text().to_string(), "typed on disk\n");
    assert_eq!(spans(&buffer), []);

    // Reloading text equal to the file's clears it too, making no edit.
    buffer.edit([(0..5, "typed")], Origin::User);
    assert_eq!(spans(&buffer), [(0..5, Author::User)]);
    let (file, stamp) = read(&path);
    assert_eq!(buffer.reload(file, Some(stamp), Origin::User), None);
    assert_eq!(spans(&buffer), []);
}

#[test]
fn undoing_a_reload_brings_the_text_back_with_no_author() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("file.txt");
    std::fs::write(&path, "disk").unwrap();
    let mut buffer = Buffer::new("");
    buffer.edit([(0..0, "user")], Origin::User);
    let (file, stamp) = Buffer::read_file(std::fs::File::open(&path).unwrap()).unwrap();
    buffer.reload(file, Some(stamp), Origin::Host("disk".into()));
    buffer
        .undo(|origin| origin == &Origin::Host("disk".into()))
        .unwrap()
        .unwrap();
    assert_eq!(buffer.text().to_string(), "user");
    assert_eq!(spans(&buffer), []);
}

#[test]
fn a_restored_backup_has_no_author() {
    let unsaved = Buffer::new("restored");
    let backup = Backup {
        id: BackupId::new(),
        meta: BackupMeta::for_buffer(&unsaved, None, "untitled"),
        text: Text::new("restored"),
    };
    let mut buffer = Buffer::from_backup(None, &backup).unwrap();
    assert_eq!(buffer.text().to_string(), "restored");
    assert_eq!(spans(&buffer), []);
    buffer.edit([(0..0, "u")], Origin::User);
    assert_eq!(spans(&buffer), [(0..1, Author::User)]);
}

#[test]
fn ten_thousand_typed_chars_stay_one_span() {
    let mut buffer = Buffer::new("disk\n");
    for ix in 0..10_000 {
        let at = 5 + ix;
        buffer.edit([(at..at, "x")], Origin::User);
    }
    assert_eq!(spans(&buffer), [(5..10_005, Author::User)]);
    // Read between keystrokes as well as after them.
    for ix in 10_000..10_100 {
        let at = 5 + ix;
        buffer.edit([(at..at, "x")], Origin::User);
        assert_eq!(spans(&buffer).len(), 1);
    }
}

#[derive(Clone, Debug)]
enum Op {
    Edit {
        at: usize,
        cut: usize,
        len: usize,
        who: usize,
    },
    Undo,
    Redo,
}

fn op() -> impl Strategy<Value = Op> {
    prop_oneof![
        6 => (0..200usize, 0..8usize, 0..6usize, 0..4usize)
            .prop_map(|(at, cut, len, who)| Op::Edit { at, cut, len, who }),
        1 => Just(Op::Undo),
        1 => Just(Op::Redo),
    ]
}

fn who(ix: usize) -> Origin {
    match ix {
        0 => Origin::User,
        1 => ai("a"),
        2 => ai("b"),
        _ => Origin::Format,
    }
}

/// The spans a per-byte model holds, joined as the tree joins them.
fn model_spans(model: &[Option<Author>]) -> Vec<(Range<usize>, Author)> {
    let mut spans: Vec<(Range<usize>, Author)> = Vec::new();
    for (ix, author) in model.iter().enumerate() {
        let Some(author) = author else { continue };
        match spans.last_mut() {
            Some((range, last)) if range.end == ix && last == author => range.end = ix + 1,
            _ => spans.push((ix..ix + 1, author.clone())),
        }
    }
    spans
}

proptest! {
    /// Against a model with one author per byte, where undoing the newest
    /// transaction restores the bytes as they were before it. Spans are read
    /// only at some steps, so batches also catch up in runs.
    #[test]
    fn spans_match_a_per_byte_model(ops in prop::collection::vec(op(), 1..80), read_every in 1..6usize) {
        let mut buffer = Buffer::new(&"d".repeat(20));
        let mut model: Vec<Option<Author>> = vec![None; 20];
        let mut undo: Vec<Vec<Option<Author>>> = Vec::new();
        let mut redo: Vec<Vec<Option<Author>>> = Vec::new();
        for (step, op) in ops.into_iter().enumerate() {
            match op {
                Op::Edit { at, cut, len, who: ix } => {
                    let start = at.min(model.len());
                    let end = (start + cut).min(model.len());
                    if start == end && len == 0 {
                        continue;
                    }
                    buffer.edit([(start..end, "x".repeat(len))], who(ix));
                    undo.push(model.clone());
                    // Any origin's fresh edit: only its own redos go, so this
                    // model stops redoing what another origin could.
                    redo.clear();
                    let author = Some(Author::from(&who(ix)));
                    model.splice(start..end, std::iter::repeat_n(author, len));
                }
                Op::Undo => {
                    let Some(before) = undo.pop() else { continue };
                    undo_any(&mut buffer);
                    redo.push(std::mem::replace(&mut model, before));
                }
                Op::Redo => {
                    let Some(after) = redo.pop() else { continue };
                    redo_any(&mut buffer);
                    undo.push(std::mem::replace(&mut model, after));
                }
            }
            prop_assert_eq!(buffer.len(), model.len());
            if step % read_every == 0 {
                prop_assert_eq!(spans(&buffer), model_spans(&model));
            }
        }
        prop_assert_eq!(spans(&buffer), model_spans(&model));
    }
}
