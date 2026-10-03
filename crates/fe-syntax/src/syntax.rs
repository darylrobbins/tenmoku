use std::{
    cell::RefCell,
    collections::VecDeque,
    ops::{ControlFlow, Range},
    time::{Duration, Instant},
};

use fe_buffer::{BufferSnapshot, Text, Version};
use tree_sitter::{
    InputEdit, Node, ParseOptions, Parser, QueryCursor, StreamingIterator as _, Tree,
};

use crate::{
    HighlightSpan, Language,
    highlight::{Capture, flatten},
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

/// How many recently highlighted ranges [`Syntax`] remembers. A frame queries
/// one range per run of consecutive visible rows.
const CACHED_RANGES: usize = 8;

/// A buffer's syntax tree, kept up to date incrementally.
///
/// [`Syntax::edit`] catches the tree up with a newer snapshot cheaply,
/// without parsing; the stale tree still highlights correctly outside the
/// edited text. Reparsing happens either on the calling thread within a time
/// budget ([`Syntax::parse_within`]) or anywhere via a [`ParseJob`].
pub struct Syntax {
    language: &'static Language,
    snapshot: BufferSnapshot,
    /// Edited to match `snapshot`, though maybe not reparsed since.
    tree: Option<Tree>,
    /// The version `tree` was last parsed at.
    parsed: Option<Version>,
    /// Recent [`Syntax::highlights`] results for the current tree, newest
    /// last, so frames that neither scroll nor edit don't query again.
    cache: RefCell<VecDeque<(Range<usize>, Vec<HighlightSpan>)>>,
}

impl Syntax {
    pub fn new(language: &'static Language, snapshot: &BufferSnapshot) -> Self {
        Self {
            language,
            snapshot: snapshot.clone(),
            tree: None,
            parsed: None,
            cache: RefCell::default(),
        }
    }

    pub fn language(&self) -> &'static Language {
        self.language
    }

    /// The snapshot the tree has been edited to match.
    pub fn version(&self) -> Version {
        self.snapshot.version()
    }

    /// Applies the edits made since the last call to the tree, so it lines up
    /// with `snapshot`. Cheap: nothing is reparsed.
    pub fn edit(&mut self, snapshot: &BufferSnapshot) {
        if snapshot.id() != self.snapshot.id() || snapshot.version() != self.version() {
            self.cache.get_mut().clear();
        }
        if snapshot.id() != self.snapshot.id()
            || snapshot.version() < self.version()
            || snapshot.len() > MAX_PARSE_BYTES
        {
            // A different buffer, history we can't map, or too big to parse.
            self.tree = None;
            self.parsed = None;
        } else if let Some(tree) = &mut self.tree
            && let Some(edit) = input_edit(self.snapshot.text(), snapshot, self.snapshot.version())
        {
            tree.edit(&edit);
        }
        self.snapshot = snapshot.clone();
    }

    /// Whether the tree is missing or hasn't been reparsed since the last edit.
    pub fn needs_parse(&self) -> bool {
        self.snapshot.len() <= MAX_PARSE_BYTES && self.parsed != Some(self.version())
    }

    /// Reparses on this thread unless that takes longer than `budget`.
    /// Returns whether the tree is now up to date.
    pub fn parse_within(&mut self, budget: Duration) -> bool {
        if !self.needs_parse() {
            return true;
        }
        match self.parse_job().parse(Some(budget)) {
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
        let usable = parsed.snapshot.id() == self.snapshot.id()
            && parsed.snapshot.version() <= self.version()
            && self.snapshot.len() <= MAX_PARSE_BYTES
            && self
                .parsed
                .is_none_or(|version| version < parsed.snapshot.version());
        if !usable {
            return;
        }
        let mut tree = parsed.tree;
        if let Some(edit) = input_edit(
            parsed.snapshot.text(),
            &self.snapshot,
            parsed.snapshot.version(),
        ) {
            tree.edit(&edit);
        }
        self.tree = Some(tree);
        self.parsed = Some(parsed.snapshot.version());
        self.cache.get_mut().clear();
    }

    pub fn tree(&self) -> Option<&Tree> {
        self.tree.as_ref()
    }

    /// Highlights within `range` of the current snapshot, sorted and
    /// non-overlapping. Only the part of the tree touching `range` is queried,
    /// so pass the visible text, not the whole file. Asking for the same range
    /// again before the tree changes doesn't query again.
    pub fn highlights(&self, range: Range<usize>) -> Vec<HighlightSpan> {
        let Some(tree) = &self.tree else {
            return Vec::new();
        };
        let text = self.snapshot.text();
        let range = range.start.min(text.len())..range.end.min(text.len());
        let mut cache = self.cache.borrow_mut();
        if let Some((_, spans)) = cache.iter().find(|(cached, _)| *cached == range) {
            return spans.clone();
        }

        let spans = self.query(tree, range.clone(), self.language.local_patterns);
        if cache.len() == CACHED_RANGES {
            cache.pop_front();
        }
        cache.push_back((range, spans.clone()));
        spans
    }

    /// Queries `tree` for the highlights in `range`. With `split`, nodes with
    /// many children are queried a child at a time; that's only correct when
    /// no pattern spans several siblings.
    pub(crate) fn query(
        &self,
        tree: &Tree,
        range: Range<usize>,
        split: bool,
    ) -> Vec<HighlightSpan> {
        let root = tree.root_node();
        let split = if split && !range.is_empty() {
            flat_paths(root, &range)
        } else {
            Vec::new()
        };
        let mut cursor = QueryCursor::new();
        cursor.set_byte_range(range.clone());
        let mut captures = Vec::new();
        self.query_node(&mut cursor, root, &range, &split, &mut captures);
        flatten(captures, range)
    }

    /// Adds the captures of the matches that start at `node` or below it.
    /// Nodes in `split` are queried only for matches starting at them, then
    /// their children are queried one by one.
    fn query_node(
        &self,
        cursor: &mut QueryCursor,
        node: Node,
        range: &Range<usize>,
        split: &[Node],
        captures: &mut Vec<Capture>,
    ) {
        let split_here = split.contains(&node);
        cursor.set_max_start_depth(split_here.then_some(0));
        let text = self.snapshot.text();
        // Predicates like `#match?` read node text straight from the rope.
        let source = |node: Node| chunks(text, node.byte_range());
        let mut matches = cursor.captures(&self.language.query, node, source);
        while let Some((found, ix)) = matches.next() {
            let capture = found.captures()[*ix];
            if let Some(highlight) = self.language.highlights[capture.index as usize] {
                let precedence = if self.language.earlier_patterns_win {
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
            self.query_node(cursor, child, range, split, captures);
            if !children.goto_next_sibling() {
                break;
            }
        }
    }
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

fn chunks(text: &Text, range: Range<usize>) -> impl Iterator<Item = &[u8]> {
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
    tree: Tree,
    snapshot: BufferSnapshot,
}

impl ParseJob {
    pub fn run(self) -> Parsed {
        self.parse(None)
            .expect("a parse without a time limit completes")
    }

    fn parse(self, budget: Option<Duration>) -> Option<Parsed> {
        let mut parser = Parser::new();
        parser
            .set_language(&self.language.grammar)
            .expect("grammar ABI is supported by this tree-sitter");
        let text = self.snapshot.text();
        let started = Instant::now();
        let mut progress = |_: &tree_sitter::ParseState| match budget {
            Some(budget) if started.elapsed() > budget => ControlFlow::Break(()),
            _ => ControlFlow::Continue(()),
        };
        let tree = parser.parse_with_options(
            &mut |offset, _| text.bytes_at(offset),
            self.old_tree.as_ref(),
            Some(ParseOptions::new().progress_callback(&mut progress)),
        )?;
        Some(Parsed {
            tree,
            snapshot: self.snapshot,
        })
    }
}

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
