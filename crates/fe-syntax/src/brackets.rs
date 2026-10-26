//! The bracket pair at the caret, and the nesting depth of the brackets in a
//! range, from the syntax tree.
//!
//! Brackets are anonymous leaves of the node they delimit, so a bracket's
//! counterpart is a sibling. Brackets inside strings and comments are not
//! leaves at all, which is what text-only matching gets wrong.

use std::{ops::Range, sync::Arc};

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
/// inside an `ERROR` node, at any depth. Work is linear in the parent's child
/// count.
///
/// Counterparts more than 4,096 siblings away are found from the parent's
/// paired brackets, kept until the tree is edited or reparsed (as for
/// [`bracket_depths`]), so only the first lookup in a parse steps through the
/// parent's children.
///
/// Known limit: injected layers (Markdown code fences, HTML `<script>`) are
/// not looked at, only the root tree.
pub fn bracket_pair(syntax: &Syntax, offset: usize) -> Option<BracketPair> {
    let root = syntax.tree()?.root_node();
    let ending = offset.checked_sub(1).map(|start| (start, offset));
    [(offset, offset + 1)]
        .into_iter()
        .chain(ending)
        .find_map(|(start, end)| pair_of(syntax, root, start, end))
}

/// The innermost pair enclosing `offset`: its opening bracket ends at or
/// before `offset` and its closing one starts at or after it. Pairs are those
/// of [`bracket_pair`]: brackets in strings and comments are not leaves, a
/// pair with a `MISSING` bracket isn't one, and brackets that are children of
/// an `ERROR` node are skipped.
///
/// None without a tree or when no pair encloses `offset`. A node with more
/// than 4,096 children is looked at through its cached paired brackets, as in
/// [`bracket_pair`].
pub fn enclosing_bracket_pair(syntax: &Syntax, offset: usize) -> Option<BracketPair> {
    let root = syntax.tree()?.root_node();
    let mut ancestor = root.descendant_for_byte_range(offset, offset);
    while let Some(node) = ancestor {
        if !node.is_error()
            && let Some(pair) = enclosing_among_children(syntax, node, offset)
        {
            return Some(pair);
        }
        ancestor = node.parent();
    }
    None
}

fn enclosing_among_children(syntax: &Syntax, parent: Node, offset: usize) -> Option<BracketPair> {
    if parent.child_count() as usize > SCAN_LIMIT {
        return enclosing_among_wide_children(syntax, parent, offset);
    }
    let mut cursor = parent.walk();
    if !cursor.goto_first_child() {
        return None;
    }
    // Open brackets: kind and node, innermost last.
    let mut open: Vec<(&str, Node)> = Vec::new();
    loop {
        let node = cursor.node();
        if let Some((other, opens)) = bracket_kind(node) {
            if opens {
                open.push((node.kind(), node));
            } else if let Some(at) = open.iter().rposition(|&(kind, _)| kind == other) {
                let (_, opening) = open[at];
                open.truncate(at);
                if !opening.is_missing()
                    && !node.is_missing()
                    && opening.end_byte() <= offset
                    && node.start_byte() >= offset
                {
                    return Some(BracketPair {
                        open: opening.byte_range(),
                        close: node.byte_range(),
                    });
                }
            }
        }
        if !cursor.goto_next_sibling() {
            return None;
        }
    }
}

/// [`enclosing_among_children`] for a node with many children, from its
/// cached paired brackets.
fn enclosing_among_wide_children(
    syntax: &Syntax,
    parent: Node,
    offset: usize,
) -> Option<BracketPair> {
    let mut cursor = parent.walk();
    let paired = syntax.wide_brackets(parent.id(), || children_paired(&mut cursor));
    // The first child ending past `offset`: the children before it end at or
    // before `offset`, the ones from it on start at or after it (brackets are
    // a byte wide).
    let boundary = cursor
        .goto_first_child_for_byte(offset)
        .unwrap_or(parent.child_count() as usize);
    let split = paired.partition_point(|&(at, _)| at < boundary);
    // The innermost open before the boundary that nothing before it closes.
    let mut depth = 0usize;
    let opening = paired[..split].iter().rev().find_map(|&(at, opens)| {
        if !opens {
            depth += 1;
            None
        } else if depth == 0 {
            Some(at)
        } else {
            depth -= 1;
            None
        }
    })?;
    let mut depth = 0usize;
    let closing = paired[split..].iter().find_map(|&(at, opens)| {
        if opens {
            depth += 1;
            None
        } else if depth == 0 {
            Some(at)
        } else {
            depth -= 1;
            None
        }
    })?;
    Some(BracketPair {
        open: parent.child(opening as u32)?.byte_range(),
        close: parent.child(closing as u32)?.byte_range(),
    })
}

