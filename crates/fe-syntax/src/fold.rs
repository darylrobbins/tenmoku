//! Fold candidates: the rows a tree's blocks, bodies, lists and multi-line
//! comments span, found by node kind.
//!
//! Each language lists its foldable node kinds in [`KINDS`] rather than in a
//! `folds.scm` query. A fold starts at its node's first row and ends at its
//! last row with text, or the row before when that row starts with the
//! node's closing delimiter, so a folded `{ … }` still shows its `}`.
//!
//! Some languages' bodies start on the row after their header, as Python's
//! do: those fold from the row where the node before them ends, such as the
//! `:` of a `def`.

use std::{
    ops::{Range, RangeInclusive},
    sync::OnceLock,
};

use fe_buffer::Text;
use tree_sitter::{Node, Point, TreeCursor};

use crate::Syntax;

/// The rows of the foldable nodes starting in `rows`, sorted, each spanning
/// at least two rows. Only the part of the tree overlapping `rows` is walked.
/// None without a tree: no language, a file over its language's
/// parse limit, or before the first parse.
pub fn fold_ranges(syntax: &Syntax, rows: Range<u32>) -> Vec<RangeInclusive<u32>> {
    let mut ranges = Vec::new();
    for_each_candidate(syntax, rows.clone(), rows, |range| ranges.push(range));
    ranges.sort_by_key(|range| (*range.start(), *range.end()));
    ranges.dedup();
    ranges
}

/// The innermost candidate headed by `row`, or if there's none, the
/// innermost one containing it.
pub fn fold_range_at(syntax: &Syntax, row: u32) -> Option<RangeInclusive<u32>> {
    let mut headed: Option<RangeInclusive<u32>> = None;
    let mut containing: Option<RangeInclusive<u32>> = None;
    let rows = row..row.saturating_add(1);
    for_each_candidate(syntax, rows.clone(), 0..rows.end, |range| {
        if *range.start() == row {
            if headed.as_ref().is_none_or(|best| range.end() < best.end()) {
                headed = Some(range);
            }
        } else if range.contains(&row) {
            let key = |range: &RangeInclusive<u32>| (*range.start(), u32::MAX - *range.end());
            if containing
                .as_ref()
                .is_none_or(|best| key(&range) > key(best))
            {
                containing = Some(range);
            }
        }
    });
    headed.or(containing)
}

/// The foldable node kinds of one language, by name.
struct Kinds {
    language: &'static str,
    /// Fold from their own first row.
    nodes: &'static [&'static str],
    /// Fold from the row where the node before them ends.
    bodies: &'static [&'static str],
}

/// Named nodes that close their parent as `}` does. Anonymous ones, such as
/// `}`, `]`, `end` and `fi`, all do.
const CLOSERS: &[&str] = &[
    "end_tag",
    "ETag",
    "jsx_closing_element",
    "fenced_code_block_delimiter",
];

/// How a multi-line comment with no child nodes can end, on a row of its
/// own.
const COMMENT_CLOSERS: &[&str] = &["*/", "-->", "=end", "]]"];

const JAVASCRIPT: &[&str] = &[
    "statement_block",
    "class_body",
    "switch_body",
    "switch_case",
    "switch_default",
    "object",
    "object_pattern",
    "array",
    "array_pattern",
    "arguments",
    "formal_parameters",
    "named_imports",
    "export_clause",
    "template_string",
    "jsx_element",
    "comment",
];

const TYPESCRIPT: &[&str] = &[
    "statement_block",
    "class_body",
    "switch_body",
    "switch_case",
    "switch_default",
    "object",
    "object_pattern",
    "array",
    "array_pattern",
    "arguments",
    "formal_parameters",
    "named_imports",
    "export_clause",
    "template_string",
    "comment",
    "interface_body",
    "enum_body",
    "object_type",
];

const TSX: &[&str] = &[
    "statement_block",
    "class_body",
    "switch_body",
    "switch_case",
    "switch_default",
    "object",
    "object_pattern",
    "array",
    "array_pattern",
    "arguments",
    "formal_parameters",
    "named_imports",
    "export_clause",
    "template_string",
    "comment",
    "interface_body",
    "enum_body",
    "object_type",
    "jsx_element",
];

