//! Language injections: code in one language embedded in another, such as
//! a Markdown fence, an HTML `<script>` or a tagged JS template string.
//!
//! A language's injections query captures the embedded nodes as
//! `@injection.content`, and names their language either with
//! `#set! injection.language "name"` or by capturing a node whose text is the
//! name as `@injection.language`. Each injection is parsed as a layer: a tree
//! of the injected language over just the content's bytes, set with
//! [`Parser::set_included_ranges`]. The content's named child nodes, such as
//! the `> ` continuing a Markdown block quote, are left out unless the
//! pattern sets `injection.include-children`. Layers can hold
//! injections of their own, up to [`MAX_DEPTH`] deep: Markdown's blocks hold
//! inline markup, which holds HTML.
//!
//! `injection.combined` parses a pattern's matches as one layer. We combine a
//! content node with its siblings of the same kind, such as the pieces of one
//! template string around its `${…}`s, rather than with every match in the
//! file, so a layer never depends on which range is being highlighted.
//!
//! Only the injections overlapping the highlighted range are found and
//! parsed, within the same time budget as the query. Their trees are kept
//! until the host tree changes. An injected language whose query isn't
//! compiled yet stays plain, and is reported so the caller can compile it off
//! the UI thread. Inside a layer's ranges its highlights win, and bytes it
//! leaves uncaptured keep the host's: fenced code stays `markup.raw` around
//! its keywords. A language that isn't in this build, such as `regex` or
//! `jsdoc`, stays plain.

use std::{collections::HashMap, ops::Range, time::Instant};

use fe_buffer::Text;
use tree_sitter::{
    Node, Parser, Query, QueryCursorOptions, QueryError, StreamingIterator as _, Tree,
};

use crate::{
    HighlightSpan, Language,
    highlight::{clip, overlay},
    language::{Injected, injected_language},
    syntax::{chunks, for_each_query_root, highlight_query, out_of_time, parse_text, until},
};

/// How many layers deep injections nest.
const MAX_DEPTH: usize = 3;

/// Captured language names longer than this aren't looked up.
const MAX_NAME_LEN: usize = 32;

/// A layer's language and the bytes it was parsed from, which identify it
/// while the host tree stays the same.
pub(crate) type LayerKey = (&'static str, Vec<Range<usize>>);

/// What highlighting keeps between calls for injections.
#[derive(Default)]
pub(crate) struct Layers {
    /// The layers parsed so far. Clear them when the host tree changes.
    pub(crate) trees: HashMap<LayerKey, Tree>,
    /// Injected languages found without a compiled query.
    pub(crate) missing: Vec<&'static str>,
}

/// A language's injections query, with the `#set!` properties of each of its
/// patterns.
pub(crate) struct InjectionQuery {
    query: Query,
    /// See [`Language::local_patterns`].
    local_patterns: bool,
    /// Capture indices of `@injection.content` and `@injection.language`.
    content: Option<u32>,
    language: Option<u32>,
    patterns: Vec<Pattern>,
}

#[derive(Default)]
struct Pattern {
    language: Option<Box<str>>,
    combined: bool,
    include_children: bool,
}

impl InjectionQuery {
    pub(crate) fn new(grammar: &tree_sitter::Language, source: &str) -> Result<Self, QueryError> {
        let query = Query::new(grammar, source)?;
        let patterns = (0..query.pattern_count())
            .map(|ix| {
                let mut pattern = Pattern::default();
                for property in query.property_settings(ix) {
                    match &*property.key {
                        "injection.language" => pattern.language = property.value.clone(),
                        "injection.combined" => pattern.combined = true,
                        "injection.include-children" => pattern.include_children = true,
                        _ => {}
                    }
                }
                pattern
            })
            .collect();
        Ok(Self {
            local_patterns: (0..query.pattern_count()).all(|ix| !query.is_pattern_non_local(ix)),
            content: query.capture_index_for_name("injection.content"),
            language: query.capture_index_for_name("injection.language"),
            patterns,
            query,
        })
    }
}

/// `spans`, the highlights of `tree` in `range`, with the highlights of the
/// layers injected into it painted over them. Layers are taken from `layers`
/// or parsed and added to it, and languages whose queries aren't compiled
/// yet are added to its missing ones.
///
/// Also returns whether every injection overlapping `range` was highlighted.
/// One isn't if finding or parsing it ran out of time before `deadline`, or
/// its language is missing.
pub(crate) fn paint(
    language: &Language,
    text: &Text,
    tree: &Tree,
    range: Range<usize>,
    deadline: Option<Instant>,
    layers: &mut Layers,
    spans: Vec<HighlightSpan>,
) -> (Vec<HighlightSpan>, bool) {
    if range.is_empty() {
        return (spans, true);
    }
    let mut painter = Painter {
        text,
        range,
        deadline,
        layers,
        parser: None,
        complete: true,
    };
    let spans = painter.paint(language, tree, None, 1, spans);
    (spans, painter.complete)
}

/// An injection found in a tree: the language of its content, and the
/// content's bytes, sorted.
struct Injection {
    language: &'static Language,
    ranges: Vec<Range<usize>>,
}

struct Painter<'a> {
    text: &'a Text,
    range: Range<usize>,
    deadline: Option<Instant>,
    layers: &'a mut Layers,
    /// Shared by the layers parsed in one call.
    parser: Option<Parser>,
    complete: bool,
}

