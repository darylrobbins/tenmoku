//! Insert mode's keyword completion, `<C-n>` and `<C-p>` (`:help
//! i_CTRL-N`): the keyword before the caret is completed from the words of
//! the buffer, as nvim completes it with `complete=.`.
//!
//! The keyword is the run of characters before the caret of the class the
//! last one has, which must be a word character, as `w` classifies them
//! ([`crate::motion::class`]). The candidates are the words starting with it,
//! each distinct one once, in the order nvim finds them: searching from the
//! caret forward (`<C-n>`) or backward (`<C-p>`), wrapping around the
//! buffer's end. As nvim's, the search reads the buffer with the keyword
//! taken out, so the word the caret is in isn't one, but what follows the
//! caret may be. A keyword of one byte only completes to longer words; with
//! no keyword before the caret the candidates are the words nvim's pattern
//! `\<\k\k` finds, taking each row's matches one after another, so a word
//! starting inside the match before (`日本語` after `a日`) isn't one.
//!
//! The first key inserts the first candidate in place of the keyword, the
//! next ones go on through the list in the same direction or back, and
//! past either end the keyword as typed comes back, as nvim's do. `<C-e>`
//! puts the typed keyword back and ends completion, `<C-y>` keeps the
//! candidate and ends it, and any other key keeps it and ends it, then does
//! what it does ([`VimState::record_key`] sees it first), as does the
//! session ending or splitting, or the text or a selection changing. Only
//! the text after the keyword changes, at every caret, as typing does, so
//! `.` and a count type the completed word, as nvim's redo does. No
//! candidate fails with the message `Pattern not found`, changing nothing,
//! but it doesn't stop a macro, as nvim's beep doesn't.
//!
//! The candidates are listed lazily, as cycling reaches them. The search is
//! a series of steps, each reading at most [`WINDOW`] bytes or so, from and
//! to graphemes' boundaries (a single grapheme longer than that is read
//! whole): finding where the keyword
//! starts, looking for the keyword (or with none, a word's start), measuring
//! a word that starts with it, and taking it. Each key takes [`STEPS`] steps
//! at least and then as many as fit in the [`search::time_limit`]; past it
//! the key changes nothing and leaves the message `Interrupted`, and the
//! next one goes on from there. Words are told apart by their length and
//! two 64-bit hashes of their bytes, taken as they're measured, so two
//! different words that collide on all three would count as one.
//!
//! Known limit: what isn't searching isn't bounded. Taking a found word
//! copies it whole, as setting up the search copies the keyword, and the
//! edit that puts a candidate in (or takes it out) is as long as it is; a
//! keyword or candidate of pathological length can hold one key past the
//! limit there. Bounding those is a follow-up card.
//!
//! Known divergence: nvim also takes words from other windows' and loaded
//! buffers and from tags (`complete=.,w,b,u,t`); here only the current
//! buffer is read. Neither 'ignorecase' nor 'infercase' applies, as with
//! nvim's defaults. Graphemes are classed whole, as nvim classes a
//! character with its composing ones; with no keyword, though, nvim doesn't
//! find a word right after a grapheme with many composing characters, which
//! this does.

use std::{
    collections::{HashSet, VecDeque},
    fmt,
    ops::Range,
};

use fe_buffer::{Anchor, ResolvedSelection, Version};
use fe_editor::EditorModel;
use fe_text::Text;
use unicode_segmentation::{GraphemeCursor, GraphemeIncomplete, UnicodeSegmentation as _};

use crate::{
    Mode, VimKey, VimState,
    motion::{Class, class},
    operator::{apply, sync},
    search::{self, Deadline},
};

use super::super::place;

/// A keyword completion under way, for a host to show
/// ([`VimState::completion`]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KeywordCompletion {
    /// The candidates listed so far, in the order nvim's menu has them: as
    /// found for `<C-n>`, the other way round for `<C-p>`.
    pub candidates: Vec<String>,
    /// The candidate in the text, or `None` for the keyword as typed.
    pub selected: Option<usize>,
    /// Where the typed keyword is at the primary caret (empty with none),
    /// as byte offsets: the candidate's text goes from its start.
    pub typed: Range<usize>,
    /// Whether every candidate is listed: the search read the whole buffer.
    pub listed_all: bool,
}

/// The completion state [`VimState`] keeps between `<C-n>` and `<C-p>`.
#[derive(Clone, Debug)]
pub(crate) struct Completion {
    scan: Scan,
    /// Where the keyword starts at the primary caret, once it's found,
    /// followed through the edits at the carets before it.
    anchor: Option<Anchor>,
    /// The typed keyword's range as of the last edit.
    typed: Range<usize>,
    /// The first key's direction: `<C-n>`'s forward.
    forward: bool,
    /// In the order found, each once.
    found: Vec<String>,
    /// 0 for the typed keyword, `n` for `found[n - 1]`.
    selected: usize,
    /// The text's version and the selections after its last edit:
    /// anything else that changes either ended it.
    version: Version,
    selections: Vec<ResolvedSelection>,
}

