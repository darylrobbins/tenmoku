//! Suggested indentation for a row, from the syntax tree, so a new line after
//! `{` or `:` is indented and a `}` outdented.
//!
//! The walk that works a row's level out is adapted from nvim-treesitter,
//! https://github.com/nvim-treesitter/nvim-treesitter, commit
//! 1a4f47b03dfe92d28de7d5654c67327ef008c9e6, path
//! `lua/nvim-treesitter/indent.lua`, license Apache-2.0: reimplemented in Rust
//! over our trees, with the changes below.
//!
//! Each language's rules are an indent query in `queries/indents/`, adapted
//! from nvim-treesitter's `indents.scm` too, run over the root tree
//! [`Syntax`] already keeps. Its captures mark nodes:
//!
//! - `@indent.begin`: the rows after the node's first row, up to its last,
//!   are one level deeper. `#set! indent.immediate` lets a node on one row
//!   begin too, and lets it go on beginning past its last row, as Python's
//!   `if x:` does for the rows of its block. With
//!   `#set! indent.start_at_same_line` the node's rows, its first included,
//!   are a level in from its parent's first row, as an unbraced `if` body's
//!   are.
//! - `@indent.end`: after this node the region has ended. A blank row after
//!   a row ending in it takes the level of what's around the construct it
//!   ends.
//! - `@indent.branch`: a row starting with the node is one level out, as a
//!   `}` or an `else`.
//! - `@indent.dedent`: the rows after the node's first row, up to its last,
//!   are one level out. With `#set! indent.immediate`, the rows after its
//!   last row are instead, as after Python's `return`.
//! - `@indent.auto`, `@indent.ignore`: a row inside the node (past its first
//!   row) keeps the previous row's indentation, as in a comment or a string.
//! - `@indent.zero`: a row starting with the node has no indentation, as a
//!   C `#define`.
//!
//! Brackets need no captures: a node with an opening `(`, `[` or `{` among
//! its children begins at the opener's row, up to its closer, and the closer
//! is a branch, in every language. A begin capture on such a node begins
//! only the rows after its brackets close, such as an unbraced `for` body
//! after its header's `)`, and a dedent capture keeps its brackets from indenting, as
//! Go's `switch` braces don't. An opener not closed begins every row after
//! it.
//!
//! A row's level is worked out as nvim-treesitter does, by walking from the
//! node at the row up to the root: from the last node of the row before for
//! a blank row, else from the row's first node. It's given relative to an
//! earlier row, so the file's own indentation is kept: the row where the
//! nearest node around that one starts, among those starting on an earlier
//! row at its first non-blank char. Inside an error, such as after an opener
//! not yet closed, it's the row before.
//!
//! The changes from nvim-treesitter, besides the brackets: a node that
//! ended on an earlier row doesn't begin, unless it begins immediately or its
//! parent has errors, so the row after a finished statement over several rows
//! isn't indented; a dedent applies inside its node or, set immediate, after
//! it, not both; a row adds a level at most once and takes one away at most
//! once, where nvim-treesitter lets each row change it once; a node starting
//! at the same line counts from its parent's row, so an unbraced body that's
//! itself a loop adds its own level too; inside an
//! `@indent.ignore` node the previous row's indentation is kept rather than
//! none; and `@indent.zero` applies only to a row starting with the node.
//!
//! Known limits: `@indent.align` (lining continuation rows up with an open
//! bracket) isn't supported; brackets begin instead. Injected layers, such as
//! a tagged template's CSS, aren't looked at: a row in one has no suggestion.

use std::{
    collections::{HashMap, HashSet},
    sync::OnceLock,
};

use fe_buffer::Text;
use tree_sitter::{
    Node, Point, Query, QueryCursor, QueryPredicate, QueryPredicateArg, StreamingIterator as _,
};

use crate::{
    Language, Syntax,
    language::{Injected, injected_language},
    syntax::chunks,
};

/// The indentation [`suggest_indent`] suggests for a row.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IndentSuggestion {
    /// `basis_row`'s indentation plus `delta` levels (fewer when negative).
    Relative { basis_row: u32, delta: i32 },
    /// No indentation, as for a C preprocessor directive.
    Zero,
}

