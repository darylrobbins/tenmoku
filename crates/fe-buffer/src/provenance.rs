//! Who wrote which bytes of a buffer: the [`Provenance`] a
//! [`Buffer`](crate::Buffer) keeps as its batches apply.
//!
//! The text is held as runs in a [`SumTree`], each a length and an author or
//! none, covering every byte, with no two neighbours alike. So an edit finds
//! its place in O(log runs), replaces the runs it touches, and shifts the rest
//! without visiting them, and typing lengthens one run.
//!
//! A batch isn't spliced in as it applies: nothing needs its spans until they
//! are next read, or an undo or redo needs the spans batches replaced. So it
//! waits in a list, and the tree catches up at that point, at O(log runs + runs
//! touched) a batch, the spans each batch replaced going into its
//! [`ReplacedSpans`]. A keystroke costs a push, where a splice would cost it
//! several times over; `docs/decisions/jkfesx77as-provenance.md` has the
//! numbers.

use std::{
    fmt,
    ops::Range,
    sync::{Arc, Mutex, MutexGuard, OnceLock, PoisonError},
};

use fe_text::Text;
use sum_tree::{Bias, ContextLessSummary, Cursor, Dimension, Item, SumTree};

use crate::{Edit, Origin};

/// Who wrote a span of text. Text with no author, as read from disk, has no
/// span.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Author {
    User,
    Ai {
        agent: Arc<str>,
    },
    /// An editor tool: `"lsp"` or `"format"`.
    Tool(&'static str),
    Host(Arc<str>),
}

impl From<&Origin> for Author {
    fn from(origin: &Origin) -> Self {
        match origin {
            Origin::User => Author::User,
            Origin::Ai { agent, .. } => Author::Ai {
                agent: agent.clone(),
            },
            Origin::Lsp => Author::Tool("lsp"),
            Origin::Format => Author::Tool("format"),
            Origin::Host(name) => Author::Host(name.clone()),
        }
    }
}

impl Author {
    /// Which author a row goes to when several wrote in it; the higher wins.
    fn row_rank(&self) -> u8 {
        match self {
            Author::Ai { .. } => 2,
            Author::User => 1,
            Author::Tool(_) | Author::Host(_) => 0,
        }
    }
}

/// A span of a batch's replaced text, as offsets into the batch's old texts
/// laid end to end.
type ReplacedSpan = (Range<usize>, Author);

/// The authored spans of the text a batch replaced, kept beside it in the
/// history: what undoing it gives back to who wrote them. A fresh batch's are
/// known once [`Provenance`] has settled it, which an undo or redo sees to
/// before it reads them. An insertion replaces nothing, so holds nothing.
#[derive(Clone, Default)]
pub(crate) struct ReplacedSpans(Option<Arc<OnceLock<Vec<ReplacedSpan>>>>);

impl ReplacedSpans {
    fn known(spans: Vec<ReplacedSpan>) -> Self {
        Self((!spans.is_empty()).then(|| Arc::new(OnceLock::from(spans))))
    }

    fn get(&self) -> &[ReplacedSpan] {
        let Some(spans) = &self.0 else {
            return &[];
        };
        let spans = spans.get();
        debug_assert!(spans.is_some(), "read before its batch settled");
        spans.map_or(&[], Vec::as_slice)
    }
}

/// The buffer's text as sorted, non-overlapping byte spans, each with an
/// [`Author`]. Adjacent spans with one author are one span, so typing keeps
/// one span. Reads take O(log spans) for each batch applied since the last.
pub struct Provenance {
    state: Mutex<State>,
}

struct State {
    runs: Runs,
    /// The batches the runs haven't taken yet, oldest first.
    pending: Vec<Pending>,
}

struct Pending {
    /// Shared with the edit log.
    edits: Arc<[Edit]>,
    author: Option<Author>,
    replaced: ReplacedSpans,
}

impl State {
    fn settle(&mut self) {
        for batch in self.pending.drain(..) {
            let incoming = Incoming::Uniform(batch.author.as_ref());
            let replaced = self.runs.splice(&batch.edits, incoming);
            if let Some(slot) = &batch.replaced.0 {
                let _ = slot.set(replaced);
            }
        }
    }
}

impl Provenance {
    /// `len` bytes of text with no author.
    pub(crate) fn unauthored(len: usize) -> Self {
        Self {
            state: Mutex::new(State {
                runs: Runs::unauthored(len),
                pending: Vec::new(),
            }),
        }
    }

