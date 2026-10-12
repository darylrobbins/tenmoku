//! The bracket pair at the caret, from the syntax tree.
//!
//! Brackets are anonymous leaves of the node they delimit, so a bracket's
//! counterpart is a sibling. Brackets inside strings and comments are not
//! leaves at all, which is what text-only matching gets wrong.

use std::ops::Range;

use tree_sitter::{Node, TreeCursor};

use crate::Syntax;

/// A bracket and its counterpart, as byte ranges. `open` is the opening one,
/// whichever of the two was at the caret.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BracketPair {
    pub open: Range<usize>,
    pub close: Range<usize>,
}

/// The pair of the bracket at `offset`: one starting there, else one ending
/// there, so a caret between `)(` pairs the `(`. Counterparts are found among
/// the bracket's siblings, counting nesting of its kind between them.
///
/// None without a tree, when no bracket is at `offset`, when the counterpart
/// is missing or is a `MISSING` node of error recovery, or when the brackets are
/// children of an `ERROR` node (a pair inside a node that is itself in an
/// `ERROR` node is still paired). Work is linear in the parent's child count.
///
/// Past 4,096 siblings between a bracket and its counterpart the pair is taken
/// from the parent's far end without counting nesting.
///
/// Known limit: injected layers (Markdown code fences, HTML `<script>`) are
/// not looked at, only the root tree.
pub fn bracket_pair(syntax: &Syntax, offset: usize) -> Option<BracketPair> {
    let root = syntax.tree()?.root_node();
    let ending = offset.checked_sub(1).map(|start| (start, offset));
    [(offset, offset + 1)]
        .into_iter()
        .chain(ending)
        .find_map(|(start, end)| pair_of(root, start, end))
}

fn pair_of(root: Node, start: usize, end: usize) -> Option<BracketPair> {
    // Descend with one cursor: `Node::parent` and `descendant_for_byte_range`
    // each cost a pass over a wide parent's children.
    let mut cursor = root.walk();
    let mut in_error = false;
    loop {
        let node = cursor.node();
        if node.child_count() == 0 {
            break;
        }
        if !cursor.goto_first_child_for_byte(start).is_some() {
            return None;
        }
        // Tree-sitter picks the first child ending after `start`, which may
        // start later: a bracket must start exactly there.
        let child = cursor.node();
        if child.start_byte() > start || child.end_byte() < end {
            return None;
        }
        in_error = node.is_error();
    }
    let node = cursor.node();
    if node.is_named() || node.byte_range() != (start..end) {
        return None;
    }
    let (opens, family) = match node.kind() {
        "(" => (true, ")"),
        "[" => (true, "]"),
        "{" => (true, "}"),
        ")" => (false, "("),
        "]" => (false, "["),
        "}" => (false, "{"),
        _ => return None,
    };
    if in_error {
        return None;
    }
    let kind = node.kind_id();
    let other = node.language().id_for_node_kind(family, false);

    // Walk from the bracket towards its counterpart, one cursor step per
    // sibling, counting nested brackets of the same kind on the way. Past
    // `SCAN_LIMIT` siblings, `scan` gives up.
    let at = cursor.clone();
    let counterpart = match scan(&mut cursor, opens, kind, other) {
        Some(found) => found?,
        None => {
            // A parent this wide has no flat runs of brackets in practice:
            // take the first bracket of the other kind from its far end.
            cursor = at;
            cursor.goto_parent();
            let jumped = if opens {
                cursor.goto_last_child()
            } else {
                cursor.goto_first_child()
            };
            if !jumped {
                return None;
            }
            let far = if opens {
                Direction::Back
            } else {
                Direction::Forward
            };
            let found = scan_from_here(&mut cursor, far, other)?;
            let past = if opens {
                found.start_byte() >= end
            } else {
                found.end_byte() <= start
            };
            if !past {
                return None;
            }
            found
        }
    };

    if counterpart.is_missing() {
        return None;
    }
    let (open, close) = if opens {
        (node, counterpart)
    } else {
        (counterpart, node)
    };
    Some(BracketPair {
        open: open.byte_range(),
        close: close.byte_range(),
    })
}

/// How many siblings [`scan`] steps over before giving up.
const SCAN_LIMIT: usize = 4096;

#[derive(Clone, Copy)]
enum Direction {
    Forward,
    Back,
}

/// Steps from the bracket at `cursor` over its later (`opens`) or earlier
/// siblings to its counterpart. `Some(None)` when there is none, None when
/// the scan hit [`SCAN_LIMIT`].
fn scan<'t>(
    cursor: &mut TreeCursor<'t>,
    opens: bool,
    kind: u16,
    other: u16,
) -> Option<Option<Node<'t>>> {
    let mut depth = 0usize;
    for _ in 0..SCAN_LIMIT {
        let moved = if opens {
            cursor.goto_next_sibling()
        } else {
            cursor.goto_previous_sibling()
        };
        if !moved {
            return Some(None);
        }
        // Ids first: most siblings are neither bracket.
        let id = cursor.node().kind_id();
        if id == kind {
            depth += 1;
        } else if id == other {
            if depth == 0 {
                return Some(Some(cursor.node()));
            }
            depth -= 1;
        }
    }
    None
}

/// The first sibling of kind `id` at or after `cursor`, stepping in `direction`.
fn scan_from_here<'t>(
    cursor: &mut TreeCursor<'t>,
    direction: Direction,
    id: u16,
) -> Option<Node<'t>> {
    for _ in 0..SCAN_LIMIT {
        if cursor.node().kind_id() == id {
            return Some(cursor.node());
        }
        let moved = match direction {
            Direction::Forward => cursor.goto_next_sibling(),
            Direction::Back => cursor.goto_previous_sibling(),
        };
        if !moved {
            return None;
        }
    }
    None
}
