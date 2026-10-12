//! Outlines and definition symbols for C++, C#, Ruby, PHP, Swift, Kotlin,
//! Scala and Lua.
//!
//! Each sample in `tests/more_languages_samples/`, named after its
//! language, is outlined and listed whole and compared with
//! `<sample>.outline` and `<sample>.symbols` next to it, in the format of
//! `tests/outline.rs`: one line per item, indented two spaces per depth,
//! `<kind> "<name>" <range>` with the range as `row:col-row:col`, 1-based
//! rows and byte columns.
//! `UPDATE_GOLDEN=1 cargo nextest run -p fe-syntax --test more_languages`
//! rewrites them; review the diff before committing it.
//!
//! Built only with the eight languages, and those it checks have no
//! symbols.

#![cfg(all(
    feature = "cpp",
    feature = "csharp",
    feature = "ruby",
    feature = "php",
    feature = "swift",
    feature = "kotlin",
    feature = "scala",
    feature = "lua",
    feature = "markdown",
    feature = "toml",
    feature = "json",
    feature = "bash",
    feature = "yaml"
))]

use std::{
    fmt::Write as _,
    fs,
    path::{Path, PathBuf},
};

use fe_buffer::Buffer;
use fe_syntax::{
    NoOutline, Outline, OutlineItem, OutlineKind, Syntax, language_by_name, language_for_path,
    outline, outline_supported, symbols, symbols_job, symbols_supported,
};

const LANGUAGES: [&str; 8] = [
    "cpp", "csharp", "kotlin", "lua", "php", "ruby", "scala", "swift",
];

/// Read at run time: the shared cargo target can keep a compile-time path
/// to a snapshot that has since been deleted.
fn samples_dir() -> PathBuf {
    Path::new(&std::env::var("CARGO_MANIFEST_DIR").unwrap())
        .join("tests")
        .join("more_languages_samples")
}

/// Every sample, with its language name and text, by language name.
/// Goldens are skipped.
fn samples() -> Vec<(PathBuf, &'static str, String)> {
    let mut samples: Vec<_> = fs::read_dir(samples_dir())
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| {
            let name = path.to_string_lossy();
            !name.ends_with(".outline") && !name.ends_with(".symbols")
        })
        .map(|path| {
            let language = language_for_path(&path).unwrap().name();
            let text = fs::read_to_string(&path).unwrap();
            (path, language, text)
        })
        .collect();
    samples.sort_by_key(|&(_, language, _)| language);
    let languages: Vec<_> = samples.iter().map(|&(_, language, _)| language).collect();
    assert_eq!(languages, LANGUAGES, "one sample per language");
    samples
}

/// The sample of `language`.
fn sample(language: &str) -> String {
    let (_, _, text) = (samples().into_iter())
        .find(|&(_, name, _)| name == language)
        .unwrap();
    text
}

/// `text` parsed whole as `language`.
fn parsed(language: &str, text: &str) -> Syntax {
    let buffer = Buffer::new(text);
    let mut syntax = Syntax::new(language_by_name(language).unwrap(), buffer.current());
    syntax.finish(syntax.parse_job().run());
    assert!(
        !syntax.tree().unwrap().root_node().has_error(),
        "the {language} text parses without errors"
    );
    syntax
}

fn symbols_of(language: &str) -> Outline {
    symbols(&parsed(language, &sample(language))).unwrap()
}

fn render(outline: &Outline, text: &str) -> String {
    let buffer = Buffer::new(text);
    let point = |offset| {
        let point = buffer.text().offset_to_point(offset);
        format!("{}:{}", point.row + 1, point.column + 1)
    };
    let mut out = String::new();
    for item in &outline.items {
        let indent = "  ".repeat(item.depth as usize);
        let (start, end) = (point(item.range.start), point(item.range.end));
        writeln!(out, "{indent}{:?} {:?} {start}-{end}", item.kind, item.name).unwrap();
    }
    if outline.truncated {
        out.push_str("truncated\n");
    }
    out
}