    fn state(&mut self) -> &mut State {
        self.state.get_mut().unwrap_or_else(PoisonError::into_inner)
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Catches the runs up with every batch, so that each knows what it
    /// replaced.
    pub(crate) fn settle(&mut self) {
        self.state().settle();
    }

    /// The runs, caught up with every batch. O(1) once caught up.
    pub(crate) fn settled(&self) -> Runs {
        let mut state = self.lock();
        state.settle();
        state.runs.clone()
    }

    /// Takes `runs`, derived from [`Provenance::settled`] with no batch
    /// recorded since.
    pub(crate) fn set_settled(&mut self, runs: Runs) {
        let state = self.state();
        debug_assert!(state.pending.is_empty());
        state.runs = runs;
    }

    /// Forgets every author; the text is `len` bytes long. Batches in the
    /// history keep the spans they replaced.
    pub(crate) fn clear(&mut self, len: usize) {
        let state = self.state();
        state.settle();
        state.runs = Runs::unauthored(len);
    }

    /// Follows a fresh batch whose new text is `author`'s. Returns what will
    /// hold the spans it replaces.
    pub(crate) fn record(&mut self, edits: &Arc<[Edit]>, author: Option<Author>) -> ReplacedSpans {
        let replaced = if edits.iter().all(|edit| edit.old.is_empty()) {
            ReplacedSpans::default()
        } else {
            ReplacedSpans(Some(Arc::new(OnceLock::new())))
        };
        self.state().pending.push(Pending {
            edits: edits.clone(),
            author,
            replaced: replaced.clone(),
        });
        replaced
    }

    /// Every span, in text order.
    pub fn spans(&self) -> Vec<(Range<usize>, Author)> {
        let runs = self.settled();
        let mut spans = Vec::new();
        let mut start = 0;
        for run in runs.0.iter() {
            if let Some(author) = &run.author {
                spans.push((start..start + run.len, author.clone()));
            }
            start += run.len;
        }
        spans
    }

    /// `rows` of `text`, the text this provenance is of, clipped to it, as
    /// runs of rows by author. A row with any AI-authored byte is that AI's,
    /// the last such span's agent; else any user-authored byte makes it the
    /// user's; else it is the last tool's or host's; else it has no author.
    ///
    /// A row's bytes are its text, not its line break, so a line break typed
    /// at the end of someone else's line doesn't claim it; an empty row's only
    /// byte is its line break. O(rows log spans).
    pub fn row_authors(&self, text: &Text, rows: Range<u32>) -> Vec<(Range<u32>, Option<Author>)> {
        let runs = self.settled();
        let count = text.row_count();
        let mut cursor = runs.0.cursor::<Offset>(());
        cursor.seek(&Offset(0), Bias::Right);
        let mut authors: Vec<(Range<u32>, Option<Author>)> = Vec::new();
        for row in rows.start.min(count)..rows.end.min(count) {
            let start = text.row_start(row);
            let len = match text.row_len(row) as usize {
                0 if row + 1 < count => 1,
                len => len,
            };
            let author = row_author(&mut cursor, start..start + len);
            match authors.last_mut() {
                Some((rows, last)) if *last == author => rows.end = row + 1,
                _ => authors.push((row..row + 1, author)),
            }
        }
        authors
    }
}

impl fmt::Debug for Provenance {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_list().entries(self.spans()).finish()
    }
}

/// The author `bytes` goes to, as [`Provenance::row_authors`] tells it.
/// `bytes` must not start before the cursor's run.
fn row_author(cursor: &mut Cursor<'_, '_, Run, Offset>, bytes: Range<usize>) -> Option<Author> {
    if bytes.is_empty() {
        return None;
    }
    cursor.seek_forward(&Offset(bytes.start), Bias::Right);
    let mut best: Option<&Author> = None;
    while let Some(run) = cursor.item()
        && cursor.start().0 < bytes.end
    {
        if let Some(author) = &run.author
            && best.is_none_or(|best| author.row_rank() >= best.row_rank())
        {
            best = Some(author);
        }
        if cursor.end().0 >= bytes.end {
            break;
        }
        cursor.next();
    }
    best.cloned()
}

/// What a batch's new text is authored by.
#[derive(Copy, Clone)]
enum Incoming<'a> {
    /// All of it by this author, or none.
    Uniform(Option<&'a Author>),
    /// These spans of it, the rest by none, as offsets into its new texts laid
    /// end to end. For undo and redo, which put back what a batch replaced.
    Spans(&'a ReplacedSpans),
}

/// The text's bytes as runs by author; see the [module docs](self). Clones
/// share the tree, so an undo can work on one and drop it if it fails.
#[derive(Clone)]
pub(crate) struct Runs(SumTree<Run>);

impl Runs {
    fn unauthored(len: usize) -> Self {
        let tree = if len == 0 {
            SumTree::new(())
        } else {
            SumTree::from_item(Run { len, author: None }, ())
        };
        Self(tree)
    }

    /// Follows a batch that undoes or redoes one which replaced `restored`:
    /// that goes back as its new text, and what it replaces is returned for
    /// the batch that undoes it in turn.
    pub(crate) fn restore(&mut self, edits: &[Edit], restored: &ReplacedSpans) -> ReplacedSpans {
        ReplacedSpans::known(self.splice(edits, Incoming::Spans(restored)))
    }

    /// Follows a batch: the spans of its old ranges go and its new text's come
    /// in. Returns the spans that went, as offsets into its old texts laid end
    /// to end. O(k log runs + runs touched) for k edits.
    fn splice(&mut self, edits: &[Edit], incoming: Incoming<'_>) -> Vec<ReplacedSpan> {
        let mut replaced = Vec::new();
        let mut next = Splice::new(&self.0);
        // Where the edit's old and new text start in the batch's, end to end.
        let (mut old_at, mut new_at) = (0, 0);
        let mut spans = 0;
        for edit in edits {
            next.copy_to(edit.old.start);
            next.skip_to(edit.old.end, |range, author| {
                let start = old_at + (range.start - edit.old.start);
                replaced.push((start..start + range.len(), author.clone()));
            });
            old_at += edit.old.len();
            match incoming {
                Incoming::Uniform(author) => next.push(edit.new.len(), author),
                Incoming::Spans(all) => {
                    let end = new_at + edit.new.len();
                    next.push_spans(all.get(), &mut spans, new_at..end);
                    new_at = end;
                }
            }
        }
        let len = self.0.summary().len;
        self.0 = next.finish(len);
        replaced
    }
}

/// Copies a tree of runs, leaving out and adding what a batch's edits do.
struct Splice<'a> {
    cursor: Cursor<'a, 'a, Run, Offset>,
    built: SumTree<Run>,
    /// How far through the old tree this has got. The cursor's run holds this
    /// offset, unless it's the tree's end.
    at: usize,
}

