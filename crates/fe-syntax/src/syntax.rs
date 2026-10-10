use std::{
    cell::{Cell, RefCell},
    collections::VecDeque,
    ops::{ControlFlow, Range},
    panic::{AssertUnwindSafe, catch_unwind, resume_unwind},
    time::{Duration, Instant},
};

use fe_buffer::{BufferId, BufferSnapshot, Text, Version};
use tree_sitter::{
    InputEdit, Node, ParseOptions, Parser, QueryCursor, QueryCursorOptions, StreamingIterator as _,
    Tree,
};

use crate::{
    HighlightSpan, Language,
    highlight::{Capture, Painted, flatten, highlighted},
    injection::{self, Layers},
};

/// How much of a file over its language's [`Language::parse_limit`] is parsed around the visible
/// text (see [`Syntax::window_job`]). At the densest measured tree, 96× its
/// source, a window's tree stays near 3 MB.
const WINDOW_BYTES: usize = 32 * 1024;

/// Nodes with more children than this are highlighted a child at a time.
///
/// Tree-sitter keeps a node's repeated children in a tree of hidden nodes
/// that is only partly balanced: in a JSON array of a million numbers, the
/// early elements sit about a thousand hidden levels deep. A query cursor
/// started above such a node walks up through those levels at every child it
/// visits, so one screen near the top of the array costs about 0.4 ms. A
/// cursor started at the child itself doesn't.
const FLAT_CHILDREN: u32 = 1024;

/// How long one [`Syntax::highlights`] call may query and parse injected
/// layers for. Sixty rows take about 0.1 ms, but a debug build or a loaded
/// machine can take longer; the highlights then stop where the time ran out,
/// and the range is queried again with twice the time, up to
/// [`QUERY_BUDGET_MAX`]. Tests have a deadline too far off to pass unless
/// they say (see `DEADLINE_AFTER`), and the `no-query-budget` feature has
/// none, so a busy machine can't make them flaky.
const QUERY_BUDGET: Duration = if cfg!(test) {
    Duration::from_secs(3600)
} else if cfg!(feature = "no-query-budget") {
    Duration::MAX
} else {
    Duration::from_millis(2)
};

/// The most time a range's query is given, after running out of 2, 4 and
/// 8 ms. One that runs out of 16 ms is pathological: what it found is cached
/// like a complete result, so it isn't queried, and drawn, forever.
const QUERY_BUDGET_MAX: Duration = QUERY_BUDGET.saturating_mul(8);

/// How long [`ParseJob::run`] lets a parse take before giving up on it.
/// Tree-sitter's progress callback is the only way to stop one, and some
/// grammars have inputs whose parse never ends (the TSX ones in
/// `fuzz/regressions/syntax_edits`), from scratch as well as incrementally.
///
/// A release-build full parse of a file at the old 8 MB limit took 0.85 s
/// for 8 MB of Rust and 1.7 s for 8 MB of dense JSON, the slowest measured.
/// [`FULL_PARSE_CAP`] is 5 s, about 3× that, so no legitimate file is cut
/// off. An incremental parse that overruns [`INCREMENTAL_PARSE_CAP`] is
/// retried from scratch, so a cap that is too tight only costs time; 2 s is
/// above any full parse of a typical file. Together they stay under 10 s, so
/// a runaway parse is abandoned within 7 s.
const INCREMENTAL_PARSE_CAP: Duration = Duration::from_secs(2);
const FULL_PARSE_CAP: Duration = Duration::from_secs(5);

/// How many recently highlighted ranges [`Syntax`] remembers. A frame queries
/// one range per run of consecutive visible rows.
const CACHED_RANGES: usize = 8;

/// How many ranges whose query ran out of time [`Syntax`] remembers. Far
/// more than a frame queries, so that cycling through a frame's ranges, more
/// than [`CACHED_RANGES`] of them, never forgets one's attempts and starts it
/// again at [`QUERY_BUDGET`], asking for a redraw forever.
const CUT_SHORT_RANGES: usize = 1024;

/// What [`Syntax::highlights`] keeps between calls.
#[derive(Default)]
struct Cache {
    /// Recent results for the current tree, newest last, so frames that
    /// neither scroll nor edit don't query again.
    ranges: VecDeque<(Range<usize>, Vec<HighlightSpan>)>,
    /// Recent ranges whose query ran out of time on the current tree, newest
    /// last, with how many times it did: the next query of one gets
    /// [`QUERY_BUDGET`] doubled that many times. A range that ran out of
    /// [`QUERY_BUDGET_MAX`] stays here, so if `ranges` drops its result, it's
    /// queried again with that much time and cached, without a redraw.
    cut_short: VecDeque<(Range<usize>, u32)>,
    /// Kept across edits, which are applied to them as to the tree.
    layers: Layers,
}

