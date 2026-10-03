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
    highlight::{Capture, flatten},
    injection::{self, Layers},
};

/// Files larger than this are shown without highlighting. A tree costs about
/// 30× its source in memory for JSON, 35–40× for Rust and 70× for Markdown,
/// and up to 100× for the densest text, such as a JSON array of single digits
/// (the benches print it). At 8 MB a code file's tree stays near 300 MB, and
/// the worst under 800 MB.
pub const MAX_PARSE_BYTES: usize = 8 * 1024 * 1024;

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
/// layers for. Sixty rows take about 0.1 ms, so only a pathological tree or
/// query reaches this; its highlights stop where the time ran out. Tests have
/// no limit, so a busy machine can't make them flaky.
const QUERY_BUDGET: Duration = if cfg!(test) {
    Duration::MAX
} else {
    Duration::from_millis(2)
};

/// How many recently highlighted ranges [`Syntax`] remembers. A frame queries
/// one range per run of consecutive visible rows.
const CACHED_RANGES: usize = 8;

/// What [`Syntax::highlights`] keeps between calls.
#[derive(Default)]
struct Cache {
    /// Recent results for the current tree, newest last, so frames that
    /// neither scroll nor edit don't query again.
    ranges: VecDeque<(Range<usize>, Vec<HighlightSpan>)>,
    /// Kept across edits, which are applied to them as to the tree.
    layers: Layers,
}

impl Cache {
    fn clear(&mut self) {
        self.ranges.clear();
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

    pub(crate) fn buffer_id(&self) -> BufferId {
        self.snapshot.id()
    }

    /// Applies the edits made since the last call to the tree, so it lines up
    /// with `snapshot`. Cheap: nothing is reparsed.
    pub fn edit(&mut self, snapshot: &BufferSnapshot) {
        let version = self.version();
        let cache = self.cache.get_mut();
        if snapshot.id() != self.snapshot.id() || snapshot.version() != version {
            cache.ranges.clear();
        }
        if snapshot.id() != self.snapshot.id()
            || snapshot.version() < version
            || snapshot.len() > MAX_PARSE_BYTES
            || self.failed.get()
        {
            // A different buffer, history we can't map, too big to parse, or
            // given up on.
            self.tree = None;
            self.parsed = None;
            self.last_reparse = None;
            cache.layers.clear();
        } else if let Some(edit) =
            input_edit(self.snapshot.text(), snapshot, self.snapshot.version())
        {
            if let Some(tree) = &mut self.tree {
                tree.edit(&edit);
            }
            cache.layers.edit(&edit);
        }
        self.snapshot = snapshot.clone();
    }

    /// Whether the tree is missing or hasn't been reparsed since the last edit.
    pub fn needs_parse(&self) -> bool {
        self.snapshot.len() <= MAX_PARSE_BYTES
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
        }
    }