impl VimState {
    /// `<C-n>` in insert mode: completes the keyword before the caret with
    /// the next candidate, as the module docs say.
    pub fn complete_next(&mut self, model: &mut EditorModel) {
        self.complete(model, true);
    }

    /// `<C-p>` in insert mode: completes the keyword before the caret with
    /// the previous candidate.
    pub fn complete_previous(&mut self, model: &mut EditorModel) {
        self.complete(model, false);
    }

    /// `<C-e>` while completing: the typed keyword goes back and completion
    /// ends. Returns whether completion was under way: otherwise the key
    /// isn't this.
    pub fn complete_cancel(&mut self, model: &mut EditorModel) -> bool {
        let Some(mut completion) = self.live_completion(model) else {
            return false;
        };
        let from = completion.selected;
        completion.selected = 0;
        self.retext(model, &completion, from);
        true
    }

    /// `<C-y>` while completing: the candidate stays and completion ends.
    /// Returns whether completion was under way.
    pub fn complete_accept(&mut self, model: &mut EditorModel) -> bool {
        // Taken, it has ended.
        self.live_completion(model).is_some()
    }

    /// The keyword completion under way in `model`, if `<C-n>` or `<C-p>`
    /// started one there and nothing has ended it: no other key, no end or
    /// split of the session, and neither the text nor a selection changed
    /// since its last edit.
    pub fn completion(&self, model: &EditorModel) -> Option<KeywordCompletion> {
        let completion = self
            .completion
            .as_ref()
            .filter(|completion| self.mode == Mode::Insert && completion.holds(model))?;
        let found = &completion.found;
        let (candidates, selected) = match completion.forward {
            true => (found.clone(), completion.selected.checked_sub(1)),
            false => (
                found.iter().rev().cloned().collect(),
                completion
                    .selected
                    .checked_sub(1)
                    .map(|ix| found.len() - 1 - ix),
            ),
        };
        Some(KeywordCompletion {
            candidates,
            selected,
            typed: completion.typed.clone(),
            listed_all: completion.scan.done(),
        })
    }

    /// A key read in insert mode: one that isn't a completion key ends
    /// completion, keeping the candidate.
    pub(crate) fn completion_key(&mut self, key: VimKey) {
        if !matches!(
            key,
            VimKey::Ctrl('n') | VimKey::Ctrl('p') | VimKey::Ctrl('e') | VimKey::Ctrl('y')
        ) {
            self.completion = None;
        }
    }

    fn complete(&mut self, model: &mut EditorModel, forward: bool) {
        if self.mode != Mode::Insert || self.session.is_none() {
            return;
        }
        let deadline = Deadline::new();
        // This key's outcome is the message now.
        self.message = None;
        let mut completion = match self.live_completion(model) {
            Some(completion) => completion,
            None => start(model, forward),
        };
        completion.scan.steps = 0;
        let from = completion.selected;
        let stepped = match forward == completion.forward {
            true => completion.onward(&deadline),
            false => completion.back(&deadline),
        };
        // Nothing has changed the text since completion began, so the
        // keyword's start is where the scan found it, in this text.
        if completion.anchor.is_none()
            && let Some(start) = completion.scan.keyword_start()
        {
            completion.anchor = Some(model.snapshot().anchor_before(start));
        }
        match stepped {
            Ok(()) if completion.found.is_empty() => {
                self.message = Some("Pattern not found".into());
            }
            Ok(()) => {
                self.retext(model, &completion, from);
                self.completion = Some(completion);
                self.completion_settled(model);
            }
            // Kept, nothing found yet or not, so the next key goes on.
            Err(search::Interrupted) => {
                self.message = Some(search::Interrupted.to_string());
                self.completion = Some(completion);
                self.completion_settled(model);
            }
        }
    }

    /// The completion under way, taken, if the text and the selections are
    /// as its last edit left them.
    fn live_completion(&mut self, model: &mut EditorModel) -> Option<Completion> {
        let completion = self.completion.take()?;
        if self.mode != Mode::Insert {
            return None;
        }
        sync(model);
        completion.holds(model).then_some(completion)
    }

    /// Changes the text after the keyword at each caret from candidate
    /// `from` to the one `completion` has selected: where a caret has
    /// `from`'s text before it, as typing put it there at every caret.
    fn retext(&self, model: &mut EditorModel, completion: &Completion, from: usize) {
        let Some(session) = &self.session else {
            return;
        };
        let typed = completion.scan.typed.len();
        let suffix = |ix: usize| match ix {
            0 => "",
            ix => &completion.found[ix - 1][typed..],
        };
        let (old, new) = (suffix(from), suffix(completion.selected));
        sync(model);
        let text = model.text().clone();
        let selections = model.selections();
        let mut edits: Vec<(Range<usize>, String)> = Vec::new();
        for selection in &selections {
            let head = selection.head();
            let range = head.saturating_sub(old.len())..head;
            if text.is_char_boundary(range.start)
                && text.range_eq(range.clone(), old)
                && !edits.iter().any(|(r, _)| r.end == head)
            {
                edits.push((range, new.to_owned()));
            }
        }
        edits.sort_by_key(|(range, _)| range.start);
        let moved: Vec<usize> = selections
            .iter()
            .map(|selection| {
                let head = selection.head();
                let shift: isize = edits
                    .iter()
                    .filter(|(range, _)| range.end <= head)
                    .map(|(range, new)| new.len() as isize - range.len() as isize)
                    .sum();
                head.saturating_add_signed(shift)
            })
            .collect();
        if !edits.is_empty() {
            let (_, snapshot) = apply(model, edits, session.origin.clone());
            place(model, &snapshot, &moved);
        }
    }

