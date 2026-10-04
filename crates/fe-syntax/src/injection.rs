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
//! parsed, within the same time budget as the query. Finding one checks the
//! deadline at every node it visits, as a template string can have
//! thousands of pieces. Their trees are kept
//! across edits, which [`Layers::edit`] applies to them as to the host tree.
//! Once the host is reparsed, a layer an edit touched is reparsed
//! incrementally from its old tree, and the others are used as they are. A
//! layer whose injection is gone is dropped when its bytes are next
//! highlighted, and one far from the recently highlighted ranges as soon as
//! it's far.
//!
//! An injected language whose query isn't compiled yet stays plain, and is
//! reported so the caller can compile it off the UI thread. Inside a layer's ranges its highlights win, and bytes it
//! leaves uncaptured keep the host's: fenced code stays `markup.raw` around
//! its keywords. A language that isn't in this build, such as `regex` or
//! `jsdoc`, stays plain.

use std::{
    collections::{HashSet, VecDeque},
    ops::Range,
    time::Instant,
};

use fe_buffer::Text;
use tree_sitter::{
    InputEdit, Node, Parser, Query, QueryCursorOptions, QueryError, StreamingIterator as _, Tree,
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

/// How many recently highlighted ranges keep the layers near them. A frame
/// highlights one range per run of consecutive visible rows.
const RECENT_RANGES: usize = 8;

/// A layer's language and the bytes it was parsed from.
#[cfg(test)]
pub(crate) type LayerKey = (&'static str, Vec<Range<usize>>);

/// What highlighting keeps between calls for injections.
#[derive(Default)]
pub(crate) struct Layers {
    kept: Vec<Layer>,
    /// Injected languages found without a compiled query, until taken by
    /// [`crate::Syntax::take_missing_languages`].
    pub(crate) missing: Vec<&'static str>,
    /// The ranges highlighted last, newest last.
    recent: VecDeque<Range<usize>>,
    /// Counts [`paint`] calls, to tell which layers the current one used.
    pass: u64,
    /// Each layer parsed, fresh or incrementally, in order.
    #[cfg(test)]
    pub(crate) parses: Vec<LayerKey>,
}

/// An injection's parsed tree, kept across edits.
struct Layer {
    language: &'static Language,
    /// The bytes it was parsed from, moved along by the edits since. Sorted
    /// and not empty.
    ranges: Vec<Range<usize>>,
    tree: Tree,
    /// Whether an edit has touched its bytes since it was parsed.
    edited: bool,
    /// The last [`Layers::pass`] that highlighted it.
    pass: u64,
}

impl Layer {
    /// From the start of its first range to the end of its last.
    fn span(&self) -> Range<usize> {
        span(&self.ranges)
    }
}

impl Layers {
    /// Forgets every layer, keeping the missing languages: highlights
    /// already shown lack them.
    pub(crate) fn clear(&mut self) {
        self.kept.clear();
        self.recent.clear();
    }

    /// Applies `edit`, just applied to the host tree, to the layers it
    /// touches or moves, so they line up with the text again. Those it
    /// touches are reparsed when next highlighted after the host is.
    pub(crate) fn edit(&mut self, edit: &InputEdit) {
        let edited = edit.start_byte..edit.old_end_byte;
        for layer in &mut self.kept {
            let span = layer.span();
            if span.end < edited.start {
                continue;
            }
            // An edit between its ranges, such as inside a template
            // string's `${…}`, leaves its bytes as they were.
            layer.edited |= layer.ranges.iter().any(|range| touches(range, &edited));
            layer.tree.edit(edit);
            for range in &mut layer.ranges {
                *range = moved(range, edit);
            }
        }
        for range in &mut self.recent {
            *range = moved(range, edit);
        }
    }

    /// The languages and ranges of the layers kept, sorted.
    #[cfg(test)]
    pub(crate) fn keys(&self) -> Vec<LayerKey> {
        self.trees().into_iter().map(|(key, _)| key).collect()
    }

    /// The layers kept, sorted, with their trees as S-expressions.
    #[cfg(test)]
    pub(crate) fn trees(&self) -> Vec<(LayerKey, String)> {
        let mut trees: Vec<_> = self
            .kept
            .iter()
            .map(|layer| {
                let key = (layer.language.name(), layer.ranges.clone());
                (key, layer.tree.root_node().to_sexp())
            })
            .collect();
        trees.sort_by(|((a, a_ranges), _), ((b, b_ranges), _)| {
            (a_ranges.first().map(|range| range.start), a)
                .cmp(&(b_ranges.first().map(|range| range.start), b))
        });
        trees
    }

    /// Drops the layers that `range`, just highlighted, would have used but
    /// didn't, as their injections are gone: those with bytes in it, and
    /// those edited in or next to it. Then [`Layers::visited`].
    fn highlighted(&mut self, range: Range<usize>, complete: bool, host_parsed: bool) {
        let pass = self.pass;
        if complete && host_parsed {
            self.kept.retain(|layer| {
                layer.pass == pass
                    || !(layer.ranges.iter().any(|bytes| overlaps(bytes, &range))
                        || layer.edited && touches(&layer.span(), &range))
            });
        }
        self.visited(range);
    }

    /// Remembers `range` as highlighted, newest, whether just now or from
    /// a cached result, and drops the layers more than its length from it
    /// and the other recent ranges.
    pub(crate) fn visited(&mut self, range: Range<usize>) {
        self.recent.retain(|recent| *recent != range);
        if self.recent.len() == RECENT_RANGES {
            self.recent.pop_front();
        }
        self.recent.push_back(range);
        let recent = &self.recent;
        self.kept.retain(|layer| {
            let span = layer.span();
            recent.iter().any(|range| {
                let near = range.start.saturating_sub(range.len())..range.end + range.len();
                touches(&span, &near)
            })
        });
    }
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

/// The tree that layers are injected into.
pub(crate) struct Host<'a> {
    pub(crate) language: &'a Language,
    pub(crate) text: &'a Text,
    pub(crate) tree: &'a Tree,
    /// Whether `tree` is parsed from `text`. If it's only been edited to
    /// match, so are the injections found in it, and layers that edits
    /// touched are used as they are, not reparsed, like the host tree.
    pub(crate) parsed: bool,
}

/// `spans`, the highlights of the host tree in `range`, with the highlights
/// of the layers injected into it painted over them. Layers are taken from
/// `layers` or parsed and added to it, and languages whose queries aren't
/// compiled yet are added to its missing ones.
///
/// Also returns whether every injection overlapping `range` was highlighted.
/// One isn't if finding or parsing it ran out of time before `deadline`, or
/// its language is missing.
pub(crate) fn paint(
    host: Host,
    range: Range<usize>,
    deadline: Option<Instant>,
    layers: &mut Layers,
    spans: Vec<HighlightSpan>,
) -> (Vec<HighlightSpan>, bool) {
    layers.pass += 1;
    if range.is_empty() {
        // Nothing to paint, but the layers edited at it are as unused as
        // if it weren't empty: those left when the whole text is deleted.
        layers.highlighted(range, true, host.parsed);
        return (spans, true);
    }
    let mut painter = Painter {
        text: host.text,
        range: range.clone(),
        deadline,
        host_parsed: host.parsed,
        layers,
        parser: None,
        complete: true,
    };
    let spans = painter.paint(host.language, host.tree, None, 1, spans);
    let complete = painter.complete;
    layers.highlighted(range, complete, host.parsed);
    (spans, complete)
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
    host_parsed: bool,
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
        let mut combined = HashSet::new();
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
            if !match_ranges(
                &content,
                pattern_ix,
                pattern,
                &mut combined,
                deadline,
                &mut ranges,
            ) {
                break;
            }
            // The nodes come in document order, so sorting only checks the
            // order. Like `intersect` and finding the layer, it takes time
            // linear in the ranges collected, each within the deadline.
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

    /// The layer for `injection`: kept from an earlier call if parsed from
    /// the same bytes and not edited since, else parsed, incrementally from a
    /// kept layer that an edit touched at the same place. `None` if parsing
    /// it ran out of time.
    fn layer(&mut self, injection: &Injection) -> Option<Tree> {
        let layers = &mut *self.layers;
        let pass = layers.pass;
        let language = injection.language;
        let same = |layer: &Layer| std::ptr::eq(layer.language, language);
        let exact = layers
            .kept
            .iter()
            .position(|layer| same(layer) && !layer.edited && layer.ranges == injection.ranges);
        let kept = exact.or_else(|| {
            // Not one this call already used: injections at the same place,
            // such as nested template strings, each keep their own.
            let span = span(&injection.ranges);
            layers.kept.iter().position(|layer| {
                same(layer) && layer.edited && layer.pass != pass && touches(&layer.span(), &span)
            })
        });
        if let Some(ix) = kept {
            let layer = &mut layers.kept[ix];
            layer.pass = pass;
            if exact.is_some() || !self.host_parsed {
                return Some(layer.tree.clone());
            }
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
            .set_language(&language.grammar)
            .expect("grammar ABI is supported by this tree-sitter");
        // Sorted, disjoint and non-empty, as tree-sitter requires.
        parser.set_included_ranges(&ranges).ok()?;
        let old = kept.map(|ix| &layers.kept[ix].tree);
        let Some(tree) = parse_text(parser, text, old, || out_of_time(deadline)) else {
            // Start the next parse afresh instead of resuming this one.
            parser.reset();
            self.complete = false;
            return None;
        };
        #[cfg(test)]
        layers
            .parses
            .push((language.name(), injection.ranges.clone()));
        let layer = Layer {
            language,
            ranges: injection.ranges.clone(),
            tree: tree.clone(),
            edited: false,
            pass,
        };
        match kept {
            Some(ix) => layers.kept[ix] = layer,
            None => layers.kept.push(layer),
        }
        Some(tree)
    }
}

/// Adds the bytes of a match's `content` nodes to `ranges`, each combined
/// with its siblings of the same kind if the pattern says so and `combined`
/// doesn't already hold its parent. Returns false, with only some added, if
/// `deadline` passes first: it's checked at every node visited, as a
/// template string can hold thousands of pieces.
fn match_ranges(
    content: &[Node],
    pattern_ix: usize,
    pattern: &Pattern,
    combined: &mut HashSet<(usize, usize)>,
    deadline: Option<Instant>,
    ranges: &mut Vec<Range<usize>>,
) -> bool {
    for &node in content {
        let parent = node.parent().filter(|_| pattern.combined);
        let Some(parent) = parent else {
            if !content_ranges(node, pattern.include_children, deadline, ranges) {
                return false;
            }
            continue;
        };
        if !combined.insert((pattern_ix, parent.id())) {
            continue;
        }
        let mut cursor = parent.walk();
        for sibling in parent.children(&mut cursor) {
            let in_time = if sibling.kind_id() == node.kind_id() {
                content_ranges(sibling, pattern.include_children, deadline, ranges)
            } else {
                !out_of_time(deadline)
            };
            if !in_time {
                return false;
            }
        }
    }
    true
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
    if out_of_time(deadline) {
        return false;
    }
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

/// From the start of the first of `ranges` to the end of the last.
fn span(ranges: &[Range<usize>]) -> Range<usize> {
    let start = ranges.first().map_or(0, |range| range.start);
    start..ranges.last().map_or(start, |range| range.end)
}

/// Whether `a` and `b` share a byte.
fn overlaps(a: &Range<usize>, b: &Range<usize>) -> bool {
    a.start < b.end && b.start < a.end
}

/// Whether `a` and `b` share a byte or meet, so that an edit at one could
/// change what the other holds.
fn touches(a: &Range<usize>, b: &Range<usize>) -> bool {
    a.start <= b.end && b.start <= a.end
}

/// `range` after `edit`, as tree-sitter moves a tree's included ranges: the
/// bytes after the edit move with it, and those it replaced collapse to its
/// start.
fn moved(range: &Range<usize>, edit: &InputEdit) -> Range<usize> {
    let offset = |offset: usize| {
        if offset >= edit.old_end_byte {
            offset - edit.old_end_byte + edit.new_end_byte
        } else {
            offset.min(edit.start_byte)
        }
    };
    offset(range.start)..offset(range.end)
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
