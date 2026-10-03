use std::{collections::HashMap, ops::Range};

use proptest::prelude::*;
use rand::{Rng, SeedableRng, rngs::StdRng};

use crate::*;

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
    while total < 100_000 {
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
    // 256 cases by default; set PROPTEST_CASES for longer runs.
    #![proptest_config(ProptestConfig::with_cases(
        std::env::var("PROPTEST_CASES").ok().and_then(|cases| cases.parse().ok()).unwrap_or(256)
    ))]

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

    #[test]
    fn undo_and_redo_round_trip(source in ascii_text(), seed in any::<u64>()) {
        let mut rng = StdRng::seed_from_u64(seed);
        let mut buffer = Buffer::new(&source);
        let mut states = vec![source.clone()];
        for _ in 0..rng.random_range(1..15) {
            let grouped = rng.random_bool(0.3);
            if grouped {
                buffer.start_transaction(Origin::User);
            }
            let mut recorded = false;
            for _ in 0..if grouped { 3 } else { 1 } {
                let text = buffer.text().to_string();
                let edits: Vec<(Range<usize>, String)> = disjoint_ranges(&mut rng, &text, 2)
                    .into_iter()
                    .map(|range| (range, random_insert(&mut rng)))
                    .collect();
                recorded |= buffer.edit(edits, Origin::User).is_some();
            }
            if grouped {
                recorded = buffer.end_transaction().is_some();
            }
            // One state per transaction, even if it left the text unchanged.
            if recorded {
                states.push(buffer.text().to_string());
            }
        }

        for expected in states.iter().rev().skip(1) {
            prop_assert!(buffer.undo(is_user).unwrap().is_some());
            prop_assert_eq!(&buffer.text().to_string(), expected);
        }
        prop_assert_eq!(buffer.undo(is_user), Ok(None));
        for expected in states.iter().skip(1) {
            prop_assert!(buffer.redo(is_user).unwrap().is_some());
            prop_assert_eq!(&buffer.text().to_string(), expected);
        }
        prop_assert_eq!(buffer.redo(is_user), Ok(None));
    }

    #[test]
    fn interleaved_undo_never_corrupts(source in ascii_text(), seed in any::<u64>()) {
        let mut rng = StdRng::seed_from_u64(seed);
        let mut buffer = Buffer::new(&source);
        let mut anchors = Vec::new();
        for _ in 0..40 {
            let before = buffer.text().to_string();
            let origin_filter: fn(&Origin) -> bool = if rng.random_bool(0.5) { is_user } else { is_ai };
            match rng.random_range(0..6) {
                0..=2 => {
                    let origin = if rng.random_bool(0.5) { Origin::User } else { ai() };
                    let edits: Vec<(Range<usize>, String)> = disjoint_ranges(&mut rng, &before, 2)
                        .into_iter()
                        .map(|range| (range, random_insert(&mut rng)))
                        .collect();
                    buffer.edit(edits, origin);
                }
                3 | 4 => match buffer.undo(origin_filter) {
                    Ok(Some(_)) => {
                        // An undo can always be redone straight away, exactly.
                        let undone = buffer.text().to_string();
                        prop_assert!(buffer.redo(origin_filter).unwrap().is_some());
                        prop_assert_eq!(buffer.text().to_string(), before.clone());
                        prop_assert!(buffer.undo(origin_filter).unwrap().is_some());
                        prop_assert_eq!(buffer.text().to_string(), undone);
                    }
                    Ok(None) => prop_assert_eq!(buffer.text().to_string(), before.clone()),
                    Err(_) => prop_assert_eq!(buffer.text().to_string(), before.clone(), "failed undo changed text"),
                },
                _ => {
                    if buffer.redo(origin_filter).is_err() {
                        prop_assert_eq!(buffer.text().to_string(), before.clone(), "failed redo changed text");
                    }
                }
            }
            let offset = rng.random_range(0..=buffer.len());
            anchors.push(buffer.current().anchor_at(offset, Bias::Right));
            let snapshot = buffer.snapshot();
            for anchor in &anchors {
                let offset = snapshot.resolve(anchor);
                prop_assert!(offset <= snapshot.len());
                prop_assert!(snapshot.text().is_char_boundary(offset));
            }
        }
    }

    #[test]
    fn undoing_ai_keeps_user_edits(source in ascii_text(), seed in any::<u64>()) {
        let mut rng = StdRng::seed_from_u64(seed);
        let mut buffer = Buffer::new(&source);
        // Each char tagged with whether the AI wrote it.
        let mut model: Vec<(char, bool)> = source.chars().map(|ch| (ch, false)).collect();

        for _ in 0..rng.random_range(1..20) {
            let len = model.len();
            if rng.random_bool(0.4) {
                // AI inserts somewhere.
                let at = rng.random_range(0..=len);
                buffer.edit([(at..at, "AI")], ai());
                model.splice(at..at, [('A', true), ('I', true)]);
            } else {
                // The user edits around, but not inside, AI text.
                let start = rng.random_range(0..=len);
                let end = (start + rng.random_range(0..4)).min(len);
                let touches_ai = model[start..end].iter().any(|(_, by_ai)| *by_ai);
                let splits_ai = start > 0 && start < len && model[start - 1].1 && model[start].1;
                if touches_ai || splits_ai {
                    continue;
                }
                buffer.edit([(start..end, "u")], Origin::User);
                model.splice(start..end, [('u', false)]);
            }
            prop_assert_eq!(
                buffer.text().to_string(),
                model.iter().map(|(ch, _)| *ch).collect::<String>()
            );
        }

        while buffer.undo(is_ai).expect("AI undo must not conflict").is_some() {}
        let expected: String = model.iter().filter(|(_, by_ai)| !by_ai).map(|(ch, _)| *ch).collect();
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
    // Restored text goes before text inserted at the same point since.
    assert_eq!(buffer.text().to_string(), "bc");
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
    for step in 0..500 {
        let anchor = buffer.current().anchor_after(buffer.len());
        anchors.insert(step, (anchor, buffer.len()));
        buffer.edit([(0..0, "ab")], Origin::User);
        for (anchor, expected) in anchors.values_mut() {
            *expected += 2;
            assert_eq!(buffer.current().resolve(anchor), *expected);
        }
    }
}