/// The indentation `row` should have, from `syntax`'s tree as it stands and
/// `text`, the text it was last edited to match. The caller makes the edit
/// first: for a new blank row, the row before ends the context.
///
/// None without a tree, for a language without an indents query, or for a
/// row in an injected layer: callers then keep copying the previous row's
/// indentation. Also None when no row before this one starts a node around
/// it and the row would be indented.
pub fn suggest_indent(syntax: &Syntax, text: &Text, row: u32) -> Option<IndentSuggestion> {
    let indents = Indents::new(syntax, text, row)?;
    let row = row as usize;
    let (level, anchor) = indents.level(row)?;
    let level = match level {
        Level::Zero => return Some(IndentSuggestion::Zero),
        Level::Keep => {
            let basis_row = indents.prev_nonblank(row)?;
            return Some(IndentSuggestion::Relative {
                basis_row: basis_row as u32,
                delta: 0,
            });
        }
        Level::Levels(level) => level,
    };
    // In an error the tree says little about the rows around, so the row
    // before comes first.
    let around = || std::iter::successors(Some(anchor), |node| node.parent());
    let in_error = around().any(|node| node.is_error());
    let prev = in_error.then(|| indents.prev_nonblank(row)).flatten();
    let starts = around()
        .map(|node| node.start_position())
        .filter(|start| start.row < row && start.column == indents.indent_cols(start.row))
        .map(|start| start.row);
    for basis_row in prev.into_iter().chain(starts) {
        let basis = match indents.level(basis_row) {
            Some((Level::Levels(basis), _)) => basis,
            Some((Level::Zero, _)) => 0,
            _ => continue,
        };
        return Some(IndentSuggestion::Relative {
            basis_row: basis_row as u32,
            delta: level - basis,
        });
    }
    (level <= 0).then_some(IndentSuggestion::Zero)
}

/// When `row` starts with a closing bracket, or a node captured
/// `@indent.end` or `@indent.branch` such as an `else` in C-like languages,
/// the row it should line up with: the opener's row for a closer, else where
/// the nearest node around it that starts on an earlier row starts, the
/// `if`'s row for an `else`.
///
/// None otherwise, and in the cases [`suggest_indent`] gives None.
pub fn closer_outdents(syntax: &Syntax, text: &Text, row: u32) -> Option<u32> {
    let indents = Indents::new(syntax, text, row)?;
    let row = row as usize;
    if indents.is_blank(row) {
        return None;
    }
    let first = indents.first_node(row)?;
    let marks = indents.marks_around(first);
    if let Some(open) = closes_brackets(first) {
        return Some(open.start_position().row as u32);
    }
    let mut node = Some(first);
    let mut closes = false;
    while let Some(n) = node.filter(|n| n.start_byte() == first.start_byte()) {
        let mark = marks.of(n);
        closes |= mark.end || mark.branch;
        node = n.parent();
    }
    if !closes {
        return None;
    }
    let mut node = Some(first);
    while let Some(n) = node {
        let start_row = n.start_position().row;
        if start_row < row {
            return Some(start_row as u32);
        }
        node = n.parent();
    }
    None
}

/// A row's level: how many levels in from the root it is.
enum Level {
    Levels(i32),
    /// Inside a node captured `@indent.auto` or `@indent.ignore`.
    Keep,
    Zero,
}

/// The captures on one node, from every match.
#[derive(Clone, Copy, Default)]
struct Mark {
    begin: bool,
    end: bool,
    branch: bool,
    dedent: bool,
    auto: bool,
    ignore: bool,
    zero: bool,
    /// `indent.immediate` was set by a match capturing it as a begin.
    immediate: bool,
    /// Captured as a begin by a match that sets
    /// `indent.start_at_same_line`, which [`Mark::begin`] doesn't count and
    /// `indent.immediate` doesn't apply to.
    begin_at_line: bool,
    /// `indent.immediate` was set by a match capturing it as a dedent.
    dedent_after: bool,
}

/// The captures of a byte range, by node id.
struct Marks(HashMap<usize, Mark>);

