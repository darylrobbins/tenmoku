//! A document outline: the symbols of a parsed file, with their kind, range
//! and nesting, for an outline panel, breadcrumbs and "go to symbol".
//!
//! Each language's symbols come from our own outline query in
//! `queries/outline/`, run over the root tree [`Syntax`] already keeps, so
//! nothing is parsed again. The queries follow the capture convention of
//! tree-sitter tags: `@definition.<kind>` on the item's node and `@name` on
//! its name. Symbols in injected layers aren't listed.
//!
//! [`symbols`] lists every definition, for a tags index: the outline plus
//! what an outline leaves out, such as fields, enum variants and module-level
//! variables. Those come from `queries/definitions/`, run after the outline
//! query as one query.

use std::{collections::HashMap, ops::Range, sync::OnceLock};

use fe_buffer::{Text, Version};
use tree_sitter::{
    Node, Query, QueryCapture, QueryCursor, QueryPredicate, QueryPredicateArg,
    StreamingIterator as _, Tree,
};

use crate::{Language, Syntax, syntax::chunks};

/// The most items an [`Outline`] holds. A file with more lists the first
/// ones found and sets [`Outline::truncated`].
pub const MAX_OUTLINE_ITEMS: usize = 10_000;

/// Names longer than this many chars are cut.
const MAX_NAME_CHARS: usize = 200;

/// What an [`OutlineItem`] is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
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
    /// A field of a struct, union, class or interface. Only [`symbols`]
    /// lists these, as it does the next two.
    Field,
    /// An enum's variant, member or constant.
    Variant,
    /// A module-level variable.
    Variable,
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
            "field" => Self::Field,
            "variant" => Self::Variant,
            "variable" => Self::Variable,
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

/// Finding an outline or symbols, which can run on any thread. See
/// [`outline_job`] and [`symbols_job`].
pub struct OutlineJob {
    language: &'static Language,
    query: &'static OutlineQuery,
    /// Whether to run the symbols query rather than the outline query.
    symbols: bool,
    tree: Tree,
    text: Text,
    version: Version,
}

/// The work of [`outline`], to run on another thread: it holds a clone of
/// the tree and the text, so `syntax` can go on being edited meanwhile.
pub fn outline_job(syntax: &Syntax) -> Result<OutlineJob, NoOutline> {
    job(syntax, false)
}

/// The outline of `syntax`'s current text, found on this thread.
pub fn outline(syntax: &Syntax) -> Result<Outline, NoOutline> {
    Ok(outline_job(syntax)?.run())
}

/// Whether the language with this name has an outline query.
pub fn outline_supported(language_name: &str) -> bool {
    query_for(language_name).is_some()
}

/// The work of [`symbols`], to run on another thread, as [`outline_job`].
pub fn symbols_job(syntax: &Syntax) -> Result<OutlineJob, NoOutline> {
    job(syntax, true)
}

/// Every definition in `syntax`'s current text, found on this thread: the
/// [`outline`]'s items, with the same kinds, names and ranges, plus fields,
/// enum variants and module-level variables. Nesting, names, the cap and
/// the errors follow the outline's rules. Languages without a definitions
/// query, such as markdown and toml, are [`NoOutline::Unsupported`].
pub fn symbols(syntax: &Syntax) -> Result<Outline, NoOutline> {
    Ok(symbols_job(syntax)?.run())
}

/// Whether the language with this name has a definitions query, for
/// [`symbols`].
pub fn symbols_supported(language_name: &str) -> bool {
    query_for(language_name).is_some_and(|query| !query.definitions.is_empty())
}

fn job(syntax: &Syntax, symbols: bool) -> Result<OutlineJob, NoOutline> {
    let language = syntax.language();
    let query = query_for(language.name())
        .filter(|query| !symbols || !query.definitions.is_empty())
        .ok_or(NoOutline::Unsupported)?;
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
        symbols,
        tree,
        text: syntax.text().clone(),
        version: syntax.version(),
    })
}