    /// Notes where the last completion edit left the text, the selections
    /// and the keyword.
    fn completion_settled(&mut self, model: &mut EditorModel) {
        sync(model);
        let version = model.snapshot().version();
        let selections = model.selections();
        if let Some(completion) = &mut self.completion {
            if let Some(anchor) = &completion.anchor {
                let start = model.snapshot().resolve(anchor);
                completion.typed = start..start + completion.scan.typed.len();
            }
            completion.version = version;
            completion.selections = selections;
        }
    }
}

impl Completion {
    /// Whether `model` is as the last edit left it.
    fn holds(&self, model: &EditorModel) -> bool {
        self.version == model.snapshot().version() && self.selections == model.selections()
    }

    /// One step in the first key's direction: to the next candidate, found
    /// now if it isn't yet, or past the last to the typed keyword.
    fn onward(&mut self, deadline: &Deadline) -> Result<(), search::Interrupted> {
        if self.selected < self.found.len() {
            self.selected += 1;
            return Ok(());
        }
        match self.find(deadline)? {
            true => self.selected += 1,
            false => self.selected = 0,
        }
        Ok(())
    }

    /// One step the other way: to the candidate before, or from the typed
    /// keyword to the last, which takes finding them all.
    fn back(&mut self, deadline: &Deadline) -> Result<(), search::Interrupted> {
        if self.selected > 0 {
            self.selected -= 1;
            return Ok(());
        }
        while self.find(deadline)? {}
        self.selected = self.found.len();
        Ok(())
    }

    /// Finds the next candidate, if there's one: whether it did.
    fn find(&mut self, deadline: &Deadline) -> Result<bool, search::Interrupted> {
        let word = self.scan.next(deadline)?;
        Ok(word.map(|word| self.found.push(word)).is_some())
    }
}

/// A completion of the keyword before the primary caret, nothing selected
/// or found yet, nor the keyword's start.
fn start(model: &mut EditorModel, forward: bool) -> Completion {
    sync(model);
    let text = model.text().clone();
    let head = model.primary_selection().head();
    let row_start = text.row_start(text.offset_to_point(head).row);
    Completion {
        scan: Scan::new(text, row_start, head, forward),
        anchor: None,
        typed: head..head,
        forward,
        found: Vec::new(),
        selected: 0,
        version: model.snapshot().version(),
        selections: model.selections(),
    }
}

/// A grapheme's class, if it's a word character's.
fn word_class(grapheme: &str) -> Option<u32> {
    match class(grapheme, false) {
        Class::Of(class) if class >= 2 => Some(class),
        _ => None,
    }
}

/// How many steps of the search one key takes before it looks at the
/// clock: each key gets at least this far.
const STEPS: u32 = 64;

/// About the most bytes one step of the search reads.
const WINDOW: usize = 4096;

/// How much of the keyword the search looks for; the rest is checked as a
/// word is measured.
const PROBE: usize = 256;

/// How far around a place the search reads to classify the graphemes there.
const LOCAL: usize = 64;

/// The search for candidates, a step at a time.
#[derive(Clone)]
struct Scan {
    phase: Phase,
    /// The text completion began in, and the primary caret then.
    original: Text,
    head: usize,
    /// The keyword, once found, and the text searched: the original with it
    /// taken out.
    typed: String,
    text: Text,
    forward: bool,
    /// Where words may still start, the first being searched: from its
    /// start forward, or from its end back.
    ranges: VecDeque<Range<usize>>,
    /// The word being measured.
    word: Option<Word>,
    /// Each word taken, as its length and hashes.
    seen: HashSet<(usize, u64, u64)>,
    /// With no keyword, where the last match of nvim's `\<\k\k` searching
    /// forward ended: a word starting before it isn't a match ([`chain`]).
    covered: usize,
    /// Searching back with no keyword, the walk back from a window's start.
    walk: Option<Walk>,
    /// With no keyword, the matches found in the last window read and not
    /// yet measured, in order.
    queued: VecDeque<usize>,
    /// The steps this key took.
    steps: u32,
}

#[derive(Clone, Copy, Debug)]
enum Phase {
    /// Walking back from the caret over the keyword: it starts at `at` so
    /// far, its characters of `class`, not before `row_start`.
    Keyword {
        at: usize,
        class: Option<u32>,
        row_start: usize,
    },
    Search {
        start: usize,
    },
}