impl Marks {
    fn of(&self, node: Node) -> Mark {
        self.0.get(&node.id()).copied().unwrap_or_default()
    }
}

/// An error with more children than this isn't matched by the patterns over
/// an error's children, such as C's `(ERROR "for" "(" ";" ";" ")")`:
/// tree-sitter's work on those grows with the square of the children, and a
/// flat error of a malformed file can hold every token in it.
const WIDE_ERROR: u32 = 256;

/// A node's own brackets: an opener among its children, and the closer
/// that ends it, if there is one.
#[derive(Clone, Copy)]
struct Region<'a> {
    open: Node<'a>,
    close: Option<Node<'a>>,
}

fn closer_for(opener: &str) -> Option<&'static str> {
    match opener {
        "(" => Some(")"),
        "[" => Some("]"),
        "{" => Some("}"),
        _ => None,
    }
}

/// The stack index for each kind of bracket, opener or closer.
fn bracket_kind(kind: &str) -> Option<usize> {
    match kind {
        "(" | ")" => Some(0),
        "[" | "]" => Some(1),
        "{" | "}" => Some(2),
        _ => None,
    }
}

/// A node's anonymous children, but closers that are missing (inserted by
/// error recovery), which don't close.
fn tokens(node: Node) -> Vec<Node> {
    let mut cursor = node.walk();
    (node.children(&mut cursor))
        .filter(|child| !child.is_named() && !child.is_missing())
        .collect()
}

/// An error's brackets, matched in order, a stack per kind, from its first
/// child up to `until` (not included) or the end. Returns the openers still
/// open, by kind, and, when `until` is a closer, the opener it closes.
/// Linear in the error's children.
fn error_brackets<'a>(
    error: Node<'a>,
    until: Option<Node<'a>>,
    before: usize,
) -> ([Vec<Node<'a>>; 3], Option<Node<'a>>) {
    let mut open: [Vec<Node>; 3] = Default::default();
    for child in tokens(error) {
        if Some(child) == until {
            let opener = bracket_kind(child.kind()).and_then(|kind| open[kind].last().copied());
            return (open, opener);
        }
        if child.start_byte() >= before {
            break;
        }
        let Some(kind) = bracket_kind(child.kind()) else {
            continue;
        };
        if closer_for(child.kind()).is_some() {
            open[kind].push(child);
        } else {
            open[kind].pop();
        }
    }
    (open, None)
}

/// The rows of an error's openers not yet closed at byte `at`.
fn error_open_rows(error: Node, at: usize) -> Vec<usize> {
    let (open, _) = error_brackets(error, None, at);
    open.iter()
        .flatten()
        .map(|opener| opener.start_position().row)
        .collect()
}

/// `node`'s own brackets: its first opener child and the last child closing
/// it. None for an error, whose brackets [`error_brackets`] matches. Linear
/// in the node's children.
fn brackets(node: Node) -> Option<Region> {
    if node.is_error() {
        return None;
    }
    let children = tokens(node);
    let ix = (children.iter()).position(|child| closer_for(child.kind()).is_some())?;
    let closer = closer_for(children[ix].kind())?;
    let close = (children[ix + 1..].iter())
        .rev()
        .find(|child| child.kind() == closer)
        .copied();
    Some(Region {
        open: children[ix],
        close,
    })
}

/// When `node` is the closer of its parent's brackets, the opener: for an
/// error, the opener it matches.
fn closes_brackets(node: Node) -> Option<Node> {
    if node.is_named() || !matches!(node.kind(), ")" | "]" | "}") {
        return None;
    }
    let parent = node.parent()?;
    if parent.is_error() {
        return error_brackets(parent, Some(node), usize::MAX).1;
    }
    let brackets = brackets(parent)?;
    (brackets.close? == node).then_some(brackets.open)
}

/// When `node` is the closer of its parent's brackets, and they indent (the
/// parent isn't captured `@indent.dedent`), the opener.
fn closes_region<'a>(marks: &Marks, node: Node<'a>) -> Option<Node<'a>> {
    let parent = node.parent()?;
    closes_brackets(node).filter(|_| !marks.of(parent).dedent)
}

