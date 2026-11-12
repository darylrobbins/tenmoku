//! The tests a file has, and the command that runs one: what `tests.run_at_cursor`
//! and `tests.run_file` ([`crate::test_commands`]) run.
//!
//! [`runnables`] walks the tree [`Syntax`] already has, as
//! [`crate::spell_ranges`] does: nothing here parses.
//!
//! - Rust: a function with an attribute whose path ends in `test`
//!   (`#[test]`, `#[tokio::test]`, `#[rstest]`) or is `#[test_case]`, and a
//!   module with `#[cfg(test)]`. Its selector is its path in the file
//!   (`tests::adds`).
//! - Go: a top-level `TestXxx(t *testing.T)`. Benchmarks, fuzz tests,
//!   examples and `TestMain` aren't run.
//! - Python: a top-level `test*` function, a `Test*` class and the `test*`
//!   methods in it (`TestAdd::test_one`), as pytest collects them.
//!
//! [`test_command`] makes a [`Task`] of one, or of a whole file, for
//! [`crate::Workbench::run_task`]: `cargo test`, `go test` or `pytest`. It
//! reads `Cargo.toml` and `go.mod` through [`ProjectFiles`], as
//! [`crate::task_defs::load`] does, and never above the root.

use std::{
    collections::BTreeMap,
    ops::Range,
    path::{Component, Path, PathBuf},
};

use fe_syntax::Syntax;
use fe_text::Text;
use tree_sitter::Node;

use kiln_l10n::{tr, verbatim};

use crate::task_defs::{Group, ProjectFiles, Source, Task, TaskCommand};

/// The languages [`runnables`] and [`test_command`] know.
pub const TEST_LANGUAGES: [&str; 3] = ["rust", "go", "python"];

/// What a [`Runnable`] runs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RunnableKind {
    /// One test.
    Test,
    /// The tests inside it: a `#[cfg(test)]` module, a `Test*` class.
    Module,
    /// A whole file's tests, which [`runnables`] doesn't list.
    File,
}

/// Something in a file that can run as tests.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Runnable {
    /// Its own name: the function's, the module's, the file's.
    pub name: String,
    pub kind: RunnableKind,
    /// The rows it covers, its attributes or decorators included.
    pub rows: Range<u32>,
    /// What names it to the test runner, from the file: `tests::adds` in
    /// Rust, `TestAdd::test_one` in Python, the function's name in Go.
    pub selector: String,
    /// Whether the selector is the test itself. Not for a Rust test whose
    /// attribute makes a module of cases (`#[rstest]`, `#[test_case]`), which
    /// runs whatever is under it.
    pub exact: bool,
}

impl Runnable {
    /// The whole of the file at `path`, which has `rows`.
    pub fn file(path: &Path, rows: u32) -> Self {
        let name = path.file_name().unwrap_or(path.as_os_str());
        Self {
            name: name.to_string_lossy().into_owned(),
            kind: RunnableKind::File,
            rows: 0..rows,
            selector: String::new(),
            exact: false,
        }
    }
}

/// The tests and test modules in `text`, outer before inner, as `syntax`'s
/// tree has them. `syntax` must be parsed since `text`'s last edit; there are
/// none without a tree, or in a language not in [`TEST_LANGUAGES`].
pub fn runnables(language: &str, syntax: &Syntax, text: &Text) -> Vec<Runnable> {
    let Some(tree) = syntax.tree() else {
        return Vec::new();
    };
    let root = tree.root_node();
    let mut found = Vec::new();
    match language {
        "rust" => rust_items(root, text, &mut Vec::new(), &mut found),
        "go" => go_tests(root, text, &mut found),
        "python" => python_items(root, text, &mut found),
        _ => {}
    }
    found
}

/// What runs from `row`: the innermost test holding it, else the innermost
/// module.
pub fn runnable_at(runnables: &[Runnable], row: u32) -> Option<&Runnable> {
    let innermost = |kind: RunnableKind| {
        (runnables.iter())
            .filter(|runnable| runnable.kind == kind && runnable.rows.contains(&row))
            .min_by_key(|runnable| runnable.rows.len())
    };
    innermost(RunnableKind::Test).or_else(|| innermost(RunnableKind::Module))
}

