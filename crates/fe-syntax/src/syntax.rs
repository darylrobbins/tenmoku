use std::{
    ops::{ControlFlow, Range},
    time::{Duration, Instant},
};

use fe_buffer::{BufferSnapshot, Text, Version};
use tree_sitter::{InputEdit, ParseOptions, Parser, QueryCursor, StreamingIterator as _, Tree};

use crate::{
    HighlightSpan, Language,
    highlight::{Capture, flatten},
};

/// Files larger than this are shown without highlighting: their trees would
/// cost several times the file's size in memory.
pub const MAX_PARSE_BYTES: usize = 16 * 1024 * 1024;

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
}

impl Syntax {
    pub fn new(language: &'static Language, snapshot: &BufferSnapshot) -> Self {
        Self {
            language,
            snapshot: snapshot.clone(),
            tree: None,
            parsed: None,
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
    }

    pub fn tree(&self) -> Option<&Tree> {
        self.tree.as_ref()
    }

    /// Highlights within `range` of the current snapshot, sorted and
    /// non-overlapping. Only the part of the tree touching `range` is queried,
    /// so pass the visible text, not the whole file.
    pub fn highlights(&self, range: Range<usize>) -> Vec<HighlightSpan> {
        let Some(tree) = &self.tree else {
            return Vec::new();
        };
        let text = self.snapshot.text();
        let range = range.start.min(text.len())..range.end.min(text.len());
        let query = &self.language.query;
        let mut cursor = QueryCursor::new();
        cursor.set_byte_range(range.clone());
        // Predicates like `#match?` read node text straight from the rope.
        let source = |node: tree_sitter::Node| chunks(text, node.byte_range());
        let mut matches = cursor.captures(query, tree.root_node(), source);
        let mut captures = Vec::new();
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
        flatten(captures, range)
    }
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