/// Whether `node` is a construct not yet finished: an error, or a token
/// loose in one (`then` with no body yet), or one whose end is missing or
/// unfinished, as a block whose `}` error recovery inserted. An error
/// elsewhere inside it, before its end, doesn't count.
fn unfinished(node: Node) -> bool {
    if node.parent().is_some_and(|parent| parent.is_error()) && node.child_count() == 0 {
        return true;
    }
    let mut node = node;
    loop {
        if node.is_error() || node.is_missing() {
            return true;
        }
        match node
            .child_count()
            .checked_sub(1)
            .and_then(|last| node.child(last as _))
        {
            Some(last) => node = last,
            None => return false,
        }
    }
}

/// What finding a row's indentation needs.
struct Indents<'a> {
    language: &'static Language,
    compiled: &'static Compiled,
    root: Node<'a>,
    text: &'a Text,
}

impl<'a> Indents<'a> {
    fn new(syntax: &'a Syntax, text: &'a Text, row: u32) -> Option<Self> {
        if syntax.failed() || row >= text.row_count() {
            return None;
        }
        let language = syntax.language();
        let compiled = query_for(language.name())?.compiled(language);
        let root = syntax.tree()?.root_node();
        let indents = Self {
            language,
            compiled,
            root,
            text,
        };
        (!indents.injected(row as usize)).then_some(indents)
    }

    /// The level of `row`, and the node that decides which earlier row it's
    /// relative to: the row's first node, or for a blank row the last node
    /// of the row before.
    fn level(&self, row: usize) -> Option<(Level, Node<'a>)> {
        let prev = self
            .is_blank(row)
            .then(|| self.prev_nonblank(row))
            .flatten();
        let Some(prev) = prev else {
            let node = self.first_node(row)?;
            return Some((self.walk(&self.marks_around(node), row, node), node));
        };
        let anchor = self.last_node(prev)?;
        let marks = self.marks_around(anchor);
        if marks.of(anchor).end {
            // The anchor ends the construct it's in: go on from that, which
            // has ended, so only what it dedents after it counts, and the
            // blocks still open around it, rather than from the node at the
            // blank row, which past an unfinished block's last token is the
            // root.
            let node = match anchor.parent() {
                Some(node) => node,
                None => self.first_node(row)?,
            };
            return Some((self.walk(&self.marks_around(node), row, node), anchor));
        }
        Some((self.walk(&marks, row, anchor), anchor))
    }

    /// nvim-treesitter's walk from `node` to the root, counting the levels
    /// `row` is in by, with the differences in the module docs.
    fn walk(&self, marks: &Marks, row: usize, node: Node) -> Level {
        if marks.of(node).zero && node.start_position().row == row {
            return Level::Zero;
        }
        let mut level = 0;
        // Each row adds a level at most once, and takes one away at most once.
        let mut added: HashSet<usize> = HashSet::new();
        let mut taken: HashSet<usize> = HashSet::new();
        let mut node = Some(node);
        while let Some(n) = node {
            node = n.parent();
            let mark = marks.of(n);
            let first = n.start_position().row;
            let last = n.end_position().row;
            let begins_at_all = mark.begin || mark.begin_at_line;
            if !begins_at_all && (mark.auto || mark.ignore) && first < row && row <= last {
                return Level::Keep;
            }
            let ended = last < row;
            let brackets = brackets(n);
            // A dedent capture keeps the brackets from indenting.
            let region = brackets.filter(|_| !mark.dedent);
            let branch = (mark.branch || closes_region(marks, n).is_some()) && first == row;
            let dedent =
                brackets.is_none() && mark.dedent && first != row && ended == mark.dedent_after;
            if (branch || dedent) && taken.insert(first) {
                level -= 1;
            }
            // Three levels, kept apart: the brackets' own, for the rows
            // they hold after the opener's, up to the closer's; the node's
            // begin, which for a bracketed node (a `for` and its header) is
            // the rows after its brackets close; and a same-line begin,
            // inherited from the parent's row (an unbraced `if` body), for
            // every row of the node, its brackets' included.
            let mut after_brackets = true;
            if let Some(region) = region {
                let open = region.open.start_position().row;
                let close = region.close.map(|close| close.start_position().row);
                after_brackets = close.is_some_and(|close| close < row);
                let held = open < row && close.is_none_or(|close| row <= close);
                if held && added.insert(open) {
                    level += 1;
                }
            }
            // An error's openers still open at the row, a level per row.
            if n.is_error() {
                let at = self.text.row_start(row as u32);
                for open in error_open_rows(n, at) {
                    if open < row && added.insert(open) {
                        level += 1;
                    }
                }
            }
            // A node that ended goes on beginning only when it's itself
            // unfinished. An error elsewhere, in its parent or inside it, as
            // a block's missing `}` or a malformed loop header, doesn't count.
            let in_error = n.parent().is_some_and(|parent| parent.has_error());
            let unfinished = ended && (mark.begin || mark.begin_at_line) && unfinished(n);
            let after = !ended || unfinished || mark.immediate;
            let rows = first != last || in_error || mark.immediate;
            let begins = mark.begin && after_brackets && rows && first != row && after;
            if begins && added.insert(first) {
                level += 1;
            }
            let parent = n.parent().map_or(first, |p| p.start_position().row);
            if mark.begin_at_line && (!ended || unfinished) && added.insert(parent) {
                level += 1;
            }
        }
        Level::Levels(level)
    }