const C: &[&str] = &[
    "compound_statement",
    "field_declaration_list",
    "enumerator_list",
    "initializer_list",
    "argument_list",
    "parameter_list",
    "case_statement",
    "preproc_if",
    "preproc_ifdef",
    "comment",
];

const CPP: &[&str] = &[
    "compound_statement",
    "field_declaration_list",
    "enumerator_list",
    "initializer_list",
    "argument_list",
    "parameter_list",
    "case_statement",
    "preproc_if",
    "preproc_ifdef",
    "comment",
    "declaration_list",
];

/// One entry per language in [`crate::language`], compiled in or not.
const KINDS: &[Kinds] = &[
    Kinds {
        language: "rust",
        nodes: &[
            "block",
            "declaration_list",
            "field_declaration_list",
            "ordered_field_declaration_list",
            "enum_variant_list",
            "field_initializer_list",
            "match_block",
            "use_list",
            "arguments",
            "parameters",
            "array_expression",
            "tuple_expression",
            "token_tree",
            "macro_definition",
            "block_comment",
        ],
        bodies: &[],
    },
    Kinds {
        language: "python",
        nodes: &[
            "argument_list",
            "parameters",
            "dictionary",
            "list",
            "set",
            "tuple",
            "dictionary_comprehension",
            "list_comprehension",
            "set_comprehension",
            "generator_expression",
        ],
        bodies: &["block"],
    },
    Kinds {
        language: "javascript",
        nodes: JAVASCRIPT,
        bodies: &[],
    },
    Kinds {
        language: "typescript",
        nodes: TYPESCRIPT,
        bodies: &[],
    },
    Kinds {
        language: "tsx",
        nodes: TSX,
        bodies: &[],
    },
    Kinds {
        language: "json",
        nodes: &["object", "array", "comment"],
        bodies: &[],
    },
    Kinds {
        language: "toml",
        nodes: &["table", "table_array_element", "array", "inline_table"],
        bodies: &[],
    },
    Kinds {
        language: "go",
        nodes: &[
            "block",
            "field_declaration_list",
            "interface_type",
            "import_spec_list",
            "var_spec_list",
            "literal_value",
            "argument_list",
            "parameter_list",
            "expression_switch_statement",
            "type_switch_statement",
            "select_statement",
            "expression_case",
            "type_case",
            "communication_case",
            "default_case",
            "comment",
        ],
        bodies: &[],
    },
    Kinds {
        language: "c",
        nodes: C,
        bodies: &[],
    },
    Kinds {
        language: "cpp",
        nodes: CPP,
        bodies: &[],
    },
    Kinds {
        language: "bash",
        nodes: &[
            "compound_statement",
            "do_group",
            "if_statement",
            "case_statement",
            "case_item",
            "subshell",
            "array",
            "heredoc_body",
        ],
        bodies: &[],
    },
    Kinds {
        language: "html",
        nodes: &["element", "script_element", "style_element", "comment"],
        bodies: &[],
    },
    Kinds {
        language: "css",
        nodes: &["block", "keyframe_block_list", "comment"],
        bodies: &[],
    },
    Kinds {
        language: "yaml",
        nodes: &[
            "block_mapping_pair",
            "block_sequence_item",
            "flow_mapping",
            "flow_sequence",
        ],
        bodies: &[],
    },
    Kinds {
        language: "markdown",
        nodes: &["section", "fenced_code_block", "html_block", "block_quote"],
        bodies: &[],
    },
    Kinds {
        language: "java",
        nodes: &[
            "block",
            "class_body",
            "interface_body",
            "enum_body",
            "annotation_type_body",
            "constructor_body",
            "module_body",
            "switch_block",
            "switch_block_statement_group",
            "array_initializer",
            "element_value_array_initializer",
            "argument_list",
            "formal_parameters",
            "block_comment",
        ],
        bodies: &[],
    },
    Kinds {
        language: "ruby",
        nodes: &[
            "class",
            "module",
            "singleton_class",
            "method",
            "singleton_method",
            "do_block",
            "block",
            "begin",
            "if",
            "unless",
            "while",
            "until",
            "for",
            "case",
            "case_match",
            "then",
            "else",
            "elsif",
            "when",
            "hash",
            "array",
            "argument_list",
            "heredoc_body",
            "comment",
        ],
        bodies: &[],
    },
    Kinds {
        language: "lua",
        nodes: &["table_constructor", "arguments", "parameters", "comment"],
        bodies: &["block"],
    },
    Kinds {
        language: "php",
        nodes: &[
            "compound_statement",
            "declaration_list",
            "enum_declaration_list",
            "switch_block",
            "case_statement",
            "default_statement",
            "match_block",
            "array_creation_expression",
            "arguments",
            "formal_parameters",
            "comment",
        ],
        bodies: &["colon_block"],
    },
    Kinds {
        language: "csharp",
        nodes: &[
            "block",
            "declaration_list",
            "enum_member_declaration_list",
            "accessor_list",
            "switch_body",
            "switch_section",
            "switch_expression",
            "initializer_expression",
            "anonymous_object_creation_expression",
            "argument_list",
            "parameter_list",
            "comment",
        ],
        bodies: &[],
    },
    Kinds {
        language: "scala",
        nodes: &[
            "template_body",
            "with_template_body",
            "block",
            "case_block",
            "enum_body",
            "arguments",
            "parameters",
            "block_comment",
        ],
        bodies: &["indented_block", "indented_cases"],
    },
    Kinds {
        language: "xml",
        nodes: &["element", "Comment"],
        bodies: &[],
    },
    Kinds {
        language: "swift",
        nodes: &[
            "class_body",
            "protocol_body",
            "enum_class_body",
            "function_body",
            "computed_property",
            "lambda_literal",
            "switch_statement",
            "switch_entry",
            "willset_didset_block",
            "array_literal",
            "dictionary_literal",
            "value_arguments",
            "tuple_expression",
            "multiline_comment",
        ],
        bodies: &["statements"],
    },
    Kinds {
        language: "kotlin",
        nodes: &[
            "class_body",
            "enum_class_body",
            "function_body",
            "control_structure_body",
            "lambda_literal",
            "when_expression",
            "catch_block",
            "finally_block",
            "anonymous_initializer",
            "value_arguments",
            "function_value_parameters",
            "collection_literal",
            "import_list",
            "multiline_comment",
        ],
        bodies: &[],
    },
    Kinds {
        language: "sql",
        nodes: &[
            "statement",
            "column_definitions",
            "subquery",
            "block",
            "case",
            "list",
            "marginalia",
        ],
        bodies: &[],
    },
    Kinds {
        language: "dockerfile",
        nodes: &[
            "run_instruction",
            "cmd_instruction",
            "entrypoint_instruction",
            "copy_instruction",
            "add_instruction",
            "env_instruction",
            "label_instruction",
            "arg_instruction",
            "healthcheck_instruction",
            "onbuild_instruction",
            "shell_instruction",
            "heredoc_block",
            "json_string_array",
        ],
        bodies: &[],
    },
    Kinds {
        language: "make",
        nodes: &["rule", "conditional", "define_directive"],
        bodies: &[],
    },
];

