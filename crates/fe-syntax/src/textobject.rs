//! The functions and classes around an offset, with the bytes of each and
//! of its body, for Vim's `af`/`if` and `ac`/`ic` text objects, as
//! nvim-treesitter-textobjects' `@function.outer`/`@function.inner` and
//! `@class.outer`/`@class.inner`.
//!
//! They're the [`outline`](crate::outline)'s items, found by its query run
//! rooted at each ancestor of the offset in turn, so a call costs the
//! offset's depth, with a cap on each ancestor, rather than the file's
//! size.

use std::ops::Range;

use fe_buffer::Text;
use tree_sitter::Node;

use crate::{OutlineKind, Syntax, outline::items_containing};

/// What a [`DefinitionObject`] is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DefinitionKind {
    /// A function or method.
    Function,
    /// A class, struct, enum, interface, trait, impl or type.
    Class,
}

/// A function or class around an offset.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DefinitionObject {
    pub kind: DefinitionKind,
    /// The bytes of its node, as its [`OutlineItem::range`](crate::OutlineItem::range).
    pub outer: Range<usize>,
    /// The bytes of its body from its first statement, expression or
    /// comment to its last: what's between the braces of a braced body, so
    /// without the braces and the blank rows at its edges, an empty range
    /// after `{` if there's nothing; else the body's node, as Python's
    /// block. `None` for a declaration without a body, as a trait method's
    /// signature.
    pub inner: Option<Range<usize>>,
}

/// The functions and classes whose node contains `offset`, innermost
/// first. None if the language has no outline query or `syntax` has no
/// tree matching its text.
pub fn definition_objects(syntax: &Syntax, offset: usize) -> Vec<DefinitionObject> {
    definition_objects_counted(syntax, offset).0
}

/// [`definition_objects`], with a bound on the syntax nodes it visited,
/// for tests of its cost: each ancestor of the offset, plus each query's
/// operations rounded up to the next hundred, plus the children looked at
/// finding the bodies' edges.
#[doc(hidden)]
pub fn definition_objects_counted(
    syntax: &Syntax,
    offset: usize,
) -> (Vec<DefinitionObject>, usize) {
    let mut visited = 0;
    let found = items_containing(syntax, offset, |text, language, kind, node| {
        let kind = match kind {
            OutlineKind::Function | OutlineKind::Method => DefinitionKind::Function,
            OutlineKind::Class
            | OutlineKind::Struct
            | OutlineKind::Enum
            | OutlineKind::Interface
            | OutlineKind::Trait
            | OutlineKind::Impl
            | OutlineKind::Type => DefinitionKind::Class,
            _ => return None,
        };
        Some(DefinitionObject {
            kind,
            outer: node.byte_range(),
            inner: body(language, node).map(|body| inner(text, body, &mut visited)),
        })
    });
    match found {
        Ok((objects, queried)) => (objects, queried + visited),
        Err(_) => (Vec::new(), visited),
    }
}

/// The body of the item `node` in `language`: its `body` field, or that
/// of the function it's bound to: a const's arrow function, a Lua
/// assignment's or table field's function.
fn body<'tree>(language: &str, node: Node<'tree>) -> Option<Node<'tree>> {
    if let Some(body) = node.child_by_field_name("body") {
        return Some(body);
    }
    let function = match language {
        "lua" => lua_function(node),
        _ => None,
    };
    (function.or_else(|| node.child_by_field_name("value")))?.child_by_field_name("body")
}

/// The function a Lua assignment gives its target, at the target's place
/// among the values: `node` is the statement when it has one target, else
/// the target.
fn lua_function(node: Node) -> Option<Node> {
    let mut cursor = node.walk();
    let (list, target) = match node.kind() {
        "assignment_statement" => {
            let list = (node.named_children(&mut cursor)).find(|n| n.kind() == "variable_list")?;
            (list, list.child_by_field_name("name")?)
        }
        _ => (node.parent().filter(|n| n.kind() == "variable_list")?, node),
    };
    let statement = list.parent()?;
    let place = (list.children_by_field_name("name", &mut cursor))
        .position(|name| name.id() == target.id())?;
    let values = (statement.named_children(&mut cursor)).find(|n| n.kind() == "expression_list")?;
    let mut value = values
        .children_by_field_name("value", &mut cursor)
        .nth(place)?;
    while value.kind() == "parenthesized_expression" {
        value = (value.named_children(&mut value.walk())).find(|n| n.kind() != "comment")?;
    }
    (value.kind() == "function_definition").then_some(value)
}