impl OutlineJob {
    /// Runs the outline or symbols query once over the whole tree.
    pub fn run(self) -> Outline {
        let compiled = if self.symbols {
            self.query.symbols(self.language)
        } else {
            self.query.outline(self.language)
        };
        let text = &self.text;
        let mut cursor = QueryCursor::new();
        // Predicates like `#match?` read node text straight from the rope.
        let source = |node: Node| chunks(text, node.byte_range());
        let mut matches = cursor.matches(&compiled.query, self.tree.root_node(), source);
        let mut found: Vec<Found> = Vec::new();
        let mut by_node: HashMap<usize, usize> = HashMap::new();
        let mut truncated = false;
        // Once truncated, the sections still open: headings past the cap
        // aren't listed, but they end those sections.
        let mut open: Option<OpenSections> = None;
        while let Some(found_match) = matches.next() {
            let captures = found_match.captures();
            let predicates = &compiled.predicates[found_match.pattern_index];
            if !predicates.iter().all(|predicate| predicate.holds(captures)) {
                continue;
            }
            let mut item: Option<(OutlineKind, Node)> = None;
            let mut name: Option<Range<usize>> = None;
            let mut names_function = false;
            for capture in captures {
                match compiled.captures[capture.index as usize] {
                    Capture::Item(kind) => item = Some((kind, capture.node)),
                    Capture::Other => {}
                    Capture::Name => {
                        let declared = declared_name(capture.node);
                        names_function |= declares_function(declared);
                        let range = declared.byte_range();
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
            // A C prototype, int f(void);, declares a function, not a
            // variable, and isn't a definition.
            if kind == OutlineKind::Variable && names_function {
                continue;
            }
            let section = match kind {
                OutlineKind::Heading(level) => Some(Section::of(node, level)),
                _ => None,
            };
            if let Some(open) = &open {
                if let Some(section) = section {
                    open.close(&mut found, node.start_byte(), section);
                }
                continue;
            }
            let name_range = match kind {
                // Headings come from tree-sitter-md, whose node kinds these
                // are.
                OutlineKind::Heading(_) => match name {
                    Some(name) if node.kind() == "atx_heading" => {
                        without_closing_sequence(text, name)
                    }
                    Some(name) => name,
                    // An empty ATX heading.
                    None => {
                        let marker = node.child(0).map_or(node.start_byte(), |m| m.end_byte());
                        marker..marker
                    }
                },
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
                let sections = extend_to_sections(&mut found);
                if sections.is_empty() {
                    break;
                }
                if let Some(section) = section {
                    sections.close(&mut found, node.start_byte(), section);
                }
                open = Some(sections);
                continue;
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
        if !truncated {
            extend_to_sections(&mut found);
        }
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
/// the container's end. Ranges found this way nest, as levels do. Returns
/// the headings whose sections reach their container's end, for headings
/// past the cap to end.
fn extend_to_sections(found: &mut [Found]) -> OpenSections {
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
    open.retain(|_, stack| !stack.is_empty());
    OpenSections(open)
}

/// The headings of each container whose sections are still open, outermost
/// first, as indexes into the found items and levels.
struct OpenSections(HashMap<usize, Vec<(usize, u8)>>);

impl OpenSections {
    fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Ends the open sections that a heading at `start`, which isn't listed,
    /// ends. They stay open for others, since such headings may come in any
    /// order.
    fn close(&self, found: &mut [Found], start: usize, section: Section) {
        for &(ix, level) in self.0.get(&section.container).into_iter().flatten() {
            let range = &mut found[ix].range;
            if section.level <= level && range.start < start {
                range.end = range.end.min(start);
            }
        }
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
            // Parenthesized and attributed declarators have no field. Their
            // other named children are modifiers, attributes and comments.
            let mut cursor = node.walk();
            node.named_children(&mut cursor).find(|child| {
                let kind = child.kind();
                kind.ends_with("_declarator") || kind.ends_with("identifier")
            })
        });
        match inner {
            Some(inner) => node = inner,
            None => break,
        }
    }
    node
}

/// Whether `name`, the node [`declared_name`] found, names a C function:
/// `f` in `int *f(void)` and `int f [[deprecated]] (void)`, but not `fp` in
/// `int (*fp)(void)`, a pointer to one.
fn declares_function(name: Node) -> bool {
    let mut parent = name.parent();
    while let Some(node) = parent.filter(|node| {
        matches!(
            node.kind(),
            "parenthesized_declarator" | "attributed_declarator"
        )
    }) {
        parent = node.parent();
    }
    parent.is_some_and(|node| node.kind() == "function_declarator")
}

/// A predicate of ours on a captured node, which the definitions queries use
/// where a pattern can't say what they mean: a scope seen through any number
/// of wrappers, or a node a pattern can't exclude.
struct Predicate {
    capture: u32,
    test: Test,
}

enum Test {
    /// `(#scope? @node kind...)`: the scope of @node, a C declaration, is
    /// one of the kinds. That's its nearest ancestor other than preprocessor
    /// conditionals and a struct's or union's member list. A struct or union
    /// without a name is no scope, so its members don't match.
    Scope(Vec<Box<str>>),
    /// `(#destructured? @node)`: @node is a name a JavaScript destructuring
    /// pattern binds, at any depth, in a declaration at the top of the
    /// program, exported or not: a and c in `const { a, b: c } = o;`, but
    /// not b, a key.
    Destructured,
    /// `(#non-function? @node)`: @node, a JavaScript variable declarator,
    /// isn't bound to a function or generator expression, also through
    /// parentheses and TypeScript's type assertions.
    NonFunction,
    /// `(#chained? @node kind)`: @node, a Python name, is a target of a
    /// chained assignment, x or y in `x = y = 1`, in a statement directly
    /// in the scope of that kind: `module` or a `class_definition`'s body.
    Chained(Box<str>),
    /// `(#unchained? @node)`: @node, a Python assignment, isn't chained:
    /// its value isn't another assignment.
    Unchained,
    /// `(#unpacked? @node kind)`: @node, a Python name, is in a tuple or
    /// list a statement directly in the scope of that kind assigns to, a in
    /// `a, *b = 1, 2`, also when that assignment is chained.
    Unpacked(Box<str>),
}

impl Predicate {
    /// The predicate, if it's one of ours with the arguments it takes.
    fn new(predicate: &QueryPredicate) -> Option<Self> {
        let (QueryPredicateArg::Capture(capture), args) = predicate.args.split_first()? else {
            return None;
        };
        let args: Vec<Box<str>> = (args.iter())
            .map(|arg| match arg {
                QueryPredicateArg::String(arg) => Some(arg.clone()),
                QueryPredicateArg::Capture(_) => None,
            })
            .collect::<Option<_>>()?;
        let scope = || match &args[..] {
            [kind] => Some(kind.clone()),
            _ => None,
        };
        let test = match &*predicate.operator {
            "scope?" if !args.is_empty() => Test::Scope(args),
            "destructured?" if args.is_empty() => Test::Destructured,
            "non-function?" if args.is_empty() => Test::NonFunction,
            "chained?" => Test::Chained(scope()?),
            "unchained?" if args.is_empty() => Test::Unchained,
            "unpacked?" => Test::Unpacked(scope()?),
            _ => return None,
        };
        Some(Self {
            capture: *capture,
            test,
        })
    }

    /// Whether the match with these captures meets the predicate. One whose
    /// node wasn't captured doesn't.
    fn holds(&self, captures: &[QueryCapture]) -> bool {
        let Some(node) = (captures.iter())
            .find(|capture| capture.index == self.capture)
            .map(|capture| capture.node)
        else {
            return false;
        };
        match &self.test {
            Test::Scope(kinds) => {
                c_scope(node).is_some_and(|scope| kinds.iter().any(|kind| **kind == *scope.kind()))
            }
            Test::Destructured => destructured(node),
            Test::NonFunction => !bound_to_function(node),
            Test::Chained(scope) => assigned(node, false)
                .is_some_and(|(statement, chained)| chained && statement_in(statement, scope)),
            Test::Unchained => !chains(node),
            Test::Unpacked(scope) => {
                assigned(node, true).is_some_and(|(statement, _)| statement_in(statement, scope))
            }
        }
    }
}

/// `node`'s field in `parent` is `field`.
fn is_field(parent: Node, field: &str, node: Node) -> bool {
    parent.child_by_field_name(field) == Some(node)
}

/// The scope [`Test::Scope`] describes.
fn c_scope(node: Node) -> Option<Node> {
    let mut scope = node.parent()?;
    while matches!(
        scope.kind(),
        "preproc_if"
            | "preproc_ifdef"
            | "preproc_elif"
            | "preproc_elifdef"
            | "preproc_else"
            | "field_declaration_list"
    ) {
        scope = scope.parent()?;
    }
    let anonymous = matches!(scope.kind(), "struct_specifier" | "union_specifier")
        && scope.child_by_field_name("name").is_none();
    (!anonymous).then_some(scope)
}

/// See [`Test::Destructured`].
fn destructured(name: Node) -> bool {
    let mut node = name;
    let mut patterns = 0;
    let declarator = loop {
        let Some(parent) = node.parent() else {
            return false;
        };
        let binds = match parent.kind() {
            "object_pattern" | "array_pattern" | "rest_pattern" => true,
            "pair_pattern" => is_field(parent, "value", node),
            "assignment_pattern" | "object_assignment_pattern" => is_field(parent, "left", node),
            "variable_declarator" if is_field(parent, "name", node) => break parent,
            _ => false,
        };
        if !binds {
            return false;
        }
        patterns += 1;
        node = parent;
    };
    let Some(declaration) = declarator.parent() else {
        return false;
    };
    let mut parent = declaration.parent();
    if let Some(export) = parent.filter(|node| node.kind() == "export_statement") {
        parent = export.parent();
    }
    patterns > 0
        && matches!(
            declaration.kind(),
            "lexical_declaration" | "variable_declaration"
        )
        && parent.is_some_and(|node| node.kind() == "program")
}

/// Whether the JavaScript variable declarator `declarator` is bound to a
/// function. See [`Test::NonFunction`].
fn bound_to_function(declarator: Node) -> bool {
    let mut value = declarator.child_by_field_name("value");
    while let Some(node) = value {
        let mut cursor = node.walk();
        let mut children = node
            .named_children(&mut cursor)
            .filter(|child| child.kind() != "comment");
        value = match node.kind() {
            "parenthesized_expression"
            | "as_expression"
            | "satisfies_expression"
            | "non_null_expression" => children.next(),
            // <F>(() => 1): the type comes first.
            "type_assertion" => children.last(),
            kind => {
                return matches!(
                    kind,
                    "arrow_function" | "function_expression" | "generator_function"
                );
            }
        };
    }
    false
}

/// The statement assigning to the Python name `name`, and whether its
/// assignment is chained. `unpacked` says whether `name` is in a tuple or
/// list target, at any depth, rather than the whole target.
fn assigned(name: Node, unpacked: bool) -> Option<(Node, bool)> {
    let mut node = name;
    let mut parent = node.parent()?;
    if unpacked {
        if !is_pattern(parent) {
            return None;
        }
        while is_pattern(parent) {
            node = parent;
            parent = node.parent()?;
        }
    }
    if parent.kind() != "assignment" || !is_field(parent, "left", node) {
        return None;
    }
    let mut assignment = parent;
    loop {
        let parent = assignment.parent()?;
        if parent.kind() != "assignment" {
            break;
        }
        if !is_field(parent, "right", assignment) {
            return None;
        }
        assignment = parent;
    }
    let statement = assignment.parent()?;
    (statement.kind() == "expression_statement").then_some((statement, chains(assignment)))
}

/// Whether the Python assignment `assignment`'s value is another
/// assignment, as in `x = y = 1`.
fn chains(assignment: Node) -> bool {
    (assignment.child_by_field_name("right")).is_some_and(|right| right.kind() == "assignment")
}

fn is_pattern(node: Node) -> bool {
    matches!(
        node.kind(),
        "pattern_list" | "tuple_pattern" | "list_pattern" | "list_splat_pattern"
    )
}

/// Whether the Python statement `statement` is directly in a scope of kind
/// `scope`: in the module, or in the body of a class definition.
fn statement_in(statement: Node, scope: &str) -> bool {
    let Some(parent) = statement.parent() else {
        return false;
    };
    match parent.kind() {
        "block" => parent
            .parent()
            .is_some_and(|owner| owner.kind() == scope && is_field(owner, "body", parent)),
        kind => kind == scope,
    }
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
/// heading; the grammar keeps them in its content. Only spaces and tabs
/// count here: `# T #` followed by a nonbreaking space keeps its `#`.
fn without_closing_sequence(text: &Text, content: Range<usize>) -> Range<usize> {
    // The end of the last char other than a space or tab and, when that
    // char ends a run of #s after a space, the end of the text before the
    // run.
    let mut end = content.start;
    let mut run: Option<usize> = None;
    let mut offset = content.start;
    let mut after_space = true;
    for chunk in chunks(text, content.clone()) {
        for c in String::from_utf8_lossy(chunk).chars() {
            let space = matches!(c, ' ' | '\t');
            if c == '#' {
                if after_space {
                    run = Some(end);
                }
            } else if !space {
                run = None;
            }
            offset += c.len_utf8();
            after_space = space;
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

/// One language's outline query, in parts concatenated in order, and its
/// definitions query, which follows them in the symbols query.
struct OutlineQuery {
    language: &'static str,
    parts: &'static [&'static str],
    /// Empty when the language has no symbols.
    definitions: &'static [&'static str],
    outline: OnceLock<Compiled>,
    symbols: OnceLock<Compiled>,
}

struct Compiled {
    query: Query,
    /// What each capture is, by capture index.
    captures: Vec<Capture>,
    /// Each pattern's predicates of ours, by pattern index. Tree-sitter
    /// checks its own, such as `#match?`.
    predicates: Vec<Vec<Predicate>>,
}

/// What a query capture is.
#[derive(Clone, Copy)]
enum Capture {
    /// `@definition.<kind>`, on the item's node.
    Item(OutlineKind),
    /// `@name`, on its name.
    Name,
    /// `@_<anything>`, on a node only predicates look at.
    Other,
}

impl OutlineQuery {
    const fn new(
        language: &'static str,
        parts: &'static [&'static str],
        definitions: &'static [&'static str],
    ) -> Self {
        Self {
            language,
            parts,
            definitions,
            outline: OnceLock::new(),
            symbols: OnceLock::new(),
        }
    }

    /// The outline query compiled for `language`'s grammar, on first use.
    fn outline(&self, language: &Language) -> &Compiled {
        (self.outline).get_or_init(|| self.compile(language, "outline", &[self.parts]))
    }

    /// The outline query followed by the definitions query, compiled as
    /// one on first use. The outline's patterns come first, so they win on
    /// the nodes both capture and every outline item keeps its kind.
    fn symbols(&self, language: &Language) -> &Compiled {
        (self.symbols)
            .get_or_init(|| self.compile(language, "symbols", &[self.parts, self.definitions]))
    }

    /// A query that doesn't compile, has a capture other than `@name`,
    /// `@definition.<kind>` and `@_<anything>`, or a predicate neither
    /// tree-sitter nor [`Predicate`] knows, is a bug: it panics naming the
    /// language.
    fn compile(&self, language: &Language, what: &str, parts: &[&[&str]]) -> Compiled {
        let source = parts.concat().concat();
        let query = Query::new(&language.grammar, &source)
            .unwrap_or_else(|error| panic!("{} {what} query: {error}", self.language));
        let captures = query
            .capture_names()
            .iter()
            .map(|&capture| match capture {
                "name" => Capture::Name,
                _ if capture.starts_with('_') => Capture::Other,
                _ => Capture::Item(OutlineKind::from_capture(capture).unwrap_or_else(|| {
                    panic!("{} {what} query: unknown capture @{capture}", self.language)
                })),
            })
            .collect();
        let predicates = (0..query.pattern_count())
            .map(|pattern| {
                (query.general_predicates(pattern).iter())
                    .map(|predicate| {
                        Predicate::new(predicate).unwrap_or_else(|| {
                            panic!(
                                "{} {what} query: unknown predicate #{}",
                                self.language, predicate.operator
                            )
                        })
                    })
                    .collect()
            })
            .collect();
        Compiled {
            query,
            captures,
            predicates,
        }
    }
}

const JAVASCRIPT: &str = include_str!("../queries/outline/javascript.scm");
const TYPESCRIPT: &str = include_str!("../queries/outline/typescript.scm");
const JAVASCRIPT_DEFINITIONS: &str = include_str!("../queries/definitions/javascript.scm");
const TYPESCRIPT_DEFINITIONS: &str = include_str!("../queries/definitions/typescript.scm");

/// The outline and definitions queries, by language name.
static QUERIES: [OutlineQuery; 10] = [
    OutlineQuery::new(
        "rust",
        &[include_str!("../queries/outline/rust.scm")],
        &[include_str!("../queries/definitions/rust.scm")],
    ),
    OutlineQuery::new(
        "python",
        &[include_str!("../queries/outline/python.scm")],
        &[include_str!("../queries/definitions/python.scm")],
    ),
    OutlineQuery::new("javascript", &[JAVASCRIPT], &[JAVASCRIPT_DEFINITIONS]),
    OutlineQuery::new(
        "typescript",
        &[JAVASCRIPT, TYPESCRIPT],
        &[JAVASCRIPT_DEFINITIONS, TYPESCRIPT_DEFINITIONS],
    ),
    OutlineQuery::new(
        "tsx",
        &[JAVASCRIPT, TYPESCRIPT],
        &[JAVASCRIPT_DEFINITIONS, TYPESCRIPT_DEFINITIONS],
    ),
    OutlineQuery::new(
        "go",
        &[include_str!("../queries/outline/go.scm")],
        &[include_str!("../queries/definitions/go.scm")],
    ),
    OutlineQuery::new(
        "java",
        &[include_str!("../queries/outline/java.scm")],
        &[include_str!("../queries/definitions/java.scm")],
    ),
    OutlineQuery::new(
        "c",
        &[include_str!("../queries/outline/c.scm")],
        &[include_str!("../queries/definitions/c.scm")],
    ),
    OutlineQuery::new(
        "markdown",
        &[include_str!("../queries/outline/markdown.scm")],
        &[],
    ),
    OutlineQuery::new("toml", &[include_str!("../queries/outline/toml.scm")], &[]),
];

fn query_for(language_name: &str) -> Option<&'static OutlineQuery> {
    QUERIES.iter().find(|query| query.language == language_name)
}