    /// The captures on `node` and the nodes around it, from the matches
    /// rooted at each of them. A capture can only be on a node inside its
    /// match's root, so these are all the captures the walk from `node`
    /// reads. Rooting the matches, rather than giving the query a range, keeps
    /// the work from growing with a wide parent's other children.
    fn marks_around(&self, node: Node) -> Marks {
        let mut marks: HashMap<usize, Mark> = HashMap::new();
        let mut cursor = QueryCursor::new();
        cursor.set_max_start_depth(Some(0));
        for around in std::iter::successors(Some(node), |node| node.parent()) {
            self.mark(&mut cursor, around, &mut marks);
        }
        Marks(marks)
    }

    /// Adds the captures of the matches rooted at `node` to `marks`.
    fn mark(&self, cursor: &mut QueryCursor, node: Node, marks: &mut HashMap<usize, Mark>) {
        let compiled = self.compiled;
        let text = self.text;
        let source = |node: Node| chunks(text, node.byte_range());
        let query = if node.is_error() && node.child_count() > WIDE_ERROR {
            &compiled.wide_error
        } else {
            &compiled.query
        };
        let mut matches = cursor.matches(query, node, source);
        while let Some(found) = matches.next() {
            let pattern = &compiled.patterns[found.pattern_index];
            let captures = found.captures();
            if !pattern.predicates.iter().all(|p| p.holds(captures)) {
                continue;
            }
            for capture in captures {
                let Some(kind) = compiled.captures[capture.index as usize] else {
                    continue;
                };
                let mark = marks.entry(capture.node.id()).or_default();
                match kind {
                    Capture::Begin if pattern.start_at_same_line => mark.begin_at_line = true,
                    Capture::Begin => {
                        mark.begin = true;
                        mark.immediate |= pattern.immediate;
                    }
                    Capture::End => mark.end = true,
                    Capture::Branch => mark.branch = true,
                    Capture::Dedent => {
                        mark.dedent = true;
                        mark.dedent_after |= pattern.immediate;
                    }
                    Capture::Auto => mark.auto = true,
                    Capture::Ignore => mark.ignore = true,
                    Capture::Zero => mark.zero = true,
                }
            }
        }
    }