fn node_text(node: Node, text: &Text) -> String {
    text.slice_to_string(node.byte_range())
}

fn named_children(node: Node) -> Vec<Node> {
    let mut cursor = node.walk();
    node.named_children(&mut cursor).collect()
}

/// The rows from `first`'s to `last`'s, both included.
fn rows_of(first: Node, last: Node) -> Range<u32> {
    first.start_position().row as u32..last.end_position().row as u32 + 1
}

/// The Rust items in `node`, a file or a module's body, whose modules are
/// `path`.
fn rust_items(node: Node, text: &Text, path: &mut Vec<String>, found: &mut Vec<Runnable>) {
    let children = named_children(node);
    for (ix, item) in children.iter().enumerate() {
        let attributes = rust_attributes(&children[..ix]);
        let first = attributes.first().copied().unwrap_or(*item);
        let Some(name) = item.child_by_field_name("name") else {
            continue;
        };
        // libtest names a raw identifier without its `r#`.
        let name = node_text(name, text);
        let name = name.strip_prefix("r#").unwrap_or(&name).to_owned();
        let selector = || {
            [path.as_slice(), std::slice::from_ref(&name)]
                .concat()
                .join("::")
        };
        match item.kind() {
            "function_item" => {
                let tests =
                    (attributes.iter()).filter_map(|attribute| test_attribute(*attribute, text));
                if let Some(exact) = tests.reduce(|a, b| a && b) {
                    found.push(Runnable {
                        name: name.clone(),
                        kind: RunnableKind::Test,
                        rows: rows_of(first, *item),
                        selector: selector(),
                        exact,
                    });
                }
            }
            "mod_item" => {
                if attributes
                    .iter()
                    .any(|attribute| cfg_test(*attribute, text))
                {
                    found.push(Runnable {
                        name: name.clone(),
                        kind: RunnableKind::Module,
                        rows: rows_of(first, *item),
                        selector: selector(),
                        exact: false,
                    });
                }
                // The tests of a module in a file of its own are that file's.
                if let Some(body) = item.child_by_field_name("body") {
                    path.push(name);
                    rust_items(body, text, path, found);
                    path.pop();
                }
            }
            _ => {}
        }
    }
}

/// The attributes of the item after `before`: the ones just before it, with
/// comments among them.
fn rust_attributes<'a>(before: &[Node<'a>]) -> Vec<Node<'a>> {
    let mut attributes: Vec<Node> = (before.iter().rev())
        .take_while(|node| {
            matches!(
                node.kind(),
                "attribute_item" | "line_comment" | "block_comment"
            )
        })
        .filter(|node| node.kind() == "attribute_item")
        .copied()
        .collect();
    attributes.reverse();
    attributes
}

/// An attribute item's path's last segment, and its arguments.
fn attribute_parts<'a>(item: Node<'a>, text: &Text) -> Option<(String, Option<Node<'a>>)> {
    let attribute = named_children(item)
        .into_iter()
        .find(|node| node.kind() == "attribute")?;
    let path = attribute.named_child(0)?;
    let segment = match path.kind() {
        "scoped_identifier" => path.child_by_field_name("name")?,
        _ => path,
    };
    Some((
        node_text(segment, text),
        attribute.child_by_field_name("arguments"),
    ))
}

/// Whether `item` makes a test of its function: `Some(true)` if the test is the
/// function, `Some(false)` if it's a module of cases named for it.
fn test_attribute(item: Node, text: &Text) -> Option<bool> {
    let (segment, _) = attribute_parts(item, text)?;
    match segment.as_str() {
        "rstest" | "test_case" => Some(false),
        segment if segment.ends_with("test") => Some(true),
        _ => None,
    }
}