#[derive(Clone, Copy, PartialEq, Eq)]
enum Class {
    Other,
    Node,
    Body,
    Closer,
}

/// Each language's [`Class`] for each node kind id, built on first use.
static CLASSES: [OnceLock<Vec<Class>>; KINDS.len()] = [const { OnceLock::new() }; KINDS.len()];

/// The [`Class`] of each of `syntax`'s node kinds, by kind id. `None` for a
/// language with nothing to fold.
fn classes(syntax: &Syntax) -> Option<&'static [Class]> {
    let language = syntax.language();
    let ix = KINDS
        .iter()
        .position(|kinds| kinds.language == language.name())?;
    let classes = CLASSES[ix].get_or_init(|| {
        let kinds = &KINDS[ix];
        let grammar = &language.grammar;
        (0..grammar.node_kind_count() as u16)
            .map(|id| {
                let named = grammar.node_kind_is_named(id);
                match grammar.node_kind_for_id(id) {
                    Some(kind) if named && kinds.nodes.contains(&kind) => Class::Node,
                    Some(kind) if named && kinds.bodies.contains(&kind) => Class::Body,
                    Some(kind) if !named || CLOSERS.contains(&kind) => Class::Closer,
                    _ => Class::Other,
                }
            })
            .collect()
    });
    Some(classes)
}

