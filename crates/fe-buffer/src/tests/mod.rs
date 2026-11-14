use std::{collections::HashMap, ops::Range};

use proptest::prelude::*;
use rand::{Rng, SeedableRng, rngs::StdRng};

use crate::*;

mod detached_hunks;
mod history_cap;
mod ideal;
mod invariants;
mod log_compaction;

fn ai() -> Origin {
    Origin::Ai {
        agent: "agent".into(),
        session: "s1".into(),
    }
}

fn is_user(origin: &Origin) -> bool {
    *origin == Origin::User
}

fn is_ai(origin: &Origin) -> bool {
    origin.is_ai()
}

/// Random char-boundary range in `text`.
fn random_range(rng: &mut impl Rng, text: &str) -> Range<usize> {
    let clip = |ix: usize| {
        let mut ix = ix.min(text.len());
        while !text.is_char_boundary(ix) {
            ix -= 1;
        }
        ix
    };
    let a = clip(rng.random_range(0..=text.len()));
    let len = if rng.random_bool(0.5) {
        0
    } else {
        rng.random_range(0..12)
    };
    let b = clip(a + len);
    a..b
}

fn random_insert(rng: &mut impl Rng) -> String {
    const PIECES: &[&str] = &["a", "bc", "\n", "é", "中", "😀", "xyz\n", ""];
    (0..rng.random_range(0..4))
        .map(|_| PIECES[rng.random_range(0..PIECES.len())])
        .collect()
}

/// Applies edits given in pre-edit coordinates to a string, back to front.
fn apply_to_string(text: &mut String, edits: &[(Range<usize>, String)]) {
    let mut sorted: Vec<_> = edits.to_vec();
    sorted.sort_by_key(|(range, _)| range.start);
    for (range, new_text) in sorted.into_iter().rev() {
        text.replace_range(range, &new_text);
    }
}

/// Picks up to `count` disjoint, non-touching ranges.
fn disjoint_ranges(rng: &mut impl Rng, text: &str, count: usize) -> Vec<Range<usize>> {
    let mut ranges: Vec<Range<usize>> = Vec::new();
    for _ in 0..count * 4 {
        if ranges.len() == count {
            break;
        }
        let range = random_range(rng, text);
        let clear = ranges
            .iter()
            .all(|other| range.end + 1 < other.start || other.end + 1 < range.start);
        if clear {
            ranges.push(range);
        }
    }
    ranges
}

#[test]
fn random_edits_match_string_model() {
    let mut rng = StdRng::seed_from_u64(0xfa57_ed17);
    let mut model = String::from("fn main() {\n    println!(\"héllo 😀\");\n}\n");
    let mut buffer = Buffer::new(&model);
    let mut total = 0;
    while total < 10_000 {
        let count = rng.random_range(1..4);
        let edits: Vec<(Range<usize>, String)> = disjoint_ranges(&mut rng, &model, count)
            .into_iter()
            .map(|range| (range, random_insert(&mut rng)))
            .collect();
        total += edits.len();
        apply_to_string(&mut model, &edits);
        buffer.edit(edits, Origin::User);
        if total % 1_000 < 4 {
            assert_eq!(buffer.text().to_string(), model);
        }
    }
    assert_eq!(buffer.text().to_string(), model);
}

#[test]
fn batch_ranges_are_pre_edit_coordinates() {
    let mut buffer = Buffer::new("abcdef");
    buffer.edit([(1..2, "XX"), (4..5, "")], Origin::User);
    assert_eq!(buffer.text().to_string(), "aXXcdf");
    let snapshot = buffer.snapshot();
    let edits = snapshot.edits_since(Version::default()).next().unwrap();
    assert_eq!(
        **edits,
        [
            Edit {
                old: 1..2,
                new: 1..3
            },
            Edit {
                old: 4..5,
                new: 5..5
            },
        ]
    );
}