impl Cache {
    /// Forgets the results for the current tree.
    fn forget_ranges(&mut self) {
        self.ranges.clear();
        self.cut_short.clear();
    }

    fn clear(&mut self) {
        self.forget_ranges();
        self.layers.clear();
    }
}

/// A buffer's syntax tree, kept up to date incrementally.
///
/// [`Syntax::edit`] catches the tree up with a newer snapshot cheaply,
/// without parsing; the stale tree still highlights correctly outside the
/// edited text. Reparsing happens either on the calling thread within a time
/// budget ([`Syntax::parse_within`]) or anywhere via a [`ParseJob`].
///
/// A file over its language's [`Language::parse_limit`] has no tree. Instead, a window of it
/// around the visible text can be parsed with [`Syntax::window_job`]; that
/// highlights approximately, as if the file started at the window.
///
/// A panic while parsing or highlighting is caught and logged, and the buffer
/// shows as plain text from then on, since trying again would most likely
/// panic again.
pub struct Syntax {
    language: &'static Language,
    snapshot: BufferSnapshot,
    /// Edited to match `snapshot`, though maybe not reparsed since.
    tree: Option<Tree>,
    /// The version `tree` was last parsed at.
    parsed: Option<Version>,
    /// A parse of only the bytes in its range, for a file too big for
    /// `tree`. Edited to match `snapshot` like `tree`, along with its range.
    window: Option<(Range<usize>, Tree)>,
    /// The version `window` was last parsed at.
    window_parsed: Option<Version>,
    cache: RefCell<Cache>,
    /// Set when parsing or highlighting panicked.
    failed: Cell<bool>,
    /// How long the last incremental reparse took, if there's been one since
    /// the tree was last dropped. A first full parse says nothing about the
    /// cost of a keystroke, so it isn't recorded.
    last_reparse: Option<Duration>,
}

impl Syntax {
    pub fn new(language: &'static Language, snapshot: &BufferSnapshot) -> Self {
        Self {
            language,
            snapshot: snapshot.clone(),
            tree: None,
            parsed: None,
            window: None,
            window_parsed: None,
            cache: RefCell::default(),
            failed: Cell::new(false),
            last_reparse: None,
        }
    }

