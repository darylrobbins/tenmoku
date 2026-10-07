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

use std::{
    cell::{OnceCell, RefCell},
    collections::HashMap,
    ops::Range,
    sync::OnceLock,
};

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
        let context = Context {
            text,
            root: self.tree.root_node(),
            cpp_scopes: OnceCell::new(),
            ruby_scopes: RefCell::default(),
            lua_targets: RefCell::default(),
        };
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
            if !(predicates.iter()).all(|predicate| predicate.holds(captures, &context)) {
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
                        let range = name_range(declared);
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
/// chain: `f` in `*(*f)(int)`, without the parameters' names, and a C++
/// qualified name, which stands for its last part: `count` in
/// `int A::X::count`.
fn declared_name(node: Node) -> Node {
    let mut node = declarator_end(node);
    while node.kind() == "qualified_identifier" {
        match node.child_by_field_name("name") {
            Some(name) => node = declarator_end(name),
            None => break,
        }
    }
    node
}

/// The node at the end of a C declarator's chain, `f` in `*(*f)(int)`, or
/// `node` itself if it isn't a declarator.
fn declarator_end(mut node: Node) -> Node {
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

/// The bytes of the name `node` stands for: its own, except for a Ruby
/// symbol, `:name` in `attr_reader :name`, which names `name`, and a C++
/// conversion operator, `operator char*() const`, named up to its
/// parameters, `operator char*`, also through a reference: `operator int&`.
fn name_range(node: Node) -> Range<usize> {
    let range = node.byte_range();
    match node.kind() {
        "simple_symbol" => (range.start + 1).min(range.end)..range.end,
        "operator_cast" => {
            let mut declarator = node.child_by_field_name("declarator");
            while let Some(inner) = declarator {
                if inner.kind() == "abstract_function_declarator" {
                    break;
                }
                // Reference and parenthesized abstract declarators have no
                // field for the declarator they wrap.
                declarator = inner.child_by_field_name("declarator").or_else(|| {
                    let mut cursor = inner.walk();
                    (inner.named_children(&mut cursor))
                        .find(|child| child.kind().starts_with("abstract_"))
                });
            }
            let parameters = declarator.and_then(|d| d.child_by_field_name("parameters"));
            range.start..parameters.map_or(range.end, |parameters| parameters.start_byte())
        }
        _ => range,
    }
}

/// Whether `name`, the node [`declared_name`] found, names a C or C++
/// function: `f` in `int *f(void)`, `int f [[deprecated]] (void)` and
/// `int X::f()`, but not `fp` in `int (*fp)(void)`, a pointer to one. A C++
/// conversion operator, `operator bool()`, is a function too.
fn declares_function(name: Node) -> bool {
    if name.kind() == "operator_cast" {
        return true;
    }
    let mut parent = name.parent();
    while let Some(node) = parent.filter(|node| {
        matches!(
            node.kind(),
            "parenthesized_declarator" | "attributed_declarator" | "qualified_identifier"
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
    /// For the Lua tests, the capture of the list holding the target.
    list: Option<u32>,
    test: Test,
}

enum Test {
    /// `(#scope? @node kind...)`: the scope of @node, a C or C++
    /// declaration, is one of the kinds. That's its nearest ancestor other
    /// than preprocessor conditionals, a struct's, union's or class's
    /// member list and C++ template wrappers, however many: `template<class
    /// T> template<class U> U A<T>::x;` is at file scope. One of those
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
    /// `(#function? @node)`, or `#not-function?` for the opposite: @node, a
    /// C++ declarator, declares a function, as [`declares_function`] says.
    Function(bool),
    /// `(#member? @node)`, or `#not-member?` for the opposite: @node, a C++
    /// definition, declaration or declarator, declares a member of a type
    /// outside it, as `int X::f() { }` and `int X::count = 0;` do, rather
    /// than of a namespace. The qualifier is looked up as C++ does among
    /// the namespaces, namespace aliases and types the file declares: its
    /// first name from the declaration's namespace outwards, then each
    /// name in the last, inline namespaces included. One that isn't found
    /// is a type: a member defined in a .cpp file usually belongs to a
    /// class its header declares. See [`CppScopes`].
    Member(bool),
    /// `(#sole? @target @list)`, or `#not-sole?` for the opposite: @target,
    /// a Lua assignment target, as M.f in `M.f = ...`, is in @list, the
    /// variable list, of one target: `local x = 1` but not `local a, b = 1,
    /// 2`. A node's parent costs its siblings, so the list is captured.
    Sole(bool),
    /// `(#function-valued? @target @list)`: @target, a target in @list, is
    /// assigned a function: the value at its place in the list of values is
    /// a function definition, also in parentheses. g but not h in `local g,
    /// h = function() end, 1`.
    FunctionValued,
    /// `(#bound? @node kind...)`: @node is a name a Scala, Swift or PHP
    /// destructuring pattern binds, at any depth, in a declaration or an
    /// assignment directly in a scope of one of the kinds: a, b and c in
    /// `val (a, (b, c)) = pair`, `let (a, (b, c)) = pair` and
    /// `[$a, [$b, $c]] = $pair;`, but not $k, a key, in `[$k => $v] = $row;`
    /// nor A in `val (A, b) = pair`, which Scala matches against a constant.
    /// The scope is the statement's parent, except that a Scala package's
    /// and a PHP namespace's braces count as the package or namespace.
    Bound(Vec<Box<str>>),
    /// `(#assigned-in? @node kind...)`: @node, a Ruby assignment, is a
    /// statement directly in a scope of one of the kinds, `program` or a
    /// `class`'s or `module`'s body, or the value of such an assignment
    /// through any number of others: B = 1 in `A = B = 1`, and C = [1, 2] in
    /// `A, B = C = [1, 2]`.
    AssignedIn(Vec<Box<str>>),
}

/// What the predicates of a match can look at besides its captures.
struct Context<'a> {
    text: &'a Text,
    root: Node<'a>,
    cpp_scopes: OnceCell<CppScopes>,
    /// The scope of each Ruby assignment [`Test::AssignedIn`] has looked
    /// at, by node id, so a long chain is climbed once.
    ruby_scopes: RefCell<HashMap<usize, Option<Node<'a>>>>,
    /// What [`Test::Sole`] and [`Test::FunctionValued`] found of each Lua
    /// target, by node id, for the whole target list at once.
    lua_targets: RefCell<HashMap<usize, LuaTarget>>,
}

/// A Lua assignment target.
#[derive(Clone, Copy)]
struct LuaTarget {
    /// It's the only one in its list.
    sole: bool,
    /// The value at its place is a function.
    function: bool,
}

/// What a C++ file declares where, for [`Test::Member`]: its namespaces,
/// namespace aliases and types by their paths from the global namespace,
/// `["A", "B"]` for `A::B`. A namespace without a name adds nothing to
/// the paths in it.
#[derive(Default)]
struct CppScopes {
    declared: HashMap<Vec<String>, CppDeclared>,
    /// The inline namespaces directly in each namespace, by name.
    inline: HashMap<Vec<String>, Vec<String>>,
    /// The namespaces each namespace's `using namespace` directives name.
    using: HashMap<Vec<String>, Vec<CppQualifier>>,
}

enum CppDeclared {
    Namespace,
    Type,
    /// `namespace M = A::B;`, declared in the namespace at `scope`.
    Alias {
        scope: Vec<String>,
        target: CppQualifier,
    },
}

/// The names before the last `::` of a qualified name, or all the names of
/// a namespace alias's target.
struct CppQualifier {
    /// Whether it starts with `::`, from the global namespace.
    global: bool,
    /// None for a part that isn't a plain name: a template, as `Box<T>`,
    /// or a decltype.
    names: Vec<Option<String>>,
}

/// What a qualifier names.
enum CppResolved {
    Namespace(Vec<String>),
    Type,
    Unknown,
}

impl Predicate {
    /// The predicate, if it's one of ours with the arguments it takes.
    fn new(predicate: &QueryPredicate) -> Option<Self> {
        let (QueryPredicateArg::Capture(capture), args) = predicate.args.split_first()? else {
            return None;
        };
        if let ("sole?" | "not-sole?" | "function-valued?", [QueryPredicateArg::Capture(list)]) =
            (&*predicate.operator, args)
        {
            let test = match &*predicate.operator {
                "sole?" => Test::Sole(true),
                "not-sole?" => Test::Sole(false),
                _ => Test::FunctionValued,
            };
            return Some(Self {
                capture: *capture,
                list: Some(*list),
                test,
            });
        }
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
            "function?" if args.is_empty() => Test::Function(true),
            "not-function?" if args.is_empty() => Test::Function(false),
            "member?" if args.is_empty() => Test::Member(true),
            "not-member?" if args.is_empty() => Test::Member(false),
            "bound?" if !args.is_empty() => Test::Bound(args),
            "assigned-in?" if !args.is_empty() => Test::AssignedIn(args),
            _ => return None,
        };
        Some(Self {
            capture: *capture,
            list: None,
            test,
        })
    }

    /// Whether the match with these captures meets the predicate. One whose
    /// node wasn't captured doesn't.
    fn holds<'a>(&self, captures: &[QueryCapture<'a>], context: &Context<'a>) -> bool {
        let Some(node) = (captures.iter())
            .find(|capture| capture.index == self.capture)
            .map(|capture| capture.node)
        else {
            return false;
        };
        let list = (self.list).and_then(|list| {
            (captures.iter())
                .find(|capture| capture.index == list)
                .map(|capture| capture.node)
        });
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
            Test::Function(function) => declares_function(declared_name(node)) == *function,
            Test::Member(member) => cpp_member(node, context) == *member,
            Test::Sole(sole) => {
                list.is_some_and(|list| lua_target(node, list, &context.lua_targets).sole == *sole)
            }
            Test::FunctionValued => {
                list.is_some_and(|list| lua_target(node, list, &context.lua_targets).function)
            }
            Test::Bound(kinds) => (bound_by(node, context.text).and_then(statement_scope))
                .is_some_and(|scope| kinds.iter().any(|kind| **kind == *scope.kind())),
            Test::AssignedIn(kinds) => ruby_assignment_scope(node, &context.ruby_scopes)
                .is_some_and(|scope| kinds.iter().any(|kind| **kind == *scope.kind())),
        }
    }
}

