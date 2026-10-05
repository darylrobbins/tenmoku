//! A document outline: the symbols of a parsed file, with their kind, range
//! and nesting, for an outline panel, breadcrumbs and "go to symbol".
//!
//! Each language's symbols come from our own outline query in
//! `queries/outline/`, run over the root tree [`Syntax`] already keeps, so
//! nothing is parsed again. The queries follow the capture convention of
//! tree-sitter tags: `@definition.<kind>` on the item's node and `@name` on
//! its name. Symbols in injected layers aren't listed.

use std::{collections::HashMap, ops::Range, sync::OnceLock};

use fe_buffer::{Text, Version};
use tree_sitter::{Node, Query, QueryCursor, StreamingIterator as _, Tree};

use crate::{Language, Syntax, syntax::chunks};

/// The most items an [`Outline`] holds. A file with more lists the first
/// ones found and sets [`Outline::truncated`].
pub const MAX_OUTLINE_ITEMS: usize = 10_000;

/// Names longer than this many chars are cut.
const MAX_NAME_CHARS: usize = 200;

/// What an [`OutlineItem`] is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum OutlineKind {
    Module,
    Class,
    Struct,
    Enum,
    Interface,
    Trait,
    Impl,
    Type,
    Function,
    Method,
    Constant,
    Macro,
    Table,
    /// A heading of level 1 to 6.
    Heading(u8),
}

impl OutlineKind {
    /// The kind a query capture names: `definition.method` is
    /// [`OutlineKind::Method`] and `definition.heading.2` is
    /// `Heading(2)`.
    pub fn from_capture(name: &str) -> Option<Self> {
        let kind = name.strip_prefix("definition.")?;
        Some(match kind {
            "module" => Self::Module,
            "class" => Self::Class,
            "struct" => Self::Struct,
            "enum" => Self::Enum,
            "interface" => Self::Interface,
            "trait" => Self::Trait,
            "impl" => Self::Impl,
            "type" => Self::Type,
            "function" => Self::Function,
            "method" => Self::Method,
            "constant" => Self::Constant,
            "macro" => Self::Macro,
            "table" => Self::Table,
            _ => match kind.strip_prefix("heading.")?.parse() {
                Ok(level @ 1..=6) => Self::Heading(level),
                _ => return None,
            },
        })
    }
}

/// One symbol of an [`Outline`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OutlineItem {
    pub name: String,
    pub kind: OutlineKind,
    /// The bytes of the item's node or, for a heading, of its section: down
    /// to the next heading of the same or a higher level.
    pub range: Range<usize>,
    /// The bytes of its name.
    pub name_range: Range<usize>,
    /// The 0-based row where the name starts.
    pub row: u32,
    /// 0 at the top level, else the parent's depth + 1.
    pub depth: u32,
    /// The index in [`Outline::items`] of the nearest earlier item whose
    /// range contains this one's.
    pub parent: Option<u32>,
}

/// The symbols of one version of a document.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Outline {
    /// In pre-order: by start, a parent before its children.
    pub items: Vec<OutlineItem>,
    /// Whether items past [`MAX_OUTLINE_ITEMS`] were left out.
    pub truncated: bool,
    /// The version of the text the items describe.
    pub version: Version,
}

impl Outline {
    /// The innermost item whose range contains `offset`, for following the
    /// caret.
    pub fn item_at(&self, offset: usize) -> Option<usize> {
        // Every item containing `offset` starts at or before it, and so is
        // the last such item or one of its ancestors.
        let last = self
            .items
            .partition_point(|item| item.range.start <= offset)
            .checked_sub(1)?;
        let mut ix = Some(last);
        while let Some(i) = ix {
            let item = &self.items[i];
            if item.range.contains(&offset) {
                return Some(i);
            }
            ix = item.parent.map(|parent| parent as usize);
        }
        None
    }
}

/// Why a [`Syntax`] has no outline.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NoOutline {
    /// The language has no outline query, or parsing gave up on the buffer.
    Unsupported,
    /// The text is over the language's parse limit, so there's no full tree.
    TooLarge,
    /// The tree is missing or hasn't been reparsed since the last edit.
    NotParsed,
}