/// Calls `found` with the candidate of each foldable node overlapping `rows`
/// that starts in `starts`, in tree order, along with those of bodies just
/// below `rows` whose header ends in it.
fn for_each_candidate(
    syntax: &Syntax,
    rows: Range<u32>,
    starts: Range<u32>,
    mut found: impl FnMut(RangeInclusive<u32>),
) {
    let (Some(tree), Some(classes)) = (syntax.tree(), classes(syntax)) else {
        return;
    };
    if rows.is_empty() {
        return;
    }
    let candidates = Candidates {
        classes,
        text: syntax.text(),
        starts,
    };
    let start = Point::new(rows.start as usize, 0);
    let past = |node: Node| node.start_position().row >= rows.end as usize;
    let mut cursor = tree.walk();
    'nodes: loop {
        if let Some(range) = candidates.of(cursor.node()) {
            found(range);
        }
        // tree-sitter's search for the first child past `start` doesn't
        // backtrack out of a hidden node that ends after `start` but holds
        // no such child, so a miss falls back to stepping over the children
        // that end at or before `start`.
        if cursor.goto_first_child_for_point(start).is_some()
            || first_child_from(&mut cursor, start)
        {
            if !past(cursor.node()) {
                continue;
            }
            if let Some(range) = candidates.after(&cursor) {
                found(range);
            }
            cursor.goto_parent();
        }
        // On to the next node overlapping `rows`: a later sibling, or one of
        // an ancestor's.
        loop {
            if cursor.goto_next_sibling() {
                if !past(cursor.node()) {
                    continue 'nodes;
                }
                if let Some(range) = candidates.after(&cursor) {
                    found(range);
                }
            }
            if !cursor.goto_parent() {
                return;
            }
        }
    }
}

/// Moves to the first child of the cursor's node that ends after `start`,
/// staying put if there is none.
fn first_child_from(cursor: &mut TreeCursor, start: Point) -> bool {
    if !cursor.goto_first_child() {
        return false;
    }
    while cursor.node().end_position() <= start {
        if !cursor.goto_next_sibling() {
            cursor.goto_parent();
            return false;
        }
    }
    true
}

struct Candidates<'a> {
    classes: &'static [Class],
    text: &'a Text,
    /// The rows a candidate may start in. Finding where one ends can take
    /// a walk over a node's children, so the others aren't measured.
    starts: Range<u32>,
}