/// Whether `node` is among the children of `parent` in `field`, which
/// can hold several.
fn in_field(parent: Node, field: &str, node: Node) -> bool {
    let mut cursor = parent.walk();
    parent
        .children_by_field_name(field, &mut cursor)
        .any(|child| child == node)
}

/// What [`Test::Sole`] and [`Test::FunctionValued`] ask of the Lua target
/// `target`, a child of `list`. The first time for a list, each of its
/// targets is paired with its value in one pass, so a long assignment
/// costs its length once.
fn lua_target(target: Node, list: Node, known: &RefCell<HashMap<usize, LuaTarget>>) -> LuaTarget {
    let mut known = known.borrow_mut();
    if let Some(&found) = known.get(&target.id()) {
        return found;
    }
    let mut cursor = list.walk();
    let targets: Vec<Node> = list.children_by_field_name("name", &mut cursor).collect();
    let values = (list.parent()).and_then(|statement| {
        let mut cursor = statement.walk();
        (statement.named_children(&mut cursor)).find(|child| child.kind() == "expression_list")
    });
    let values: Vec<Node> = values.map_or_else(Vec::new, |values| {
        let mut cursor = values.walk();
        values
            .children_by_field_name("value", &mut cursor)
            .collect()
    });
    let sole = targets.len() == 1;
    for (ix, target) in targets.into_iter().enumerate() {
        let function = values.get(ix).copied().is_some_and(is_lua_function);
        known.insert(target.id(), LuaTarget { sole, function });
    }
    known.get(&target.id()).copied().unwrap_or(LuaTarget {
        sole: false,
        function: false,
    })
}

