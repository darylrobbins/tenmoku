//! `EditStream` writes an AI's edit chunk by chunk while the user types.

use fe_buffer::{Buffer, EditStream, Origin, StreamError, TransactionId, UndoError};
use proptest::{prelude::*, sample::Index};

fn ai() -> Origin {
    Origin::Ai {
        agent: "a".into(),
        session: "s".into(),
    }
}

fn text(buffer: &Buffer) -> String {
    buffer.text().slice_to_string(0..buffer.len())
}

fn user(buffer: &mut Buffer, at: usize, new_text: &str) {
    user_replace(buffer, at..at, new_text);
}

fn user_replace(buffer: &mut Buffer, range: std::ops::Range<usize>, new_text: &str) {
    buffer.edit([(range, new_text)], Origin::User);
}

fn find(buffer: &Buffer, needle: &str) -> usize {
    text(buffer).find(needle).expect("needle in the text")
}

/// Streams five chunks into `head [old] tail` while the user types at both
/// edges of the region, some inside an open transaction, and before and after
/// it. Returns the buffer, `Zhead [XN1-2-3-4-5Y] tail!`, and the stream's
/// transaction.
fn type_during_a_stream() -> (Buffer, TransactionId) {
    let mut buffer = Buffer::new("head [old] tail");
    let start = find(&buffer, "old");
    let mut stream = EditStream::begin(&buffer, start..start + 3, ai());
    assert_eq!(text(&buffer), "head [old] tail");
    assert_eq!(stream.range(buffer.current()), 6..9);
    assert_eq!(stream.transaction(), None);
    assert_eq!(stream.origin(), &ai());
    // The user's anchors after the region: one at its closing edge, one
    // further on.
    let snapshot = buffer.snapshot();
    let bracket = snapshot.anchor_after(find(&buffer, "]"));
    let tail = snapshot.anchor_before(find(&buffer, "tail"));

    stream.push(&mut buffer, "N1").unwrap();
    let id = stream.transaction().expect("the first chunk landed");
    assert_eq!(text(&buffer), "head [N1] tail");

    // As vim's insert session does: the user's edits at both edges, in one
    // open transaction, around a chunk.
    buffer.start_transaction(Origin::User);
    let at = stream.range(buffer.current()).start;
    user(&mut buffer, at, "X");
    stream.push(&mut buffer, "-2").unwrap();
    let at = stream.range(buffer.current()).end;
    user(&mut buffer, at, "Y");
    assert_eq!(text(&buffer), "head [XN1-2Y] tail");
    stream.push(&mut buffer, "-3").unwrap();
    buffer.end_transaction();
    assert_eq!(text(&buffer), "head [XN1-2-3Y] tail");

    // Before and after the region, each in its own transaction.
    user(&mut buffer, 0, "Z");
    stream.push(&mut buffer, "-4").unwrap();
    let len = buffer.len();
    user(&mut buffer, len, "!");
    stream.push(&mut buffer, "-5").unwrap();
    assert_eq!(text(&buffer), "Zhead [XN1-2-3-4-5Y] tail!");
    assert_eq!(stream.transaction(), Some(id));

    let range = stream.range(buffer.current());
    assert_eq!(&text(&buffer)[range], "N1-2-3-4-5");
    assert_eq!(buffer.current().resolve(&bracket), find(&buffer, "]"));
    assert_eq!(buffer.current().resolve(&tail), find(&buffer, "tail"));
    assert_eq!(stream.finish(&mut buffer), Some(id));
    (buffer, id)
}

/// The M8 exit test: an AI transaction applied during simulated typing
/// rebases its anchors, and undo stays grouped by origin.
#[test]
fn typing_during_a_stream() {
    // One undo of the AI restores the old target text, keeping the user's,
    // and redo brings the stream back.
    let (mut buffer, id) = type_during_a_stream();
    assert_eq!(buffer.undo(|o| o.is_ai()), Ok(Some(id)));
    assert_eq!(text(&buffer), "Zhead [XoldY] tail!");
    assert_eq!(buffer.undo(|o| o.is_ai()), Ok(None));
    assert_eq!(buffer.redo(|o| o.is_ai()), Ok(Some(id)));
    assert_eq!(text(&buffer), "Zhead [XN1-2-3-4-5Y] tail!");

    // The user's undo removes only the user's text; then the AI's undo
    // restores the old target text.
    let (mut buffer, id) = type_during_a_stream();
    let mut undone = 0;
    while buffer.undo(|o| *o == Origin::User).unwrap().is_some() {
        undone += 1;
    }
    assert_eq!(undone, 3);
    assert_eq!(text(&buffer), "head [N1-2-3-4-5] tail");

    assert_eq!(buffer.undo(|o| o.is_ai()), Ok(Some(id)));
    assert_eq!(text(&buffer), "head [old] tail");
    assert_eq!(buffer.undo(|o| o.is_ai()), Ok(None));
    assert_eq!(buffer.redo(|o| o.is_ai()), Ok(Some(id)));
    assert_eq!(text(&buffer), "head [N1-2-3-4-5] tail");
}