/// Compares `outline` with the golden `<sample><suffix>`, or rewrites it
/// under `UPDATE_GOLDEN`. Returns the difference, if any.
fn compare(path: &Path, suffix: &str, outline: &Outline, text: &str) -> Option<String> {
    let file_name = path.file_name().unwrap().to_string_lossy();
    let golden = path.with_file_name(format!("{file_name}{suffix}"));
    let actual = render(outline, text);
    if std::env::var_os("UPDATE_GOLDEN").is_some() {
        fs::write(&golden, &actual).unwrap();
        return None;
    }
    let expected = fs::read_to_string(&golden).unwrap_or_default();
    (expected != actual)
        .then(|| format!("{file_name}{suffix}\n--- expected\n{expected}--- actual\n{actual}"))
}

/// The chain of an item's ancestors' kinds and names, innermost first.
fn ancestors(outline: &Outline, item: &OutlineItem) -> Vec<(OutlineKind, String)> {
    let mut chain = Vec::new();
    let mut parent = item.parent;
    while let Some(ix) = parent {
        let item = &outline.items[ix as usize];
        chain.push((item.kind, item.name.clone()));
        parent = item.parent;
    }
    chain
}

/// The one item of this kind and name.
fn find<'a>(outline: &'a Outline, kind: OutlineKind, name: &str) -> &'a OutlineItem {
    let found: Vec<_> = (outline.items.iter())
        .filter(|item| item.kind == kind && item.name == name)
        .collect();
    assert_eq!(found.len(), 1, "one {kind:?} {name:?} in {outline:#?}");
    found[0]
}

fn chain(links: &[(OutlineKind, &str)]) -> Vec<(OutlineKind, String)> {
    (links.iter())
        .map(|&(kind, name)| (kind, name.to_owned()))
        .collect()
}

fn is_type_or_module(kind: OutlineKind) -> bool {
    use OutlineKind::*;
    matches!(
        kind,
        Module | Class | Struct | Enum | Interface | Trait | Impl | Type
    )
}

#[test]
fn outlines_match_golden_files() {
    let mut failures = Vec::new();
    for (path, language, text) in samples() {
        let outline = outline(&parsed(language, &text)).unwrap();
        let nested_method = (outline.items.iter()).any(|item| {
            item.kind == OutlineKind::Method
                && (item.parent)
                    .is_some_and(|parent| is_type_or_module(outline.items[parent as usize].kind))
        });
        assert!(
            nested_method,
            "{language}: a method nested in a type or module"
        );
        failures.extend(compare(&path, ".outline", &outline, &text));
    }
    assert!(
        failures.is_empty(),
        "outlines differ from the golden files. If the change is intended, rerun with \
         UPDATE_GOLDEN=1 and review the diff.\n\n{}",
        failures.join("\n")
    );
}

#[test]
fn symbols_match_golden_files() {
    let mut failures = Vec::new();
    for (path, language, text) in samples() {
        let syntax = parsed(language, &text);
        let symbols = symbols(&syntax).unwrap();
        let background = symbols_job(&syntax).unwrap();
        let background = std::thread::spawn(move || background.run());
        assert_eq!(background.join().unwrap(), symbols, "{language}");
        let outline = outline(&syntax).unwrap();
        for item in &outline.items {
            let found = (symbols.items.iter()).any(|symbol| {
                symbol.kind == item.kind
                    && symbol.range == item.range
                    && symbol.name == item.name
                    && symbol.name_range == item.name_range
            });
            assert!(found, "{language}: {:?} {:?}", item.kind, item.name);
        }
        assert!(
            symbols.items.len() > outline.items.len(),
            "{language} has symbols the outline leaves out"
        );
        failures.extend(compare(&path, ".symbols", &symbols, &text));
    }
    assert!(
        failures.is_empty(),
        "symbols differ from the golden files. If the change is intended, rerun with \
         UPDATE_GOLDEN=1 and review the diff.\n\n{}",
        failures.join("\n")
    );
}