/// Whether the Lua expression `value` is a function, parenthesized or not.
fn is_lua_function(mut value: Node) -> bool {
    while value.kind() == "parenthesized_expression" {
        let mut cursor = value.walk();
        let Some(inner) =
            (value.named_children(&mut cursor)).find(|child| child.kind() != "comment")
        else {
            return false;
        };
        value = inner;
    }
    value.kind() == "function_definition"
}

/// The declaration or assignment statement whose destructuring pattern
/// binds `name`. See [`Test::Bound`].
fn bound_by<'a>(name: Node<'a>, text: &Text) -> Option<Node<'a>> {
    // A Scala name starting with a capital or a backquote, in a pattern,
    // stands for a value to match, unless something@ binds it.
    let parent = name.parent()?;
    if name.kind() == "identifier"
        && !(parent.kind() == "capture_pattern" && in_field(parent, "name", name))
        && (chunks(text, name.byte_range()).next())
            .and_then(|chunk| String::from_utf8_lossy(chunk).chars().next())
            .is_some_and(|c| c.is_uppercase() || c == '`')
    {
        return None;
    }
    // A Swift tuple label, as left in (left: b, right: c), isn't a name.
    if name.kind() == "simple_identifier" {
        let mut next = name.next_sibling();
        while let Some(comment) =
            next.filter(|next| matches!(next.kind(), "comment" | "multiline_comment"))
        {
            next = comment.next_sibling();
        }
        if next.is_some_and(|next| next.kind() == ":") {
            return None;
        }
    }
    let mut node = name;
    loop {
        let parent = node.parent()?;
        let binds = match parent.kind() {
            // Scala.
            "tuple_pattern" | "named_tuple_pattern" => true,
            // (name = n): the label comes first and binds nothing.
            "named_pattern" => parent.named_child(0) != Some(node),
            "typed_pattern" | "case_class_pattern" | "repeat_pattern" => {
                in_field(parent, "pattern", node)
            }
            "infix_pattern" => in_field(parent, "left", node) || in_field(parent, "right", node),
            "capture_pattern" => {
                in_field(parent, "name", node) || in_field(parent, "pattern", node)
            }
            "val_definition" | "var_definition" => {
                return in_field(parent, "pattern", node).then_some(parent);
            }
            // Swift.
            "pattern" => true,
            "property_declaration" => return in_field(parent, "name", node).then_some(parent),
            // PHP. In [$k => $v], $k is a key, which is read.
            "list_literal" => {
                let mut next = node.next_sibling();
                while let Some(comment) =
                    next.filter(|next| matches!(next.kind(), "comment" | "multiline_comment"))
                {
                    next = comment.next_sibling();
                }
                next.is_none_or(|next| next.kind() != "=>")
            }
            "by_ref" => true,
            "assignment_expression" => {
                let statement = parent.parent()?;
                return (is_field(parent, "left", node)
                    && statement.kind() == "expression_statement")
                    .then_some(statement);
            }
            _ => false,
        };
        if !binds {
            return None;
        }
        node = parent;
    }
}