/// How many levels [`content_start`] and [`content_end`] look down a
/// node's edges for whitespace.
const MAX_EDGE_DEPTH: usize = 3;

/// The most bytes a hidden token at a node's edge, outside its visible
/// children, has to be for [`content_start`] and [`content_end`] to check
/// whether it's whitespace, as Go's line-ending terminator.
const MAX_HIDDEN_WHITESPACE: usize = 8;

/// The bytes of `body` inside its braces, if it has them, without the
/// whitespace tokens at its edges, counting in `visited` the children
/// looked at.
fn inner(text: &Text, body: Node, visited: &mut usize) -> Range<usize> {
    let count = body.child_count();
    *visited += count as usize;
    let braces = (count >= 2)
        .then(|| body.child(0).zip(body.child(count - 1)))
        .flatten()
        .filter(|(open, close)| open.kind() == "{" && close.kind() == "}");
    let edges = |first: Node, last: Node, visited: &mut usize| {
        let start = content_start(text, first, MAX_EDGE_DEPTH, visited);
        start..content_end(text, last, MAX_EDGE_DEPTH, visited).max(start)
    };
    let Some((open, _)) = braces else {
        return edges(body, body, visited);
    };
    let inside = 1..count - 1;
    let first = (inside.clone()).find_map(|i| body.child(i).filter(|child| !blank(*child)));
    let last = (inside.rev()).find_map(|i| body.child(i).filter(|child| !blank(*child)));
    match first.zip(last) {
        Some((first, last)) => edges(first, last, visited),
        None => open.end_byte()..open.end_byte(),
    }
}

/// Where `node` starts, or, when it starts with whitespace, a token or a
/// hidden one before its first child, where its first other child's
/// content starts, up to `depth` levels down. Any other node keeps its
/// start, so an expression, a string or a comment is never cut.
fn content_start(text: &Text, node: Node, depth: usize, visited: &mut usize) -> usize {
    let Some(first) = node.child(0).filter(|_| depth > 0) else {
        return node.start_byte();
    };
    if blank(first) {
        let count = node.child_count();
        *visited += count as usize;
        return (1..count)
            .find_map(|i| node.child(i).filter(|child| !blank(*child)))
            .map_or(node.start_byte(), |child| {
                content_start(text, child, depth - 1, visited)
            });
    }
    match whitespace(text, node.start_byte()..first.start_byte()) {
        true => content_start(text, first, depth - 1, visited),
        false => node.start_byte(),
    }
}

/// Where `node` ends, or, when it ends with whitespace, as Go's statements
/// end with a hidden line-ending terminator, where its last other child's
/// content ends, up to `depth` levels down. As [`content_start`], a node
/// is cut only at whitespace.
fn content_end(text: &Text, node: Node, depth: usize, visited: &mut usize) -> usize {
    let count = node.child_count();
    if depth == 0 || count == 0 {
        return node.end_byte();
    }
    *visited += count as usize;
    let Some(last) = node.child(count - 1) else {
        return node.end_byte();
    };
    if blank(last) {
        return (0..count - 1)
            .rev()
            .find_map(|i| node.child(i).filter(|child| !blank(*child)))
            .map_or(node.end_byte(), |child| {
                content_end(text, child, depth - 1, visited)
            });
    }
    match whitespace(text, last.end_byte()..node.end_byte()) {
        true => content_end(text, last, depth - 1, visited),
        false => node.end_byte(),
    }
}

/// Whether `range` is a few bytes of whitespace or nothing: a node's edge
/// outside its visible children.
fn whitespace(text: &Text, range: Range<usize>) -> bool {
    range.len() <= MAX_HIDDEN_WHITESPACE
        && (text.chunks_in_range(range)).all(|chunk| chunk.bytes().all(|b| b.is_ascii_whitespace()))
}

/// Whether `token` is whitespace alone, or empty, as a missing one.
fn blank(token: Node) -> bool {
    token.start_byte() == token.end_byte() || (!token.is_named() && token.kind().trim().is_empty())
}