#[test]
fn members_and_variables() {
    use OutlineKind::*;
    let members = [
        ("csharp", Field, "Total", Class, "Order"),
        ("csharp", Variant, "Open", Enum, "Status"),
        ("kotlin", Field, "label", Class, "Circle"),
        ("kotlin", Variant, "NORTH", Enum, "Direction"),
        ("swift", Field, "radius", Class, "Circle"),
        ("swift", Variant, "north", Enum, "Direction"),
        ("php", Field, "name", Class, "Model"),
        ("php", Variant, "Active", Enum, "Status"),
        ("scala", Field, "name", Class, "Circle"),
    ];
    for (language, kind, name, parent_kind, parent) in members {
        let symbols = symbols_of(language);
        let nested = (symbols.items.iter()).any(|item| {
            item.kind == kind
                && item.name == name
                && ancestors(&symbols, item).first() == Some(&(parent_kind, parent.to_owned()))
        });
        assert!(
            nested,
            "{language}: {kind:?} {name:?} under {parent_kind:?} {parent:?}"
        );
    }
    // Swift's stored property: let y in struct Point.
    let swift = symbols_of("swift");
    let y = find(&swift, Field, "y");
    assert_eq!(ancestors(&swift, y), chain(&[(Struct, "Point")]));

    for (language, name) in [("lua", "x"), ("kotlin", "origin"), ("swift", "limit")] {
        let symbols = symbols_of(language);
        let item = find(&symbols, Variable, name);
        assert_eq!(item.parent, None, "{language}: {name} is at the top");
    }

    // Locals and parameters, which no sample lists. Record, case class and
    // val parameters declare properties, and aren't here.
    let hidden: [(&str, &[&str]); 8] = [
        (
            "cpp",
            &["local", "param", "argc", "argv", "result", "factor"],
        ),
        ("csharp", &["local", "index", "message", "a", "b"]),
        ("kotlin", &["local", "doubled", "value", "shape"]),
        (
            "lua",
            &[
                "self", "label", "doubled", "value", "name", "event", "amount",
            ],
        ),
        ("php", &["local", "temp", "param"]),
        ("ruby", &["local", "temp", "arg", "attrs", "amount"]),
        (
            "scala",
            &["squared", "doubled", "value", "radius", "shape", "degrees"],
        ),
        (
            "swift",
            &["dx", "doubled", "value", "other", "size", "index"],
        ),
    ];
    for (language, names) in hidden {
        let symbols = symbols_of(language);
        for item in &symbols.items {
            assert!(
                !names.contains(&item.name.as_str()),
                "{language}: {:?} {:?} is a local or parameter",
                item.kind,
                item.name
            );
        }
    }
}

#[test]
fn methods_nest_under_their_type() {
    use OutlineKind::*;
    let cpp = symbols_of("cpp");
    let area = find(&cpp, Method, "area");
    assert_eq!(
        ancestors(&cpp, area),
        chain(&[(Class, "Shape"), (Module, "shapes")])
    );

    let ruby = symbols_of("ruby");
    let initialize = find(&ruby, Method, "initialize");
    assert_eq!(
        ancestors(&ruby, initialize),
        chain(&[(Class, "Invoice"), (Module, "Billing")])
    );
    let name = find(&ruby, Field, "name");
    assert_eq!(
        ancestors(&ruby, name),
        chain(&[(Class, "Invoice"), (Module, "Billing")])
    );

    // Lua declares no types and nests lexically: function M:describe() is a
    // method at the top, after the table M, not under it. A method in a
    // table's constructor nests under the table's module.
    let lua = symbols_of("lua");
    let describe = find(&lua, Method, "describe");
    assert_eq!(ancestors(&lua, describe), []);
    let deposit = find(&lua, Method, "deposit");
    assert_eq!(ancestors(&lua, deposit), chain(&[(Module, "Account")]));
}

