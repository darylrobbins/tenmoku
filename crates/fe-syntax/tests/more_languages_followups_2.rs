//! Outlines and definition symbols the review of the third round of C++,
//! Lua, Scala, Swift and Ruby queries found wrong: C++ using directives
//! in nested namespaces and comments in aliases and directives; Swift
//! tuple labels; Scala capture and named tuple patterns; parenthesized
//! Ruby chains; long Lua assignments.
//!
//! Built only with the languages `more_languages.rs` needs.

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

use fe_buffer::Buffer;
use fe_syntax::{Outline, OutlineKind, Syntax, language_by_name, outline, symbols};

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

/// `outline`, after checking that each item's ancestors are exactly the
/// earlier items whose ranges contain its own, as [`Outline::item_at`]
/// relies on, and that each name is in its item.
fn nested(outline: Outline) -> Outline {
    for (ix, item) in outline.items.iter().enumerate() {
        assert!(
            item.range.start <= item.name_range.start && item.name_range.end <= item.range.end,
            "{:?} is in its item",
            item.name
        );
        let mut ancestors = Vec::new();
        let mut parent = item.parent;
        while let Some(p) = parent {
            ancestors.push(p as usize);
            parent = outline.items[p as usize].parent;
        }
        ancestors.reverse();
        let containing: Vec<usize> = (0..ix)
            .filter(|&other| {
                let range = &outline.items[other].range;
                range.start <= item.range.start && item.range.end <= range.end
            })
            .collect();
        assert_eq!(ancestors, containing, "the ancestors of {:?}", item.name);
    }
    outline
}

/// An item's kind, name and parent's name.
type Listed = (OutlineKind, String, Option<String>);

fn listed(outline: &Outline) -> Vec<Listed> {
    (outline.items.iter())
        .map(|item| {
            let parent = item
                .parent
                .map(|ix| outline.items[ix as usize].name.clone());
            (item.kind, item.name.clone(), parent)
        })
        .collect()
}

/// Each symbol of `text` in `language`.
fn symbols_of(language: &str, text: &str) -> Vec<Listed> {
    listed(&nested(symbols(&parsed(language, text)).unwrap()))
}

/// Each outline item of `text` in `language`.
fn outline_of(language: &str, text: &str) -> Vec<Listed> {
    listed(&nested(outline(&parsed(language, text)).unwrap()))
}

fn item(kind: OutlineKind, name: &str, parent: Option<&str>) -> Listed {
    (kind, name.to_owned(), parent.map(str::to_owned))
}

/// Both listings of `text`, which must be alike.
fn both(language: &str, text: &str) -> Vec<Listed> {
    let symbols = symbols_of(language, text);
    assert_eq!(outline_of(language, text), symbols);
    symbols
}

/// A using directive's target is looked up from the enclosing namespaces
/// too, not in the directive's own namespace again: B::N is A::N.
#[test]
fn cpp_using_directives_in_nested_namespaces() {
    use OutlineKind::*;
    let text = "\
namespace A { namespace N { void f(); extern int x; } }
namespace B { using namespace A; }
void B::N::f() {}
int B::N::x = 0;
namespace C { namespace D { using namespace A; } }
void C::D::N::f() {}
int C::D::N::x = 1;
";
    let symbols = symbols_of("cpp", text);
    let names: Vec<_> = symbols
        .iter()
        .map(|(kind, name, _)| (*kind, &name[..]))
        .collect();
    assert_eq!(
        names,
        [
            (Module, "A"),
            (Module, "N"),
            (Variable, "x"),
            (Module, "B"),
            (Function, "f"),
            (Variable, "x"),
            (Module, "C"),
            (Module, "D"),
            (Function, "f"),
            (Variable, "x"),
        ]
    );
}

/// Comments in an alias or a directive aren't its target.
#[test]
fn cpp_comments_in_aliases_and_directives() {
    use OutlineKind::*;
    let text = "\
namespace A { namespace N { void f(); extern int x; } }
namespace M /* c */ = A;
void M::N::f() {}
int M::N::x = 0;
namespace B { using namespace A /* c */; }
void B::N::f() {}
int B::N::x = 1;
namespace D { using /* c */ namespace A; }
void D::N::f() {}
";
    let symbols = symbols_of("cpp", text);
    let names: Vec<_> = symbols
        .iter()
        .map(|(kind, name, _)| (*kind, &name[..]))
        .collect();
    assert_eq!(
        names,
        [
            (Module, "A"),
            (Module, "N"),
            (Variable, "x"),
            (Function, "f"),
            (Variable, "x"),
            (Module, "B"),
            (Function, "f"),
            (Variable, "x"),
            (Module, "D"),
            (Function, "f"),
        ]
    );
}