/// Finding an outline, which can run on any thread. See [`outline_job`].
pub struct OutlineJob {
    language: &'static Language,
    query: &'static OutlineQuery,
    tree: Tree,
    text: Text,
    version: Version,
}

/// The work of [`outline`], to run on another thread: it holds a clone of
/// the tree and the text, so `syntax` can go on being edited meanwhile.
pub fn outline_job(syntax: &Syntax) -> Result<OutlineJob, NoOutline> {
    let language = syntax.language();
    let query = query_for(language.name()).ok_or(NoOutline::Unsupported)?;
    if syntax.failed() {
        return Err(NoOutline::Unsupported);
    }
    if syntax.text().len() > language.parse_limit() {
        return Err(NoOutline::TooLarge);
    }
    let tree = match syntax.tree() {
        Some(tree) if !syntax.needs_parse() => tree.clone(),
        _ => return Err(NoOutline::NotParsed),
    };
    Ok(OutlineJob {
        language,
        query,
        tree,
        text: syntax.text().clone(),
        version: syntax.version(),
    })
}

/// The outline of `syntax`'s current text, found on this thread.
pub fn outline(syntax: &Syntax) -> Result<Outline, NoOutline> {
    Ok(outline_job(syntax)?.run())
}

/// Whether the language with this name has an outline query.
pub fn outline_supported(language_name: &str) -> bool {
    query_for(language_name).is_some()
}

impl OutlineJob {
    /// Runs the outline query once over the whole tree.
    pub fn run(self) -> Outline {
        let compiled = self.query.compiled(self.language);
        let text = &self.text;
        let mut cursor = QueryCursor::new();
        // Predicates like `#match?` read node text straight from the rope.
        let source = |node: Node| chunks(text, node.byte_range());
        let mut matches = cursor.matches(&compiled.query, self.tree.root_node(), source);
        let mut found: Vec<Found> = Vec::new();
        let mut by_node: HashMap<usize, usize> = HashMap::new();
        let mut truncated = false;
        while let Some(found_match) = matches.next() {
            let mut item: Option<(OutlineKind, Node)> = None;
            let mut name: Option<Range<usize>> = None;
            for capture in found_match.captures() {
                match compiled.kinds[capture.index as usize] {
                    Some(kind) => item = Some((kind, capture.node)),
                    None => {
                        let range = declared_name(capture.node).byte_range();
                        name = Some(match name {
                            Some(name) => merge(name, range),
                            None => range,
                        });
                    }
                }
            }
            let Some((kind, node)) = item else {
                continue;
            };
            let mut section = None;
            let name_range = match kind {
                // Headings come from tree-sitter-md, whose node kinds these
                // are.
                OutlineKind::Heading(level) => {
                    section = Some(Section::of(node, level));
                    match name {
                        Some(name) if node.kind() == "atx_heading" => {
                            without_closing_sequence(text, name)
                        }
                        Some(name) => name,
                        // An empty ATX heading.
                        None => {
                            let marker = node.child(0).map_or(node.start_byte(), |m| m.end_byte());
                            marker..marker
                        }
                    }
                }
                _ => match name {
                    Some(name) => name,
                    None => continue,
                },
            };
            let pattern = found_match.pattern_index;
            if let Some(&ix) = by_node.get(&node.id()) {
                let found = &mut found[ix];
                if pattern == found.pattern {
                    // The same pattern again, as when a repeated @name
                    // matches fewer of the names: the name runs from the
                    // first to the last.
                    found.name_range = merge(found.name_range.clone(), name_range);
                } else if pattern < found.pattern {
                    // The first pattern in the query wins, as Method over
                    // Function for a fn in an impl.
                    *found = Found {
                        pattern,
                        kind,
                        range: node.byte_range(),
                        name_range,
                        section,
                    };
                }
                continue;
            }
            if found.len() == MAX_OUTLINE_ITEMS {
                truncated = true;
                break;
            }
            by_node.insert(node.id(), found.len());
            found.push(Found {
                pattern,
                kind,
                range: node.byte_range(),
                name_range,
                section,
            });
        }
        extend_to_sections(&mut found);
        // By start, longer first, so a parent comes before its children.
        found.sort_by_key(|found| (found.range.start, std::cmp::Reverse(found.range.end)));
        let mut items: Vec<OutlineItem> = Vec::with_capacity(found.len());
        let mut stack: Vec<usize> = Vec::new();
        for found in found {
            while let Some(&top) = stack.last() {
                let parent = &items[top].range;
                if parent.start <= found.range.start && found.range.end <= parent.end {
                    break;
                }
                stack.pop();
            }
            let parent = stack.last().copied();
            let name_range = found.name_range;
            items.push(OutlineItem {
                name: name(text, name_range.clone()),
                kind: found.kind,
                range: found.range,
                row: text.offset_to_point(name_range.start).row,
                name_range,
                depth: parent.map_or(0, |parent| items[parent].depth + 1),
                parent: parent.map(|parent| parent as u32),
            });
            stack.push(items.len() - 1);
        }
        Outline {
            items,
            truncated,
            version: self.version,
        }
    }
}