/// The scope [`Test::Bound`] describes.
fn statement_scope(statement: Node) -> Option<Node> {
    let parent = statement.parent()?;
    let owner = parent.parent();
    match (parent.kind(), owner.map(|owner| owner.kind())) {
        ("template_body", Some("package_clause"))
        | ("compound_statement", Some("namespace_definition")) => owner,
        _ => Some(parent),
    }
}

/// The scope [`Test::AssignedIn`] describes: the parent of the statement
/// the assignment chain `assignment` is in, or the class or module whose
/// body that is. `known` holds the scopes found so far, of every node
/// climbed through, since each step up costs the node's depth.
fn ruby_assignment_scope<'a>(
    assignment: Node<'a>,
    known: &RefCell<HashMap<usize, Option<Node<'a>>>>,
) -> Option<Node<'a>> {
    let mut known = known.borrow_mut();
    let mut climbed = Vec::new();
    let mut node = assignment;
    let scope = loop {
        if let Some(&scope) = known.get(&node.id()) {
            break scope;
        }
        climbed.push(node.id());
        let Some(parent) = node.parent() else {
            break None;
        };
        match parent.kind() {
            "assignment" if is_field(parent, "right", node) => {}
            // H, I = J = 1, 2 assigns J = 1 and 2.
            "right_assignment_list" => {}
            // A = (B = 1): parentheses around the one statement change
            // nothing.
            "parenthesized_statements" if sole_statement(parent) => {}
            "body_statement" => break parent.parent(),
            _ => break Some(parent),
        }
        node = parent;
    };
    for id in climbed {
        known.insert(id, scope);
    }
    scope
}