#[test]
fn carriage_returns_are_normalised() {
    let mut buffer = Buffer::from_file_contents("a\r\nb\r\n");
    assert_eq!(buffer.line_ending(), LineEnding::CrLf);
    buffer.edit([(1..1, "\r\nc")], Origin::User);
    assert_eq!(buffer.text().to_string(), "a\nc\nb\n");
    let mut saved = Vec::new();
    buffer.write_to(&mut saved).unwrap();
    assert_eq!(saved, b"a\r\nc\r\nb\r\n");
}

/// A model where every char has an id, so we can check that anchors stay
/// attached to the character on their biased side for as long as it exists.
#[derive(Clone)]
struct IdText {
    chars: Vec<(char, u32)>,
    next_id: u32,
}

impl IdText {
    fn new(text: &str) -> Self {
        let chars: Vec<_> = text.chars().zip(0..).collect();
        let next_id = chars.len() as u32;
        Self { chars, next_id }
    }

    fn apply(&mut self, edits: &[Edit], text: &Text) {
        // ASCII only, so byte offsets are char indices.
        for edit in edits.iter().rev() {
            // Within one batch, `edit.new` is already in post-batch coordinates.
            let inserted: Vec<_> = text
                .slice_to_string(edit.new.clone())
                .chars()
                .map(|ch| {
                    self.next_id += 1;
                    (ch, self.next_id)
                })
                .collect();
            self.chars.splice(edit.old.clone(), inserted);
        }
    }

    fn index_of(&self, id: u32) -> Option<usize> {
        self.chars.iter().position(|(_, other)| *other == id)
    }
}

fn ascii_text() -> impl Strategy<Value = String> {
    proptest::collection::vec(proptest::char::range('a', 'e'), 0..60)
        .prop_map(|chars| chars.into_iter().collect())
}

proptest! {
    #[test]
    fn anchors_follow_their_characters(source in ascii_text(), seed in any::<u64>()) {
        let mut rng = StdRng::seed_from_u64(seed);
        let mut buffer = Buffer::new(&source);
        let mut model = IdText::new(&source);
        let events = buffer.subscribe();

        // (anchor, the id it's attached to, refreshed copy)
        let snapshot = buffer.snapshot();
        let mut anchors = Vec::new();
        for _ in 0..20 {
            let offset = rng.random_range(0..=source.len());
            let bias = if rng.random_bool(0.5) { Bias::Left } else { Bias::Right };
            let attached = match bias {
                Bias::Left => offset.checked_sub(1).map(|ix| model.chars[ix].1),
                Bias::Right => model.chars.get(offset).map(|(_, id)| *id),
            };
            let anchor = snapshot.anchor_at(offset, bias);
            anchors.push((anchor, attached, anchor));
        }

        for _ in 0..rng.random_range(1..12) {
            let text = buffer.text().to_string();
            let count = rng.random_range(1..3);
            let edits: Vec<(Range<usize>, String)> = disjoint_ranges(&mut rng, &text, count)
                .into_iter()
                .map(|range| {
                    let len = rng.random_range(0..3);
                    (range, "xyz"[..len].to_string())
                })
                .collect();
            buffer.edit(edits, Origin::User);
            for event in events.drain() {
                if let BufferEvent::Edited { edits, .. } = event {
                    model.apply(&edits, buffer.text());
                }
            }
            for (_, _, refreshed) in &mut anchors {
                *refreshed = buffer.current().refresh(refreshed);
            }
        }

        let snapshot = buffer.snapshot();
        prop_assert_eq!(
            snapshot.text().to_string(),
            model.chars.iter().map(|(ch, _)| *ch).collect::<String>()
        );
        let all: Vec<Anchor> = anchors.iter().map(|(anchor, _, _)| *anchor).collect();
        let batch = snapshot.resolve_many(&all);
        let mut resolved = Vec::new();
        for (ix, (anchor, attached, refreshed)) in anchors.iter().enumerate() {
            prop_assert_eq!(batch[ix], snapshot.resolve(anchor), "resolve_many must match resolve");
            let offset = snapshot.resolve(anchor);
            prop_assert_eq!(offset, snapshot.resolve(refreshed), "refreshing must not move an anchor");
            if let Some(ix) = attached.and_then(|id| model.index_of(id)) {
                let expected = match anchor.bias() {
                    Bias::Left => ix + 1,
                    Bias::Right => ix,
                };
                prop_assert_eq!(offset, expected);
            }
            resolved.push((anchor, offset));
        }
        // Order is preserved among anchors with the same bias. (With mixed
        // biases, anchors strictly inside a replaced range can cross.)
        for (a, a_offset) in &resolved {
            for (b, b_offset) in &resolved {
                if a.bias == b.bias && a.offset < b.offset {
                    prop_assert!(a_offset <= b_offset);
                }
            }
        }
    }

    #[test]
    fn edits_from_old_snapshots_rebase(source in ascii_text(), seed in any::<u64>()) {
        let mut rng = StdRng::seed_from_u64(seed);
        let mut buffer = Buffer::new(&source);
        let base = buffer.snapshot();

        // Disjoint, non-touching edits against the same base, applied as
        // several separate batches in random order. Since they don't
        // interfere, the result must equal applying them all at once.
        let all: Vec<(Range<usize>, String)> = disjoint_ranges(&mut rng, &source, 6)
            .into_iter()
            .map(|range| (range, ["", "Q", "RS\n"][rng.random_range(0..3)].to_string()))
            .collect();
        let mut expected = source.clone();
        apply_to_string(&mut expected, &all);

        let mut pending = all.clone();
        while !pending.is_empty() {
            let take = rng.random_range(1..=pending.len());
            let batch: Vec<_> = pending.drain(..take).collect();
            let origin = if rng.random_bool(0.5) { Origin::User } else { ai() };
            prop_assert!(buffer.edit_from(&base, batch, origin).is_ok());
        }
        prop_assert_eq!(buffer.text().to_string(), expected);
    }

}