impl<'a> Splice<'a> {
    fn new(tree: &'a SumTree<Run>) -> Self {
        let mut cursor = tree.cursor::<Offset>(());
        cursor.seek(&Offset(0), Bias::Right);
        Self {
            cursor,
            built: SumTree::new(()),
            at: 0,
        }
    }

    /// Copies the rest of the old tree, `len` bytes in all, and returns the
    /// new one.
    fn finish(mut self, len: usize) -> SumTree<Run> {
        self.copy_to(len);
        self.built
    }

    /// Appends `len` bytes by `author`, joining the last run if it's theirs.
    fn push(&mut self, len: usize, author: Option<&Author>) {
        if len == 0 {
            return;
        }
        if self
            .built
            .last()
            .is_some_and(|last| last.author.as_ref() == author)
        {
            self.built.update_last(|last| last.len += len, ());
        } else {
            let author = author.cloned();
            self.built.push(Run { len, author }, ());
        }
    }

    /// Appends `range` of a batch's new text, end to end, from sorted `spans`,
    /// the bytes no span covers by no author. `from` is the first span not yet
    /// used up, kept from one call to the next.
    fn push_spans(&mut self, spans: &[ReplacedSpan], from: &mut usize, range: Range<usize>) {
        let mut at = range.start;
        while at < range.end {
            let Some((span, author)) = spans.get(*from) else {
                self.push(range.end - at, None);
                return;
            };
            if span.end <= at {
                *from += 1;
            } else if span.start > at {
                let end = span.start.min(range.end);
                self.push(end - at, None);
                at = end;
            } else {
                let end = span.end.min(range.end);
                self.push(end - at, Some(author));
                at = end;
            }
        }
    }

    /// Moves on over the part of the cursor's run up to `target`, returning
    /// the range it covered and its author.
    fn advance(&mut self, target: usize) -> Option<(Range<usize>, Option<&'a Author>)> {
        let run = self.cursor.item()?;
        let run_end = self.cursor.end().0;
        let end = run_end.min(target);
        let range = self.at..end;
        self.at = end;
        if run_end <= target {
            self.cursor.next();
        }
        Some((range, run.author.as_ref()))
    }

    /// Copies the old tree's text up to `target`.
    fn copy_to(&mut self, target: usize) {
        if self.at >= target {
            return;
        }
        // The first run by hand, as it's the one that can join what's built,
        // or may be only part copied. The runs after it are unlike their
        // neighbours already, so they go in whole.
        if let Some((range, author)) = self.advance(target) {
            self.push(range.len(), author);
        }
        if self.at < target {
            let whole = self.cursor.slice(&Offset(target), Bias::Right);
            self.built.append(whole, ());
            self.at = self.cursor.start().0;
        }
        if self.at < target
            && let Some((range, author)) = self.advance(target)
        {
            self.push(range.len(), author);
        }
    }

    /// Passes over the old tree's text up to `target`, telling `replaced` of
    /// each authored part.
    fn skip_to(&mut self, target: usize, mut replaced: impl FnMut(Range<usize>, &Author)) {
        while self.at < target {
            let Some((range, author)) = self.advance(target) else {
                return;
            };
            if let Some(author) = author {
                replaced(range, author);
            }
        }
    }
}

#[derive(Clone, Debug)]
struct Run {
    len: usize,
    author: Option<Author>,
}

#[derive(Clone, Debug, Default)]
struct RunSummary {
    len: usize,
}

impl Item for Run {
    type Summary = RunSummary;

    fn summary(&self, (): ()) -> RunSummary {
        RunSummary { len: self.len }
    }
}

impl ContextLessSummary for RunSummary {
    fn zero() -> Self {
        Self::default()
    }

    fn add_summary(&mut self, summary: &Self) {
        self.len += summary.len;
    }
}

/// A byte offset into the text, to seek the tree by.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
struct Offset(usize);

impl Dimension<'_, RunSummary> for Offset {
    fn zero((): ()) -> Self {
        Self::default()
    }

    fn add_summary(&mut self, summary: &RunSummary, (): ()) {
        self.0 += summary.len;
    }
}