/// A word starting with the keyword, as far as it's been measured.
#[derive(Clone, Debug)]
struct Word {
    start: usize,
    class: u32,
    /// Measured up to here: the keyword's bytes among them checked.
    at: usize,
    /// Its end, once found.
    end: Option<usize>,
    /// How much to read next: more each step, up to [`WINDOW`].
    read: usize,
    hashes: (u64, u64),
}

impl fmt::Debug for Scan {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Scan")
            .field("phase", &self.phase)
            .field("typed", &self.typed)
            .field("ranges", &self.ranges)
            .field("word", &self.word)
            .finish_non_exhaustive()
    }
}

/// `range` of `text` (char boundaries), borrowed from the rope if one
/// chunk holds it, else copied into `buf`.
fn read<'a>(text: &'a Text, range: Range<usize>, buf: &'a mut String) -> &'a str {
    let mut chunks = text.chunks_in_range(range.clone());
    let first = chunks.next().unwrap_or("");
    if first.len() == range.len() {
        return first;
    }
    buf.clear();
    buf.push_str(first);
    buf.extend(chunks);
    buf
}

/// `at`, or the nearest char boundary of `text` before it.
fn floor(text: &Text, mut at: usize) -> usize {
    at = at.min(text.len());
    while !text.is_char_boundary(at) {
        at -= 1;
    }
    at
}

/// `at`, or the nearest char boundary of `text` after it.
fn ceil(text: &Text, mut at: usize) -> usize {
    at = at.min(text.len());
    while !text.is_char_boundary(at) {
        at += 1;
    }
    at
}

/// Runs a grapheme cursor's query at `at` over pieces of `text`, giving it
/// the pieces and the context before them it asks for.
fn graphemes_at<T>(
    text: &Text,
    at: usize,
    query: impl Fn(&mut GraphemeCursor, &str, usize) -> Result<T, GraphemeIncomplete>,
    fallback: T,
) -> T {
    let piece = |range: Range<usize>| (text.slice_to_string(range.clone()), range.start);
    let mut cursor = GraphemeCursor::new(at, text.len(), true);
    let (mut chunk, mut start) =
        piece(floor(text, at.saturating_sub(LOCAL))..ceil(text, at + LOCAL));
    loop {
        match query(&mut cursor, &chunk, start) {
            Ok(found) => return found,
            Err(GraphemeIncomplete::NextChunk) => {
                let from = start + chunk.len();
                if from >= text.len() {
                    return fallback;
                }
                (chunk, start) = piece(from..ceil(text, from + LOCAL));
            }
            Err(GraphemeIncomplete::PrevChunk) => {
                if start == 0 {
                    return fallback;
                }
                (chunk, start) = piece(floor(text, start.saturating_sub(LOCAL))..start);
            }
            Err(GraphemeIncomplete::PreContext(end)) => {
                let (context, from) = piece(floor(text, end.saturating_sub(LOCAL))..end);
                cursor.provide_context(&context, from);
            }
            Err(GraphemeIncomplete::InvalidOffset) => return fallback,
        }
    }
}

/// Whether a grapheme of `text` starts at `at`.
fn is_boundary(text: &Text, at: usize) -> bool {
    graphemes_at(
        text,
        at,
        |cursor, chunk, start| cursor.is_boundary(chunk, start),
        true,
    )
}

/// Where the grapheme of `text` at `at` (a boundary) ends.
fn next_boundary(text: &Text, at: usize) -> usize {
    let query = |cursor: &mut GraphemeCursor, chunk: &str, start| {
        cursor
            .next_boundary(chunk, start)
            .map(|found| found.unwrap_or(text.len()))
    };
    graphemes_at(text, at, query, text.len())
}

/// Where the grapheme of `text` before `at` (a boundary) starts.
fn prev_boundary(text: &Text, at: usize) -> usize {
    let query = |cursor: &mut GraphemeCursor, chunk: &str, start| {
        cursor
            .prev_boundary(chunk, start)
            .map(|found| found.unwrap_or(0))
    };
    graphemes_at(text, at, query, 0)
}

/// The grapheme boundary of `text` at or after `at`.
fn boundary_after(text: &Text, at: usize) -> usize {
    let at = floor(text, at);
    match is_boundary(text, at) {
        true => at,
        false => next_boundary(text, at),
    }
}

/// The grapheme boundary of `text` at or before `at`.
fn boundary_before(text: &Text, at: usize) -> usize {
    let at = floor(text, at);
    match is_boundary(text, at) {
        true => at,
        false => prev_boundary(text, at),
    }
}

/// The class of the grapheme of `text` before `at` (a boundary), if it's a
/// word character's: the whole grapheme, however long.
fn class_before(text: &Text, at: usize) -> Option<u32> {
    let from = prev_boundary(text, at);
    let mut buf = String::new();
    (from < at).then(|| word_class(read(text, from..at, &mut buf)))?
}

/// The class of the grapheme of `text` at `at` (a boundary), if it's a word
/// character's.
fn class_at(text: &Text, at: usize) -> Option<u32> {
    let end = next_boundary(text, at);
    let mut buf = String::new();
    (at < end).then(|| word_class(read(text, at..end, &mut buf)))?
}