/// Whether `parenthesized` has one statement, comments aside.
fn sole_statement(parenthesized: Node) -> bool {
    let mut cursor = parenthesized.walk();
    parenthesized
        .named_children(&mut cursor)
        .filter(|child| child.kind() != "comment")
        .count()
        == 1
}

/// `node`'s field in `parent` is `field`.
fn is_field(parent: Node, field: &str, node: Node) -> bool {
    parent.child_by_field_name(field) == Some(node)
}

/// Whether the C++ definition, declaration or declarator `node` declares a
/// member of a type outside it. See [`Test::Member`].
fn cpp_member(mut node: Node, context: &Context) -> bool {
    if let Some(declarator) = (node.kind() != "qualified_identifier")
        .then(|| node.child_by_field_name("declarator"))
        .flatten()
    {
        node = declarator;
    }
    let node = declarator_end(node);
    if node.kind() != "qualified_identifier" {
        return false;
    }
    let qualifier = CppQualifier::of_name(node, context.text);
    let scopes = (context.cpp_scopes).get_or_init(|| cpp_scopes(context.root, context.text));
    let from = cpp_namespace_of(node, context.text);
    let mut lookup = CppLookup {
        budget: CPP_LOOKUP_BUDGET,
        active: Vec::new(),
    };
    !matches!(
        scopes.resolve(&from, &qualifier, &mut lookup),
        CppResolved::Namespace(_)
    )
}

/// The most namespaces one [`CppScopes::resolve`] looks in, aliases and
/// inline namespaces included, so a cycle of aliases ends.
const CPP_LOOKUP_BUDGET: u32 = 64;

/// The state of one [`CppScopes::resolve`].
struct CppLookup {
    /// What's left of [`CPP_LOOKUP_BUDGET`].
    budget: u32,
    /// The `using namespace` directives being resolved, each by the path of
    /// the namespace it's in and its index there. Resolving one's target
    /// skips it, so the search goes on in the enclosing namespaces, as in
    /// namespace B { using namespace A; }, where A is the global one.
    active: Vec<(Vec<String>, usize)>,
}

impl CppQualifier {
    /// The qualifier of `name`, a qualified identifier: A::B in A::B::f,
    /// and none, from the global namespace, in ::f.
    fn of_name(mut name: Node, text: &Text) -> Self {
        let global = name.child_by_field_name("scope").is_none();
        let mut names = Vec::new();
        loop {
            if let Some(scope) = name.child_by_field_name("scope") {
                names
                    .push((scope.kind() == "namespace_identifier").then(|| node_text(text, scope)));
            }
            match (name.child_by_field_name("name")).filter(|n| n.kind() == "qualified_identifier")
            {
                Some(inner) => name = inner,
                None => break,
            }
        }
        Self { global, names }
    }

    /// The names of `name`, a namespace identifier or a nested namespace
    /// specifier as in `namespace A::B` and `namespace M = ::A::B;`.
    fn of_namespace(name: Node, text: &Text) -> Self {
        let (global, names) = namespace_names(name, text);
        let names = names.into_iter().map(|(name, _)| Some(name)).collect();
        Self { global, names }
    }