    /// Whether `row` is inside an injection of a language of this build:
    /// past the content's first row, and starting before its end.
    fn injected(&self, row: usize) -> bool {
        let Some(injections) =
            query_for(self.language.name()).and_then(|query| query.injections(self.language))
        else {
            return false;
        };
        let Some(content) = injections.content else {
            return false;
        };
        let at = self.text.row_start(row as u32) + self.indent_cols(row);
        let text = self.text;
        let mut cursor = QueryCursor::new();
        cursor.set_byte_range(at..at + 1);
        let source = |node: Node| chunks(text, node.byte_range());
        let mut matches = cursor.matches(&injections.query, self.root, source);
        while let Some(found) = matches.next() {
            let mut inside = false;
            let mut name = injections.languages[found.pattern_index].clone();
            for capture in found.captures() {
                let node = capture.node;
                if capture.index == content {
                    inside |= node.start_position().row < row && at < node.end_byte();
                } else if Some(capture.index) == injections.language {
                    name = Some(text.slice_to_string(node.byte_range()).into());
                }
            }
            let known =
                name.is_some_and(|name| !matches!(injected_language(&name), Injected::Unknown));
            if inside && known {
                return true;
            }
        }
        false
    }

    /// The node at `row`'s first non-blank char, or at its end if blank.
    fn first_node(&self, row: usize) -> Option<Node<'a>> {
        self.node_at(row, self.indent_cols(row))
    }

    /// The node at `row`'s last non-blank char, before a trailing comment
    /// unless the row is all comment.
    fn last_node(&self, row: usize) -> Option<Node<'a>> {
        let line = self.text.row_to_string(row as u32);
        let end = line.trim_end().len();
        let node = self.node_at(row, end.checked_sub(1)?)?;
        if !node.kind().contains("comment") {
            return Some(node);
        }
        let first = self.first_node(row)?;
        if first.id() == node.id() {
            return Some(node);
        }
        let column = node.start_position().column.min(end);
        let end = line[..column].trim_end().len();
        match end.checked_sub(1) {
            Some(column) => self.node_at(row, column),
            None => Some(node),
        }
    }

    fn node_at(&self, row: usize, column: usize) -> Option<Node<'a>> {
        let start = Point::new(row, column);
        let end = Point::new(row, column + 1);
        self.root.descendant_for_point_range(start, end)
    }

    /// Bytes of indentation at the start of `row`.
    fn indent_cols(&self, row: usize) -> usize {
        let line = self.text.row_to_string(row as u32);
        line.len() - line.trim_start_matches([' ', '\t']).len()
    }

    fn is_blank(&self, row: usize) -> bool {
        self.text.row_to_string(row as u32).trim().is_empty()
    }

    fn prev_nonblank(&self, row: usize) -> Option<usize> {
        (0..row).rev().find(|&row| !self.is_blank(row))
    }
}

/// A language's indents query, and the injections query that finds the
/// rows it doesn't look at.
struct IndentQuery {
    language: &'static str,
    /// Concatenated in order, as nvim-treesitter's `; inherits:` does.
    parts: &'static [&'static str],
    injections: Option<&'static str>,
    compiled: OnceLock<Compiled>,
    injections_compiled: OnceLock<Option<Injections>>,
}

struct Compiled {
    query: Query,
    /// The same query without the patterns rooted at an error, for an
    /// error with more than [`WIDE_ERROR`] children.
    wide_error: Query,
    /// What each capture marks, by capture index; None for `@_<anything>`.
    captures: Vec<Option<Capture>>,
    /// By pattern index.
    patterns: Vec<Pattern>,
}

#[derive(Clone, Copy)]
enum Capture {
    Begin,
    End,
    Branch,
    Dedent,
    Auto,
    Ignore,
    Zero,
}

struct Pattern {
    immediate: bool,
    start_at_same_line: bool,
    /// Tree-sitter checks its own, such as `#match?`.
    predicates: Vec<Predicate>,
}

/// nvim-treesitter's node predicates on a capture: `#kind-eq?`,
/// `#not-kind-eq?`, `#has-parent?` and `#not-has-parent?`, each with a list
/// of node kinds.
struct Predicate {
    capture: u32,
    /// Whether to test the parent's kind rather than the node's.
    parent: bool,
    negated: bool,
    kinds: Vec<Box<str>>,
}