fn pair_of(syntax: &Syntax, root: Node, start: usize, end: usize) -> Option<BracketPair> {
    // Descend with one cursor: `Node::parent` and `descendant_for_byte_range`
    // each cost a pass over a wide parent's children.
    let mut cursor = root.walk();
    let mut in_error = false;
    // The leaf's parent and its index among the parent's children.
    let mut parent = None;
    loop {
        let node = cursor.node();
        if node.child_count() == 0 {
            break;
        }
        let index = cursor.goto_first_child_for_byte(start)?;
        parent = Some((node, index));
        // Tree-sitter picks the first child ending after `start`, which may
        // start later: a bracket must start exactly there.
        let child = cursor.node();
        if child.start_byte() > start || child.end_byte() < end {
            return None;
        }
        in_error |= node.is_error();
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
    let counterpart = match scan(&mut cursor, opens, kind, other) {
        Some(found) => found?,
        None => {
            let (parent, index) = parent?;
            wide_counterpart(syntax, parent, index, opens)?
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

/// A paired bracket and how many pairs enclose it: 0 for an outermost pair.
/// Both brackets of a pair have the same depth.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BracketDepth {
    pub range: Range<usize>,
    pub depth: usize,
}

/// The paired brackets overlapping `range`, in order, with their depths, for
/// colouring pairs by nesting. Empty without a tree.
///
/// Pairs are found as [`bracket_pair`] finds them, among siblings, so
/// brackets in strings and comments are never included, nor are brackets
/// whose counterpart is missing or a `MISSING` node, or brackets that are
/// children of an `ERROR` node. Brackets that don't pair don't count towards
/// the depth of the brackets inside them.
///
/// Empty while the tree has been edited but not reparsed
/// ([`Syntax::needs_parse`]): an edited tree's leaves are moved, not
/// re-lexed, so a bracket that quotes now enclose would still be a leaf.
///
/// Only the part of the tree touching `range` is walked, apart from pairing
/// each walked node's children. The pairing of a node with more than 4,096
/// children, a long list, is kept until the tree is edited or reparsed, so
/// its children are stepped through once a parse, not once a call.
///
/// Known limits: injected layers (Markdown code fences, HTML `<script>`) are
/// not looked at, only the root tree.
pub fn bracket_depths(syntax: &Syntax, range: Range<usize>) -> Vec<BracketDepth> {
    let mut found = Vec::new();
    let Some(tree) = syntax.tree() else {
        return found;
    };
    if range.is_empty() || syntax.needs_parse() || syntax.failed() {
        return found;
    }
    let mut cursor = tree.walk();
    // The levels entered, the innermost last, each on the cursor's node at
    // that level.
    let mut levels = Vec::new();
    if !enter(syntax, &mut cursor, &range, 0, &mut levels) {
        return found;
    }
    loop {
        let level = levels.last_mut().expect("a level is entered");
        let node = cursor.node();
        let past = node.start_byte() >= range.end;
        if !past {
            let bracket = level
                .paired
                .get(level.next)
                .filter(|&&(index, _)| index == level.index)
                .map(|&(_, opens)| opens);
            if let Some(opens) = bracket {
                level.next += 1;
                if !opens {
                    level.depth -= 1;
                }
                if node.end_byte() > range.start {
                    found.push(BracketDepth {
                        range: node.byte_range(),
                        depth: level.depth,
                    });
                }
                if opens {
                    level.depth += 1;
                }
            } else if node.child_count() > 0 && node.end_byte() > range.start {
                let depth = level.depth;
                if enter(syntax, &mut cursor, &range, depth, &mut levels) {
                    continue;
                }
            }
        }
        // On to the next sibling, or out of levels that are done.
        let mut done = past;
        loop {
            if !done && cursor.goto_next_sibling() {
                levels.last_mut().expect("a level is entered").index += 1;
                break;
            }
            levels.pop();
            if levels.is_empty() {
                return found;
            }
            cursor.goto_parent();
            done = false;
        }
    }
}

/// A node's paired bracket children: child index and whether it opens, in
/// child order.
pub(crate) type Paired = Arc<[(usize, bool)]>;

/// A node whose children [`bracket_depths`] is walking.
struct Level {
    /// The paired brackets among the children: child index and whether it
    /// opens, in child order.
    paired: Paired,
    /// The first of `paired` not yet passed.
    next: usize,
    /// The depth of a bracket opening at the cursor.
    depth: usize,
    /// The cursor's child index.
    index: usize,
}

/// Moves `cursor` from its node to the first child that reaches `range`,
/// pushing the level. False, with the cursor unmoved, when no child does.
fn enter(
    syntax: &Syntax,
    cursor: &mut TreeCursor,
    range: &Range<usize>,
    depth: usize,
    levels: &mut Vec<Level>,
) -> bool {
    let node = cursor.node();
    let paired: Paired = if node.is_error() {
        Arc::new([])
    } else if node.child_count() as usize > SCAN_LIMIT {
        syntax.wide_brackets(node.id(), || children_paired(cursor))
    } else {
        children_paired(cursor).into()
    };
    let index = match cursor.goto_first_child_for_byte(range.start) {
        Some(index) => index,
        // The seek gives up in some gaps between children, such as Go's
        // blank lines: step to the first child ending past the start.
        None => {
            if !cursor.goto_first_child() {
                return false;
            }
            let mut index = 0;
            while cursor.node().end_byte() <= range.start {
                if !cursor.goto_next_sibling() {
                    cursor.goto_parent();
                    return false;
                }
                index += 1;
            }
            index
        }
    };
    // Pairs opened, and closed, before the first child walked.
    let next = paired.partition_point(|&(at, _)| at < index);
    let depth = paired[..next].iter().fold(
        depth,
        |depth, &(_, opens)| if opens { depth + 1 } else { depth - 1 },
    );
    levels.push(Level {
        paired,
        next,
        depth,
        index,
    });
    true
}

/// Whether `node` is a bracket: the kind it pairs with, and whether it opens.
fn bracket_kind(node: Node) -> Option<(&'static str, bool)> {
    if node.is_named() || node.child_count() > 0 {
        return None;
    }
    Some(match node.kind() {
        "(" => (")", true),
        "[" => ("]", true),
        "{" => ("}", true),
        ")" => ("(", false),
        "]" => ("[", false),
        "}" => ("{", false),
        _ => return None,
    })
}

/// The paired brackets among the children of the cursor's node, by child
/// index, nested pairs counted. A pair with a `MISSING` bracket isn't one.
/// Leaves the cursor where it was.
fn children_paired(cursor: &mut TreeCursor) -> Vec<(usize, bool)> {
    let mut paired = Vec::new();
    if !cursor.goto_first_child() {
        return paired;
    }
    // Open brackets: child index, kind, and whether it's `MISSING`.
    let mut open: Vec<(usize, &str, bool)> = Vec::new();
    let mut index = 0;
    loop {
        let node = cursor.node();
        if let Some((other, opens)) = bracket_kind(node) {
            if opens {
                open.push((index, node.kind(), node.is_missing()));
            } else if let Some(at) = open.iter().rposition(|&(_, kind, _)| kind == other) {
                // Unpaired opens inside this pair stay unpaired.
                let (start, _, missing) = open[at];
                open.truncate(at);
                if !missing && !node.is_missing() {
                    paired.push((start, true));
                    paired.push((index, false));
                }
            }
        }
        if !cursor.goto_next_sibling() {
            break;
        }
        index += 1;
    }
    cursor.goto_parent();
    paired.sort_unstable();
    paired
}

/// How many siblings [`scan`] steps over before giving up, and how many
/// children make a node wide for [`bracket_depths`].
const SCAN_LIMIT: usize = 4096;

/// The counterpart of the paired bracket that is child `index` of `parent`,
/// from the parent's cached paired brackets, None when it isn't paired.
fn wide_counterpart<'t>(
    syntax: &Syntax,
    parent: Node<'t>,
    index: usize,
    opens: bool,
) -> Option<Node<'t>> {
    let paired = syntax.wide_brackets(parent.id(), || children_paired(&mut parent.walk()));
    let at = paired.binary_search_by_key(&index, |&(at, _)| at).ok()?;
    let mut depth = 0usize;
    let counterpart = if opens {
        paired[at + 1..].iter().find_map(|&(at, opens)| {
            if opens {
                depth += 1;
                None
            } else if depth == 0 {
                Some(at)
            } else {
                depth -= 1;
                None
            }
        })
    } else {
        paired[..at].iter().rev().find_map(|&(at, opens)| {
            if !opens {
                depth += 1;
                None
            } else if depth == 0 {
                Some(at)
            } else {
                depth -= 1;
                None
            }
        })
    }?;
    parent.child(counterpart as u32)
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
