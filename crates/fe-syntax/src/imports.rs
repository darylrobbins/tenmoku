//! The modules a JavaScript, TypeScript or TSX file imports: the strings of
//! `import … from 'm'`, `import 'm'`, `export … from 'm'`,
//! `import X = require('m')` and `require('m')`.
//!
//! They come from our own query in `queries/imports/`, run over the tree
//! [`Syntax`] already keeps, so the grammar decides what is code and what is
//! a comment, string, template, regex or JSX text.

use std::{
    cell::{Cell, RefCell},
    ops::{ControlFlow, Range},
    sync::OnceLock,
};

use fe_buffer::Text;
use tree_sitter::{Parser, Query, QueryCursor, QueryCursorOptions, StreamingIterator as _, Tree};

use crate::{
    Language, Syntax,
    syntax::{chunks, parse_text},
};

/// Why [`module_specifiers`] found nothing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ImportsError {
    /// The language has no imports query, or parsing gave up on the buffer.
    Unsupported,
    /// The tree is missing or hasn't been reparsed since the last edit.
    NotParsed,
    /// `cancelled` returned true.
    Cancelled,
}

/// One language's imports query, in parts concatenated in order.
struct ImportsQuery {
    language: &'static str,
    parts: &'static [&'static str],
    compiled: OnceLock<Compiled>,
}

struct Compiled {
    query: Query,
    module: u32,
    recovered: u32,
}

const JAVASCRIPT: &str = include_str!("../queries/imports/javascript.scm");
const TYPESCRIPT: &str = include_str!("../queries/imports/typescript.scm");

static QUERIES: [ImportsQuery; 3] = [
    ImportsQuery::new("javascript", &[JAVASCRIPT]),
    ImportsQuery::new("typescript", &[JAVASCRIPT, TYPESCRIPT]),
    ImportsQuery::new("tsx", &[JAVASCRIPT, TYPESCRIPT]),
];

impl ImportsQuery {
    const fn new(language: &'static str, parts: &'static [&'static str]) -> Self {
        Self {
            language,
            parts,
            compiled: OnceLock::new(),
        }
    }

    /// The query compiled for `language`'s grammar, on first use. One that
    /// doesn't compile is a bug: it panics naming the language.
    fn compiled(&self, language: &Language) -> &Compiled {
        self.compiled.get_or_init(|| {
            let query = Query::new(&language.grammar, &self.parts.concat())
                .unwrap_or_else(|error| panic!("{} imports query: {error}", self.language));
            let module = query
                .capture_index_for_name("module")
                .unwrap_or_else(|| panic!("{} imports query: no @module", self.language));
            let recovered = query
                .capture_index_for_name("recovered")
                .unwrap_or_else(|| panic!("{} imports query: no @recovered", self.language));
            Compiled {
                query,
                module,
                recovered,
            }
        })
    }
}

fn query_for(language_name: &str) -> Option<&'static ImportsQuery> {
    QUERIES.iter().find(|query| query.language == language_name)
}

/// Whether the language with this name has an imports query.
pub fn module_specifiers_supported(language_name: &str) -> bool {
    query_for(language_name).is_some()
}