/// Whether `item` is `#[cfg(test)]`, or `#[cfg(all(test, …))]`.
fn cfg_test(item: Node, text: &Text) -> bool {
    let Some((segment, Some(arguments))) = attribute_parts(item, text) else {
        return false;
    };
    let arguments: String = (node_text(arguments, text).chars())
        .filter(|c| !c.is_whitespace())
        .collect();
    segment == "cfg"
        && (arguments == "(test)"
            || arguments
                .strip_prefix("(all(")
                .is_some_and(|rest| rest.split([',', ')']).any(|word| word == "test")))
}

/// Go's tests: `func TestXxx(t *testing.T)` at the top level, `Xxx` not
/// starting with a lowercase letter, as `go test` finds them.
fn go_tests(root: Node, text: &Text, found: &mut Vec<Runnable>) {
    for function in named_children(root) {
        if function.kind() != "function_declaration" {
            continue;
        }
        let Some(name) = function.child_by_field_name("name") else {
            continue;
        };
        let name = node_text(name, text);
        let named_as_test = (name.strip_prefix("Test"))
            .is_some_and(|rest| !rest.starts_with(|c: char| c.is_lowercase()));
        if !named_as_test || name == "TestMain" {
            continue;
        }
        let parameters = (function.child_by_field_name("parameters"))
            .map(named_children)
            .unwrap_or_default();
        let takes_t = match parameters.as_slice() {
            [parameter] => (parameter.child_by_field_name("type")).is_some_and(|ty| {
                let ty: String = node_text(ty, text).split_whitespace().collect();
                ty == "*testing.T"
            }),
            _ => false,
        };
        if takes_t {
            found.push(Runnable {
                name: name.clone(),
                kind: RunnableKind::Test,
                rows: rows_of(function, function),
                selector: name,
                exact: true,
            });
        }
    }
}

/// A Python definition, and where it starts: at its first decorator if it has
/// any.
fn undecorated(node: Node) -> (Node, Node) {
    match node.kind() {
        "decorated_definition" => match node.child_by_field_name("definition") {
            Some(definition) => (definition, node),
            None => (node, node),
        },
        _ => (node, node),
    }
}

/// The definition `node` is, if it's of `kind`, with its name.
fn python_definition<'a>(
    node: Node<'a>,
    kind: &str,
    text: &Text,
) -> Option<(Node<'a>, Node<'a>, String)> {
    let (definition, start) = undecorated(node);
    if definition.kind() != kind {
        return None;
    }
    let name = node_text(definition.child_by_field_name("name")?, text);
    Some((definition, start, name))
}

/// Python's tests as pytest collects them by default: `test*` functions at the
/// top level and in `Test*` classes, and those classes.
fn python_items(root: Node, text: &Text, found: &mut Vec<Runnable>) {
    let test = |definition: Node, start: Node, name: String, selector: String| Runnable {
        name,
        kind: RunnableKind::Test,
        rows: rows_of(start, definition),
        selector,
        exact: true,
    };
    for node in named_children(root) {
        if let Some((function, start, name)) = python_definition(node, "function_definition", text)
        {
            if name.starts_with("test") {
                found.push(test(function, start, name.clone(), name));
            }
            continue;
        }
        let Some((class, start, class_name)) = python_definition(node, "class_definition", text)
        else {
            continue;
        };
        if !class_name.starts_with("Test") {
            continue;
        }
        found.push(Runnable {
            name: class_name.clone(),
            kind: RunnableKind::Module,
            rows: rows_of(start, class),
            selector: class_name.clone(),
            exact: false,
        });
        let body = class.child_by_field_name("body").map(named_children);
        for node in body.unwrap_or_default() {
            if let Some((method, start, name)) =
                python_definition(node, "function_definition", text)
                && name.starts_with("test")
            {
                let selector = format!("{class_name}::{name}");
                found.push(test(method, start, name, selector));
            }
        }
    }
}

/// A Rust package's manifest's file name.
const CARGO_TOML: &str = verbatim!("Cargo.toml");