impl Candidates<'_> {
    fn class(&self, node: Node) -> Class {
        self.classes
            .get(node.kind_id() as usize)
            .copied()
            .unwrap_or(Class::Other)
    }

    /// The candidate of the first node past `rows`, at the cursor or after
    /// comments following it, for a body whose header ends in `self.starts`.
    fn after(&self, cursor: &TreeCursor) -> Option<RangeInclusive<u32>> {
        let mut cursor = cursor.clone();
        while cursor.node().is_extra() {
            if !cursor.goto_next_sibling() {
                return None;
            }
        }
        self.of(cursor.node())
    }

    /// The rows `node` folds, if it's foldable, starts in `self.starts`
    /// and spans more than one.
    fn of(&self, node: Node) -> Option<RangeInclusive<u32>> {
        let first = match self.class(node) {
            Class::Node => node.start_position().row,
            Class::Body => {
                let parent = node.parent()?;
                match header(parent, node) {
                    Some(header) => header.end_position().row,
                    None => parent.start_position().row,
                }
            }
            Class::Other | Class::Closer => return None,
        };
        if !self.starts.contains(&u32::try_from(first).ok()?) {
            return None;
        }
        let mut last = self.last_text_row(node, first);
        if last > first && self.closes_on(node, last) {
            last -= 1;
        }
        let row = |row: usize| u32::try_from(row).ok();
        (last > first).then_some(row(first)?..=row(last)?)
    }

    /// The last row of `node` with text on it, no earlier than `first`.
    fn last_text_row(&self, node: Node, first: usize) -> usize {
        let end = node.end_position();
        let mut last = end.row;
        let mut column = end.column;
        while last > first && self.blank(last, column) {
            last -= 1;
            column = usize::MAX;
        }
        last
    }

    /// Whether `node`'s row `last` starts with its closing delimiter.
    fn closes_on(&self, node: Node, last: usize) -> bool {
        let Some(closer) = last_child(&mut node.walk()) else {
            return self.opaque_closes_on(node, last);
        };
        // The node's only delimiter may be an opening one, left unclosed.
        self.class(closer) == Class::Closer
            && closer.start_byte() > node.start_byte()
            && closer.start_position().row == last
            && self.starts_row(closer.start_position())
    }

    /// Whether `node`'s row `last` holds just a comment's closing delimiter,
    /// for a node with no children, such as many grammars' comments.
    fn opaque_closes_on(&self, node: Node, last: usize) -> bool {
        let end = node.end_position();
        let columns = if last == end.row {
            end.column
        } else {
            usize::MAX
        };
        let Some(range) = self.row_range(last, columns) else {
            return false;
        };
        let mut chars = self
            .text
            .chunks_in_range(range)
            .flat_map(str::chars)
            .skip_while(|ch| ch.is_whitespace());
        // Longer than any closer, so a long first word isn't collected.
        let word: String = chars
            .by_ref()
            .take_while(|ch| !ch.is_whitespace())
            .take(5)
            .collect();
        COMMENT_CLOSERS.contains(&word.as_str()) && chars.all(char::is_whitespace)
    }

    /// Whether `point` has only whitespace before it on its row.
    fn starts_row(&self, point: Point) -> bool {
        self.blank(point.row, point.column)
    }

    /// Whether the first `columns` bytes of `row` are whitespace.
    fn blank(&self, row: usize, columns: usize) -> bool {
        self.row_range(row, columns).is_none_or(|range| {
            self.text
                .chunks_in_range(range)
                .flat_map(str::chars)
                .all(char::is_whitespace)
        })
    }

    /// The bytes of the first `columns` of `row`, if it's in the text.
    fn row_range(&self, row: usize, columns: usize) -> Option<Range<usize>> {
        let row = u32::try_from(row)
            .ok()
            .filter(|&row| row < self.text.row_count())?;
        let start = self.text.row_start(row);
        let len = (self.text.row_len(row) as usize).min(columns);
        Some(start..start + len)
    }
}

/// The last of `parent`'s children before `node` that isn't an extra, such
/// as a comment. One pass forward over them: `Node::prev_sibling` rescans
/// the earlier siblings on each call, so stepping back over many comments
/// with it takes time quadratic in their number.
fn header<'tree>(parent: Node<'tree>, node: Node<'tree>) -> Option<Node<'tree>> {
    let mut cursor = parent.walk();
    if !cursor.goto_first_child() {
        return None;
    }
    let mut header = None;
    loop {
        let child = cursor.node();
        if child == node {
            return header;
        }
        if !child.is_extra() {
            header = Some(child);
        }
        if !cursor.goto_next_sibling() {
            return header;
        }
    }
}

/// The last child of the cursor's node with any text, leaving the cursor on
/// it.
fn last_child<'tree>(cursor: &mut TreeCursor<'tree>) -> Option<Node<'tree>> {
    if !cursor.goto_last_child() {
        return None;
    }
    loop {
        let node = cursor.node();
        if node.end_byte() > node.start_byte() {
            return Some(node);
        }
        if !cursor.goto_previous_sibling() {
            return None;
        }
    }
}

#[cfg(test)]
mod large_prefix_tests;
#[cfg(test)]
mod lookup_tests;
#[cfg(test)]
mod tests;