#[test]
fn supported_flags() {
    for language in LANGUAGES {
        assert!(outline_supported(language), "{language}");
        assert!(symbols_supported(language), "{language}");
    }
    for (language, text) in [
        ("markdown", "# A\n"),
        ("toml", "[a]\nb = 1\n"),
        ("json", "{\"a\": 1}\n"),
        ("yaml", "a: 1\n"),
    ] {
        let syntax = parsed(language, text);
        assert!(!symbols_supported(language), "{language}");
        assert_eq!(symbols(&syntax).err(), Some(NoOutline::Unsupported));
        assert_eq!(symbols_job(&syntax).err(), Some(NoOutline::Unsupported));
    }
}

/// Each item's kind, name and its parent's name, for `text` in `language`.
fn listed(language: &str, text: &str) -> Vec<(OutlineKind, String, Option<String>)> {
    let symbols = symbols(&parsed(language, text)).unwrap();
    (symbols.items.iter())
        .map(|item| {
            let parent = item
                .parent
                .map(|ix| symbols.items[ix as usize].name.clone());
            (item.kind, item.name.clone(), parent)
        })
        .collect()
}

fn item(
    kind: OutlineKind,
    name: &str,
    parent: Option<&str>,
) -> (OutlineKind, String, Option<String>) {
    (kind, name.to_owned(), parent.map(str::to_owned))
}