impl Painter<'_> {
    /// `spans` with the highlights of `tree`'s injections painted over them.
    /// `parent` is the ranges `tree` was parsed from if it's a layer itself,
    /// at `depth - 1`.
    fn paint(
        &mut self,
        language: &Language,
        tree: &Tree,
        parent: Option<&[Range<usize>]>,
        depth: usize,
        mut spans: Vec<HighlightSpan>,
    ) -> Vec<HighlightSpan> {
        if depth > MAX_DEPTH {
            return spans;
        }
        for injection in self.find(language, tree, parent) {
            let Some(layer) = self.layer(&injection) else {
                continue;
            };
            let own = highlight_query(
                injection.language,
                self.text,
                &layer,
                self.range.clone(),
                injection.language.local_patterns,
                self.deadline,
            );
            let own = self.paint(
                injection.language,
                &layer,
                Some(&injection.ranges),
                depth + 1,
                own,
            );
            spans = overlay(spans, clip(own, &injection.ranges));
        }
        spans
    }

    /// The injections in `tree` overlapping the range, in a language of
    /// this build. With `parent`, their content is cut to those ranges. If
    /// time runs out, the ones found so far.
    fn find(
        &mut self,
        language: &Language,
        tree: &Tree,
        parent: Option<&[Range<usize>]>,
    ) -> Vec<Injection> {
        let Some(query) = &language.injections else {
            return Vec::new();
        };
        let (text, deadline) = (self.text, self.deadline);
        // Each match's pattern, content nodes and language node.
        let mut matches = Vec::new();
        for_each_query_root(tree, &self.range, query.local_patterns, |cursor, node| {
            let source = |node: Node| chunks(text, node.byte_range());
            let mut progress = |_: &_| until(deadline);
            let options = QueryCursorOptions::new().progress_callback(&mut progress);
            let mut iter = cursor.matches_with_options(&query.query, node, source, options);
            while let Some(found) = iter.next() {
                let captured = |ix| {
                    found
                        .captures()
                        .iter()
                        .filter(move |capture| Some(capture.index) == ix)
                        .map(|capture| capture.node)
                };
                let content: Vec<Node> = captured(query.content).collect();
                let name = captured(query.language).next();
                matches.push((found.pattern_index, content, name));
            }
        });

        let mut injections = Vec::new();
        // The pattern and parent node of each combined injection so far.
        let mut combined = Vec::new();
        for (pattern_ix, content, name) in matches {
            if out_of_time(deadline) {
                break;
            }
            let pattern = &query.patterns[pattern_ix];
            let name = match (&pattern.language, name) {
                (Some(name), _) => name.to_string(),
                (None, Some(node)) if node.byte_range().len() <= MAX_NAME_LEN => {
                    chunks(text, node.byte_range())
                        .map(String::from_utf8_lossy)
                        .collect()
                }
                _ => continue,
            };
            let language = match injected_language(&name) {
                Injected::Ready(language) => language,
                Injected::Missing(name) => {
                    if !self.layers.missing.contains(&name) {
                        self.layers.missing.push(name);
                    }
                    self.complete = false;
                    continue;
                }
                Injected::Unknown => continue,
            };
            let mut ranges = Vec::new();
            for node in content {
                let parent = node.parent().filter(|_| pattern.combined);
                let Some(parent) = parent else {
                    content_ranges(node, pattern.include_children, deadline, &mut ranges);
                    continue;
                };
                if combined.contains(&(pattern_ix, parent.id())) {
                    continue;
                }
                combined.push((pattern_ix, parent.id()));
                let mut cursor = parent.walk();
                for sibling in parent.children(&mut cursor) {
                    if sibling.kind_id() == node.kind_id()
                        && !content_ranges(sibling, pattern.include_children, deadline, &mut ranges)
                    {
                        break;
                    }
                }
            }
            if out_of_time(deadline) {
                break;
            }
            ranges.sort_by_key(|range| range.start);
            if let Some(parent) = parent {
                ranges = intersect(&ranges, parent);
            }
            let overlaps = ranges
                .iter()
                .any(|range| range.start < self.range.end && self.range.start < range.end);
            if overlaps {
                injections.push(Injection { language, ranges });
            }
        }
        if out_of_time(deadline) {
            // Some may be missing. The next call looks again, and finds the
            // layers parsed so far kept.
            self.complete = false;
        }
        injections
    }

    /// The layer for `injection`, parsed unless it already was. `None` if
    /// parsing it ran out of time.
    fn layer(&mut self, injection: &Injection) -> Option<Tree> {
        let key = (injection.language.name(), injection.ranges.clone());
        if let Some(tree) = self.layers.trees.get(&key) {
            return Some(tree.clone());
        }
        let deadline = self.deadline;
        if out_of_time(deadline) {
            self.complete = false;
            return None;
        }
        let text = self.text;
        let point = |offset| {
            let point = text.offset_to_point(offset);
            tree_sitter::Point::new(point.row as usize, point.column as usize)
        };
        let mut ranges = Vec::with_capacity(injection.ranges.len());
        for range in &injection.ranges {
            if out_of_time(deadline) {
                self.complete = false;
                return None;
            }
            ranges.push(tree_sitter::Range {
                start_byte: range.start,
                end_byte: range.end,
                start_point: point(range.start),
                end_point: point(range.end),
            });
        }
        let parser = self.parser.get_or_insert_with(Parser::new);
        parser
            .set_language(&injection.language.grammar)
            .expect("grammar ABI is supported by this tree-sitter");
        // Sorted, disjoint and non-empty, as tree-sitter requires.
        parser.set_included_ranges(&ranges).ok()?;
        let Some(tree) = parse_text(parser, text, None, || out_of_time(deadline)) else {
            // Start the next parse afresh instead of resuming this one.
            parser.reset();
            self.complete = false;
            return None;
        };
        self.layers.trees.insert(key, tree.clone());
        Some(tree)
    }
}