    pub fn language(&self) -> &'static Language {
        self.language
    }

    /// The snapshot the tree has been edited to match.
    pub fn version(&self) -> Version {
        self.snapshot.version()
    }

    /// The text of the snapshot the tree has been edited to match.
    pub(crate) fn text(&self) -> &Text {
        self.snapshot.text()
    }

    pub(crate) fn buffer_id(&self) -> BufferId {
        self.snapshot.id()
    }

    /// Applies the edits made since the last call to the tree, so it lines up
    /// with `snapshot`. Cheap: nothing is reparsed.
    pub fn edit(&mut self, snapshot: &BufferSnapshot) {
        let version = self.version();
        let cache = self.cache.get_mut();
        if snapshot.id() != self.snapshot.id() || snapshot.version() != version {
            cache.forget_ranges();
        }
        let mapped = snapshot.id() == self.snapshot.id()
            && snapshot.version() >= version
            && !self.failed.get();
        if !mapped {
            // A different buffer, history we can't map, or given up on.
            self.window = None;
            self.window_parsed = None;
        } else if let edits = input_edits(self.snapshot.text(), snapshot, self.snapshot.version())
            && !edits.is_empty()
        {
            if let Some(tree) = &mut self.tree {
                for edit in &edits {
                    tree.edit(edit);
                }
            }
            if let Some((range, tree)) = &mut self.window {
                for edit in &edits {
                    tree.edit(edit);
                }
                *range = included_range(tree);
            }
            for edit in &edits {
                cache.layers.edit(edit);
            }
        }
        if !mapped || snapshot.len() > self.language.parse_limit() {
            // Too big to parse whole: only a window highlights, and the
            // layers it needs are parsed afresh.
            if self.tree.take().is_some() || !mapped {
                cache.layers.clear();
            }
            self.parsed = None;
            self.last_reparse = None;
        }
        self.snapshot = snapshot.clone();
    }

    /// Whether the tree is missing or hasn't been reparsed since the last edit.
    pub fn needs_parse(&self) -> bool {
        self.snapshot.len() <= self.language.parse_limit()
            && self.parsed != Some(self.version())
            && !self.failed.get()
    }

    /// Whether parsing or highlighting panicked, leaving the buffer plain text.
    pub fn failed(&self) -> bool {
        self.failed.get()
    }

    /// Reparses on this thread unless that takes longer than `budget`.
    /// Returns whether the tree no longer needs parsing.
    pub fn parse_within(&mut self, budget: Duration) -> bool {
        let started = Instant::now();
        self.parse_until(budget, || started.elapsed() > budget)
    }

    /// Reparses on this thread, giving up as soon as `out_of_time` returns
    /// true; tree-sitter asks it every so often while parsing. Returns
    /// whether the tree no longer needs parsing.
    ///
    /// Returns false without parsing if the last incremental reparse took
    /// longer than `budget`, since this one would most likely overrun too. A
    /// background parse that finishes within it turns this back on.
    pub fn parse_until(&mut self, budget: Duration, out_of_time: impl FnMut() -> bool) -> bool {
        if !self.needs_parse() {
            return true;
        }
        if self.last_reparse.is_some_and(|took| took > budget) {
            return false;
        }
        match self.parse_job().parse(out_of_time) {
            Some(parsed) => {
                self.finish(parsed);
                true
            }
            None => false,
        }
    }

    /// A reparse of the current text that can run on any thread. Pass the
    /// result to [`Syntax::finish`].
    pub fn parse_job(&self) -> ParseJob {
        ParseJob {
            language: self.language,
            snapshot: self.snapshot.clone(),
            old_tree: self.tree.clone(),
            window: None,
        }
    }

    /// For a file over the language's parse limit, a parse of the 32 KB
    /// around `visible`, which can run on any thread. Pass the result to
    /// [`Syntax::finish`]. `None` for a smaller file, one given up on, or
    /// when the window would hold no row start to begin at, as in the middle
    /// of a very long line.
    ///
    /// The window starts at a row start, where an item most likely starts,
    /// and ends on a char boundary. It's parsed from scratch as if the file
    /// held only its text, so a string or comment opened above it highlights
    /// wrongly.
    pub fn window_job(&self, visible: Range<usize>) -> Option<ParseJob> {
        let text = self.snapshot.text();
        if text.len() <= self.language.parse_limit() || self.failed.get() {
            return None;
        }
        let middle = (visible.start.min(text.len()) + visible.end.min(text.len())) / 2;
        let start = middle
            .saturating_sub(WINDOW_BYTES / 2)
            .min(text.len() - WINDOW_BYTES);
        let end = text.clip_offset(start + WINDOW_BYTES, fe_buffer::Bias::Left);
        let point = text.offset_to_point(start);
        let start = if point.column == 0 {
            start
        } else {
            text.point_to_offset(fe_buffer::Point::new(point.row + 1, 0))
        };
        (start < end).then(|| ParseJob {
            language: self.language,
            snapshot: self.snapshot.clone(),
            old_tree: None,
            window: Some(start..end),
        })
    }

    /// The text the current window was parsed from, edited along with it,
    /// if there is one. Its highlights are used while there's no full tree.
    pub fn window(&self) -> Option<Range<usize>> {
        self.window.as_ref().map(|(range, _)| range.clone())
    }

    /// Installs a parse result. One for an older version than the current
    /// snapshot is still newer than the tree's last parse, so it's edited
    /// forward and used; [`Syntax::needs_parse`] stays true.
    pub fn finish(&mut self, parsed: Parsed) {
        let Some(mut tree) = parsed.tree else {
            self.failed.set(true);
            self.tree = None;
            self.window = None;
            self.cache.get_mut().clear();
            return;
        };
        let version = parsed.snapshot.version();
        let usable = parsed.snapshot.id() == self.snapshot.id()
            && version <= self.version()
            && if parsed.window.is_some() {
                // Another window at the same version is for another view.
                self.snapshot.len() > self.language.parse_limit()
                    && self.window_parsed.is_none_or(|current| current <= version)
            } else {
                self.snapshot.len() <= self.language.parse_limit()
                    && self.parsed.is_none_or(|current| current < version)
            };
        if !usable || self.failed.get() {
            return;
        }
        for edit in input_edits(parsed.snapshot.text(), &self.snapshot, version) {
            tree.edit(&edit);
        }
        self.cache.get_mut().forget_ranges();
        if parsed.window.is_some() {
            self.window = Some((included_range(&tree), tree));
            self.window_parsed = Some(version);
            return;
        }
        if parsed.incremental {
            self.last_reparse = Some(parsed.elapsed);
        }
        self.tree = Some(tree);
        self.parsed = Some(version);
        self.window = None;
        self.window_parsed = None;
        // The layers stay: they were edited along with the old tree.
    }

    pub fn tree(&self) -> Option<&Tree> {
        self.tree.as_ref()
    }

    /// Highlights within `range` of the current snapshot, sorted and
    /// non-overlapping, including those of injected languages (see
    /// [`crate::injection`]). Only the part of the tree touching `range` is
    /// queried, and only the injections overlapping it are parsed, so pass
    /// the visible text, not the whole file. Injected layers far from the
    /// recently highlighted ranges are dropped. Asking for the same range again
    /// before the tree changes doesn't query again.
    ///
    /// A query that runs out of [`QUERY_BUDGET`] returns the highlights found
    /// so far, which color the top of the range and leave the rest plain. They
    /// aren't cached: the next call queries the range again with twice the
    /// time, and [`Syntax::take_missing_languages`] returns the host language
    /// so that call comes without waiting for input. A range that runs out
    /// of [`QUERY_BUDGET_MAX`] too keeps what it found, cached like a complete
    /// result. If time runs out while finding or parsing injections, or an
    /// injected language isn't compiled yet, the result isn't cached either:
    /// the next call tries again, keeping the layers parsed so far.
    ///
    /// Without a full tree, the window's highlights are used, those in
    /// `range` ∩ [`Syntax::window`].
    pub fn highlights(&self, range: Range<usize>) -> Vec<HighlightSpan> {
        if self.failed.get() {
            return Vec::new();
        }
        let text = self.snapshot.text();
        let range = range.start.min(text.len())..range.end.min(text.len());
        let (tree, parsed, range) = match (&self.tree, &self.window) {
            (Some(tree), _) => (tree, self.parsed, range),
            (None, Some((window, tree))) => {
                let start = range.start.max(window.start);
                let end = range.end.min(window.end);
                if start >= end {
                    return Vec::new();
                }
                (tree, self.window_parsed, start..end)
            }
            (None, None) => return Vec::new(),
        };
        let mut cache = self.cache.borrow_mut();
        let cache = &mut *cache;
        if let Some((_, spans)) = cache.ranges.iter().find(|(cached, _)| *cached == range) {
            // Shown again, so its layers are as recent as a query's.
            cache.layers.visited(range);
            return spans.clone();
        }

        let parsed = parsed == Some(self.version());
        let attempt = cache.cut_short.iter().position(|(cut, _)| *cut == range);
        let attempts = attempt.map_or(0, |attempt| cache.cut_short[attempt].1);
        let budget = QUERY_BUDGET.saturating_mul(1 << attempts);
        let layers = &mut cache.layers;
        let query = || {
            // `None` for a budget too long to be a time.
            let deadline = Instant::now().checked_add(budget);
            let language = self.language;
            let (spans, finished) = highlight_query_finished(
                language,
                text,
                tree,
                range.clone(),
                language.local_patterns,
                deadline,
            );
            let host = injection::Host {
                language,
                text,
                tree,
                parsed,
            };
            let (spans, complete) = injection::paint(host, range.clone(), deadline, layers, spans);
            (spans, finished, complete)
        };
        let Ok((spans, finished, complete)) = catch_unwind(AssertUnwindSafe(query)) else {
            log::error!(
                "{}: highlighting panicked; showing plain text",
                self.language.name()
            );
            self.failed.set(true);
            return Vec::new();
        };
        let spans = highlighted(spans);
        if let Some(attempt) = attempt {
            cache.cut_short.remove(attempt);
        }
        // Whether the next call gets more time for what this one didn't find.
        let retry = !finished && budget < QUERY_BUDGET_MAX;
        if retry {
            // Draw again, so the rows past the cut get their colors.
            let name = self.language.name();
            if !cache.layers.missing.contains(&name) {
                cache.layers.missing.push(name);
            }
        }
        if !finished {
            if cache.cut_short.len() == CUT_SHORT_RANGES {
                cache.cut_short.pop_front();
            }
            cache
                .cut_short
                .push_back((range.clone(), attempts + u32::from(retry)));
        }
        if complete && !retry {
            if cache.ranges.len() == CACHED_RANGES {
                cache.ranges.pop_front();
            }
            cache.ranges.push_back((range, spans.clone()));
        }
        spans
    }

    /// Takes the injected languages that [`Syntax::highlights`] has found
    /// uncompiled since the last call, so their code showed plain. Compile
    /// each with [`crate::language_by_name`] off the UI thread, as that takes
    /// up to 20 ms, then highlight again.
    ///
    /// A language compiled since it was found is still returned: the
    /// highlights already shown lack it all the same, and need redrawing.
    /// So is the buffer's own language after a query that ran out of time
    /// and wasn't cached, though it's compiled: the rows past the cut showed
    /// plain, and the next call has more time for them.
    pub fn take_missing_languages(&self) -> Vec<&'static str> {
        std::mem::take(&mut self.cache.borrow_mut().layers.missing)
    }

    /// [`Syntax::highlights`], after compiling the injected languages it
    /// needs on this thread, highlighting again while any were missing.
    #[cfg(test)]
    pub(crate) fn highlights_compiling(&self, range: Range<usize>) -> Vec<HighlightSpan> {
        loop {
            let spans = self.highlights(range.clone());
            let missing = self.take_missing_languages();
            if missing.is_empty() {
                return spans;
            }
            for name in missing {
                crate::language_by_name(name);
            }
        }
    }

    /// Queries `tree` for the highlights in `range` without injections,
    /// stopping after `budget`. See [`highlight_query_finished`].
    #[cfg(test)]
    pub(crate) fn query(
        &self,
        tree: &Tree,
        range: Range<usize>,
        split: bool,
        budget: Duration,
    ) -> Vec<HighlightSpan> {
        let deadline = Instant::now().checked_add(budget);
        highlighted(highlight_query(
            self.language,
            self.snapshot.text(),
            tree,
            range,
            split,
            deadline,
        ))
    }

    /// The languages and ranges of the injected layers kept, sorted.
    #[cfg(test)]
    pub(crate) fn layers(&self) -> Vec<injection::LayerKey> {
        self.cache.borrow().layers.keys()
    }

    /// A copy of the injected layers kept.
    #[cfg(test)]
    pub(crate) fn kept_layers(&self) -> injection::Layers {
        self.cache.borrow().layers.clone()
    }

    /// [`Syntax::layers`], with their trees as S-expressions.
    #[cfg(test)]
    pub(crate) fn layer_trees(&self) -> Vec<(injection::LayerKey, String)> {
        self.cache.borrow().layers.trees()
    }

    /// The injected layers parsed since the last call, fresh or
    /// incrementally, in order.
    #[cfg(test)]
    pub(crate) fn take_layer_parses(&self) -> Vec<injection::LayerKey> {
        std::mem::take(&mut self.cache.borrow_mut().layers.parses)
    }
}