/// An item as the query found it, before nesting.
struct Found {
    pattern: usize,
    kind: OutlineKind,
    range: Range<usize>,
    name_range: Range<usize>,
    /// For a heading, what its section extends over.
    section: Option<Section>,
}

/// Where a heading's section can reach: the heading's level, and the
/// container it's in.
#[derive(Clone, Copy)]
struct Section {
    level: u8,
    container: usize,
    container_end: usize,
}

impl Section {
    /// The container is the nearest ancestor other than the grammar's own
    /// sections: the document, a block quote or a list item.
    fn of(heading: Node, level: u8) -> Self {
        let mut container = heading.parent();
        while let Some(node) = container.filter(|node| node.kind() == "section") {
            container = node.parent();
        }
        let container = container.unwrap_or(heading);
        Self {
            level,
            container: container.id(),
            container_end: container.end_byte(),
        }
    }
}

/// Gives each heading the range of its section: from the heading to the
/// next heading of the same or a higher level in its container, else to
/// the container's end. Ranges found this way nest, as levels do.
fn extend_to_sections(found: &mut [Found]) {
    let mut headings: Vec<(usize, Section)> = (found.iter().enumerate())
        .filter_map(|(ix, found)| Some((ix, found.section?)))
        .collect();
    headings.sort_by_key(|&(ix, _)| found[ix].range.start);
    // The open headings of each container, outermost first.
    let mut open: HashMap<usize, Vec<(usize, u8)>> = HashMap::new();
    for (ix, section) in headings {
        let start = found[ix].range.start;
        found[ix].range.end = section.container_end;
        let stack = open.entry(section.container).or_default();
        while let Some(&(top, level)) = stack.last() {
            if level < section.level {
                break;
            }
            found[top].range.end = start;
            stack.pop();
        }
        stack.push((ix, section.level));
    }
}

/// `a` and `b` and everything between.
fn merge(a: Range<usize>, b: Range<usize>) -> Range<usize> {
    a.start.min(b.start)..a.end.max(b.end)
}

/// The node that names what `node` declares. That's `node` itself, except
/// for a C declarator, which stands for the identifier at the end of its
/// chain: `f` in `*(*f)(int)`, without the parameters' names.
fn declared_name(mut node: Node) -> Node {
    while node.kind().ends_with("_declarator") {
        let inner = node.child_by_field_name("declarator").or_else(|| {
            // Parenthesized and attributed declarators have no field.
            let mut cursor = node.walk();
            node.named_children(&mut cursor)
                .find(|child| !matches!(child.kind(), "ms_call_modifier" | "attribute_declaration"))
        });
        match inner {
            Some(inner) => node = inner,
            None => break,
        }
    }
    node
}

/// The text of `range` with whitespace runs collapsed to one space, trimmed
/// and cut at [`MAX_NAME_CHARS`].
fn name(text: &Text, range: Range<usize>) -> String {
    let mut name = String::new();
    let mut chars = 0;
    let mut space = false;
    'chunks: for chunk in chunks(text, range) {
        for c in String::from_utf8_lossy(chunk).chars() {
            if c.is_whitespace() {
                space = !name.is_empty();
                continue;
            }
            if space {
                name.push(' ');
                chars += 1;
                space = false;
            }
            if chars >= MAX_NAME_CHARS {
                break 'chunks;
            }
            name.push(c);
            chars += 1;
        }
    }
    name.truncate(name.trim_end().len());
    name
}