impl Scan {
    /// A search from `head` in `text`, starting with the keyword before it.
    fn new(text: Text, row_start: usize, head: usize, forward: bool) -> Self {
        Self {
            phase: Phase::Keyword {
                at: head,
                class: None,
                row_start,
            },
            original: text.clone(),
            head,
            typed: String::new(),
            text,
            forward,
            ranges: VecDeque::new(),
            word: None,
            seen: HashSet::new(),
            covered: 0,
            walk: None,
            queued: VecDeque::new(),
            steps: 0,
        }
    }

    /// Where the keyword starts, once that's found.
    fn keyword_start(&self) -> Option<usize> {
        match self.phase {
            Phase::Search { start } => Some(start),
            Phase::Keyword { .. } => None,
        }
    }

    /// Whether every candidate is found.
    fn done(&self) -> bool {
        matches!(self.phase, Phase::Search { .. })
            && self.word.is_none()
            && self.queued.is_empty()
            && self.ranges.iter().all(Range::is_empty)
    }

    /// One step more, or `Interrupted` if it's past `deadline` (looked at
    /// every [`STEPS`] steps): nothing is read for it then.
    fn step(&mut self, deadline: &Deadline) -> Result<(), search::Interrupted> {
        self.steps += 1;
        match self.steps.is_multiple_of(STEPS) && deadline.expired() {
            true => Err(search::Interrupted),
            false => Ok(()),
        }
    }

    /// The next word not taken before, in the search's order, or `None`
    /// past the last.
    fn next(&mut self, deadline: &Deadline) -> Result<Option<String>, search::Interrupted> {
        loop {
            self.step(deadline)?;
            if let Phase::Keyword { .. } = self.phase {
                self.keyword_step();
                continue;
            }
            if let Some(word) = &self.word {
                if let Some(end) = word.end {
                    let start = word.start;
                    self.word = None;
                    return Ok(Some(self.text.slice_to_string(start..end)));
                }
                self.measure_step();
                continue;
            }
            // With no keyword, the next of a window's matches.
            let queued = match self.forward {
                true => self.queued.pop_front(),
                false => self.queued.pop_back(),
            };
            if let Some(at) = queued {
                self.begin_word(at);
                continue;
            }
            let Some(range) = self.ranges.front() else {
                return Ok(None);
            };
            if range.is_empty() {
                // The next range starts a row's matches over.
                self.ranges.pop_front();
                self.covered = 0;
                continue;
            }
            let range = range.clone();
            let found = match (self.forward, self.typed.is_empty()) {
                (true, false) => self.probe_forward(range),
                (false, false) => self.probe_back(range),
                (true, true) => {
                    self.starts_forward(range);
                    None
                }
                (false, true) => {
                    self.starts_back(range);
                    None
                }
            };
            if let Some(at) = found {
                self.begin_word(at);
            }
        }
    }

    /// Walks back over the keyword a window at a time, then sets up the
    /// search: the keyword taken out, the ranges to search in order.
    fn keyword_step(&mut self) {
        let Phase::Keyword {
            mut at,
            mut class,
            row_start,
        } = self.phase
        else {
            return;
        };
        // Graphemes whole: the window starts at one's start.
        let from = boundary_before(&self.original, at.saturating_sub(WINDOW)).max(row_start);
        let mut buf = String::new();
        let s = read(&self.original, from..at, &mut buf);
        let mut ended = from == row_start;
        for (off, grapheme) in s.grapheme_indices(true).rev() {
            let of = word_class(grapheme);
            if of.is_none() || class.is_some_and(|class| Some(class) != of) {
                ended = true;
                break;
            }
            class = of;
            at = from + off;
        }
        if !ended {
            self.phase = Phase::Keyword {
                at,
                class,
                row_start,
            };
            return;
        }
        let start = at;
        self.typed = self.original.slice_to_string(start..self.head);
        self.text = self.original.clone();
        self.text.replace(start..self.head, "");
        // Before the keyword, where `<C-p>` starts: a word starting there
        // is found last.
        let back = match start == row_start {
            true => start,
            false => prev_boundary(&self.text, start),
        };
        let len = self.text.len();
        self.ranges = match self.forward {
            true => [start..len, 0..start].into(),
            false => [0..back, back..len].into(),
        };
        self.phase = Phase::Search { start };
    }

    /// Looks for the keyword's first bytes from the start of `range` on,
    /// a window at a time: the first place they're found.
    fn probe_forward(&mut self, range: Range<usize>) -> Option<usize> {
        let probe = &self.typed[..floor_str(&self.typed, PROBE)];
        let Range { start: lo, end: hi } = range;
        let end = ceil(&self.text, lo + WINDOW + probe.len());
        let mut buf = String::new();
        let s = read(&self.text, lo..end, &mut buf);
        let (found, next) = match s.find(probe) {
            Some(ix) if lo + ix < hi => {
                let at = lo + ix;
                (
                    Some(at),
                    at + s[ix..].chars().next().map_or(1, char::len_utf8),
                )
            }
            Some(_) => (None, hi),
            None if end == self.text.len() => (None, hi),
            // Not a match there, but one may start in the last bytes.
            None => (None, floor_str_from(s, s.len() + 1 - probe.len(), lo)),
        };
        self.ranges[0].start = next.min(hi);
        found
    }

