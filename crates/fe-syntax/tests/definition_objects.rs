//! The functions and classes around an offset, innermost first, with their
//! nodes and bodies, for Vim's `af`/`if` and `ac`/`ic`.

#![cfg(all(
    feature = "rust",
    feature = "python",
    feature = "typescript",
    feature = "go",
    feature = "lua",
    feature = "html"
))]

use std::{fmt::Write as _, fs, path::Path, time::Duration};

use fe_buffer::Buffer;
use fe_syntax::{
    DefinitionKind::{self, Class, Function},
    OutlineKind, Syntax, definition_objects, definition_objects_counted, language_by_name,
    language_for_path, outline,
};

/// `text` parsed whole as `language`.
fn parsed(language: &str, text: &str) -> Syntax {
    let buffer = Buffer::new(text);
    let mut syntax = Syntax::new(language_by_name(language).unwrap(), buffer.current());
    // No deadline: a loaded machine can't make the parse give up.
    assert!(syntax.parse_until(Duration::MAX, || false));
    syntax
}

type Object<'a> = (DefinitionKind, &'a str, Option<&'a str>);

/// The objects around the first `at` in `text`, as the texts of their outer
/// and inner ranges.
fn objects_at<'a>(syntax: &Syntax, text: &'a str, at: &str) -> Vec<Object<'a>> {
    let offset = text
        .find(at)
        .unwrap_or_else(|| panic!("{at:?} not in the text"));
    objects(syntax, text, offset)
}

fn objects<'a>(syntax: &Syntax, text: &'a str, offset: usize) -> Vec<Object<'a>> {
    (definition_objects(syntax, offset).into_iter())
        .map(|object| {
            let inner = object.inner.map(|inner| &text[inner]);
            (object.kind, &text[object.outer], inner)
        })
        .collect()
}

const RUST: &str = "\
fn top(a: u32) -> u32 {
    let b = a + 1;

    fn nested() {
        helper();
    }

    b
}

impl S {
    fn method(&self) {}

    fn one_line(&self) { self.x() }
}

trait T {
    fn sig(&self) -> u32;
}
";

#[test]
fn definition_objects_rust_python_ts() {
    let syntax = parsed("rust", RUST);
    let top = &RUST[..RUST.find("\n\nimpl").unwrap()];
    let top_body = "let b = a + 1;\n\n    fn nested() {\n        helper();\n    }\n\n    b";
    let nested = "fn nested() {\n        helper();\n    }";
    // On the name, in the body, and in a nested fn.
    assert_eq!(
        objects_at(&syntax, RUST, "top"),
        [(Function, top, Some(top_body))]
    );
    assert_eq!(
        objects_at(&syntax, RUST, "+ 1"),
        [(Function, top, Some(top_body))]
    );
    assert_eq!(
        objects_at(&syntax, RUST, "helper"),
        [
            (Function, nested, Some("helper();")),
            (Function, top, Some(top_body)),
        ]
    );
    // An impl's methods: an empty body is empty after its `{`.
    let impl_ = &RUST[RUST.find("impl").unwrap()..RUST.find("\n\ntrait").unwrap()];
    let impl_body = "fn method(&self) {}\n\n    fn one_line(&self) { self.x() }";
    let found = definition_objects(&syntax, RUST.find("method").unwrap());
    let open = RUST.find("{}").unwrap() + 1;
    assert_eq!(found[0].inner, Some(open..open));
    assert_eq!(
        objects_at(&syntax, RUST, "method"),
        [
            (Function, "fn method(&self) {}", Some("")),
            (Class, impl_, Some(impl_body)),
        ]
    );
    assert_eq!(
        objects_at(&syntax, RUST, "x()"),
        [
            (
                Function,
                "fn one_line(&self) { self.x() }",
                Some("self.x()")
            ),
            (Class, impl_, Some(impl_body)),
        ]
    );
    // A raw string body keeps its delimiters, which are hidden tokens.
    let raw = "fn f() -> &'static str { r#\"hello\"# }\nfn g() -> &'static str {\n    r#\"\"#\n}\n";
    let syntax_raw = parsed("rust", raw);
    let inner = |at: &str| objects_at(&syntax_raw, raw, at)[0].2;
    assert_eq!(inner("hello"), Some("r#\"hello\"#"));
    assert_eq!(inner("fn g"), Some("r#\"\"#"));

    // Between items, on the blank row and in the impl's.
    assert_eq!(objects_at(&syntax, RUST, "\n\nimpl"), []);
    let between = RUST.find("{}\n").unwrap() + 3;
    assert_eq!(
        objects(&syntax, RUST, between),
        [(Class, impl_, Some(impl_body))]
    );

    let python = "\
class C:
    x = 1

    def m(self):
        def inner():
            return 2
        return inner


def f():
    pass
";
    let syntax = parsed("python", python);
    let class = python.trim_end().split("\n\n\n").next().unwrap();
    let class_body = &class[class.find("x = 1").unwrap()..];
    let m = &class[class.find("def m").unwrap()..];
    let m_body = &m[m.find("def inner").unwrap()..];
    assert_eq!(
        objects_at(&syntax, python, "x = 1"),
        [(Class, class, Some(class_body))]
    );
    assert_eq!(
        objects_at(&syntax, python, "return 2"),
        [
            (
                Function,
                "def inner():\n            return 2",
                Some("return 2")
            ),
            (Function, m, Some(m_body)),
            (Class, class, Some(class_body)),
        ]
    );
    assert_eq!(
        objects_at(&syntax, python, "pass"),
        [(Function, "def f():\n    pass", Some("pass"))]
    );

    let ts = "\
class Point {
    norm(): number {
        return this.x;
    }
}
const add = (a: number, b: number) => {
    return a + b;
};
const twice = (n: number) => n * 2;
";
    let syntax = parsed("typescript", ts);
    let class = &ts[..ts.find("\nconst").unwrap()];
    let class_body = &class[class.find("norm").unwrap()..class.rfind("\n}").unwrap()];
    assert_eq!(
        objects_at(&syntax, ts, "this"),
        [
            (Function, class_body, Some("return this.x;")),
            (Class, class, Some(class_body)),
        ]
    );
    let add = &ts[ts.find("add").unwrap()..ts.find(";\nconst twice").unwrap()];
    assert_eq!(
        objects_at(&syntax, ts, "a + b"),
        [(Function, add, Some("return a + b;"))]
    );
    assert_eq!(
        objects_at(&syntax, ts, "add"),
        [(Function, add, Some("return a + b;"))]
    );
    assert_eq!(
        objects_at(&syntax, ts, "n * 2"),
        [(Function, "twice = (n: number) => n * 2", Some("n * 2"))]
    );
}