/// A tuple label isn't a name, at the top, in a type and in a function.
#[test]
fn swift_labeled_tuple_patterns() {
    use OutlineKind::*;
    let text = "\
let (a, (left: b, right: c)) = (1, (left: 2, right: 3))
struct S {
    let (d, (l: e, r: f)) = (1, (l: 2, r: 3))
}
func g() {
    let (h, (m: i, n: j)) = (1, (m: 2, n: 3))
}
";
    assert_eq!(
        symbols_of("swift", text),
        [
            item(Variable, "a", None),
            item(Variable, "b", None),
            item(Variable, "c", None),
            item(Struct, "S", None),
            item(Field, "d", Some("S")),
            item(Field, "e", Some("S")),
            item(Field, "f", Some("S")),
            item(Function, "g", None),
        ]
    );
}

/// A name before @ in a pattern and a name a named tuple pattern's label
/// is for are bound, the label is not, also with a tuple in it.
#[test]
fn scala_capture_and_named_tuple_patterns() {
    use OutlineKind::*;
    let text = "\
val (all @ x, y) = pair
val (name = n, age = a) = person
val (p = (q, r), s = t) = nested
package pkg { val (u = v) = w }
class C {
  val (k @ l, m) = pair
  val (label = o, other = (z, y2)) = t
}
def f() = {
  val (loc @ lx, ly) = pair
  val (lab = lo) = t
}
";
    let names: Vec<_> = symbols_of("scala", text)
        .into_iter()
        .map(|(kind, name, _)| (kind, name))
        .collect();
    let expected: Vec<_> = [
        (Variable, "all"),
        (Variable, "x"),
        (Variable, "y"),
        (Variable, "n"),
        (Variable, "a"),
        (Variable, "q"),
        (Variable, "r"),
        (Variable, "t"),
        (Module, "pkg"),
        (Variable, "v"),
        (Class, "C"),
        (Field, "k"),
        (Field, "l"),
        (Field, "m"),
        (Field, "o"),
        (Field, "z"),
        (Field, "y2"),
        (Function, "f"),
    ]
    .into_iter()
    .map(|(kind, name)| (kind, name.to_owned()))
    .collect();
    assert_eq!(names, expected);
}

/// Parentheses around an assignment in a chain change nothing.
#[test]
fn ruby_parenthesized_chains() {
    use OutlineKind::*;
    let text = "\
A = (B = 1)
C, D = (E = [1, 2])
class K
  F = (G = 1)
  H, I = (J = [1, 2])
end
module M
  N = (O = 1)
  P, Q = (R = [1, 2])
end
def m
  local = (S = 1)
end
foo((T = 1))
";
    let names: Vec<_> = both("ruby", text)
        .into_iter()
        .map(|(kind, name, _)| (kind, name))
        .collect();
    let expected: Vec<_> = [
        (Constant, "A"),
        (Constant, "B"),
        (Constant, "C"),
        (Constant, "D"),
        (Constant, "E"),
        (Class, "K"),
        (Constant, "F"),
        (Constant, "G"),
        (Constant, "H"),
        (Constant, "I"),
        (Constant, "J"),
        (Module, "M"),
        (Constant, "N"),
        (Constant, "O"),
        (Constant, "P"),
        (Constant, "Q"),
        (Constant, "R"),
        (Function, "m"),
    ]
    .into_iter()
    .map(|(kind, name)| (kind, name.to_owned()))
    .collect();
    assert_eq!(names, expected);
}

/// A long Lua assignment costs its length, not its square or more, with no
/// function among the values, with one and with a table for its one value.
#[test]
fn lua_long_assignments_scale() {
    let n = 20_000;
    let targets: Vec<String> = (0..n).map(|i| format!("a{i}")).collect();
    let values: Vec<String> = (0..n).map(|i| i.to_string()).collect();
    let plain = format!("{} = {}\n", targets.join(", "), values.join(", "));
    let mut mixed = values.clone();
    mixed[n - 1] = "function() end".to_owned();
    let mixed = format!("{} = {}\n", targets.join(", "), mixed.join(", "));
    let table = format!("{} = {{}}\n", targets.join(", "));
    for text in [plain, mixed, table] {
        let syntax = parsed("lua", &text);
        let start = std::time::Instant::now();
        let outline = outline(&syntax).unwrap();
        assert!(start.elapsed().as_secs() < 5, "{:?}", start.elapsed());
        assert!(outline.items.len() <= 1);
        let start = std::time::Instant::now();
        symbols(&syntax).unwrap();
        assert!(start.elapsed().as_secs() < 5, "{:?}", start.elapsed());
    }
}
