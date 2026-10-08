//! The modules a JavaScript, TypeScript or TSX file imports: the strings of
//! `import … from 'm'`, `import 'm'`, `export … from 'm'`,
//! `import X = require('m')` and `require('m')`.
//!
//! They come from our own query in `queries/imports/`, run over the tree
//! [`Syntax`] already keeps, so the grammar decides what is code and what is
//! a comment, string, template, regex or JSX text.

use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
    ops::{ControlFlow, Range},
    sync::OnceLock,
};

use fe_buffer::{Buffer, Text};
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
    arguments: u32,
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
            let arguments = query
                .capture_index_for_name("arguments")
                .unwrap_or_else(|| panic!("{} imports query: no @arguments", self.language));
            Compiled {
                query,
                module,
                recovered,
                arguments,
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
    // A misread of the grammar that swallows the rest of the file after
    // `<Foo extends="/" />` (a JSX attribute named `extends` is read as a
    // type parameter's constraint) is cured in the text, once: every such
    // attribute is respelled, and the file parsed again whole. That is one
    // extra parse however many attributes there are.
    // (A genuine `<T extends U>` is respelled too, so the original tree is
    // read as well and the two results merged.)
    if tree.root_node().has_error() {
        match parse_corrected(language, text, &poll) {
            Ok(Some((reparsed, buffer))) => {
                query_range(
                    compiled,
                    &reparsed,
                    buffer.text(),
                    0..text.len(),
                    &poll,
                    &mut found,
                );
            }
            Ok(None) => {}
            Err(error) => return Err(error),
        }
    }
    query_range(compiled, tree, text, 0..text.len(), &poll, &mut found);
    if stopped.get() {
        return Err(ImportsError::Cancelled);
    }
    found.sort_by_key(|range| range.start);
    found.dedup();
    Ok(found)
}

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
    // Whether each `arguments` node holds a lone argument, by node id: its
    // children are walked once however many strings it has.
    let mut lone: HashMap<usize, bool> = HashMap::new();
    let mut matches =
        cursor.matches_with_options(&compiled.query, tree.root_node(), source, options);
    while let Some(found_match) = matches.next() {
        if poll() {
            break;
        }
        // (A string's own `parent()` walks its siblings, so a `require(…)`
        // of many arguments gets its `arguments` node from the match.)
        let arguments = found_match
            .captures()
            .iter()
            .find(|capture| capture.index == compiled.arguments)
            .map(|capture| capture.node);
        for capture in found_match.captures() {
            let node = capture.node;
            if !range.contains(&node.start_byte()) {
                continue;
            }
            let is = if capture.index == compiled.module {
                is_module(node, arguments, &mut lone)
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

/// How many bytes the recovery scan reads between polls.
const SCAN_POLL_BYTES: usize = 4096;

/// What the scan last saw, apart from whitespace and comments.
#[derive(Clone, Copy, PartialEq)]
enum Token {
    Less,
    Name,
    Other,
}

/// The offsets of each `extends` that follows `<` and a name, comments and
/// any whitespace aside: `<Foo extends …`. Read as a type parameter's
/// constraint, it breaks the grammar when it is really a JSX attribute. One
/// pass over the text, polling every [`SCAN_POLL_BYTES`] bytes however they
/// are spent; `None` once `poll` says to stop.
fn extends_after_tag_name(text: &str, poll: &dyn Fn() -> bool) -> Option<Vec<usize>> {
    #[derive(PartialEq)]
    enum Mode {
        Code,
        Block,
        Line,
        Quoted(u8),
    }
    let bytes = text.as_bytes();
    let mut found = Vec::new();
    let mut mode = Mode::Code;
    let mut history = [Token::Other; 2];
    let push = |history: &mut [Token; 2], token| *history = [history[1], token];
    let mut polled = 0;
    let mut i = 0;
    while i < bytes.len() {
        if i - polled >= SCAN_POLL_BYTES {
            polled = i;
            if poll() {
                return None;
            }
        }
        let byte = bytes[i];
        match mode {
            Mode::Block => {
                if byte == b'*' && bytes.get(i + 1) == Some(&b'/') {
                    mode = Mode::Code;
                    i += 1;
                }
                i += 1;
            }
            Mode::Line => {
                if byte == b'\n' {
                    mode = Mode::Code;
                }
                i += 1;
            }
            Mode::Quoted(quote) => {
                match byte {
                    b'\\' => i += 1,
                    b'\n' => mode = Mode::Code,
                    _ if byte == quote => mode = Mode::Code,
                    _ => {}
                }
                i += 1;
            }
            Mode::Code => {
                let ch = text[i..].chars().next()?;
                if ch.is_whitespace() {
                    i += ch.len_utf8();
                } else if byte == b'/' && bytes.get(i + 1) == Some(&b'*') {
                    mode = Mode::Block;
                    i += 2;
                } else if byte == b'/' && bytes.get(i + 1) == Some(&b'/') {
                    mode = Mode::Line;
                    i += 2;
                } else if byte == b'\'' || byte == b'"' {
                    mode = Mode::Quoted(byte);
                    push(&mut history, Token::Other);
                    i += 1;
                } else if ch == '_' || ch == '$' || ch.is_alphanumeric() {
                    let start = i;
                    while let Some(ch) = text[i..].chars().next() {
                        if !(ch == '_' || ch == '$' || ch.is_alphanumeric()) {
                            break;
                        }
                        i += ch.len_utf8();
                        if i - polled >= SCAN_POLL_BYTES {
                            polled = i;
                            if poll() {
                                return None;
                            }
                        }
                    }
                    if &text[start..i] == "extends" && history == [Token::Less, Token::Name] {
                        found.push(start);
                    }
                    push(&mut history, Token::Name);
                } else {
                    push(
                        &mut history,
                        if byte == b'<' {
                            Token::Less
                        } else {
                            Token::Other
                        },
                    );
                    i += ch.len_utf8();
                }
            }
        }
    }
    Some(found)
}

/// The text with each `extends` after `<` and a name respelled `extendz`,
/// and its tree. A genuine constraint (`<T extends U>`) is respelled too,
/// which can only break the reading of that one construct: the caller
/// reads the original tree as well. No offset changes. `Ok(None)` if there
/// is no such word or the parse failed.
fn parse_corrected(
    language: &Language,
    text: &Text,
    poll: &dyn Fn() -> bool,
) -> Result<Option<(Tree, Buffer)>, ImportsError> {
    let source = text.slice_to_string(0..text.len());
    let Some(found) = extends_after_tag_name(&source, poll) else {
        return Err(ImportsError::Cancelled);
    };
    if found.is_empty() {
        return Ok(None);
    }
    let mut bytes = source.into_bytes();
    for start in found {
        bytes[start + 6] = b'z';
    }
    let Ok(corrected) = String::from_utf8(bytes) else {
        return Ok(None);
    };
    let buffer = Buffer::new(&corrected);
    let mut parser = Parser::new();
    if parser.set_language(&language.grammar).is_err() {
        return Ok(None);
    }
    match parse_text(&mut parser, buffer.text(), None, poll) {
        Some(tree) => Ok(Some((tree, buffer))),
        None if poll() => Err(ImportsError::Cancelled),
        None => Ok(None),
    }
}

/// Whether a captured string is a module: not the text of a regex (see
/// [`after_division`]), and, in a `require(…)`, the only argument, comments
/// aside.
fn is_module(
    string: tree_sitter::Node,
    arguments: Option<tree_sitter::Node>,
    lone: &mut HashMap<usize, bool>,
) -> bool {
    match arguments {
        Some(arguments) => {
            *lone.entry(arguments.id()).or_insert_with(|| {
                #[cfg(test)]
                tests::ARGUMENT_VALIDATIONS.with(|count| count.set(count.get() + 1));
                let mut cursor = arguments.walk();
                // (Counting stops at a second argument.)
                let args = arguments
                    .named_children(&mut cursor)
                    .filter(|child| child.kind() != "comment")
                    .take(2)
                    .count();
                args == 1
            })
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