    /// Installs a parse result. One for an older version than the current
    /// snapshot is still newer than the tree's last parse, so it's edited
    /// forward and used; [`Syntax::needs_parse`] stays true.
    pub fn finish(&mut self, parsed: Parsed) {
        let Some(mut tree) = parsed.tree else {
            self.failed.set(true);
            self.tree = None;
            self.cache.get_mut().clear();
            return;
        };
        let usable = parsed.snapshot.id() == self.snapshot.id()
            && parsed.snapshot.version() <= self.version()
            && self.snapshot.len() <= MAX_PARSE_BYTES
            && self
                .parsed
                .is_none_or(|version| version < parsed.snapshot.version());
        if !usable || self.failed.get() {
            return;
        }
        if let Some(edit) = input_edit(
            parsed.snapshot.text(),
            &self.snapshot,
            parsed.snapshot.version(),
        ) {
            tree.edit(&edit);
        }
        if parsed.incremental {
            self.last_reparse = Some(parsed.elapsed);
        }
        self.tree = Some(tree);
        self.parsed = Some(parsed.snapshot.version());
        // The layers stay: they were edited along with the old tree.
        self.cache.get_mut().ranges.clear();
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
    /// so far, and they're cached like any others. But if time runs out while
    /// finding or parsing injections, or an injected language isn't compiled
    /// yet (see [`Syntax::missing_languages`]), the result isn't cached: the
    /// next call tries again, keeping the layers parsed so far.
    pub fn highlights(&self, range: Range<usize>) -> Vec<HighlightSpan> {
        let Some(tree) = self.tree.as_ref().filter(|_| !self.failed.get()) else {
            return Vec::new();
        };
        let text = self.snapshot.text();
        let range = range.start.min(text.len())..range.end.min(text.len());
        let mut cache = self.cache.borrow_mut();
        if let Some((_, spans)) = cache.ranges.iter().find(|(cached, _)| *cached == range) {
            return spans.clone();
        }

        let parsed = self.parsed == Some(self.version());
        let layers = &mut cache.layers;
        let query = || {
            // `None` for a budget too long to be a time.
            let deadline = Instant::now().checked_add(QUERY_BUDGET);
            let language = self.language;
            let spans = highlight_query(
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
            injection::paint(host, range.clone(), deadline, layers, spans)
        };
        let Ok((spans, complete)) = catch_unwind(AssertUnwindSafe(query)) else {
            log::error!(
                "{}: highlighting panicked; showing plain text",
                self.language.name()
            );
            self.failed.set(true);
            return Vec::new();
        };
        if complete {
            if cache.ranges.len() == CACHED_RANGES {
                cache.ranges.pop_front();
            }
            cache.ranges.push_back((range, spans.clone()));
        }
        spans
    }

    /// Injected languages that [`Syntax::highlights`] found uncompiled, so
    /// their code showed plain. Compile each with [`crate::language_by_name`]
    /// off the UI thread, as that takes up to 20 ms, then highlight again.
    pub fn missing_languages(&self) -> Vec<&'static str> {
        let missing = &mut self.cache.borrow_mut().layers.missing;
        missing.retain(|name| crate::loaded_language(name).is_none());
        missing.clone()
    }

    /// [`Syntax::highlights`], after compiling the injected languages it
    /// needs on this thread.
    #[cfg(test)]
    pub(crate) fn highlights_compiling(&self, range: Range<usize>) -> Vec<HighlightSpan> {
        loop {
            let spans = self.highlights(range.clone());
            let missing = self.missing_languages();
            if missing.is_empty() {
                return spans;
            }
            for name in missing {
                crate::language_by_name(name);
            }
        }
    }

    /// Queries `tree` for the highlights in `range` without injections,
    /// stopping after `budget`. See [`highlight_query`].
    #[cfg(test)]
    pub(crate) fn query(
        &self,
        tree: &Tree,
        range: Range<usize>,
        split: bool,
        budget: Duration,
    ) -> Vec<HighlightSpan> {
        let deadline = Instant::now().checked_add(budget);
        highlight_query(
            self.language,
            self.snapshot.text(),
            tree,
            range,
            split,
            deadline,
        )
    }

    /// The languages and ranges of the injected layers kept, sorted.
    #[cfg(test)]
    pub(crate) fn layers(&self) -> Vec<injection::LayerKey> {
        self.cache.borrow().layers.keys()
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
/// in `range`, stopping at `deadline`. With `split`, nodes with many children
/// are queried a child at a time; that's only correct when no pattern spans
/// several siblings.
pub(crate) fn highlight_query(
    language: &Language,
    text: &Text,
    tree: &Tree,
    range: Range<usize>,
    split: bool,
    deadline: Option<Instant>,
) -> Vec<HighlightSpan> {
    fault_point("query");
    let mut captures = Vec::new();
    for_each_query_root(tree, &range, split, |cursor, node| {
        // Predicates like `#match?` read node text straight from the rope.
        let source = |node: Node| chunks(text, node.byte_range());
        let mut progress = |_: &_| until(deadline);
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
    if out_of_time(deadline) {
        log::debug!(
            "{}: highlighting {range:?} ran out of time; showing what was found",
            language.name()
        );
    }
    flatten(captures, range)
}

/// Whether `deadline` has passed. `None` never does.
pub(crate) fn out_of_time(deadline: Option<Instant>) -> bool {
    deadline.is_some_and(|deadline| Instant::now() >= deadline)
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
}

/// The result of a [`ParseJob`].
pub struct Parsed {
    /// `None` if parsing panicked.
    tree: Option<Tree>,
    snapshot: BufferSnapshot,
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
    /// Parses to the end. Doesn't panic: if parsing does, the result makes
    /// [`Syntax::finish`] give up on highlighting.
    pub fn run(self) -> Parsed {
        self.parse(|| false)
            .expect("a parse without a time limit completes")
    }

    /// `None` if `out_of_time` says so before the parse completes.
    fn parse(self, out_of_time: impl FnMut() -> bool) -> Option<Parsed> {
        let started = Instant::now();
        let tree = match catch_unwind(AssertUnwindSafe(|| self.parse_tree(out_of_time))) {
            Ok(tree) => Some(tree?),
            Err(_) => {
                log::error!(
                    "{}: parsing panicked; showing plain text",
                    self.language.name()
                );
                None
            }
        };
        Some(Parsed {
            tree,
            snapshot: self.snapshot,
            elapsed: started.elapsed(),
            incremental: self.old_tree.is_some(),
        })
    }

    fn parse_tree(&self, out_of_time: impl FnMut() -> bool) -> Option<Tree> {
        let mut parser = Parser::new();
        parser
            .set_language(&self.language.grammar)
            .expect("grammar ABI is supported by this tree-sitter");
        parse_text(
            &mut parser,
            self.snapshot.text(),
            self.old_tree.as_ref(),
            out_of_time,
        )
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
    let mut progress = |_: &tree_sitter::ParseState| {
        if out_of_time() {
            ControlFlow::Break(())
        } else {
            ControlFlow::Continue(())
        }
    };
    // Tree-sitter calls `read` from C, which a panic can't unwind
    // through (it aborts), so it's caught there and resumed after.
    let mut panic = None;
    let mut read = |offset: usize, _: tree_sitter::Point| {
        let bytes = catch_unwind(AssertUnwindSafe(|| {
            fault_point("read");
            text.bytes_at(offset)
        }));
        bytes.unwrap_or_else(|payload| {
            panic.get_or_insert(payload);
            &[]
        })
    };
    let tree = parser.parse_with_options(
        &mut read,
        old_tree,
        Some(ParseOptions::new().progress_callback(&mut progress)),
    );
    if let Some(payload) = panic {
        resume_unwind(payload);
    }
    tree
}

#[cfg(test)]
mod reparse_budget_tests;

#[cfg(test)]
thread_local! {
    /// Where the next parse or query on this thread panics, for tests.
    pub(crate) static FAULT: Cell<Option<&'static str>> = const { Cell::new(None) };
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

/// One edit covering every change from version `from` (whose text is `old`)
/// to `new`. Tree-sitter needs each edit's row and column as well as its
/// bytes, and those are only known for the first and last texts, so the
/// batches in between are merged into a single span. Text outside it keeps
/// its parse.
fn input_edit(old: &Text, new: &BufferSnapshot, from: Version) -> Option<InputEdit> {
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
    let (start, old_end, new_end) = hull?;
    let point = |text: &Text, offset: usize| {
        let point = text.offset_to_point(offset);
        tree_sitter::Point::new(point.row as usize, point.column as usize)
    };
    Some(InputEdit {
        start_byte: start,
        old_end_byte: old_end,
        new_end_byte: new_end,
        start_position: point(old, start),
        old_end_position: point(old, old_end),
        new_end_position: point(new.text(), new_end),
    })
}
