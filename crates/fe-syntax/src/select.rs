//! The syntax node enclosing a selection, for expanding it.

use std::ops::Range;

use tree_sitter::Node;

use crate::Syntax;

/// The next larger selection around `range`, as byte offsets.
///
/// For a non-empty `range`: the smallest named node of the root tree that
/// contains it and is strictly larger. With none, the root's range if that is
/// larger, else `None`.
///
/// For an empty `range` at `offset`: first the leaf touching `offset` whose
/// text is all word characters (alphanumeric or `_`: an identifier, keyword or
/// number), the one starting at `offset` (or around it) before the one ending
/// there; else the smallest named node containing `offset`. Feeding each
/// result back in therefore grows the selection on every call until the root.
///
/// Zero-width `MISSING` nodes of error recovery are skipped; `ERROR` nodes
/// count as named. None without a tree.
///
/// Known limit: injected layers are not looked at, only the root tree.
pub fn larger_syntax_node(syntax: &Syntax, range: Range<usize>) -> Option<Range<usize>> {
    let root = syntax.tree()?.root_node();
    if range.start < range.end {
        return enclosing(root, &range).map(|node| node.byte_range());
    }
    let offset = range.start;
    let inside = descend(root, offset, offset);
    if let Some(leaf) = inside.leaf.filter(|leaf| is_word(syntax, leaf)) {
        return Some(leaf.byte_range());
    }
    let before = offset
        .checked_sub(1)
        .map(|prev| descend(root, prev, offset));
    if let Some(leaf) = before
        .as_ref()
        .and_then(|path| path.leaf)
        .filter(|leaf| leaf.end_byte() == offset && is_word(syntax, leaf))
    {
        return Some(leaf.byte_range());
    }
    [Some(inside), before]
        .into_iter()
        .flatten()
        .filter_map(|path| path.named)
        .filter(|node| node.start_byte() < node.end_byte())
        .min_by_key(|node| (node.byte_range().len(), node.start_byte() != offset))
        .map(|node| node.byte_range())
}

/// The smallest named node containing `range` and strictly larger than it.
fn enclosing<'t>(root: Node<'t>, range: &Range<usize>) -> Option<Node<'t>> {
    let mut cursor = root.walk();
    let mut best = None;
    loop {
        let node = cursor.node();
        if node.start_byte() > range.start || node.end_byte() < range.end {
            break;
        }
        if node.is_named() && !node.is_missing() && node.byte_range().len() > range.len() {
            best = Some(node);
        }
        if node.child_count() == 0 || cursor.goto_first_child_for_byte(range.start).is_none() {
            break;
        }
    }
    best
}

/// What a descent towards a byte found.
struct Path<'t> {
    /// The smallest named node on the way that contains the offset, ends
    /// included.
    named: Option<Node<'t>>,
    /// The leaf the descent ended on, if it contains the position.
    leaf: Option<Node<'t>>,
}

/// Descend with one cursor towards the child containing `at`, keeping the
/// smallest named node on the way that also contains `offset`, ends included.
fn descend<'t>(root: Node<'t>, at: usize, offset: usize) -> Path<'t> {
    let mut cursor = root.walk();
    let mut named = None;
    let mut leaf = None;
    loop {
        let node = cursor.node();
        if node.start_byte() > at || node.end_byte() < offset {
            break;
        }
        if node.is_named() && !node.is_missing() && node.start_byte() <= offset {
            named = Some(node);
        }
        if node.child_count() == 0 {
            if node.end_byte() > at && !node.is_missing() {
                leaf = Some(node);
            }
            break;
        }
        if cursor.goto_first_child_for_byte(at).is_none() {
            break;
        }
    }
    Path { named, leaf }
}

fn is_word(syntax: &Syntax, leaf: &Node) -> bool {
    let range = leaf.byte_range();
    !range.is_empty()
        && syntax
            .text()
            .chunks_in_range(range)
            .flat_map(str::chars)
            .all(|ch| ch.is_alphanumeric() || ch == '_')
}