/// What a language with no test runner says.
pub fn no_runner(language: &str) -> String {
    tr!("workbench-lsp-tests-no-runner", language = language).into()
}

/// The task that runs `runnable`, of the file at `path`, in `language`, in the
/// folder `root`: why not, in words, if it can't be told.
///
/// # Errors
///
/// When `language` has no runner, `path` isn't under `root`, or no package
/// above it in the folder holds it (a Rust file needs a `Cargo.toml` with a
/// `[package]`, a Go file a `go.mod`).
pub fn test_command(
    files: &dyn ProjectFiles,
    language: &str,
    runnable: &Runnable,
    path: &Path,
    root: &Path,
) -> Result<Task, String> {
    if !TEST_LANGUAGES.contains(&language) {
        return Err(no_runner(language));
    }
    let Ok(relative) = path.strip_prefix(root) else {
        let path = path.display().to_string();
        return Err(tr!("workbench-lsp-tests-not-in-folder", path = path).into());
    };
    let (argv, cwd, source, matcher) = match language {
        "rust" => {
            let (dir, argv) = cargo_test(files, runnable, path, root, relative)?;
            (argv, dir, Source::Cargo, None)
        }
        "go" => {
            let (dir, argv) = go_test(files, runnable, path, root, relative)?;
            (argv, dir, Source::File, Some("go"))
        }
        _ => (
            pytest(runnable, relative),
            root.to_owned(),
            Source::File,
            Some("python"),
        ),
    };
    Ok(Task {
        label: tr!("workbench-lsp-tests-task-label", name = &runnable.name).into(),
        command: TaskCommand::Argv(argv),
        cwd,
        env: BTreeMap::new(),
        group: Some(Group::Test),
        default: false,
        source,
        problem_matcher: matcher.map(str::to_owned),
    })
}

/// `path`'s words joined with `/`, as a command line names a file whatever the
/// platform.
fn slashed(path: &Path) -> String {
    let words = path.components().filter_map(|component| match component {
        Component::Normal(word) => Some(word.to_string_lossy()),
        _ => None,
    });
    words.collect::<Vec<_>>().join("/")
}

/// The nearest directory above `path`, up to `root`, with a file `has`
/// accepts, and what it made of that file.
fn nearest<T>(
    files: &dyn ProjectFiles,
    path: &Path,
    root: &Path,
    name: &str,
    has: impl Fn(&str) -> Option<T>,
) -> Option<(PathBuf, T)> {
    let mut dir = path.parent()?;
    loop {
        let file = dir.join(name);
        if files.exists(&file)
            && let Some(found) = files.read_to_string(&file).ok().and_then(|text| has(&text))
        {
            return Some((dir.to_owned(), found));
        }
        if dir == root {
            return None;
        }
        dir = dir.parent().filter(|dir| dir.starts_with(root))?;
    }
}

/// A `Cargo.toml`'s package, if it has one: a workspace's has none.
struct Package {
    name: String,
    /// The targets its manifest declares, in its order: their kind, and
    /// their name and path if given.
    declared: Vec<(TargetKind, Option<String>, Option<String>)>,
    /// The kinds of target Cargo doesn't find by their place:
    /// `autolib = false`, `autobins = false` and so on.
    undiscovered: Vec<TargetKind>,
}

impl Package {
    /// Whether Cargo finds targets of `kind` by their place.
    fn discovers(&self, kind: TargetKind) -> bool {
        !self.undiscovered.contains(&kind)
    }
}