#[test]
fn go_and_lua() {
    let go = "\
package p

type S struct {
\tx int
}

func (s S) Get() int {
\treturn s.x
}
";
    let syntax = parsed("go", go);
    let get = &go[go.find("func").unwrap()..go.len() - 1];
    assert_eq!(
        objects_at(&syntax, go, "return"),
        [(Function, get, Some("return s.x"))]
    );
    let found = objects_at(&syntax, go, "x int");
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].0, Class);

    let lua = "\
local function f(a)
  return a
end

M.g = function()
  print(1)
end
";
    let syntax = parsed("lua", lua);
    let f = &lua[..lua.find("\n\nM.g").unwrap()];
    assert_eq!(
        objects_at(&syntax, lua, "return"),
        [(Function, f, Some("return a"))]
    );
    let g = &lua[lua.find("M.g").unwrap()..lua.len() - 1];
    assert_eq!(
        objects_at(&syntax, lua, "print"),
        [(Function, g, Some("print(1)"))]
    );
}

#[test]
fn definition_objects_bodyless_and_unsupported() {
    let syntax = parsed("rust", RUST);
    let trait_ = &RUST[RUST.find("trait").unwrap()..RUST.len() - 1];
    assert_eq!(
        objects_at(&syntax, RUST, "sig"),
        [
            (Function, "fn sig(&self) -> u32;", None),
            (Class, trait_, Some("fn sig(&self) -> u32;")),
        ]
    );
    // Outside any definition, and past the end.
    assert_eq!(objects(&syntax, RUST, RUST.len() - 1), []);
    assert_eq!(objects(&syntax, RUST, RUST.len()), []);
    assert_eq!(objects(&syntax, RUST, RUST.len() + 10), []);

    // A language without an outline query.
    let html = "<p>fn f() {}</p>\n";
    assert_eq!(objects(&parsed("html", html), html, 5), []);

    // No tree yet.
    let fresh = Syntax::new(
        language_by_name("rust").unwrap(),
        Buffer::new(RUST).current(),
    );
    assert_eq!(objects(&fresh, RUST, RUST.find("helper").unwrap()), []);
}