/// Adds `node`'s bytes to `ranges`, leaving out its named children's unless
/// `include_children`. Anonymous children are the host grammar's view of the
/// content's own punctuation, so they stay. Returns false, with only some
/// added, if `deadline` passes first.
fn content_ranges(
    node: Node,
    include_children: bool,
    deadline: Option<Instant>,
    ranges: &mut Vec<Range<usize>>,
) -> bool {
    let mut start = node.start_byte();
    if !include_children {
        let mut cursor = node.walk();
        for child in node.named_children(&mut cursor) {
            if out_of_time(deadline) {
                return false;
            }
            if start < child.start_byte() {
                ranges.push(start..child.start_byte());
            }
            start = start.max(child.end_byte());
        }
    }
    if start < node.end_byte() {
        ranges.push(start..node.end_byte());
    }
    true
}

/// The non-empty parts of `ranges` inside `within`. Both are sorted and
/// don't overlap.
fn intersect(ranges: &[Range<usize>], within: &[Range<usize>]) -> Vec<Range<usize>> {
    let mut parts = Vec::new();
    let mut first = 0;
    for range in ranges {
        while within
            .get(first)
            .is_some_and(|outer| outer.end <= range.start)
        {
            first += 1;
        }
        for outer in within[first..]
            .iter()
            .take_while(|outer| outer.start < range.end)
        {
            let part = range.start.max(outer.start)..range.end.min(outer.end);
            if !part.is_empty() {
                parts.push(part);
            }
        }
    }
    parts
}

#[cfg(test)]
mod tests;