#[test]
fn conflicts_and_undone() {
    // A user edit inside the target before the first chunk.
    let mut buffer = Buffer::new("abc def ghi");
    let mut stream = EditStream::begin(&buffer, 4..7, ai());
    user_replace(&mut buffer, 5..6, "E");
    let before = text(&buffer);
    assert_eq!(stream.push(&mut buffer, "xyz"), Err(StreamError::Conflict));
    assert_eq!(text(&buffer), before);
    assert_eq!(stream.transaction(), None);
    assert_eq!(stream.push(&mut buffer, "xyz"), Err(StreamError::Conflict));
    assert_eq!(stream.finish(&mut buffer), None);
    assert_eq!(text(&buffer), before);
    assert_eq!(buffer.undo(|o| o.is_ai()), Ok(None));

    // A first push of only a held \r still checks the target.
    let mut buffer = Buffer::new("abc def ghi");
    let mut stream = EditStream::begin(&buffer, 4..7, ai());
    user_replace(&mut buffer, 5..6, "E");
    assert_eq!(stream.push(&mut buffer, "\r"), Err(StreamError::Conflict));
    assert_eq!(stream.push(&mut buffer, "\n"), Err(StreamError::Conflict));
    assert_eq!(stream.finish(&mut buffer), None);
    assert_eq!(text(&buffer), "abc dEf ghi");

    // Empty and held-back pushes after the AI's edit was undone are refused
    // too, and the stream stays dead once it's redone.
    for chunk in ["", "\r"] {
        let mut buffer = Buffer::new("abc def ghi");
        let mut stream = EditStream::begin(&buffer, 4..7, ai());
        stream.push(&mut buffer, "one").unwrap();
        let id = stream.transaction().unwrap();
        buffer.undo(|o| o.is_ai()).unwrap();
        assert_eq!(stream.push(&mut buffer, chunk), Err(StreamError::Undone));
        assert_eq!(buffer.redo(|o| o.is_ai()), Ok(Some(id)));
        assert_eq!(stream.push(&mut buffer, "two"), Err(StreamError::Undone));
        assert_eq!(stream.finish(&mut buffer), Some(id));
        assert_eq!(text(&buffer), "abc one ghi");
    }

    // Trimmed from the history, or compacted away.
    let mut buffer = Buffer::new("abc def ghi");
    buffer.set_history_limit(16);
    let mut stream = EditStream::begin(&buffer, 4..7, ai());
    stream.push(&mut buffer, "one").unwrap();
    buffer.edit([(0..0, "x".repeat(32))], ai());
    assert_eq!(stream.push(&mut buffer, ""), Err(StreamError::Undone));
    let mut buffer = Buffer::new("abc def ghi");
    let mut stream = EditStream::begin(&buffer, 4..7, ai());
    stream.push(&mut buffer, "one").unwrap();
    let version = buffer.version();
    buffer.compact_log(version);
    assert_eq!(stream.push(&mut buffer, "\r"), Err(StreamError::Undone));
    assert_eq!(text(&buffer), "abc one ghi");

    // A push after the AI's edit was undone.
    let mut buffer = Buffer::new("abc def ghi");
    let mut stream = EditStream::begin(&buffer, 4..7, ai());
    stream.push(&mut buffer, "one").unwrap();
    user(&mut buffer, 0, ">");
    let id = stream.transaction().unwrap();
    assert_eq!(buffer.undo(|o| o.is_ai()), Ok(Some(id)));
    assert_eq!(text(&buffer), ">abc def ghi");
    assert_eq!(stream.push(&mut buffer, "two"), Err(StreamError::Undone));
    assert_eq!(text(&buffer), ">abc def ghi");
    assert_eq!(stream.push(&mut buffer, "two"), Err(StreamError::Undone));
    assert_eq!(buffer.redo(|o| o.is_ai()), Ok(Some(id)));
    assert_eq!(text(&buffer), ">abc one ghi");

    // A user edit inside the streamed text blocks undoing the AI, and the
    // stream carries on at the region's end.
    let mut buffer = Buffer::new("abc def ghi");
    let mut stream = EditStream::begin(&buffer, 4..7, ai());
    stream.push(&mut buffer, "one ").unwrap();
    stream.push(&mut buffer, "two").unwrap();
    let id = stream.transaction().unwrap();
    let w = find(&buffer, "w");
    user_replace(&mut buffer, w..w + 1, "W");
    assert_eq!(text(&buffer), "abc one tWo ghi");
    assert_eq!(buffer.undo(|o| o.is_ai()), Err(UndoError::Conflict(id)));
    assert_eq!(text(&buffer), "abc one tWo ghi");
    stream.push(&mut buffer, " three").unwrap();
    assert_eq!(text(&buffer), "abc one tWo three ghi");
    assert_eq!(stream.range(buffer.current()), 4..17);
}