    /// The namespace `using namespace A::B;` names.
    fn of_using(target: Node, text: &Text) -> Self {
        let mut qualifier = match target.kind() {
            "qualified_identifier" => Self::of_name(target, text),
            _ => Self {
                global: false,
                names: Vec::new(),
            },
        };
        (qualifier.names).push(Some(node_text(text, declared_name(target))));
        qualifier
    }
}

/// Whether the namespace identifier or nested namespace specifier `name`
/// starts with `::`, and its names, each with whether `inline` marks it,
/// as B in `namespace A::inline B`.
fn namespace_names(name: Node, text: &Text) -> (bool, Vec<(String, bool)>) {
    let mut global = false;
    let mut inline = false;
    let mut names = Vec::new();
    // In order, without recursion: the specifiers nest.
    let mut stack = vec![name];
    while let Some(node) = stack.pop() {
        match node.kind() {
            "namespace_identifier" => {
                names.push((node_text(text, node), inline));
                inline = false;
            }
            "inline" => inline = true,
            "::" => global |= names.is_empty() && node.start_byte() == name.start_byte(),
            _ => {
                let mut cursor = node.walk();
                let children: Vec<Node> = node.children(&mut cursor).collect();
                stack.extend(children.into_iter().rev());
            }
        }
    }
    (global, names)
}

impl CppScopes {
    /// What `qualifier` names when a declaration in the namespace at
    /// `from` uses it. `lookup` counts down the namespaces looked in.
    fn resolve(
        &self,
        from: &[String],
        qualifier: &CppQualifier,
        lookup: &mut CppLookup,
    ) -> CppResolved {
        let Some((first, rest)) = qualifier.names.split_first() else {
            return CppResolved::Namespace(Vec::new());
        };
        let Some(first) = first else {
            return CppResolved::Type;
        };
        let outermost = if qualifier.global { 0 } else { from.len() };
        let mut resolved = (0..=outermost)
            .rev()
            .find_map(|len| self.member(&from[..len], first, lookup))
            .unwrap_or(CppResolved::Unknown);
        for name in rest {
            resolved = match (resolved, name) {
                (CppResolved::Namespace(path), Some(name)) => {
                    (self.member(&path, name, lookup)).unwrap_or(CppResolved::Unknown)
                }
                (CppResolved::Namespace(_), None) => CppResolved::Type,
                // Everything in a type is a member of it.
                (resolved, _) => return resolved,
            };
        }
        resolved
    }

    /// What `name` names in the namespace at `path`, or in an inline
    /// namespace in it or one its `using namespace` directives name, if
    /// the file declares it.
    fn member(&self, path: &[String], name: &str, lookup: &mut CppLookup) -> Option<CppResolved> {
        lookup.budget = lookup.budget.checked_sub(1)?;
        let mut full = path.to_vec();
        full.push(name.to_owned());
        match self.declared.get(&full) {
            Some(CppDeclared::Namespace) => return Some(CppResolved::Namespace(full)),
            Some(CppDeclared::Type) => return Some(CppResolved::Type),
            Some(CppDeclared::Alias { scope, target }) => {
                // An alias names a namespace, also one declared elsewhere,
                // as in namespace fs = std::filesystem;. Nothing is known
                // to be in that one.
                return Some(match self.resolve(scope, target, lookup) {
                    CppResolved::Namespace(path) => CppResolved::Namespace(path),
                    _ => CppResolved::Namespace(full),
                });
            }
            None => {}
        }
        let inline = (self.inline.get(path).into_iter().flatten()).find_map(|inline| {
            let mut path = path.to_vec();
            path.push(inline.clone());
            self.member(&path, name, lookup)
        });
        if inline.is_some() {
            return inline;
        }
        let directives = self.using.get(path)?;
        for (ix, target) in directives.iter().enumerate() {
            let key = (path.to_vec(), ix);
            if lookup.active.contains(&key) {
                continue;
            }
            // Active through the lookup in the namespace it names too, so
            // a directive that names its own namespace or a cycle ends at
            // once instead of spending the budget.
            lookup.active.push(key);
            let found = match self.resolve(path, target, lookup) {
                CppResolved::Namespace(used) => self.member(&used, name, lookup),
                _ => None,
            };
            lookup.active.pop();
            if found.is_some() {
                return found;
            }
        }
        None
    }
}