#[test]
fn edit_from_reports_conflicts() {
    let mut buffer = Buffer::new("let x = 1;\n");
    let base = buffer.snapshot();
    // The user edits inside the range the AI wants to replace.
    buffer.edit([(8..9, "2")], Origin::User);
    let result = buffer.edit_from(&base, [(4..9, "y = 3")], ai());
    assert_eq!(result, Err(EditError::Conflict { index: 0 }));
    assert_eq!(buffer.text().to_string(), "let x = 2;\n");

    // Typing at the edges of the range is kept.
    let mut buffer = Buffer::new("abc");
    let base = buffer.snapshot();
    buffer.edit([(1..1, "<"), (2..2, ">")], Origin::User);
    buffer.edit_from(&base, [(1..2, "B")], ai()).unwrap();
    assert_eq!(buffer.text().to_string(), "a<B>c");

    let other = Buffer::new("abc").snapshot();
    assert_eq!(
        buffer.edit_from(&other, [(0..1, "z")], ai()),
        Err(EditError::ForeignSnapshot)
    );
}

#[test]
fn undo_conflicts_instead_of_clobbering() {
    let mut buffer = Buffer::new("fn f() {}\n");
    buffer.edit([(8..8, "todo!()")], ai());
    // The user edits inside the AI's text.
    buffer.edit([(12..14, "")], Origin::User);
    let ai_id = match buffer.undo(is_ai) {
        Err(UndoError::Conflict(id)) => id,
        other => panic!("expected a conflict, got {other:?}"),
    };
    assert_eq!(
        buffer.text().to_string(),
        "fn f() {todo)}\n",
        "nothing applied"
    );
    // Undoing the user's edit first makes the AI's undoable again.
    buffer.undo(is_user).unwrap();
    assert_eq!(buffer.undo(is_ai), Ok(Some(ai_id)));
    assert_eq!(buffer.text().to_string(), "fn f() {}\n");
}