#[test]
fn finish_keeps_and_crlf() {
    // Finishing mid-stream keeps what landed, as one AI transaction.
    let mut buffer = Buffer::new("let x = old;");
    let mut stream = EditStream::begin(&buffer, 8..11, ai());
    stream.push(&mut buffer, "comp").unwrap();
    user(&mut buffer, 0, "// ");
    stream.push(&mut buffer, "ute(").unwrap();
    let id = stream.transaction().unwrap();
    assert_eq!(stream.finish(&mut buffer), Some(id));
    assert_eq!(text(&buffer), "// let x = compute(;");
    assert_eq!(buffer.undo(|o| o.is_ai()), Ok(Some(id)));
    assert_eq!(text(&buffer), "// let x = old;");
    assert_eq!(buffer.undo(|o| o.is_ai()), Ok(None));

    // Dropping a stream keeps what landed too.
    let mut buffer = Buffer::new("old");
    let mut stream = EditStream::begin(&buffer, 0..3, ai());
    stream.push(&mut buffer, "new").unwrap();
    drop(stream);
    assert_eq!(text(&buffer), "new");
    buffer.undo(|o| o.is_ai()).unwrap();
    assert_eq!(text(&buffer), "old");

    // A \r\n split across two chunks becomes one \n.
    let mut buffer = Buffer::new("[]");
    let mut stream = EditStream::begin(&buffer, 1..1, ai());
    stream.push(&mut buffer, "a\r").unwrap();
    assert_eq!(text(&buffer), "[a]");
    assert_eq!(stream.range(buffer.current()), 1..2);
    stream.push(&mut buffer, "\nb\r\nc\r").unwrap();
    assert_eq!(text(&buffer), "[a\nb\nc]");
    // A lone \r held at the end is flushed by finish, as a \n.
    let id = stream.transaction().unwrap();
    assert_eq!(stream.finish(&mut buffer), Some(id));
    assert_eq!(text(&buffer), "[a\nb\nc\n]");
    buffer.undo(|o| o.is_ai()).unwrap();
    assert_eq!(text(&buffer), "[]");

    // A stream of only a held \r lands it on finish, over the target.
    let mut buffer = Buffer::new("xoldx");
    let mut stream = EditStream::begin(&buffer, 1..4, ai());
    stream.push(&mut buffer, "\r").unwrap();
    assert_eq!(text(&buffer), "xoldx");
    assert_eq!(stream.transaction(), None);
    assert!(stream.finish(&mut buffer).is_some());
    assert_eq!(text(&buffer), "x\nx");
}

#[derive(Clone, Debug)]
enum Op {
    /// Inserts into the text before the region (`after == false`) or after
    /// it, at either end too, so at the region's edges.
    Insert {
        after: bool,
        at: Index,
        text: String,
    },
    Delete {
        after: bool,
        from: Index,
        to: Index,
    },
    Push(String),
    StartTransaction,
    EndTransaction,
}