/// The path of the namespace `node` is declared in, through the names of
/// the namespaces and types around it.
fn cpp_namespace_of(node: Node, text: &Text) -> Vec<String> {
    let mut parts = Vec::new();
    let mut ancestor = node.parent();
    while let Some(node) = ancestor {
        parts.extend(cpp_scope_names(node, text).into_iter().rev());
        ancestor = node.parent();
    }
    parts.reverse();
    parts
}

/// The names `node` adds to the paths of what's declared in it: a
/// namespace's names and a class's, struct's, union's or enum's name.
/// Empty for anything else, a namespace without a name included.
fn cpp_scope_names(node: Node, text: &Text) -> Vec<String> {
    let Some(name) = node.child_by_field_name("name") else {
        return Vec::new();
    };
    match node.kind() {
        "namespace_definition" => (namespace_names(name, text).1)
            .into_iter()
            .map(|(name, _)| name)
            .collect(),
        "class_specifier" | "struct_specifier" | "union_specifier" | "enum_specifier" => {
            match name.kind() {
                // struct Outer::Inner { }.
                "qualified_identifier" => {
                    let mut names: Vec<String> = (CppQualifier::of_name(name, text).names)
                        .into_iter()
                        .flatten()
                        .collect();
                    names.push(node_text(text, declared_name(name)));
                    names
                }
                // A specialization, struct Box<int> { }.
                "template_type" => (name.child_by_field_name("name"))
                    .map(|name| node_text(text, name))
                    .into_iter()
                    .collect(),
                _ => vec![node_text(text, name)],
            }
        }
        _ => Vec::new(),
    }
}

/// See [`CppScopes`]. Function bodies are left out: what's declared in one
/// can't be named from outside it.
fn cpp_scopes(root: Node, text: &Text) -> CppScopes {
    let mut scopes = CppScopes::default();
    // Nodes to look at, each with the index in `paths` of the path of the
    // namespace or type it's in.
    let mut paths: Vec<Vec<String>> = vec![Vec::new()];
    let mut stack = vec![(root, 0)];
    while let Some((node, path_ix)) = stack.pop() {
        let mut inner = path_ix;
        match node.kind() {
            "compound_statement" => continue,
            "namespace_definition"
            | "class_specifier"
            | "struct_specifier"
            | "union_specifier"
            | "enum_specifier" => {
                let namespace = node.kind() == "namespace_definition";
                let names = match node.child_by_field_name("name") {
                    // inline namespace v1 and namespace A::inline v2.
                    Some(name) if namespace => {
                        let mut cursor = node.walk();
                        let inline = node.children(&mut cursor).any(|c| c.kind() == "inline");
                        let mut names = namespace_names(name, text).1;
                        if let Some(last) = names.last_mut() {
                            last.1 |= inline;
                        }
                        names
                    }
                    _ => (cpp_scope_names(node, text).into_iter())
                        .map(|name| (name, false))
                        .collect(),
                };
                if !names.is_empty() {
                    let mut path = paths[path_ix].clone();
                    for (name, inline) in names {
                        if inline {
                            scopes
                                .inline
                                .entry(path.clone())
                                .or_default()
                                .push(name.clone());
                        }
                        path.push(name);
                        // namespace A::B declares A too; struct A::B only
                        // defines B.
                        if namespace {
                            scopes.declared.insert(path.clone(), CppDeclared::Namespace);
                        }
                    }
                    if !namespace {
                        scopes.declared.insert(path.clone(), CppDeclared::Type);
                    }
                    paths.push(path);
                    inner = paths.len() - 1;
                }
            }
            "alias_declaration" => {
                if let Some(name) = node.child_by_field_name("name") {
                    let mut path = paths[path_ix].clone();
                    path.push(node_text(text, name));
                    scopes.declared.insert(path, CppDeclared::Type);
                }
            }
            "type_definition" => {
                let mut cursor = node.walk();
                for declarator in node.children_by_field_name("declarator", &mut cursor) {
                    let mut path = paths[path_ix].clone();
                    path.push(node_text(text, declared_name(declarator)));
                    scopes.declared.insert(path, CppDeclared::Type);
                }
            }
            // using namespace A::B;
            "using_declaration" => {
                let mut cursor = node.walk();
                let directive = node.children(&mut cursor).any(|c| c.kind() == "namespace");
                let target = (node.named_children(&mut cursor))
                    .filter(|child| matches!(child.kind(), "identifier" | "qualified_identifier"))
                    .last();
                if let Some(target) = target.filter(|_| directive) {
                    let path = paths[path_ix].clone();
                    let target = CppQualifier::of_using(target, text);
                    scopes.using.entry(path).or_default().push(target);
                }
            }
            "namespace_alias_definition" => {
                let name = node.child_by_field_name("name");
                let mut cursor = node.walk();
                let target = (node.named_children(&mut cursor)).find(|child| {
                    Some(*child) != name
                        && matches!(
                            child.kind(),
                            "namespace_identifier" | "nested_namespace_specifier"
                        )
                });
                if let (Some(name), Some(target)) = (name, target) {
                    let mut path = paths[path_ix].clone();
                    path.push(node_text(text, name));
                    let alias = CppDeclared::Alias {
                        scope: paths[path_ix].clone(),
                        target: CppQualifier::of_namespace(target, text),
                    };
                    scopes.declared.insert(path, alias);
                }
            }
            _ => {}
        }
        let mut cursor = node.walk();
        let children: Vec<Node> = node.named_children(&mut cursor).collect();
        stack.extend(children.into_iter().rev().map(|child| (child, inner)));
    }
    scopes
}