    /// Looks for the keyword's first bytes from the end of `range` back, a
    /// window at a time: the last place they start.
    fn probe_back(&mut self, range: Range<usize>) -> Option<usize> {
        let probe = &self.typed[..floor_str(&self.typed, PROBE)];
        let Range { start: lo, end: hi } = range;
        // A match starting before `hi` may run on past it.
        let end = floor(&self.text, hi - 1 + probe.len()).max(hi);
        let from = floor(&self.text, end.saturating_sub(WINDOW + probe.len())).max(lo);
        let mut buf = String::new();
        let s = read(&self.text, from..end, &mut buf);
        let found = s.rfind(probe).map(|ix| from + ix).filter(|&at| at < hi);
        self.ranges[0].end = found.unwrap_or(from);
        found
    }

    /// With no keyword: the matches of nvim's chain ([`chain`]) in the
    /// next window from the start of `range` on, queued.
    fn starts_forward(&mut self, range: Range<usize>) {
        let Range { start: lo, end: hi } = range;
        let text = &self.text;
        // Graphemes whole: the window ends at one's end.
        let end = boundary_after(text, lo + WINDOW).min(hi);
        let mut buf = String::new();
        let s = read(text, lo..end, &mut buf);
        let (matches, covered) = chain(text, s, lo, self.covered);
        self.queued.extend(matches.into_iter().map(|(at, _)| at));
        self.covered = covered;
        self.ranges[0].start = end;
    }

    /// With no keyword: the matches of nvim's chain ([`chain`]) in the
    /// next window back from the end of `range`, queued. The chain runs
    /// forward, so whether a match covers the window's start is walked to
    /// first ([`Walk`]).
    fn starts_back(&mut self, range: Range<usize>) {
        let Range { start: lo, end: hi } = range;
        let text = &self.text;
        // Graphemes whole: the window starts at one's start.
        let from = boundary_before(text, hi.saturating_sub(WINDOW)).max(lo);
        let covered = match from > lo {
            true => match self.walk.as_ref().and_then(|walk| walk.covers(from)) {
                Some(covered) => covered,
                None => {
                    let mut walk = match self.walk.take() {
                        Some(walk) if walk.at == from => walk,
                        _ => Walk::new(text, from),
                    };
                    walk.step(text);
                    self.walk = Some(walk);
                    return;
                }
            },
            false => false,
        };
        let covered = match covered {
            true => next_boundary(text, from),
            false => 0,
        };
        let mut buf = String::new();
        let s = read(text, from..hi, &mut buf);
        let (matches, _) = chain(text, s, from, covered);
        self.queued.extend(matches.into_iter().map(|(at, _)| at));
        self.ranges[0].end = from;
    }

    /// Starts measuring the word at `at`, if one starting with the keyword
    /// may start there: at a word's start (as `\<` has it), and for a
    /// keyword of one byte with one more word character after it, or with
    /// none, two word characters, as nvim's patterns `\<f\k`, `\<fo` and
    /// `\<\k\k` match.
    fn begin_word(&mut self, at: usize) {
        let text = &self.text;
        // A match of the keyword inside a grapheme isn't a word's start.
        if !is_boundary(text, at) {
            return;
        }
        let Some(class) = class_at(text, at) else {
            return;
        };
        if class_before(text, at) == Some(class) {
            return;
        }
        if self.typed.len() <= 1 && class_at(text, next_boundary(text, at)).is_none() {
            return;
        }
        self.word = Some(Word {
            start: at,
            class,
            at,
            end: None,
            read: LOCAL,
            hashes: (0xcbf2_9ce4_8422_2325, 0),
        });
    }

    /// Measures the word a window more: its graphemes while they're its
    /// class, checking they start with the keyword. At its end it's taken,
    /// unless it doesn't start with the keyword or was taken before.
    fn measure_step(&mut self) {
        let Some(word) = &mut self.word else {
            return;
        };
        // Graphemes whole: the window ends at one's end.
        let end = boundary_after(&self.text, word.at + word.read);
        word.read = (word.read * 2).min(WINDOW);
        let mut buf = String::new();
        let s = read(&self.text, word.at..end, &mut buf);
        let typed = self.typed.as_bytes();
        let mut ended = end == self.text.len();
        let mut at = word.at;
        for (off, grapheme) in s.grapheme_indices(true) {
            if word_class(grapheme) != Some(word.class) {
                ended = true;
                break;
            }
            let checked = at - word.start;
            if checked < typed.len() {
                let n = grapheme.len().min(typed.len() - checked);
                if grapheme.as_bytes()[..n] != typed[checked..checked + n] {
                    self.word = None;
                    return;
                }
            }
            for &byte in grapheme.as_bytes() {
                let (a, b) = &mut word.hashes;
                *a = (*a ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3);
                *b = b.rotate_left(5) ^ u64::from(byte);
                *b = b.wrapping_mul(0x9e37_79b9_7f4a_7c15);
            }
            at = word.at + off + grapheme.len();
        }
        word.at = at;
        if !ended {
            return;
        }
        let len = word.at - word.start;
        if len < typed.len() || !self.seen.insert((len, word.hashes.0, word.hashes.1)) {
            self.word = None;
            return;
        }
        word.end = Some(word.at);
    }
}