/// The byte range of each module string `syntax`'s text imports, inside its
/// quotes, in document order. Escapes stay as written. `cancelled` is polled
/// while the query runs, and the query stops with [`ImportsError::Cancelled`]
/// once it returns true.
///
/// A statement with a syntax error elsewhere in it still reports its string,
/// as far as tree-sitter's recovery makes it a statement.
pub fn module_specifiers(
    syntax: &Syntax,
    cancelled: &mut dyn FnMut() -> bool,
) -> Result<Vec<Range<usize>>, ImportsError> {
    let language = syntax.language();
    let query = query_for(language.name()).ok_or(ImportsError::Unsupported)?;
    if syntax.failed() {
        return Err(ImportsError::Unsupported);
    }
    let tree = match syntax.tree() {
        Some(tree) if !syntax.needs_parse() => tree,
        _ => return Err(ImportsError::NotParsed),
    };
    let compiled = query.compiled(language);
    let text = syntax.text();
    let cancelled = RefCell::new(cancelled);
    let stopped = Cell::new(false);
    let poll = || {
        if stopped.get() || (cancelled.borrow_mut())() {
            stopped.set(true);
        }
        stopped.get()
    };
    let mut found = Vec::new();
    // The tree being read, and from where. A misread of the grammar that
    // swallows the rest of the file starts a new tree after it (see
    // [`swallowed_from`]), so the modules after it are still found.
    let mut owned;
    let mut tree = tree;
    let mut from = 0;
    for reparses in 0..=MAX_REPARSES {
        let resume = if reparses < MAX_REPARSES {
            swallowed_from(tree, from)
        } else {
            None
        };
        let to = resume.unwrap_or(text.len());
        query_range(compiled, tree, text, from..to, &poll, &mut found);
        if stopped.get() {
            return Err(ImportsError::Cancelled);
        }
        let Some(resume) = resume else { break };
        let Some(reparsed) = parse_after(language, text, resume, &poll) else {
            if stopped.get() {
                return Err(ImportsError::Cancelled);
            }
            break;
        };
        owned = reparsed;
        tree = &owned;
        from = resume;
    }
    found.sort_by_key(|range| range.start);
    found.dedup();
    Ok(found)
}

/// The most trees read after the first in one file.
const MAX_REPARSES: usize = 16;

/// Adds the modules of `tree` that start within `range` to `found`, stopping
/// once `poll` says to.
fn query_range(
    compiled: &Compiled,
    tree: &Tree,
    text: &Text,
    range: Range<usize>,
    poll: &dyn Fn() -> bool,
    found: &mut Vec<Range<usize>>,
) {
    let mut progress = |_: &_| {
        if poll() {
            ControlFlow::Break(())
        } else {
            ControlFlow::Continue(())
        }
    };
    let options = QueryCursorOptions::new().progress_callback(&mut progress);
    let mut cursor = QueryCursor::new();
    cursor.set_byte_range(range.clone());
    let source = |node: tree_sitter::Node| chunks(text, node.byte_range());
    let mut matches =
        cursor.matches_with_options(&compiled.query, tree.root_node(), source, options);
    while let Some(found_match) = matches.next() {
        if poll() {
            break;
        }
        for capture in found_match.captures() {
            let node = capture.node;
            if !range.contains(&node.start_byte()) {
                continue;
            }
            let is = if capture.index == compiled.module {
                is_module(node)
            } else if capture.index == compiled.recovered {
                is_recovered(node, text)
            } else {
                false
            };
            if is {
                found.push(inside_quotes(node));
            }
        }
    }
}

/// Where the grammar's misread of a TSX element swallows the rest of the
/// file, at or after `from`: the `/>` that it reads as a regular expression
/// after `<Foo extends="/" />` (a JSX attribute named `extends` is read as a
/// type parameter's constraint). What follows is read again from there.
fn swallowed_from(tree: &Tree, from: usize) -> Option<usize> {
    let root = tree.root_node();
    if !root.has_error() {
        return None;
    }
    let mut cursor = root.walk();
    loop {
        let node = cursor.node();
        let here = node.end_byte() > from && node.has_error();
        if node.kind() == "regex" && node.start_byte() >= from {
            let misread = node.prev_sibling().is_some_and(|prev| {
                let mut children = prev.walk();
                prev.kind() == "ERROR"
                    && prev
                        .children(&mut children)
                        .any(|c| c.kind() == "type_parameter")
            });
            if misread {
                return Some(node.start_byte() + 1);
            }
        }
        if here && cursor.goto_first_child() {
            continue;
        }
        while !cursor.goto_next_sibling() {
            if !cursor.goto_parent() {
                return None;
            }
        }
    }
}

/// The tree of the text from `start` on, or `None` if it stopped or
/// couldn't be parsed.
fn parse_after(
    language: &Language,
    text: &Text,
    start: usize,
    poll: &dyn Fn() -> bool,
) -> Option<Tree> {
    let mut parser = Parser::new();
    parser.set_language(&language.grammar).ok()?;
    let point = |offset| {
        let point = text.offset_to_point(offset);
        tree_sitter::Point::new(point.row as usize, point.column as usize)
    };
    let end = text.len();
    parser
        .set_included_ranges(&[tree_sitter::Range {
            start_byte: start,
            end_byte: end,
            start_point: point(start),
            end_point: point(end),
        }])
        .ok()?;
    parse_text(&mut parser, text, None, poll)
}