fn user_text() -> impl Strategy<Value = String> {
    proptest::collection::vec(
        prop_oneof![
            4 => proptest::char::range('a', 'e'),
            1 => Just('\n'),
            1 => Just('é'),
        ],
        0..6,
    )
    .prop_map(|chars| chars.into_iter().collect())
}

fn chunk() -> impl Strategy<Value = String> {
    proptest::collection::vec(
        prop_oneof![
            4 => proptest::char::range('A', 'E'),
            1 => Just('\n'),
            1 => Just('\r'),
            1 => Just('😀'),
        ],
        0..6,
    )
    .prop_map(|chars| chars.into_iter().collect())
}

fn op() -> impl Strategy<Value = Op> {
    prop_oneof![
        3 => (any::<bool>(), any::<Index>(), user_text())
            .prop_map(|(after, at, text)| Op::Insert { after, at, text }),
        2 => (any::<bool>(), any::<Index>(), any::<Index>())
            .prop_map(|(after, from, to)| Op::Delete { after, from, to }),
        3 => chunk().prop_map(Op::Push),
        1 => Just(Op::StartTransaction),
        1 => Just(Op::EndTransaction),
    ]
}

/// A char boundary of `s`, ends included.
fn boundary(s: &str, index: &Index) -> usize {
    let boundaries: Vec<usize> = s
        .char_indices()
        .map(|(ix, _)| ix)
        .chain([s.len()])
        .collect();
    boundaries[index.index(boundaries.len())]
}

fn normalize(s: &str) -> String {
    s.replace("\r\n", "\n").replace('\r', "\n")
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 64, ..ProptestConfig::default() })]

    /// The buffer is `before + region + after`, where the user edits only
    /// `before` and `after` and the region is the old target text until a
    /// chunk lands, then the chunks so far.
    #[test]
    fn interleaved_matches_model(
        mut before in user_text(),
        target in user_text(),
        mut after in user_text(),
        ops in proptest::collection::vec(op(), 0..40),
    ) {
        let mut buffer = Buffer::new(&format!("{before}{target}{after}"));
        let start = before.len();
        let mut stream = EditStream::begin(&buffer, start..start + target.len(), ai());
        let mut streamed = String::new();
        let mut open = false;
        let region = |streamed: &str| {
            let landed = normalize(streamed.strip_suffix('\r').unwrap_or(streamed));
            if landed.is_empty() { target.clone() } else { landed }
        };
        for op in ops {
            let mid = region(&streamed);
            match op {
                Op::Insert { after: mut in_after, at, text } => {
                    let mut at = boundary(if in_after { &after } else { &before }, &at);
                    // At an empty region the two edges meet, and the
                    // insertion goes before the region.
                    if in_after && at == 0 && mid.is_empty() {
                        in_after = false;
                        at = before.len();
                    }
                    let offset = if in_after {
                        after.insert_str(at, &text);
                        before.len() + mid.len() + at
                    } else {
                        before.insert_str(at, &text);
                        at
                    };
                    buffer.edit([(offset..offset, text.as_str())], Origin::User);
                }
                Op::Delete { after: in_after, from, to } => {
                    let side = if in_after { &mut after } else { &mut before };
                    let (a, b) = (boundary(side, &from), boundary(side, &to));
                    let range = a.min(b)..a.max(b);
                    side.replace_range(range.clone(), "");
                    let shift = if in_after { before.len() + mid.len() } else { 0 };
                    buffer.edit([(range.start + shift..range.end + shift, "")], Origin::User);
                }
                Op::Push(chunk) => {
                    stream.push(&mut buffer, &chunk).unwrap();
                    streamed.push_str(&chunk);
                }
                Op::StartTransaction => {
                    if !open {
                        buffer.start_transaction(Origin::User);
                        open = true;
                    }
                }
                Op::EndTransaction => {
                    if open {
                        buffer.end_transaction();
                        open = false;
                    }
                }
            }
            let mid = region(&streamed);
            prop_assert_eq!(text(&buffer), format!("{before}{mid}{after}"));
            prop_assert_eq!(
                stream.range(buffer.current()),
                before.len()..before.len() + mid.len()
            );
        }
        let id = stream.transaction();
        prop_assert_eq!(buffer.undo(|o| o.is_ai()), Ok(id));
        prop_assert_eq!(text(&buffer), format!("{before}{target}{after}"));
    }
}