/// Queries `tree`, of `language` and parsed from `text`, for the highlights
/// in `range`, stopping at `deadline`, with the runs `@none` cleared. With `split`, nodes with many children
/// are queried a child at a time; that's only correct when no pattern spans
/// several siblings. Also returns whether the query ran to the end, rather
/// than stopping at the deadline.
pub(crate) fn highlight_query_finished(
    language: &Language,
    text: &Text,
    tree: &Tree,
    range: Range<usize>,
    split: bool,
    deadline: Option<Instant>,
) -> (Vec<Painted>, bool) {
    fault_point("query");
    let mut captures = Vec::new();
    // Set once the deadline passes. As in finding injections, a split tree
    // can have a root per short statement, each queried too briefly for the
    // progress callback to check the deadline, so it's checked at each root.
    let mut stopped = false;
    for_each_query_root(tree, &range, split, |cursor, node| {
        if stopped || out_of_time(deadline) {
            stopped = true;
            return;
        }
        // Predicates like `#match?` read node text straight from the rope.
        let source = |node: Node| chunks(text, node.byte_range());
        let mut progress = |_: &_| {
            let flow = until(deadline);
            stopped |= flow.is_break();
            flow
        };
        let options = QueryCursorOptions::new().progress_callback(&mut progress);
        let mut matches = cursor.captures_with_options(&language.query, node, source, options);
        while let Some((found, ix)) = matches.next() {
            let capture = found.captures()[*ix];
            if let Some(highlight) = language.highlights[capture.index as usize] {
                let precedence = if language.earlier_patterns_win {
                    usize::MAX - found.pattern_index
                } else {
                    found.pattern_index
                };
                captures.push(Capture {
                    range: capture.node.byte_range(),
                    highlight,
                    precedence,
                });
            }
        }
    });
    if stopped {
        log::debug!(
            "{}: highlighting {range:?} ran out of time; showing what was found",
            language.name()
        );
    }
    (flatten(captures, range), !stopped)
}