impl Predicate {
    fn new(predicate: &QueryPredicate) -> Option<Self> {
        let (QueryPredicateArg::Capture(capture), kinds) = predicate.args.split_first()? else {
            return None;
        };
        let (parent, negated) = match &*predicate.operator {
            "kind-eq?" => (false, false),
            "not-kind-eq?" => (false, true),
            "has-parent?" => (true, false),
            "not-has-parent?" => (true, true),
            _ => return None,
        };
        let kinds = (kinds.iter())
            .map(|kind| match kind {
                QueryPredicateArg::String(kind) => Some(kind.clone()),
                QueryPredicateArg::Capture(_) => None,
            })
            .collect::<Option<_>>()?;
        Some(Self {
            capture: *capture,
            parent,
            negated,
            kinds,
        })
    }

    fn holds(&self, captures: &[tree_sitter::QueryCapture]) -> bool {
        captures
            .iter()
            .filter(|capture| capture.index == self.capture)
            .all(|capture| {
                let node = if self.parent {
                    capture.node.parent()
                } else {
                    Some(capture.node)
                };
                let kind_is = node.is_some_and(|node| {
                    let kind = node.kind();
                    self.kinds.iter().any(|k| **k == *kind)
                });
                kind_is != self.negated
            })
    }
}

/// An injections query: which capture is the content, and each pattern's
/// language when set rather than captured.
struct Injections {
    query: Query,
    content: Option<u32>,
    language: Option<u32>,
    languages: Vec<Option<Box<str>>>,
}

impl IndentQuery {
    const fn new(
        language: &'static str,
        parts: &'static [&'static str],
        injections: Option<&'static str>,
    ) -> Self {
        Self {
            language,
            parts,
            injections,
            compiled: OnceLock::new(),
            injections_compiled: OnceLock::new(),
        }
    }

    /// A query that doesn't compile, or has a capture or predicate we don't
    /// know, is a bug: it panics naming the language.
    fn compiled(&self, language: &Language) -> &Compiled {
        self.compiled.get_or_init(|| {
            let name = self.language;
            let source = self.parts.concat();
            let query = Query::new(&language.grammar, &source)
                .unwrap_or_else(|error| panic!("{name} indents query: {error}"));
            let captures = (query.capture_names().iter())
                .map(|&capture| match capture {
                    "indent.begin" => Some(Capture::Begin),
                    "indent.end" => Some(Capture::End),
                    "indent.branch" => Some(Capture::Branch),
                    "indent.dedent" => Some(Capture::Dedent),
                    "indent.auto" => Some(Capture::Auto),
                    "indent.ignore" => Some(Capture::Ignore),
                    "indent.zero" => Some(Capture::Zero),
                    _ if capture.starts_with('_') => None,
                    _ => panic!("{name} indents query: unknown capture @{capture}"),
                })
                .collect();
            let patterns = (0..query.pattern_count())
                .map(|ix| {
                    let set =
                        |key: &str| (query.property_settings(ix).iter()).any(|p| &*p.key == key);
                    let predicates = (query.general_predicates(ix).iter())
                        .map(|predicate| {
                            Predicate::new(predicate).unwrap_or_else(|| {
                                panic!(
                                    "{name} indents query: unknown predicate #{}",
                                    predicate.operator
                                )
                            })
                        })
                        .collect();
                    Pattern {
                        immediate: set("indent.immediate"),
                        start_at_same_line: set("indent.start_at_same_line"),
                        predicates,
                    }
                })
                .collect();
            let mut wide_error = Query::new(&language.grammar, &source)
                .unwrap_or_else(|error| panic!("{name} indents query: {error}"));
            for ix in 0..wide_error.pattern_count() {
                let pattern = &source[query.start_byte_for_pattern(ix)..];
                if pattern
                    .trim_start_matches(['(', '[', ' ', '\n'])
                    .starts_with("ERROR")
                {
                    wide_error.disable_pattern(ix);
                }
            }
            Compiled {
                query,
                wide_error,
                captures,
                patterns,
            }
        })
    }

    fn injections(&self, language: &Language) -> Option<&Injections> {
        (self.injections_compiled)
            .get_or_init(|| {
                let source = self.injections?;
                let query = Query::new(&language.grammar, source)
                    .unwrap_or_else(|error| panic!("{} injections query: {error}", self.language));
                let languages = (0..query.pattern_count())
                    .map(|ix| {
                        (query.property_settings(ix).iter())
                            .find(|p| &*p.key == "injection.language")
                            .and_then(|p| p.value.clone())
                    })
                    .collect();
                Some(Injections {
                    content: query.capture_index_for_name("injection.content"),
                    language: query.capture_index_for_name("injection.language"),
                    languages,
                    query,
                })
            })
            .as_ref()
    }
}