#[test]
fn new_edits_discard_only_their_origins_redo() {
    let mut buffer = Buffer::new("");
    buffer.edit([(0..0, "a")], Origin::User);
    buffer.edit([(1..1, "b")], ai());
    buffer.undo(is_user).unwrap();
    buffer.undo(is_ai).unwrap();
    assert_eq!(buffer.text().to_string(), "");
    buffer.edit([(0..0, "c")], Origin::User);
    assert_eq!(buffer.redo(is_user), Ok(None));
    assert!(buffer.redo(is_ai).unwrap().is_some());
    // Restored text goes after text inserted at the same point since
    // (matching the reference model in `ideal.rs`).
    assert_eq!(buffer.text().to_string(), "cb");
}

#[test]
fn transactions_group_and_nest() {
    let mut buffer = Buffer::new("");
    let id = buffer.start_transaction(Origin::User);
    buffer.edit([(0..0, "a")], Origin::User);
    assert_eq!(buffer.start_transaction(Origin::User), id);
    buffer.edit([(1..1, "b")], Origin::User);
    assert_eq!(buffer.end_transaction(), None);
    // An AI edit in the middle gets its own transaction.
    buffer.edit([(2..2, "!")], ai());
    buffer.edit([(2..2, "c")], Origin::User);
    assert_eq!(buffer.end_transaction(), Some(id));
    assert_eq!(buffer.text().to_string(), "abc!");

    buffer.undo(is_user).unwrap();
    assert_eq!(buffer.text().to_string(), "!");

    // An empty transaction leaves no history.
    buffer.start_transaction(Origin::User);
    assert_eq!(buffer.end_transaction(), None);
    buffer.redo(is_user).unwrap();
    assert_eq!(buffer.text().to_string(), "abc!");
}

#[test]
fn events_describe_each_batch() {
    let mut buffer = Buffer::new("hello");
    let events = buffer.subscribe();
    let id = buffer.edit([(0..1, "J")], Origin::User).unwrap();
    buffer.undo(is_user).unwrap();
    buffer.did_save();

    let events = events.drain();
    assert_eq!(events.len(), 3);
    match &events[0] {
        BufferEvent::Edited {
            transaction,
            origin,
            kind,
            from,
            to,
            edits,
            ..
        } => {
            assert_eq!(*transaction, id);
            assert_eq!(*origin, Origin::User);
            assert_eq!(*kind, EditKind::Edit);
            assert!(from < to);
            assert_eq!(
                **edits,
                [Edit {
                    old: 0..1,
                    new: 0..1
                }]
            );
        }
        other => panic!("unexpected {other:?}"),
    }
    assert!(matches!(
        &events[1],
        BufferEvent::Edited {
            kind: EditKind::Undo,
            ..
        }
    ));
    assert!(matches!(&events[2], BufferEvent::Saved { .. }));
    assert!(!buffer.is_dirty());
}

#[test]
fn dropped_subscriptions_are_pruned() {
    let mut buffer = Buffer::new("");
    drop(buffer.subscribe());
    buffer.edit([(0..0, "a")], Origin::User);
    assert!(buffer.subscribers.is_empty());
}

#[test]
fn selections_follow_concurrent_edits() {
    let mut buffer = Buffer::new("one two three");
    let snapshot = buffer.snapshot();
    let selections = SelectionSet::from_resolved(
        &snapshot,
        [
            ResolvedSelection {
                range: 4..7,
                reversed: false,
            },
            ResolvedSelection {
                range: 13..13,
                reversed: false,
            },
            ResolvedSelection {
                range: 5..9,
                reversed: true,
            },
        ],
    );
    assert_eq!(selections.len(), 2, "overlapping selections merge");

    // Text inserted at a selection's edges doesn't join it.
    buffer.edit([(4..4, "[["), (9..9, "]]"), (0..0, ">> ")], ai());
    let resolved = selections.resolve(buffer.current());
    let text = buffer.text().to_string();
    assert_eq!(text, ">> one [[two t]]hree");
    assert_eq!(&text[resolved[0].range.clone()], "two t");
    assert_eq!(resolved[1].range, 20..20);
}