fn package(manifest: &str) -> Option<Package> {
    let manifest: toml::Table = manifest.parse().ok()?;
    let name = manifest.get("package")?.get("name")?.as_str()?.to_owned();
    let field = |entry: &toml::Value, key| entry.get(key)?.as_str().map(str::to_owned);
    let declare = |kind, entry: &toml::Value| (kind, field(entry, "name"), field(entry, "path"));
    let mut declared: Vec<_> = manifest
        .get("lib")
        .map(|lib| declare(TargetKind::Lib, lib))
        .into_iter()
        .collect();
    for (key, kind) in [
        ("bin", TargetKind::Bin),
        ("test", TargetKind::Test),
        ("bench", TargetKind::Bench),
        ("example", TargetKind::Example),
    ] {
        let entries = manifest.get(key).and_then(toml::Value::as_array);
        declared.extend(
            entries
                .into_iter()
                .flatten()
                .map(|entry| declare(kind, entry)),
        );
    }
    let switches = [
        ("autolib", TargetKind::Lib),
        ("autobins", TargetKind::Bin),
        ("autotests", TargetKind::Test),
        ("autobenches", TargetKind::Bench),
        ("autoexamples", TargetKind::Example),
    ];
    let package_table = manifest.get("package")?;
    let undiscovered = (switches.into_iter())
        .filter(|(key, _)| package_table.get(key).and_then(toml::Value::as_bool) == Some(false))
        .map(|(_, kind)| kind)
        .collect();
    Some(Package {
        name,
        declared,
        undiscovered,
    })
}

/// Whether a `Cargo.toml` is a workspace's.
fn is_workspace(manifest: &str) -> bool {
    (manifest.parse::<toml::Table>()).is_ok_and(|manifest| manifest.contains_key("workspace"))
}

/// What kind of crate a Cargo target is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TargetKind {
    Lib,
    Bin,
    Test,
    Bench,
    Example,
}

impl TargetKind {
    /// The flag of `cargo test` that picks a target of the kind.
    fn flag(self) -> &'static str {
        match self {
            Self::Lib => "--lib",
            Self::Bin => "--bin",
            Self::Test => "--test",
            Self::Bench => "--bench",
            Self::Example => "--example",
        }
    }

    /// Where Cargo looks for targets of the kind by their name.
    fn dir(self) -> &'static str {
        match self {
            Self::Lib => "src",
            Self::Bin => "src/bin",
            Self::Test => "tests",
            Self::Bench => "benches",
            Self::Example => "examples",
        }
    }
}

/// A crate of a package's.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Target {
    kind: TargetKind,
    name: String,
    /// Its root file, from the package's directory.
    root: PathBuf,
}

/// The root of `package`'s target of `kind` named `name` that names no path,
/// as Cargo finds it: the first of its places that's there, else the first.
fn inferred_root(
    files: &dyn ProjectFiles,
    dir: &Path,
    kind: TargetKind,
    name: &str,
    package: &str,
) -> PathBuf {
    let mut places = Vec::new();
    match kind {
        TargetKind::Lib => places.push(PathBuf::from("src/lib.rs")),
        _ => {
            if kind == TargetKind::Bin && name == package {
                places.push(PathBuf::from("src/main.rs"));
            }
            let at = Path::new(kind.dir());
            places.push(at.join(format!("{name}.rs")));
            places.push(at.join(name).join("main.rs"));
        }
    }
    let found = places.iter().find(|place| files.exists(&dir.join(place)));
    found.unwrap_or(&places[0]).clone()
}

/// The target Cargo would find by `file`'s place, if it's a root's: the
/// library at `src/lib.rs`, the package's binary at `src/main.rs`, and the
/// `x` binary, test, bench or example at `src/bin/x.rs`, `tests/x.rs` and so
/// on, or with a `main.rs` in `src/bin/x/`, `tests/x/`… for a file in there.
fn placed_target(file: &Path, package: &str) -> Option<Target> {
    let words = words(file)?;
    let target = |kind, name: &str, root: PathBuf| Target {
        kind,
        name: name.to_owned(),
        root,
    };
    match words
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>()
        .as_slice()
    {
        ["src", "lib.rs"] => Some(target(TargetKind::Lib, package, file.to_owned())),
        ["src", "main.rs"] => Some(target(TargetKind::Bin, package, file.to_owned())),
        [first, rest @ ..] => {
            let (kind, rest) = match (*first, rest) {
                ("src", ["bin", rest @ ..]) => (TargetKind::Bin, rest),
                ("tests", rest) => (TargetKind::Test, rest),
                ("benches", rest) => (TargetKind::Bench, rest),
                ("examples", rest) => (TargetKind::Example, rest),
                _ => return None,
            };
            let at = Path::new(kind.dir());
            match rest {
                [file] => {
                    let name = file.strip_suffix(".rs")?;
                    Some(target(kind, name, at.join(file)))
                }
                [name, ..] => Some(target(kind, name, at.join(name).join("main.rs"))),
                [] => None,
            }
        }
        [] => None,
    }
}