/// With no keyword, the places nvim's `\<\k\k` matches in `s`, the text
/// of `text` from `base` (both graphemes' boundaries), each with where it
/// ends, and where the last ends. nvim takes a row's matches one after
/// another from its start, each search going on from where the last match
/// ended ('cpoptions' has `c`), so a word start inside a match isn't one:
/// in `a日本語` the match `a日` hides `日本語`. `covered` is where the match
/// before `s` ended.
fn chain(text: &Text, s: &str, base: usize, mut covered: usize) -> (Vec<(usize, usize)>, usize) {
    let graphemes: Vec<(usize, &str)> = s.grapheme_indices(true).collect();
    let end = base + s.len();
    let mut matches = Vec::new();
    let mut before = class_before(text, base);
    for (ix, &(off, grapheme)) in graphemes.iter().enumerate() {
        let at = base + off;
        let of = word_class(grapheme);
        if of.is_some() && of != before && at >= covered {
            // The grapheme after: in the window, or the next one's first.
            let next = match graphemes.get(ix + 1) {
                Some(&(_, next)) => word_class(next).map(|_| next.len()),
                None => class_at(text, end).map(|_| next_boundary(text, end) - end),
            };
            if let Some(len) = next {
                covered = at + grapheme.len() + len;
                matches.push((at, covered));
            }
        }
        before = of;
    }
    (matches, covered)
}

/// A walk back from `at` with no keyword, to tell whether a match of
/// nvim's chain ([`chain`]) starts at the grapheme before it and so covers
/// it: the graphemes before it that could each start a match (a word of
/// one grapheme with a word character after it) come in a run, and from the
/// run's start every other one matches. A window at a time, as a run of
/// one-grapheme words of alternating classes may be long. The windows are
/// the ones searching back reads next, so it notes at each window's start
/// how far into the run it was, and answers for them too.
#[derive(Clone, Debug)]
struct Walk {
    at: usize,
    /// Walked back to here, the run's length so far, and the class of the
    /// grapheme here.
    to: usize,
    run: usize,
    after: Option<u32>,
    /// The windows' starts it passed with the run going on, and how long
    /// the run was there.
    passed: Vec<(usize, usize)>,
    /// The run's whole length, once its start is found.
    length: Option<usize>,
}

impl Walk {
    fn new(text: &Text, at: usize) -> Self {
        Self {
            at,
            to: at,
            run: 0,
            after: class_at(text, at),
            passed: Vec::new(),
            length: None,
        }
    }

    /// Whether a match covers `at`, or the start of a window it passed, if
    /// it knows.
    fn covers(&self, at: usize) -> Option<bool> {
        let length = self.length?;
        let into = match at == self.at {
            true => 0,
            false => self.passed.iter().find(|&&(start, _)| start == at)?.1,
        };
        Some((length - into) % 2 == 1)
    }

    /// Walks back a window more.
    fn step(&mut self, text: &Text) {
        let from = boundary_before(text, self.to.saturating_sub(WINDOW));
        let mut buf = String::new();
        let s = read(text, from..self.to, &mut buf);
        let graphemes: Vec<(usize, &str)> = s.grapheme_indices(true).collect();
        for (ix, &(_, grapheme)) in graphemes.iter().enumerate().rev() {
            let of = word_class(grapheme);
            let before = match ix {
                0 => class_before(text, from),
                ix => word_class(graphemes[ix - 1].1),
            };
            // It could start a match: a word's start, a word character after.
            if of.is_none() || of == before || self.after.is_none() {
                self.length = Some(self.run);
                return;
            }
            self.run += 1;
            self.after = of;
        }
        self.to = from;
        self.passed.push((from, self.run));
        if from == 0 {
            self.length = Some(self.run);
        }
    }
}

/// The nearest char boundary of `s` at or before `at`.
fn floor_str(s: &str, at: usize) -> usize {
    let mut at = at.min(s.len());
    while !s.is_char_boundary(at) {
        at -= 1;
    }
    at
}

/// `base` plus the nearest char boundary of `s` at or before `at`, but past
/// `base` by one character at least.
fn floor_str_from(s: &str, at: usize, base: usize) -> usize {
    let first = s.chars().next().map_or(1, char::len_utf8);
    base + floor_str(s, at).max(first)
}

#[cfg(test)]
mod tests {
    use super::{Deadline, Phase, Scan};
    use fe_text::Text;