/// [`highlight_query_finished`]'s highlights, for tests that don't need to
/// know whether it finished.
#[cfg(test)]
pub(crate) fn highlight_query(
    language: &Language,
    text: &Text,
    tree: &Tree,
    range: Range<usize>,
    split: bool,
    deadline: Option<Instant>,
) -> Vec<Painted> {
    highlight_query_finished(language, text, tree, range, split, deadline).0
}

/// Whether `deadline` has passed. `None` never does.
pub(crate) fn out_of_time(deadline: Option<Instant>) -> bool {
    deadline.is_some_and(|deadline| checked_deadline() || Instant::now() >= deadline)
}

/// Counts a deadline check, and says whether a test wants the deadline
/// passed by now.
#[cfg(test)]
fn checked_deadline() -> bool {
    let checks = DEADLINE_CHECKS.get() + 1;
    DEADLINE_CHECKS.set(checks);
    checks > DEADLINE_AFTER.get()
}

#[cfg(not(test))]
fn checked_deadline() -> bool {
    false
}

/// Tells tree-sitter to stop once `deadline` has passed.
pub(crate) fn until(deadline: Option<Instant>) -> ControlFlow<()> {
    if out_of_time(deadline) {
        ControlFlow::Break(())
    } else {
        ControlFlow::Continue(())
    }
}