/// `path`'s words, if they're all plain names.
fn words(path: &Path) -> Option<Vec<String>> {
    (path.components())
        .map(|component| match component {
            Component::Normal(word) => Some(word.to_string_lossy().into_owned()),
            _ => None,
        })
        .collect()
}

/// The package in `dir`'s targets that could hold `file`, from there: those
/// its manifest declares, then those Cargo finds by their place that it
/// doesn't, a declared one's root or library taking the place's, and none of
/// a kind the manifest turns discovery off for (`autotests = false`).
fn targets(files: &dyn ProjectFiles, dir: &Path, package: &Package, file: &Path) -> Vec<Target> {
    let mut targets: Vec<Target> = (package.declared.iter())
        .map(|(kind, name, path)| {
            let name = name.clone().unwrap_or_else(|| package.name.clone());
            let root = match path {
                Some(path) => normalized(Path::new(path)),
                None => inferred_root(files, dir, *kind, &name, &package.name),
            };
            Target {
                kind: *kind,
                name,
                root,
            }
        })
        .collect();
    let lib = Path::new("src/lib.rs");
    let main = Path::new("src/main.rs");
    let placed = [lib, main, file]
        .into_iter()
        .filter(|place| *place == file || files.exists(&dir.join(place)))
        .filter_map(|place| placed_target(place, &package.name))
        .filter(|target| package.discovers(target.kind));
    for target in placed.collect::<Vec<_>>() {
        let claimed = targets.iter().any(|declared| {
            declared.root == target.root
                || (declared.kind == TargetKind::Lib && target.kind == TargetKind::Lib)
        });
        if !claimed && !targets.contains(&target) {
            targets.push(target);
        }
    }
    targets
}

/// `path` without its `.` words.
fn normalized(path: &Path) -> PathBuf {
    path.components()
        .filter(|component| *component != Component::CurDir)
        .collect()
}

/// Where `file`, from its package's directory, is in its crate.
struct Place<'a> {
    /// The target it's in, if just the one of them can hold it: a module in
    /// `src` can be the library's or the binary's.
    target: Option<&'a Target>,
    /// Its module path in the target.
    modules: Vec<String>,
    /// Whether it's the target's root.
    root: bool,
}

/// Where `file` is among `targets`: the one it's the root of, else the
/// module path from the nearest root's directory. A file in `src` that no
/// target holds is a module there.
fn place<'a>(file: &Path, targets: &'a [Target]) -> Option<Place<'a>> {
    if let Some(target) = targets.iter().find(|target| target.root == file) {
        return Some(Place {
            target: Some(target),
            modules: Vec::new(),
            root: true,
        });
    }
    let holding: Vec<(&Target, &Path)> = (targets.iter())
        .filter_map(|target| Some((target, target.root.parent()?)))
        .filter(|(_, dir)| file.starts_with(dir))
        .collect();
    let depth = |dir: &Path| dir.components().count();
    let deepest = holding.iter().map(|(_, dir)| depth(dir)).max();
    let nearest: Vec<_> = (holding.iter())
        .filter(|(_, dir)| Some(depth(dir)) == deepest)
        .collect();
    let dir = match nearest.first() {
        Some((_, dir)) => *dir,
        None if file.starts_with("src") => Path::new("src"),
        None => return None,
    };
    let mut modules = words(file.strip_prefix(dir).ok()?)?;
    let last = modules.pop()?;
    modules.push(last.strip_suffix(".rs")?.to_owned());
    Some(Place {
        target: match nearest.as_slice() {
            [(target, _)] => Some(*target),
            _ => None,
        },
        modules: module_path(modules),
        root: false,
    })
}

