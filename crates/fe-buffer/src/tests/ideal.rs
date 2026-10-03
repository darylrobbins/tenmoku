//! Differential tests against a reference model of selective undo.
//!
//! The reference never removes characters. Each has an id, the transaction
//! that inserted it and the transactions that deleted it; it is visible while
//! its inserter is active and none of its deleters are. Undo and redo just
//! flip a transaction's active flag. That makes it slow and obviously right
//! about *what* text an undo should produce, independent of how the buffer
//! maps positions through later edits.
//!
//! The buffer must either refuse an undo or redo with a conflict and change
//! nothing, or produce text that:
//! - contains exactly the reference's characters (inserted characters are
//!   all distinct, so this pins down precisely what was removed and
//!   restored); and
//! - never reorders characters that were ever visible together.
//!
//! Beyond that, order is a tie-break: text restored where other transactions
//! have since inserted text could sensibly go on either side of it. Where
//! there's no such ambiguity (a single origin, undone in order) the buffer
//! must match the reference exactly. The reference's own tie-break puts new
//! text right after the visible character before it, and a replacement's new
//! text right after what it replaced.

use std::{collections::HashSet, fmt::Write as _, ops::Range};

use proptest::prelude::*;
use rand::{Rng, SeedableRng, rngs::StdRng};

use crate::*;

#[derive(Clone)]
struct IdealChar {
    ch: char,
    inserted_by: Option<TransactionId>,
    deleted_by: Vec<TransactionId>,
}

#[derive(Default)]
struct Ideal {
    /// Every character ever present, in document order.
    chars: Vec<IdealChar>,
    undone: HashSet<TransactionId>,
}

impl Ideal {
    fn new(text: &str) -> Self {
        Self {
            chars: text
                .chars()
                .map(|ch| IdealChar {
                    ch,
                    inserted_by: None,
                    deleted_by: Vec::new(),
                })
                .collect(),
            undone: Default::default(),
        }
    }

    fn active(&self, transaction: TransactionId) -> bool {
        !self.undone.contains(&transaction)
    }

    fn visible(&self, ch: &IdealChar) -> bool {
        ch.inserted_by.is_none_or(|t| self.active(t))
            && ch.deleted_by.iter().all(|&t| !self.active(t))
    }

    fn text(&self) -> String {
        self.chars
            .iter()
            .filter(|ch| self.visible(ch))
            .map(|ch| ch.ch)
            .collect()
    }

    /// Applies one batch, given as in `Buffer::edit` against `buffer_text`
    /// (the buffer's text before the batch). Edits are translated by
    /// character identity, which characters go and which character new text
    /// follows, because in mixed sessions the reference's order may
    /// legitimately differ from the buffer's.
    fn edit(
        &mut self,
        transaction: TransactionId,
        buffer_text: &str,
        edits: &[(Range<usize>, String)],
    ) {
        for (range, new_text) in edits.iter().rev() {
            let deleted: Vec<char> = buffer_text[range.clone()].chars().collect();
            for ch in &deleted {
                let ix = self.visible_index_of(*ch);
                self.chars[ix].deleted_by.push(transaction);
            }
            // New text goes right after the character it follows: the last
            // one it replaces, or else the visible one before it.
            let after = deleted
                .last()
                .copied()
                .or_else(|| buffer_text[..range.start].chars().last());
            let at = after.map_or(0, |ch| self.index_of(ch) + 1);
            let new_chars = new_text.chars().map(|ch| IdealChar {
                ch,
                inserted_by: Some(transaction),
                deleted_by: Vec::new(),
            });
            self.chars.splice(at..at, new_chars);
        }
    }

    /// Whether undoing (or redoing) `transaction` only has to touch its own
    /// text: everything it wrote (or, for redo, everything it deleted) is
    /// still there, unchanged and unsplit by text from other transactions.
    fn intact(&self, transaction: TransactionId, redo: bool) -> bool {
        let mine = |ch: &IdealChar| {
            if redo {
                ch.deleted_by.contains(&transaction)
            } else {
                ch.inserted_by == Some(transaction)
            }
        };
        let visible: Vec<&IdealChar> = self.chars.iter().filter(|ch| self.visible(ch)).collect();
        let positions: Vec<usize> = (0..visible.len()).filter(|&ix| mine(visible[ix])).collect();
        let total = self.chars.iter().filter(|ch| mine(ch)).count();
        positions.len() == total && positions.windows(2).all(|pair| pair[1] == pair[0] + 1)
    }