/// Calls `run` with a cursor limited to `range` and each node to query
/// `tree` from, so that together they find each match touching `range` once.
/// That's just the root, unless `split`: then each node on [`flat_paths`] is
/// queried only for matches starting at it, and its children one by one.
pub(crate) fn for_each_query_root<'tree>(
    tree: &'tree Tree,
    range: &Range<usize>,
    split: bool,
    mut run: impl FnMut(&mut QueryCursor, Node<'tree>),
) {
    fn visit<'tree>(
        cursor: &mut QueryCursor,
        node: Node<'tree>,
        range: &Range<usize>,
        split: &[Node],
        run: &mut impl FnMut(&mut QueryCursor, Node<'tree>),
    ) {
        let split_here = split.contains(&node);
        cursor.set_max_start_depth(split_here.then_some(0));
        run(cursor, node);
        if !split_here {
            return;
        }
        let mut children = node.walk();
        if children.goto_first_child_for_byte(range.start).is_none() {
            return;
        }
        loop {
            let child = children.node();
            if child.start_byte() >= range.end {
                break;
            }
            visit(cursor, child, range, split, run);
            if !children.goto_next_sibling() {
                break;
            }
        }
    }

    let root = tree.root_node();
    let split = if split && !range.is_empty() {
        flat_paths(root, range)
    } else {
        Vec::new()
    };
    let mut cursor = QueryCursor::new();
    cursor.set_byte_range(range.clone());
    visit(&mut cursor, root, range, &split, &mut run);
}

/// The nodes to query a child at a time for `range`: each node with more than
/// [`FLAT_CHILDREN`] children on the way down to either end of the range, and
/// the nodes above it. A flat node that lies wholly inside the range isn't
/// split; its children are all visible anyway.
pub(crate) fn flat_paths<'tree>(root: Node<'tree>, range: &Range<usize>) -> Vec<Node<'tree>> {
    let mut split = Vec::new();
    for offset in [range.start, range.end - 1] {
        let mut cursor = root.walk();
        // The nodes on the way down since the last flat one.
        let mut path = Vec::new();
        loop {
            let node = cursor.node();
            path.push(node);
            if node.child_count() > FLAT_CHILDREN {
                split.append(&mut path);
            }
            if cursor.goto_first_child_for_byte(offset).is_none() {
                break;
            }
        }
    }
    split
}

pub(crate) fn chunks(text: &Text, range: Range<usize>) -> impl Iterator<Item = &[u8]> {
    // An edited, not yet reparsed tree can have a node end inside a char.
    let start = text.clip_offset(range.start, fe_buffer::Bias::Left);
    let end = text
        .clip_offset(range.end, fe_buffer::Bias::Right)
        .max(start);
    text.chunks_in_range(start..end).map(str::as_bytes)
}

/// A reparse that can run on any thread. See [`Syntax::parse_job`].
pub struct ParseJob {
    language: &'static Language,
    snapshot: BufferSnapshot,
    old_tree: Option<Tree>,
    /// The only text to parse, for a [`Syntax::window_job`].
    window: Option<Range<usize>>,
}

/// How one parse attempt ended.
enum Attempt {
    Tree(Tree),
    Overran,
    Panicked,
}

/// The result of a [`ParseJob`].
pub struct Parsed {
    /// `None` if parsing panicked or overran its cap.
    tree: Option<Tree>,
    snapshot: BufferSnapshot,
    /// The text parsed, if not all of it.
    window: Option<Range<usize>>,
    /// How long the parse took.
    pub(crate) elapsed: Duration,
    /// Whether it started from an earlier tree.
    pub(crate) incremental: bool,
}

impl Parsed {
    pub(crate) fn buffer_id(&self) -> BufferId {
        self.snapshot.id()
    }
}