fn node_text(text: &Text, node: Node) -> String {
    (chunks(text, node.byte_range()))
        .map(String::from_utf8_lossy)
        .collect()
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
            | "template_declaration"
    ) {
        scope = scope.parent()?;
    }
    let anonymous = matches!(
        scope.kind(),
        "struct_specifier" | "union_specifier" | "class_specifier"
    ) && scope.child_by_field_name("name").is_none();
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
            | "non_null_expression"
            // (function <T>(x: T) {})<string>: the function comes first.
            | "instantiation_expression" => children.next(),
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
static QUERIES: [OutlineQuery; 18] = [
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
        "cpp",
        &[include_str!("../queries/outline/cpp.scm")],
        &[include_str!("../queries/definitions/cpp.scm")],
    ),
    OutlineQuery::new(
        "csharp",
        &[include_str!("../queries/outline/csharp.scm")],
        &[include_str!("../queries/definitions/csharp.scm")],
    ),
    OutlineQuery::new(
        "ruby",
        &[include_str!("../queries/outline/ruby.scm")],
        &[include_str!("../queries/definitions/ruby.scm")],
    ),
    OutlineQuery::new(
        "php",
        &[include_str!("../queries/outline/php.scm")],
        &[include_str!("../queries/definitions/php.scm")],
    ),
    OutlineQuery::new(
        "swift",
        &[include_str!("../queries/outline/swift.scm")],
        &[include_str!("../queries/definitions/swift.scm")],
    ),
    OutlineQuery::new(
        "kotlin",
        &[include_str!("../queries/outline/kotlin.scm")],
        &[include_str!("../queries/definitions/kotlin.scm")],
    ),
    OutlineQuery::new(
        "scala",
        &[include_str!("../queries/outline/scala.scm")],
        &[include_str!("../queries/definitions/scala.scm")],
    ),
    OutlineQuery::new(
        "lua",
        &[include_str!("../queries/outline/lua.scm")],
        &[include_str!("../queries/definitions/lua.scm")],
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