    fn index_of(&self, ch: char) -> usize {
        self.chars
            .iter()
            .position(|c| c.ch == ch)
            .expect("characters are unique")
    }

    fn visible_index_of(&self, ch: char) -> usize {
        let ix = self.index_of(ch);
        assert!(
            self.visible(&self.chars[ix]),
            "buffer deleted a character the reference hides"
        );
        ix
    }
}

/// Hands out characters that never repeat, of 3 and 4 bytes.
struct UniqueChars(u32);

impl UniqueChars {
    fn next(&mut self, rng: &mut impl Rng) -> char {
        self.0 += 1;
        let base = if rng.random_bool(0.2) {
            0x2_0000
        } else {
            0x4E00
        };
        char::from_u32(base + self.0).expect("in a CJK block")
    }

    fn string(&mut self, rng: &mut impl Rng, len: usize) -> String {
        (0..len).map(|_| self.next(rng)).collect()
    }
}

fn origin(ix: usize) -> Origin {
    match ix {
        0 => Origin::User,
        1 => Origin::Ai {
            agent: "a".into(),
            session: "1".into(),
        },
        _ => Origin::Ai {
            agent: "b".into(),
            session: "2".into(),
        },
    }
}

type Filter = (&'static str, fn(&Origin) -> bool);

const FILTERS: [Filter; 3] = [
    ("user", |o| *o == Origin::User),
    ("ai", |o| o.is_ai()),
    ("any", |_| true),
];

/// Char boundaries of `text` worth editing at: random ones, plus the edges
/// of recent edits, where tie-break rules matter most.
fn pick_offset(rng: &mut impl Rng, text: &str, hot: &[usize]) -> usize {
    if !hot.is_empty() && rng.random_bool(0.5) {
        let offset = hot[rng.random_range(0..hot.len())].min(text.len());
        if text.is_char_boundary(offset) {
            return offset;
        }
    }
    let boundaries: Vec<usize> = text
        .char_indices()
        .map(|(ix, _)| ix)
        .chain([text.len()])
        .collect();
    boundaries[rng.random_range(0..boundaries.len())]
}

/// Sorted, non-overlapping edits (touching allowed, at most one insertion
/// per offset), with fresh unique text.
fn random_batch(
    rng: &mut impl Rng,
    text: &str,
    hot: &[usize],
    chars: &mut UniqueChars,
) -> Vec<(Range<usize>, String)> {
    let mut points: Vec<usize> = (0..rng.random_range(1..=4))
        .map(|_| pick_offset(rng, text, hot))
        .collect();
    points.sort_unstable();
    points.dedup();
    let mut edits = Vec::new();
    let mut ix = 0;
    while ix < points.len() {
        let start = points[ix];
        let (end, next) = if ix + 1 < points.len() && rng.random_bool(0.5) {
            (points[ix + 1], ix + 1)
        } else {
            (start, ix + 1)
        };
        let len = if start == end {
            rng.random_range(1..4)
        } else {
            rng.random_range(0..3)
        };
        let new_text = chars.string(rng, len);
        edits.push((start..end, new_text));
        ix = next;
    }
    edits
}

#[derive(Clone, Debug)]
enum Op {
    Edit(Origin, Vec<(Range<usize>, String)>),
    Start(Origin),
    End,
    Undo(usize),
    Redo(usize),
}

/// Clips replayed edits to `text` (removing earlier ops can shrink it) and
/// drops any that no longer fit the batch rules.
fn sanitize(text: &str, edits: &[(Range<usize>, String)]) -> Vec<(Range<usize>, String)> {
    let clip = |mut ix: usize| {
        ix = ix.min(text.len());
        while !text.is_char_boundary(ix) {
            ix -= 1;
        }
        ix
    };
    let mut out: Vec<(Range<usize>, String)> = Vec::new();
    for (range, new_text) in edits {
        let range = clip(range.start)..clip(range.end);
        let fits = out.last().is_none_or(|(last, _)| {
            range.start > last.end
                || (range.start == last.end && !(range.is_empty() && last.is_empty()))
        });
        if fits {
            out.push((range, new_text.clone()));
        }
    }
    out
}

#[derive(Default, Debug, Clone, Copy)]
struct Stats {
    attempts: usize,
    refused: usize,
    /// Refusals where the transaction's text was still intact, so a smarter
    /// buffer could have done it.
    avoidable: usize,
}

/// Runs `ops` against both the buffer and the reference. Returns undo/redo
/// statistics, or a description of the first divergence.
fn execute(initial: &str, ops: &[Op], single_origin: bool) -> Result<Stats, String> {
    let mut buffer = Buffer::new(initial);
    let mut ideal = Ideal::new(initial);
    let mut stats = Stats::default();
    // Every text the buffer has shown, to check that order is never broken.
    let mut history: Vec<String> = vec![initial.to_string()];
    let mut open = false;
    for (step, op) in ops.iter().enumerate() {
        let before = buffer.text().to_string();
        match op {
            Op::Edit(origin, edits) => {
                let edits = sanitize(&before, edits);
                if let Some(id) = buffer.edit(edits.clone(), origin.clone()) {
                    ideal.edit(id, &before, &edits);
                }
            }
            Op::Start(origin) => {
                if !open {
                    buffer.start_transaction(origin.clone());
                    open = true;
                }
            }
            Op::End => {
                buffer.end_transaction();
                open = false;
            }
            Op::Undo(filter) | Op::Redo(filter) => {
                open = false;
                let redo = matches!(op, Op::Redo(_));
                let filter = FILTERS[*filter].1;
                let result = if redo {
                    buffer.redo(filter)
                } else {
                    buffer.undo(filter)
                };
                if !matches!(result, Ok(None)) {
                    stats.attempts += 1;
                }
                match result {
                    Ok(Some(id)) if redo => {
                        ideal.undone.remove(&id);
                    }
                    Ok(Some(id)) => {
                        ideal.undone.insert(id);
                    }
                    Ok(None) => {}
                    Err(error) => {
                        if single_origin {
                            return Err(format!(
                                "step {step}: {op:?} refused in a single-origin session: {error}"
                            ));
                        }
                        if buffer.text().to_string() != before {
                            return Err(format!("step {step}: refused {op:?} changed the text"));
                        }
                        let UndoError::Conflict(id) = error;
                        stats.refused += 1;
                        if ideal.intact(id, redo) {
                            stats.avoidable += 1;
                        }
                    }
                }
            }
        }
        let (actual, expected) = (buffer.text().to_string(), ideal.text());
        let differ = |what: &str| {
            format!("step {step}: {what}\n  buffer:    {actual:?}\n  reference: {expected:?}")
        };
        if single_origin && actual != expected {
            return Err(differ("buffer and reference differ"));
        }
        let mut actual_chars: Vec<char> = actual.chars().collect();
        let mut expected_chars: Vec<char> = expected.chars().collect();
        actual_chars.sort_unstable();
        expected_chars.sort_unstable();
        if actual_chars != expected_chars {
            return Err(differ("buffer and reference have different characters"));
        }
        if let Some(earlier) = history
            .iter()
            .find(|earlier| !keeps_order(earlier, &actual))
        {
            return Err(differ(&format!(
                "characters were reordered relative to {earlier:?}"
            )));
        }
        history.push(actual);
    }
    Ok(stats)
}

/// Whether the characters of `now` that also appear in `earlier` are still
/// in `earlier`'s order. Characters are unique, so this is well defined.
fn keeps_order(earlier: &str, now: &str) -> bool {
    let rank: std::collections::HashMap<char, usize> = earlier
        .chars()
        .enumerate()
        .map(|(ix, ch)| (ch, ix))
        .collect();
    let ranks: Vec<usize> = now
        .chars()
        .filter_map(|ch| rank.get(&ch).copied())
        .collect();
    ranks.is_sorted()
}

/// Prints each step of a session (run a test with `--nocapture`).
#[allow(dead_code)]
fn trace(initial: &str, ops: &[Op]) {
    let mut buffer = Buffer::new(initial);
    let events = buffer.subscribe();
    eprintln!("start: {initial:?}");
    for op in ops {
        let result = match op {
            Op::Edit(origin, edits) => {
                let edits = sanitize(&buffer.text().to_string(), edits);
                format!("{:?}", buffer.edit(edits, origin.clone()))
            }
            Op::Start(origin) => format!("{:?}", buffer.start_transaction(origin.clone())),
            Op::End => format!("{:?}", buffer.end_transaction()),
            Op::Undo(filter) => format!("{:?}", buffer.undo(FILTERS[*filter].1)),
            Op::Redo(filter) => format!("{:?}", buffer.redo(FILTERS[*filter].1)),
        };
        let edits: Vec<_> = events
            .drain()
            .into_iter()
            .filter_map(|event| match event {
                BufferEvent::Edited { kind, edits, .. } => Some((kind, edits)),
                _ => None,
            })
            .collect();
        eprintln!(
            "{op:?} -> {result}\n    {edits:?}\n    {:?}",
            buffer.text().to_string()
        );
    }
}

/// Generates a random session's ops, steering edits toward the edges of
/// recent changes.
fn generate(seed: u64, single_origin: bool) -> (String, Vec<Op>) {
    let mut rng = StdRng::seed_from_u64(seed);
    let mut chars = UniqueChars(0);
    let initial_len = rng.random_range(0..12);
    let initial = chars.string(&mut rng, initial_len);
    let mut buffer = Buffer::new(&initial);
    let events = buffer.subscribe();
    let mut hot: Vec<usize> = Vec::new();
    let mut open: Option<Origin> = None;
    let mut ops = Vec::new();
    for _ in 0..60 {
        let text = buffer.text().to_string();
        let roll = rng.random_range(0..100);
        let filter = if single_origin {
            0
        } else {
            rng.random_range(0..FILTERS.len())
        };
        let op = if roll < 50 {
            let origin = match &open {
                Some(origin) if rng.random_bool(0.7) => origin.clone(),
                _ if single_origin => Origin::User,
                _ => origin(rng.random_range(0..3)),
            };
            let edits = random_batch(&mut rng, &text, &hot, &mut chars);
            buffer.edit(edits.clone(), origin.clone());
            Op::Edit(origin, edits)
        } else if roll < 58 {
            match open.take() {
                Some(_) => {
                    buffer.end_transaction();
                    Op::End
                }
                None => {
                    let origin = if single_origin {
                        Origin::User
                    } else {
                        origin(rng.random_range(0..3))
                    };
                    buffer.start_transaction(origin.clone());
                    open = Some(origin.clone());
                    Op::Start(origin)
                }
            }
        } else if roll < 80 {
            open = None;
            let _ = buffer.undo(FILTERS[filter].1);
            Op::Undo(filter)
        } else {
            open = None;
            let _ = buffer.redo(FILTERS[filter].1);
            Op::Redo(filter)
        };
        ops.push(op);
        for event in events.drain() {
            if let BufferEvent::Edited { edits, .. } = event {
                hot = edits
                    .iter()
                    .flat_map(|edit| [edit.new.start, edit.new.end])
                    .collect();
            }
        }
    }
    (initial, ops)
}

/// Greedily removes ops, and edits within ops, while the session still
/// fails, to produce a small reproduction.
fn minimize(initial: &str, mut ops: Vec<Op>, single_origin: bool) -> Vec<Op> {
    let fails = |ops: &[Op]| execute(initial, ops, single_origin).is_err();
    loop {
        let mut shrunk = false;
        for ix in (0..ops.len()).rev() {
            let mut candidate = ops.clone();
            candidate.remove(ix);
            if fails(&candidate) {
                ops = candidate;
                shrunk = true;
                continue;
            }
            if let Op::Edit(origin, edits) = &ops[ix] {
                for edit_ix in (0..edits.len()).rev() {
                    let mut fewer = edits.clone();
                    fewer.remove(edit_ix);
                    let mut candidate = ops.clone();
                    candidate[ix] = Op::Edit(origin.clone(), fewer);
                    if fails(&candidate) {
                        ops = candidate;
                        shrunk = true;
                        break;
                    }
                }
            }
        }
        if !shrunk {
            return ops;
        }
    }
}

/// Runs a random session. With `single_origin`, only user edits and user
/// undo/redo happen, so every undo and redo must succeed.
fn run_session(seed: u64, single_origin: bool) -> Result<Stats, TestCaseError> {
    let (initial, ops) = generate(seed, single_origin);
    match execute(&initial, &ops, single_origin) {
        Ok(stats) => Ok(stats),
        Err(_) => {
            let small = minimize(&initial, ops, single_origin);
            let error = execute(&initial, &small, single_origin).unwrap_err();
            let mut report = format!("{error}\ninitial: {initial:?}\nminimal ops:\n");
            for op in &small {
                writeln!(report, "  {op:?}").unwrap();
            }
            Err(TestCaseError::fail(report))
        }
    }
}

proptest! {
    // `run_session` minimises failures itself; shrinking the seed would just
    // run (and minimise) unrelated sessions.
    #![proptest_config(ProptestConfig { max_shrink_iters: 0, ..ProptestConfig::default() })]

    /// With one origin, undo and redo must always succeed and match.
    #[test]
    fn single_origin_matches_reference(seed in any::<u64>()) {
        run_session(seed, true)?;
    }

    /// With several origins, every undo or redo that succeeds must match;
    /// the rest must be clean refusals.
    #[test]
    fn mixed_origins_match_reference(seed in any::<u64>()) {
        run_session(seed, false)?;
    }
}

/// How often mixed sessions refuse an undo or redo whose text was intact.
/// Those refusals are safe but avoidable in principle, so this guards against
/// the rate creeping up. (The sessions are adversarial: edits cluster at the
/// edges of earlier edits, where ordering is ambiguous.)
#[test]
fn avoidable_refusals_stay_rare() {
    let mut total = Stats::default();
    for seed in 0..100 {
        let stats = run_session(seed, false).expect("matches reference");
        total.attempts += stats.attempts;
        total.refused += stats.refused;
        total.avoidable += stats.avoidable;
    }
    eprintln!("{total:?}");
    let rate = total.avoidable as f64 / total.attempts as f64;
    assert!(
        rate < AVOIDABLE_REFUSAL_BUDGET,
        "{total:?}: {:.1}% avoidable",
        rate * 100.
    );
}

/// Measured at 1.8% (29 of 1,595 attempts). The sessions are seeded, so the
/// rate only moves when the code does; mistakes in collapse tracking push it
/// to 2.4% or more.
const AVOIDABLE_REFUSAL_BUDGET: f64 = 0.022;

#[test]
fn adjacent_deletions_undone_out_of_order_are_never_misordered() {
    let mut buffer = Buffer::new("xaby");
    let ai = origin(1);
    buffer.edit([(1..2, "")], ai.clone()); // delete "a"
    buffer.edit([(1..2, "")], Origin::User); // delete "b"
    assert_eq!(buffer.text().to_string(), "xy");
    buffer.undo(|o| o.is_ai()).unwrap(); // restore "a" first...
    assert_eq!(buffer.text().to_string(), "xay");
    // ...then "b". Offsets alone can't tell whether "b" belongs before or
    // after the restored "a", so this is refused rather than guessed (an
    // earlier version silently produced "xbay").
    match buffer.undo(|o| *o == Origin::User) {
        Ok(_) => assert_eq!(buffer.text().to_string(), "xaby"),
        Err(UndoError::Conflict(_)) => assert_eq!(buffer.text().to_string(), "xay"),
    }
}

#[test]
fn undoing_in_reverse_order_still_works() {
    // The same two deletions undone newest-first are exact.
    let mut buffer = Buffer::new("xaby");
    buffer.edit([(1..2, "")], origin(1));
    buffer.edit([(1..2, "")], Origin::User);
    buffer.undo(|o| *o == Origin::User).unwrap();
    buffer.undo(|o| o.is_ai()).unwrap();
    assert_eq!(buffer.text().to_string(), "xaby");
}

/// Minimal sessions the fuzzer found, kept so they're checked on every run.
mod regressions {
    use super::*;

    fn ai_a() -> Origin {
        origin(1)
    }

    fn ai_b() -> Origin {
        origin(2)
    }

    fn edit(origin: Origin, edits: &[(Range<usize>, &str)]) -> Op {
        Op::Edit(
            origin,
            edits
                .iter()
                .map(|(range, text)| (range.clone(), text.to_string()))
                .collect(),
        )
    }

    const USER: usize = 0;
    const AI: usize = 1;
    const ANY: usize = 2;

    fn check(initial: &str, ops: &[Op]) {
        if let Err(error) = execute(initial, ops, false) {
            trace(initial, ops);
            panic!("{error}");
        }
    }

    /// Redone text whose spot a later replacement swallowed goes to that
    /// replacement's start, as the replaced text sat before its new text.
    #[test]
    fn redo_inside_a_later_replacement() {
        check(
            "𠀁丂𠀃丄丅丆万",
            &[
                edit(Origin::User, &[(23..23, "与丏丐")]),
                edit(ai_b(), &[(32..32, "业")]),
                Op::Undo(USER),
                edit(ai_a(), &[(20..29, "丞")]),
                Op::Redo(USER),
            ],
        );
    }

    /// Text put back where another undo had already reinserted text must not
    /// be guessed into the wrong order.
    #[test]
    fn restore_next_to_reinserted_text() {
        check(
            "",
            &[
                edit(ai_a(), &[(0..0, "丁𠀂七")]),
                edit(Origin::User, &[(3..7, "与")]),
                edit(Origin::User, &[(6..6, "乩")]),
                edit(ai_b(), &[(6..6, "乪")]),
                Op::Undo(USER),
                edit(ai_b(), &[(6..53, "亁亂")]),
                Op::Redo(USER),
                Op::Undo(ANY),
            ],
        );
    }

    /// A position collapsed by a replacement can't be trusted once that
    /// replacement is undone.
    #[test]
    fn redo_after_its_collapsing_edit_was_undone() {
        check(
            "丁𠀂七",
            &[
                edit(Origin::User, &[(3..3, "丈𠀉")]),
                edit(ai_a(), &[(14..14, "丰丱串")]),
                Op::Undo(AI),
                edit(Origin::User, &[(0..30, "丿")]),
                edit(ai_b(), &[(6..6, "𠁁乂")]),
                Op::Undo(USER),
                Op::Redo(AI),
            ],
        );
    }

    #[test]
    fn deep_history_chain() {
        check(
            "丁",
            &[
                edit(ai_a(), &[(3..3, "与")]),
                edit(ai_a(), &[(0..0, "专且丕")]),
                edit(Origin::User, &[(12..12, "严𠀦丧")]),
                Op::Undo(ANY),
                edit(ai_b(), &[(3..57, "丳")]),
                Op::Redo(USER),
                Op::Undo(USER),
                Op::Undo(ANY),
                Op::Redo(AI),
                Op::Undo(AI),
                Op::Redo(USER),
            ],
        );
    }

    /// ...even when the collapsed position was passed on through another
    /// undo/redo cycle first.
    #[test]
    fn collapse_passed_through_history() {
        check(
            "丁丂七丄丅𠀆万丈三",
            &[
                edit(Origin::User, &[(6..6, "乑乒乓")]),
                Op::Undo(ANY),
                edit(ai_a(), &[(3..28, "乭")]),
                Op::Redo(USER),
                Op::Undo(USER),
                Op::Undo(AI),
                Op::Redo(USER),
            ],
        );
    }
}