impl ParseJob {
    /// Parses to the end, within [`INCREMENTAL_PARSE_CAP`] when starting from
    /// an earlier tree and [`FULL_PARSE_CAP`] from scratch. An incremental
    /// parse that overruns is retried once from scratch. Doesn't panic or
    /// hang: if parsing panics or both attempts overrun, the result makes
    /// [`Syntax::finish`] give up on highlighting.
    pub fn run(self) -> Parsed {
        self.run_until(|cap| {
            let started = Instant::now();
            move || started.elapsed() > cap
        })
    }

    /// [`ParseJob::run`] with each attempt's stop check made by `deadline`
    /// from the cap that attempt is under, so a test can force an overrun.
    pub(crate) fn run_until<F: FnMut() -> bool>(
        self,
        mut deadline: impl FnMut(Duration) -> F,
    ) -> Parsed {
        let started = Instant::now();
        let incremental = self.old_tree.is_some();
        let first_cap = if incremental {
            INCREMENTAL_PARSE_CAP
        } else {
            FULL_PARSE_CAP
        };
        let mut tree = self.attempt(self.old_tree.as_ref(), deadline(first_cap));
        if incremental && matches!(tree, Attempt::Overran) {
            log::warn!(
                "{}: incremental parse took over {INCREMENTAL_PARSE_CAP:?}; parsing from scratch",
                self.language.name()
            );
            tree = self.attempt(None, deadline(FULL_PARSE_CAP));
        }
        let tree = match tree {
            Attempt::Tree(tree) => Some(tree),
            Attempt::Overran => {
                log::error!(
                    "{}: parsing took too long; showing plain text",
                    self.language.name()
                );
                None
            }
            Attempt::Panicked => None,
        };
        Parsed {
            tree,
            snapshot: self.snapshot,
            window: self.window,
            elapsed: started.elapsed(),
            incremental,
        }
    }

    /// `None` if `out_of_time` says so before the parse completes.
    fn parse(self, out_of_time: impl FnMut() -> bool) -> Option<Parsed> {
        let started = Instant::now();
        let tree = match self.attempt(self.old_tree.as_ref(), out_of_time) {
            Attempt::Tree(tree) => Some(tree),
            Attempt::Overran => return None,
            Attempt::Panicked => None,
        };
        Some(Parsed {
            tree,
            snapshot: self.snapshot,
            window: self.window,
            elapsed: started.elapsed(),
            incremental: self.old_tree.is_some(),
        })
    }

    /// One parse from `old_tree`, which stops when `out_of_time` says so.
    fn attempt(&self, old_tree: Option<&Tree>, out_of_time: impl FnMut() -> bool) -> Attempt {
        match catch_unwind(AssertUnwindSafe(|| self.parse_tree(old_tree, out_of_time))) {
            Ok(Some(tree)) => Attempt::Tree(tree),
            Ok(None) => Attempt::Overran,
            Err(_) => {
                log::error!(
                    "{}: parsing panicked; showing plain text",
                    self.language.name()
                );
                Attempt::Panicked
            }
        }
    }

    fn parse_tree(
        &self,
        old_tree: Option<&Tree>,
        out_of_time: impl FnMut() -> bool,
    ) -> Option<Tree> {
        let mut parser = Parser::new();
        parser
            .set_language(&self.language.grammar)
            .expect("grammar ABI is supported by this tree-sitter");
        if let Some(window) = &self.window {
            let text = self.snapshot.text();
            let point = |offset| {
                let point = text.offset_to_point(offset);
                tree_sitter::Point::new(point.row as usize, point.column as usize)
            };
            parser
                .set_included_ranges(&[tree_sitter::Range {
                    start_byte: window.start,
                    end_byte: window.end,
                    start_point: point(window.start),
                    end_point: point(window.end),
                }])
                .expect("one range is in order");
        }
        parse_text(&mut parser, self.snapshot.text(), old_tree, out_of_time)
    }
}

/// Parses `text` with `parser`, set up with its language and any included
/// ranges. `None` if `out_of_time` stopped it.
pub(crate) fn parse_text(
    parser: &mut Parser,
    text: &Text,
    old_tree: Option<&Tree>,
    mut out_of_time: impl FnMut() -> bool,
) -> Option<Tree> {
    // Tree-sitter calls `read` and `progress` from C, which a panic can't
    // unwind through (it aborts), so it's caught there, the parse stopped,
    // and the panic resumed after.
    let panic = RefCell::new(None);
    let mut progress =
        |_: &tree_sitter::ParseState| match catch_unwind(AssertUnwindSafe(&mut out_of_time)) {
            Ok(false) => ControlFlow::Continue(()),
            Ok(true) => ControlFlow::Break(()),
            Err(payload) => {
                panic.borrow_mut().get_or_insert(payload);
                ControlFlow::Break(())
            }
        };
    let mut read = |offset: usize, _: tree_sitter::Point| {
        let bytes = catch_unwind(AssertUnwindSafe(|| {
            fault_point("read");
            text.bytes_at(offset)
        }));
        bytes.unwrap_or_else(|payload| {
            panic.borrow_mut().get_or_insert(payload);
            &[]
        })
    };
    let tree = parser.parse_with_options(
        &mut read,
        old_tree,
        Some(ParseOptions::new().progress_callback(&mut progress)),
    );
    if let Some(payload) = panic.into_inner() {
        resume_unwind(payload);
    }
    tree
}