/// Whether a captured string is a module: not the text of a regex (see
/// [`after_division`]), and, in a `require(…)`, the only argument, comments
/// aside.
fn is_module(string: tree_sitter::Node) -> bool {
    match string.parent() {
        Some(arguments) if arguments.kind() == "arguments" => {
            let mut cursor = arguments.walk();
            // (Counting stops at a second argument: a call of many strings is
            // read once per string.)
            let args = arguments
                .named_children(&mut cursor)
                .filter(|child| child.kind() != "comment")
                .take(2)
                .count();
            args == 1
        }
        _ => !after_division(string),
    }
}

/// Whether `string`, a statement of its own at the top level, is the module
/// of a re-export the grammar broke at a line break: `export { a }` or
/// `export` then `{ a }`, and `from`, none of them ended by a `;` (the
/// grammar inserts none), then the string. Comments may be anywhere.
fn is_recovered(string: tree_sitter::Node, text: &Text) -> bool {
    let semicolon = |node: tree_sitter::Node| {
        let mut cursor = node.walk();
        node.children(&mut cursor)
            .any(|child| child.kind() == ";" && !child.is_missing())
    };
    // An expression statement of just the identifier `word`, without `;`.
    let word = |node: tree_sitter::Node, word: &str| {
        let mut cursor = node.walk();
        let mut named = node
            .named_children(&mut cursor)
            .filter(|child| child.kind() != "comment");
        node.kind() == "expression_statement"
            && !semicolon(node)
            && named.next().is_some_and(|child| {
                child.kind() == "identifier" && text.slice_to_string(child.byte_range()) == word
            })
            && named.next().is_none()
    };
    let Some(statement) = string.parent() else {
        return false;
    };
    let mut before = std::iter::successors(statement.prev_sibling(), |n| n.prev_sibling())
        .filter(|node| node.kind() != "comment");
    let (Some(from), Some(first)) = (before.next(), before.next()) else {
        return false;
    };
    if !word(from, "from") {
        return false;
    }
    match first.kind() {
        "export_statement" => {
            let mut cursor = first.walk();
            let clause = first
                .named_children(&mut cursor)
                .any(|child| child.kind() == "export_clause");
            clause && !semicolon(first) && first.child_by_field_name("source").is_none()
        }
        "statement_block" => before.next().is_some_and(|export| word(export, "export")),
        _ => false,
    }
}

/// Whether the string is the module of an `import` that is really the text
/// of a regular expression: the grammar reads the `/` after `export default
/// class {}` or `function () {}` as a division by a missing operand, and the
/// regex's text as code.
fn after_division(string: tree_sitter::Node) -> bool {
    let Some(import) = string.parent().filter(|n| n.kind() == "import_statement") else {
        return false;
    };
    let Some(export) = import
        .prev_named_sibling()
        .filter(|n| n.kind() == "export_statement")
    else {
        return false;
    };
    export
        .child_by_field_name("value")
        .filter(|value| value.kind() == "binary_expression")
        .and_then(|value| value.child_by_field_name("right"))
        .is_some_and(|right| right.is_missing())
}

/// The bytes of a string node between its quotes. A string missing its
/// closing quote, which tree-sitter's recovery may produce, ends at the
/// node's end.
fn inside_quotes(node: tree_sitter::Node) -> Range<usize> {
    let range = node.byte_range();
    let quote = |child: Option<tree_sitter::Node>| {
        child.is_some_and(|child| matches!(child.kind(), "\"" | "'"))
    };
    let start = if quote(node.child(0)) {
        (range.start + 1).min(range.end)
    } else {
        range.start
    };
    let closed = node.child_count() > 1 && quote(node.child(node.child_count() - 1));
    let end = if closed { range.end - 1 } else { range.end };
    start..end.max(start)
}

#[cfg(test)]
mod tests;