/// Bindings, members and names the first review found missing or wrong.
#[test]
fn bindings_and_qualified_names() {
    use OutlineKind::*;
    let cases: [(&str, &str, Vec<_>); 12] = [
        // Several targets are several variables, but one assigned a
        // function is a function.
        (
            "lua",
            "local a, b = 1, 2\nlocal c, d\ne, f = 3, 4\nlocal g, h = function() end, 1\n\
             local i = function() end\n",
            ["a", "b", "c", "d", "e", "f"]
                .map(|name| item(Variable, name, None))
                .into_iter()
                .chain([
                    item(Function, "g", None),
                    item(Variable, "h", None),
                    item(Function, "i", None),
                ])
                .collect(),
        ),
        // A default value's identifiers don't name the property.
        (
            "kotlin",
            "package p.q\n\nclass C(val x: Int = seed, y: Int = seed)\n",
            vec![
                item(Module, "p.q", None),
                item(Class, "C", None),
                item(Field, "x", Some("C")),
            ],
        ),
        // Member functions through pointers, references and scopes; a
        // pointer to a function is a field.
        (
            "cpp",
            "namespace A { namespace B {\nstruct X {\n    X **clone();\n    int &ref();\n    \
             int (*fp)(int);\n    void f();\n};\n} }\n\
             A::B::X **A::B::X::clone() { return 0; }\nint &A::B::X::ref() { static int r; return r; }\n\
             void A::B::X::f() {}\n",
            vec![
                item(Module, "A", None),
                item(Module, "B", Some("A")),
                item(Struct, "X", Some("B")),
                item(Method, "clone", Some("X")),
                item(Method, "ref", Some("X")),
                item(Field, "fp", Some("X")),
                item(Method, "f", Some("X")),
                item(Method, "clone", None),
                item(Method, "ref", None),
                item(Method, "f", None),
            ],
        ),
        // A function qualified by a namespace the file defines is free.
        (
            "cpp",
            "namespace N { int f(); }\nint N::f() { return 1; }\n\
             namespace M::K { int g(); }\nint M::K::g() { return 2; }\n",
            vec![
                item(Module, "N", None),
                item(Function, "f", None),
                item(Module, "M::K", None),
                item(Function, "g", None),
            ],
        ),
        // Conversion operators keep their whole target type; a variable
        // template is a variable, a static member's definition a field.
        (
            "cpp",
            "struct X {\n    operator char*();\n    operator char();\n    static int count;\n};\n\
             X::operator char*() { return 0; }\nint X::count = 0;\n\
             template <class T> T value = {};\nnamespace N { extern int x; }\nint N::x = 0;\n\
             struct Z {\n    template <class T> static constexpr T zero = T();\n    \
             template <class T> void tf(T);\n};\n",
            vec![
                item(Struct, "X", None),
                item(Method, "operator char*", Some("X")),
                item(Method, "operator char", Some("X")),
                item(Field, "count", Some("X")),
                item(Method, "operator char*", None),
                item(Field, "count", None),
                item(Variable, "value", None),
                item(Module, "N", None),
                item(Variable, "x", Some("N")),
                item(Variable, "x", None),
                item(Struct, "Z", None),
                item(Field, "zero", Some("Z")),
                item(Method, "tf", Some("Z")),
            ],
        ),
        // Only a case class's first parameter list declares fields.
        (
            "scala",
            "case class C(x: Int)(ctx: String)\ncase class D[T] private (y: T)\n",
            vec![
                item(Class, "C", None),
                item(Field, "x", Some("C")),
                item(Class, "D", None),
                item(Field, "y", Some("D")),
            ],
        ),
        // Each name of a compound binding, and named givens.
        (
            "scala",
            "val (a, b) = pair\nval c, d = 0\ntrait T { val e, f: Int }\n\
             given ord: Ordering[Int] = Ordering.Int\ngiven Ordering[Long] = Ordering.Long\n\
             object O { given inner: Int = 1 }\ndef g = { given local: Int = 1; val v = 2; v }\n",
            vec![
                item(Variable, "a", None),
                item(Variable, "b", None),
                item(Variable, "c", None),
                item(Variable, "d", None),
                item(Trait, "T", None),
                item(Field, "e", Some("T")),
                item(Field, "f", Some("T")),
                item(Variable, "ord", None),
                item(Module, "O", None),
                item(Field, "inner", Some("O")),
                item(Function, "g", None),
            ],
        ),
        (
            "swift",
            "let (a, b) = pair\nstruct S { var (c, d) = (1, 2) }\n\
             @freestanding(expression)\nmacro m(_ v: Int) -> Int = #externalMacro(module: \"M\", type: \"T\")\n",
            vec![
                item(Variable, "a", None),
                item(Variable, "b", None),
                item(Struct, "S", None),
                item(Field, "c", Some("S")),
                item(Field, "d", Some("S")),
                item(Macro, "m", None),
            ],
        ),
        (
            "php",
            "<?php\nlist($a, $b) = $pair;\n[$c, $d] = $pair;\n\
             class K { public function __construct(private int $id, int $plain) {} }\n",
            vec![
                item(Variable, "a", None),
                item(Variable, "b", None),
                item(Variable, "c", None),
                item(Variable, "d", None),
                item(Class, "K", None),
                item(Method, "__construct", Some("K")),
                item(Field, "id", Some("__construct")),
            ],
        ),
        // Constants assigned through a scope, named by the last constant.
        (
            "ruby",
            "Config::VERSION = \"1.0\"\n::TOP = 1\nA::B, C = 1, 2\n",
            vec![
                item(Constant, "VERSION", None),
                item(Constant, "TOP", None),
                item(Constant, "B", None),
                item(Constant, "C", None),
            ],
        ),
        // Function-local bindings aren't listed.
        (
            "swift",
            "func f() { let (x, y) = (1, 2); _ = x + y }\n",
            vec![item(Function, "f", None)],
        ),
        (
            "php",
            "<?php\nfunction f($p) { list($x, $y) = $p; }\n",
            vec![item(Function, "f", None)],
        ),
    ];
    for (language, text, expected) in cases {
        assert_eq!(listed(language, text), expected, "{language}: {text}");
    }
}