/// The modules of a file's `words`, a `mod.rs` being its directory's.
fn module_path(mut words: Vec<String>) -> Vec<String> {
    if words.last().is_some_and(|word| word == "mod") {
        words.pop();
    }
    words
}

/// `cargo test` of `runnable` in the file at `path`, `relative` to `root`, and
/// where it runs: the workspace's directory, as the paths in its output are
/// from there, else the package's.
fn cargo_test(
    files: &dyn ProjectFiles,
    runnable: &Runnable,
    path: &Path,
    root: &Path,
    relative: &Path,
) -> Result<(PathBuf, Vec<String>), String> {
    let shown = slashed(relative);
    let (dir, package) = nearest(files, path, root, CARGO_TOML, package)
        .ok_or_else(|| tr!("workbench-lsp-tests-no-cargo-package", path = &shown).to_string())?;
    let within = path.strip_prefix(&dir).unwrap_or(relative);
    let targets = targets(files, &dir, &package, within);
    let place = place(within, &targets)
        .ok_or_else(|| tr!("workbench-lsp-tests-which-crate", path = &shown).to_string())?;
    let mut argv: Vec<String> = ["cargo", "test", "-p", &package.name]
        .map(str::to_owned)
        .into();
    match place.target {
        // A library's tests, or a module's, are named well enough by their path.
        Some(target) if target.kind == TargetKind::Lib => {
            if runnable.kind == RunnableKind::File && place.root {
                argv.push(target.kind.flag().to_owned());
            }
        }
        Some(target) => argv.extend([target.kind.flag().to_owned(), target.name.clone()]),
        None => {}
    }
    let mut modules = place.modules;
    if runnable.kind != RunnableKind::File {
        modules.push(runnable.selector.clone());
    }
    let filter = modules.join("::");
    let exact = runnable.kind == RunnableKind::Test && runnable.exact;
    match runnable.kind {
        RunnableKind::Test => argv.push(filter),
        // What's under it, and not `tests_more` beside it.
        _ if !filter.is_empty() => argv.push(format!("{filter}::")),
        _ => {}
    }
    argv.push("--".to_owned());
    if exact {
        argv.push("--exact".to_owned());
    }
    argv.push("--nocapture".to_owned());
    let manifest = dir.join(CARGO_TOML);
    let workspace = nearest(files, &manifest, root, CARGO_TOML, |manifest| {
        is_workspace(manifest).then_some(())
    });
    Ok((workspace.map_or(dir, |(workspace, ())| workspace), argv))
}

/// `go test` of `runnable` in the file at `path`, run in its package's
/// directory, as a failing test's lines name its files from there.
fn go_test(
    files: &dyn ProjectFiles,
    runnable: &Runnable,
    path: &Path,
    root: &Path,
    relative: &Path,
) -> Result<(PathBuf, Vec<String>), String> {
    nearest(files, path, root, "go.mod", |_| Some(())).ok_or_else(|| {
        tr!("workbench-lsp-tests-no-go-mod", path = slashed(relative)).to_string()
    })?;
    let mut argv: Vec<String> = vec!["go".to_owned(), "test".to_owned()];
    if runnable.kind != RunnableKind::File {
        argv.extend(["-run".to_owned(), format!("^{}$", runnable.selector)]);
    }
    argv.push(".".to_owned());
    let package = path.parent().unwrap_or(root);
    Ok((package.to_owned(), argv))
}

/// pytest of `runnable` in the file `relative` to the root, where it runs,
/// its failures as Python's own tracebacks, which the matcher reads.
fn pytest(runnable: &Runnable, relative: &Path) -> Vec<String> {
    let file = slashed(relative);
    let target = match runnable.kind {
        RunnableKind::File => file,
        _ => format!("{file}::{}", runnable.selector),
    };
    ["python", "-m", "pytest", "--tb=native", &target]
        .map(str::to_owned)
        .into()
}