#[test]
fn snapshots_are_isolated_and_sendable() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<BufferSnapshot>();
    assert_send_sync::<Subscription>();

    let mut buffer = Buffer::new("abc");
    let before = buffer.snapshot();
    let anchor = before.anchor_after(1);
    buffer.edit([(0..0, "xyz")], Origin::User);
    assert_eq!(before.text().to_string(), "abc");
    assert_eq!(before.resolve(&anchor), 1);
    assert_eq!(buffer.current().resolve(&anchor), 4);
    let handle = std::thread::spawn(move || before.text().len());
    assert_eq!(handle.join().unwrap(), 3);
}

#[test]
fn anchor_resolution_is_consistent_across_many_versions() {
    // Anchors created at different versions all agree with a model of
    // offsets shifted by a simple insertion at the start each step.
    let mut buffer = Buffer::new("x");
    let mut anchors: HashMap<usize, (Anchor, usize)> = HashMap::new();
    for step in 0..100 {
        let anchor = buffer.current().anchor_after(buffer.len());
        anchors.insert(step, (anchor, buffer.len()));
        buffer.edit([(0..0, "ab")], Origin::User);
        for (anchor, expected) in anchors.values_mut() {
            *expected += 2;
            assert_eq!(buffer.current().resolve(anchor), *expected);
        }
    }
}

#[test]
fn accessors_and_small_apis() {
    let mut buffer = Buffer::from_reader("one\r\ntwo".as_bytes()).unwrap();
    assert_eq!(buffer.text().to_string(), "one\ntwo");
    assert_eq!(buffer.line_ending(), LineEnding::CrLf);
    buffer.set_line_ending(LineEnding::Lf);
    let mut saved = Vec::new();
    buffer.write_to(&mut saved).unwrap();
    assert_eq!(saved, b"one\ntwo");
    assert!(!buffer.is_empty());

    let snapshot = buffer.snapshot();
    assert_eq!(snapshot.id(), buffer.id());
    assert!(!snapshot.is_empty());
    let anchor = snapshot.anchor_after(2);
    assert_eq!(anchor.version(), snapshot.version());
    assert_eq!(anchor.bias(), Bias::Right);
    let debug = format!("{snapshot:?}");
    assert!(
        debug.contains("BufferSnapshot") && debug.contains("len: 7"),
        "{debug}"
    );

    // A backwards range is the same as the forwards one.
    #[allow(clippy::reversed_empty_ranges)]
    let backwards = 3..1;
    buffer.edit([(backwards, "X")], Origin::User);
    assert_eq!(buffer.text().to_string(), "oX\ntwo");

    let selection = ResolvedSelection {
        range: 1..4,
        reversed: true,
    };
    assert_eq!((selection.head(), selection.tail()), (1, 4));
    let set = SelectionSet::caret(buffer.current(), 0);
    assert!(!set.is_empty());
    assert_eq!(set.iter().count(), 1);
}

#[test]
#[should_panic(expected = "newer than snapshot")]
fn resolving_a_newer_anchor_panics() {
    let mut buffer = Buffer::new("abc");
    let old = buffer.snapshot();
    buffer.edit([(0..0, "x")], Origin::User);
    let anchor = buffer.current().anchor_after(1);
    old.resolve(&anchor);
}

#[test]
fn dirty_tracking() {
    let mut buffer = Buffer::new("abc");
    assert!(!buffer.is_dirty());
    buffer.edit([(0..0, "x")], Origin::User);
    assert!(buffer.is_dirty());
    buffer.did_save();
    assert!(!buffer.is_dirty());
    buffer.undo(|_| true).unwrap();
    assert!(
        buffer.is_dirty(),
        "undoing past a save makes the buffer dirty again"
    );
}