/// The text a window's tree was parsed from, edited along with it.
fn included_range(tree: &Tree) -> Range<usize> {
    let ranges = tree.included_ranges();
    ranges
        .first()
        .map_or(0..0, |range| range.start_byte..range.end_byte)
}

#[cfg(test)]
mod reparse_budget_tests;

#[cfg(test)]
mod input_edit_tests;
#[cfg(test)]
mod runaway_parse_tests;
#[cfg(test)]
mod window_tests;

#[cfg(test)]
thread_local! {
    /// Where the next parse or query on this thread panics, for tests.
    pub(crate) static FAULT: Cell<Option<&'static str>> = const { Cell::new(None) };
    /// How many times [`out_of_time`] has checked a deadline on this thread.
    pub(crate) static DEADLINE_CHECKS: Cell<usize> = const { Cell::new(0) };
    /// After how many checks every deadline on this thread has passed, so a
    /// test can make it pass at any step.
    pub(crate) static DEADLINE_AFTER: Cell<usize> = const { Cell::new(usize::MAX) };
}

/// Panics if a test asked for a fault at `point`.
#[cfg(test)]
fn fault_point(point: &'static str) {
    if FAULT.get() == Some(point) {
        FAULT.set(None);
        panic!("fault injected at {point}");
    }
}

#[cfg(not(test))]
fn fault_point(_: &'static str) {}

/// The edits taking the tree from version `from` (whose text is `old`) to
/// `new`, to apply in order. Tree-sitter needs each edit's row and column as
/// well as its bytes. Those are known for the texts at both ends, so a single
/// batch gives one edit per cursor and text between cursors keeps its parse.
/// With more batches the intermediate texts are gone, and they merge into one
/// span; text outside it keeps its parse.
fn input_edits(old: &Text, new: &BufferSnapshot, from: Version) -> Vec<InputEdit> {
    let point = |text: &Text, offset: usize| {
        let point = text.offset_to_point(offset);
        tree_sitter::Point::new(point.row as usize, point.column as usize)
    };
    let mut batches = new.edits_since(from).filter(|batch| !batch.is_empty());
    let (Some(first_batch), second) = (batches.next(), batches.next()) else {
        return Vec::new();
    };
    if second.is_none() {
        // Edits are sorted and don't overlap, so edit k starts at `new.start`
        // once the earlier ones are applied.
        return first_batch
            .iter()
            .map(|edit| {
                let start = point(new.text(), edit.new.start);
                let old_start = point(old, edit.old.start);
                let old_end = point(old, edit.old.end);
                let old_end_position = if old_end.row == old_start.row {
                    tree_sitter::Point::new(
                        start.row,
                        start.column + old_end.column - old_start.column,
                    )
                } else {
                    tree_sitter::Point::new(start.row + old_end.row - old_start.row, old_end.column)
                };
                InputEdit {
                    start_byte: edit.new.start,
                    old_end_byte: edit.new.start + edit.old.len(),
                    new_end_byte: edit.new.end,
                    start_position: start,
                    old_end_position,
                    new_end_position: point(new.text(), edit.new.end),
                }
            })
            .collect();
    }
    // Start, end in `old`, and end in the text after the batches so far.
    let mut hull: Option<(usize, usize, usize)> = None;
    for batch in new.edits_since(from) {
        let (Some(first), Some(last)) = (batch.first(), batch.last()) else {
            continue;
        };
        hull = Some(match hull {
            None => (first.old.start, last.old.end, last.new.end),
            Some((start, old_end, current_end)) => {
                // `current_end` and the batch's old range share coordinates.
                let end = current_end.max(last.old.end);
                (
                    start.min(first.old.start),
                    old_end + (end - current_end),
                    last.new.end + (end - last.old.end),
                )
            }
        });
    }
    let Some((start, old_end, new_end)) = hull else {
        return Vec::new();
    };
    vec![InputEdit {
        start_byte: start,
        old_end_byte: old_end,
        new_end_byte: new_end,
        start_position: point(old, start),
        old_end_position: point(old, old_end),
        new_end_position: point(new.text(), new_end),
    }]
}