    /// The words a scan of `text` finds for the keyword `typed` from its
    /// start.
    fn scan(text: &str, typed: &str, forward: bool) -> Vec<String> {
        // The keyword before the caret, taken out to search.
        let original = format!("{typed}{text}");
        let mut scan = Scan::new(Text::new(&original), 0, typed.len(), forward);
        let deadline = Deadline::never();
        let mut words = Vec::new();
        while let Some(word) = scan.next(&deadline).unwrap() {
            words.push(word);
        }
        assert!(matches!(scan.phase, Phase::Search { start: 0 }));
        assert_eq!(scan.typed, typed);
        assert!(scan.done());
        words
    }

    #[test]
    fn words_start_at_word_starts() {
        assert_eq!(scan(" foo food fool", "f", true), ["foo", "food", "fool"]);
        assert_eq!(scan(" foo food fool", "f", false), ["fool", "food", "foo"]);
        assert_eq!(scan(" xfoo f fo", "f", true), ["fo"]);
        assert_eq!(scan(" fo fox", "fo", true), ["fo", "fox"]);
        assert_eq!(scan(" fo fox", "fo", false), ["fox", "fo"]);
        assert_eq!(scan(" a bb c.dd", "", true), ["bb", "dd"]);
        assert_eq!(scan(" a bb c.dd", "", false), ["dd", "bb"]);
        assert_eq!(scan(" x.foo", "f", true), ["foo"]);
        assert_eq!(scan(" café cafés", "caf", true), ["café", "cafés"]);
        assert_eq!(scan(" 日本語 日本人", "日", false), ["日本人", "日本語"]);
        // Each distinct word once.
        assert_eq!(scan(" ab\ncd ab", "a", true), ["ab"]);
    }

    /// Words, rows and runs without one longer than a step reads.
    #[test]
    fn words_span_windows() {
        let long = format!("f{}", "o".repeat(20_000));
        let text = format!(
            " {} {long} fa\n{}\nfb",
            "x".repeat(10_000),
            "y.".repeat(9_000)
        );
        assert_eq!(scan(&text, "f", true), [long.as_str(), "fa", "fb"]);
        assert_eq!(scan(&text, "f", false), ["fb", "fa", long.as_str()]);
        assert_eq!(scan(&text, "", true)[1..3], [long.as_str(), "fa"]);
        let keyword = "k".repeat(10_000);
        assert_eq!(
            scan(&format!(" {keyword}s"), &keyword, true),
            [format!("{keyword}s")]
        );
    }

    /// Graphemes longer than a step reads, before a word, in one and in the
    /// keyword, are classed whole: an emoji with combining marks isn't a
    /// word character's class, a letter with them is.
    #[test]
    fn long_graphemes_stay_whole() {
        let marks = "\u{301}".repeat(3_000);
        let emoji = format!("\u{1F642}{marks}");
        let text = format!(" {emoji}foo");
        assert_eq!(scan(&text, "f", true), ["foo"]);
        assert_eq!(scan(&text, "f", false), ["foo"]);
        // As nvim's first: a grapheme of a class of its own before a word.
        assert_eq!(scan(&text, "", true)[0], emoji);
        let word = format!("e{marks}x");
        assert_eq!(scan(&format!(" {word}"), "e", true), [word.as_str()]);
        assert_eq!(scan(&format!(" {word}"), "e", false), [word.as_str()]);
        assert_eq!(
            scan(&format!(" {word}"), &format!("e{marks}"), true),
            [word]
        );
    }
    /// With no keyword, the words are nvim's chain of `\<\k\k` matches: a
    /// word start inside the match before isn't one.
    #[test]
    fn no_keyword_follows_nvims_chain() {
        assert_eq!(scan(" a日本語", "", true), ["a"]);
        assert_eq!(scan(" a日本語", "", false), ["a"]);
        assert_eq!(scan(" a日b本c語d", "", true), ["a", "b", "c"]);
        assert_eq!(scan(" a日b本c語d", "", false), ["c", "b", "a"]);
        assert_eq!(scan(" ab日本 cd", "", true), ["ab", "日本", "cd"]);
        assert_eq!(scan(" 日a本", "", true), ["日"]);
        // Runs of one-grapheme words of alternating classes, longer than
        // a window: every other one matches from the run's start, both
        // ways and whichever class it starts with.
        let letter = |ix: u32| char::from(b'a' + (ix % 26) as u8).to_string();
        let pairs: String = (0..3_000).map(|ix| format!("{}日", letter(ix))).collect();
        let letters = |order: &mut dyn Iterator<Item = u32>| {
            let mut seen = Vec::new();
            for ix in order {
                if !seen.contains(&letter(ix)) {
                    seen.push(letter(ix));
                }
            }
            seen
        };
        let text = format!(" {pairs}");
        assert_eq!(scan(&text, "", true), letters(&mut (0..3_000)));
        assert_eq!(scan(&text, "", false), letters(&mut (0..3_000).rev()));
        let text = format!(" 日{pairs}");
        assert_eq!(scan(&text, "", true), ["日"]);
        assert_eq!(scan(&text, "", false), ["日"]);
    }
}