const C: &str = include_str!("../queries/indents/c.scm");
const JAVASCRIPT: &str = include_str!("../queries/indents/javascript.scm");
const JSX: &str = include_str!("../queries/indents/jsx.scm");
const TYPESCRIPT: &str = include_str!("../queries/indents/typescript.scm");

/// The injections query each language uses, as `language.rs` sets them:
/// the grammar crates' own.
/// TypeScript's feature brings JavaScript's grammar crate without its
/// feature, for this query.
#[cfg(any(feature = "javascript", feature = "typescript"))]
const JAVASCRIPT_INJECTIONS: Option<&str> = Some(tree_sitter_javascript::INJECTIONS_QUERY);
#[cfg(not(any(feature = "javascript", feature = "typescript")))]
const JAVASCRIPT_INJECTIONS: Option<&str> = None;
#[cfg(feature = "lua")]
const LUA_INJECTIONS: Option<&str> = Some(tree_sitter_lua::INJECTIONS_QUERY);
#[cfg(not(feature = "lua"))]
const LUA_INJECTIONS: Option<&str> = None;

/// The indents queries, by language name.
static QUERIES: [IndentQuery; 12] = [
    IndentQuery::new("rust", &[include_str!("../queries/indents/rust.scm")], None),
    IndentQuery::new(
        "python",
        &[include_str!("../queries/indents/python.scm")],
        None,
    ),
    IndentQuery::new("javascript", &[JAVASCRIPT, JSX], JAVASCRIPT_INJECTIONS),
    IndentQuery::new(
        "typescript",
        &[JAVASCRIPT, TYPESCRIPT],
        JAVASCRIPT_INJECTIONS,
    ),
    IndentQuery::new(
        "tsx",
        &[
            JAVASCRIPT,
            TYPESCRIPT,
            JSX,
            include_str!("../queries/indents/tsx.scm"),
        ],
        JAVASCRIPT_INJECTIONS,
    ),
    IndentQuery::new("go", &[include_str!("../queries/indents/go.scm")], None),
    IndentQuery::new("c", &[C], None),
    IndentQuery::new(
        "cpp",
        &[C, include_str!("../queries/indents/cpp.scm")],
        None,
    ),
    IndentQuery::new("java", &[include_str!("../queries/indents/java.scm")], None),
    IndentQuery::new("json", &[include_str!("../queries/indents/json.scm")], None),
    IndentQuery::new(
        "lua",
        &[include_str!("../queries/indents/lua.scm")],
        LUA_INJECTIONS,
    ),
    IndentQuery::new("css", &[include_str!("../queries/indents/css.scm")], None),
];

fn query_for(language_name: &str) -> Option<&'static IndentQuery> {
    QUERIES.iter().find(|query| query.language == language_name)
}

#[cfg(all(test, feature = "typescript", feature = "css"))]
mod tests {
    use fe_buffer::Buffer;

    use crate::{Syntax, language_by_name};

    /// Runs without the `javascript` feature too: `cargo test -p fe-syntax
    /// --no-default-features --features typescript,css --lib indent`.
    #[test]
    fn typescript_injections_without_javascript() {
        let text = "const s = css`\n  color: red;\n`;\nf();";
        for language in ["typescript", "tsx"] {
            let buffer = Buffer::new(text);
            let language = language_by_name(language).unwrap();
            let mut syntax = Syntax::new(language, &buffer.snapshot());
            syntax.finish(syntax.parse_job().run());
            assert_eq!(super::suggest_indent(&syntax, buffer.text(), 1), None);
            assert!(super::suggest_indent(&syntax, buffer.text(), 3).is_some());
        }
    }
}