#[test]
fn definition_objects_is_bounded() {
    // Modules of 20 rows, each an impl with a method; an impl of 3,000
    // methods; a fn of 20,000 blank rows around one call; 5,000 one-row
    // fns.
    let mut text = String::from("pub mod outer {\n");
    for i in 0..1_000 {
        writeln!(text, "    pub mod m{i} {{\n        impl S{i} {{").unwrap();
        writeln!(
            text,
            "            fn f{i}(&self) {{\n                let a = {i};"
        )
        .unwrap();
        text.push_str("                if a > 0 {\n");
        text.push_str(&"                    g(a);\n".repeat(11));
        text.push_str("                }\n            }\n        }\n    }\n");
    }
    text.push_str("}\nimpl Big {\n");
    for i in 0..3_000 {
        writeln!(text, "    fn m{i}(&self) {{\n        h({i});\n    }}").unwrap();
    }
    text.push_str("}\nfn blank() {\n");
    text.push_str(&"\n".repeat(10_000));
    text.push_str("    x();\n");
    text.push_str(&"\n".repeat(10_000));
    text.push_str("}\n");
    for i in 0..5_000 {
        writeln!(text, "fn t{i}() {{ k({i}); }}").unwrap();
    }
    assert!(
        text.lines().count() >= 50_000,
        "{} rows",
        text.lines().count()
    );
    let syntax = parsed("rust", &text);

    let cases = [
        ("g(a)", "fn f500(", vec![Function, Class]),
        ("h(2999)", "fn m2999(", vec![Function, Class]),
        ("x();", "fn blank(", vec![Function]),
        ("k(4999)", "fn t4999(", vec![Function]),
    ];
    for (at, innermost, kinds) in cases {
        let start = text.find(innermost).unwrap();
        let offset = start + text[start..].find(at).unwrap();
        let (found, work) = definition_objects_counted(&syntax, offset);
        let found_kinds: Vec<_> = found.iter().map(|object| object.kind).collect();
        assert_eq!(found_kinds, kinds, "{at}");
        assert!(text[found[0].outer.clone()].starts_with(innermost), "{at}");
        // The ancestors, each query's operations, capped on Big's
        // methods at about 10,000, and the body's edge tokens.
        assert!(work <= 20_000, "{at}: {work}");
    }
    let blank = definition_objects(&syntax, text.find("x();").unwrap());
    assert_eq!(&text[blank[0].inner.clone().unwrap()], "x();");
    let (_, work) = definition_objects_counted(&syntax, text.find("x();").unwrap());
    assert!(
        work <= 2_000,
        "a blank body's edges cost its tokens, not its rows: {work}"
    );
}

/// At every offset of every outline sample, the objects are the whole
/// outline's functions and classes containing it, innermost first.
#[test]
fn samples_match_the_whole_outline() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests");
    let mut paths: Vec<_> = ["outline_samples", "more_languages_samples"]
        .into_iter()
        .flat_map(|dir| fs::read_dir(root.join(dir)).unwrap())
        .map(|entry| entry.unwrap().path())
        .filter(|path| {
            path.extension()
                .is_none_or(|ext| ext != "outline" && ext != "symbols")
        })
        .collect();
    paths.sort();
    let mut checked = 0;
    for path in paths {
        let Some(language) = language_for_path(&path) else {
            continue;
        };
        let text = fs::read_to_string(&path).unwrap();
        let syntax = parsed(language.name(), &text);
        let Ok(outline) = outline(&syntax) else {
            continue;
        };
        let mut mismatches = String::new();
        for offset in (0..=text.len()).filter(|&offset| text.is_char_boundary(offset)) {
            let mut expected = Vec::new();
            let mut ix = outline.item_at(offset);
            while let Some(i) = ix {
                let item = &outline.items[i];
                let kind = match item.kind {
                    OutlineKind::Function | OutlineKind::Method => Some(Function),
                    OutlineKind::Class
                    | OutlineKind::Struct
                    | OutlineKind::Enum
                    | OutlineKind::Interface
                    | OutlineKind::Trait
                    | OutlineKind::Impl
                    | OutlineKind::Type => Some(Class),
                    _ => None,
                };
                if let Some(kind) = kind
                    && item.range.contains(&offset)
                {
                    expected.push((kind, item.range.clone()));
                }
                ix = item.parent.map(|parent| parent as usize);
            }
            let found: Vec<_> = (definition_objects(&syntax, offset).into_iter())
                .map(|object| (object.kind, object.outer))
                .collect();
            if found != expected {
                writeln!(mismatches, "at {offset}: {found:?} != {expected:?}").unwrap();
            }
        }
        assert!(mismatches.is_empty(), "{}:\n{mismatches}", path.display());
        checked += 1;
    }
    assert!(checked >= 15, "{checked} samples checked");
}