/// An ATX heading's content without its closing sequence, as in `## D ##`:
/// the `#`s at the end when a space comes before them, or when they're all
/// there is, and the spaces around them. CommonMark leaves them out of the
/// heading; the grammar keeps them in its content.
fn without_closing_sequence(text: &Text, content: Range<usize>) -> Range<usize> {
    // The end of the last char other than whitespace and, when that char
    // ends a run of #s after a space, the end of the text before the run.
    let mut end = content.start;
    let mut run: Option<usize> = None;
    let mut offset = content.start;
    let mut after_space = true;
    for chunk in chunks(text, content.clone()) {
        for c in String::from_utf8_lossy(chunk).chars() {
            if c == '#' {
                if after_space {
                    run = Some(end);
                }
            } else if !c.is_whitespace() {
                run = None;
            }
            offset += c.len_utf8();
            after_space = c.is_whitespace();
            if !after_space {
                end = offset;
            }
        }
    }
    match run {
        Some(before) => content.start..before,
        None => content,
    }
}

/// One language's outline query, in parts concatenated in order.
struct OutlineQuery {
    language: &'static str,
    parts: &'static [&'static str],
    compiled: OnceLock<Compiled>,
}

struct Compiled {
    query: Query,
    /// What each capture is, by capture index: an item's kind, or `None`
    /// for `@name`.
    kinds: Vec<Option<OutlineKind>>,
}

impl OutlineQuery {
    const fn new(language: &'static str, parts: &'static [&'static str]) -> Self {
        Self {
            language,
            parts,
            compiled: OnceLock::new(),
        }
    }

    /// The query compiled for `language`'s grammar, on first use. A query
    /// that doesn't compile, or has a capture other than `@name` and
    /// `@definition.<kind>`, is a bug: it panics naming the language.
    fn compiled(&self, language: &Language) -> &Compiled {
        self.compiled.get_or_init(|| {
            let source = self.parts.concat();
            let query = Query::new(&language.grammar, &source)
                .unwrap_or_else(|error| panic!("{} outline query: {error}", self.language));
            let kinds = query
                .capture_names()
                .iter()
                .map(|&capture| match capture {
                    "name" => None,
                    _ => Some(OutlineKind::from_capture(capture).unwrap_or_else(|| {
                        panic!(
                            "{} outline query: unknown capture @{capture}",
                            self.language
                        )
                    })),
                })
                .collect();
            Compiled { query, kinds }
        })
    }
}

const JAVASCRIPT: &str = include_str!("../queries/outline/javascript.scm");
const TYPESCRIPT: &str = include_str!("../queries/outline/typescript.scm");

/// The outline queries, by language name.
static QUERIES: [OutlineQuery; 10] = [
    OutlineQuery::new("rust", &[include_str!("../queries/outline/rust.scm")]),
    OutlineQuery::new("python", &[include_str!("../queries/outline/python.scm")]),
    OutlineQuery::new("javascript", &[JAVASCRIPT]),
    OutlineQuery::new("typescript", &[JAVASCRIPT, TYPESCRIPT]),
    OutlineQuery::new("tsx", &[JAVASCRIPT, TYPESCRIPT]),
    OutlineQuery::new("go", &[include_str!("../queries/outline/go.scm")]),
    OutlineQuery::new("java", &[include_str!("../queries/outline/java.scm")]),
    OutlineQuery::new("c", &[include_str!("../queries/outline/c.scm")]),
    OutlineQuery::new(
        "markdown",
        &[include_str!("../queries/outline/markdown.scm")],
    ),
    OutlineQuery::new("toml", &[include_str!("../queries/outline/toml.scm")]),
];

fn query_for(language_name: &str) -> Option<&'static OutlineQuery> {
    QUERIES.iter().find(|query| query.language == language_name)
}
