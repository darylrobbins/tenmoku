//! Outlines and definition symbols the second review of the C++, Lua,
//! Scala, Swift, PHP and Ruby queries found wrong: C++ qualifiers naming
//! namespaces, aliases or types in other scopes, reference conversions and
//! member templates of class templates; Lua functions among several
//! targets; nested destructuring; annotated Scala constructors; chained
//! Ruby constants.
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

/// A qualifier is looked up from where the definition is, through
/// namespace aliases, not by its last name alone: A::N is a namespace even
/// though B::N is a type.
#[test]
fn cpp_qualifiers_resolve_by_scope_and_alias() {
    use OutlineKind::*;
    let text = "\
namespace A { namespace N { void f(); extern int x; } }
namespace B { struct N { void f(); static int x; }; }
void A::N::f() {}
int A::N::x = 0;
void B::N::f() {}
int B::N::x = 1;
void ::A::N::g() {}
namespace A { void N::h() {} int N::y = 2; }
namespace M = A::N;
void M::k() {}
int M::z = 3;
namespace D { namespace L = ::A::N; }
void D::L::j() {}
int D::L::w = 4;
namespace C { struct A { void f(); static int v; }; void A::f() {} int A::v = 5; }
namespace E { inline namespace v1 { struct S { void f(); static int u; }; } }
void E::S::f() {}
int E::S::u = 6;
";
    assert_eq!(
        symbols_of("cpp", text),
        [
            item(Module, "A", None),
            item(Module, "N", Some("A")),
            item(Variable, "x", Some("N")),
            item(Module, "B", None),
            item(Struct, "N", Some("B")),
            item(Method, "f", Some("N")),
            item(Field, "x", Some("N")),
            // A::N is a namespace and B::N a type.
            item(Function, "f", None),
            item(Variable, "x", None),
            item(Method, "f", None),
            item(Field, "x", None),
            item(Function, "g", None),
            // N, from inside A, is A::N.
            item(Module, "A", None),
            item(Function, "h", Some("A")),
            item(Variable, "y", Some("A")),
            // Through an alias, also one declared in a namespace.
            item(Function, "k", None),
            item(Variable, "z", None),
            item(Module, "D", None),
            item(Function, "j", None),
            item(Variable, "w", None),
            // A, from inside C, is C::A, a type.
            item(Module, "C", None),
            item(Struct, "A", Some("C")),
            item(Method, "f", Some("A")),
            item(Field, "v", Some("A")),
            item(Method, "f", Some("C")),
            item(Field, "v", Some("C")),
            // A type in an inline namespace.
            item(Module, "E", None),
            item(Module, "v1", Some("E")),
            item(Struct, "S", Some("v1")),
            item(Method, "f", Some("S")),
            item(Field, "u", Some("S")),
            item(Method, "f", None),
            item(Field, "u", None),
        ]
    );

    // C++20's nested inline namespace, an alias of a namespace declared
    // elsewhere and a using directive.
    let text = "\
namespace P::inline Q { namespace R {} }
void P::R::f() {}
namespace fs = std::filesystem;
void fs::g() {}
int fs::x = 0;
namespace outer { namespace S {} }
using namespace outer;
void S::h() {}
int S::y = 1;
";
    let top: Vec<_> = (symbols_of("cpp", text).into_iter())
        .filter(|(kind, _, _)| !matches!(kind, Module))
        .collect();
    assert_eq!(
        top,
        [
            item(Function, "f", None),
            item(Function, "g", None),
            item(Variable, "x", None),
            item(Function, "h", None),
            item(Variable, "y", None),
        ]
    );
}

/// A conversion to a reference is named by its target type, the same in
/// its declaration and its definition whatever follows the parameters.
#[test]
fn cpp_reference_conversions_are_named_by_their_target() {
    use OutlineKind::*;
    let text = "\
struct W {
    operator int&() const;
    operator int&&() &&;
    operator const char*&() noexcept;
};
W::operator int&() const { static int i; return i; }
W::operator int&&() && { static int j; return static_cast<int&&>(j); }
W::operator const char*&() noexcept { static const char *s; return s; }
";
    assert_eq!(
        outline_of("cpp", text),
        [
            item(Struct, "W", None),
            item(Method, "operator int&", Some("W")),
            item(Method, "operator int&&", Some("W")),
            item(Method, "operator const char*&", Some("W")),
            item(Method, "operator int&", None),
            item(Method, "operator int&&", None),
            item(Method, "operator const char*&", None),
        ]
    );
}

/// A member template of a class template defined outside it, under two
/// template headers, is still a member.
#[test]
fn cpp_member_templates_of_class_templates() {
    use OutlineKind::*;
    let text = "\
template<class T> struct A {
    template<class U> static U x;
    template<class U> void g(U);
};
template<class T> template<class U> U A<T>::x = U();
template<class T> template<class U> void A<T>::g(U) {}
namespace N {
template<class T> struct B { template<class U> static U y; };
template<class T> template<class U> U B<T>::y = U();
template<class T> T pi = T(3);
}
void f() { int local = 0; }
";
    assert_eq!(
        symbols_of("cpp", text),
        [
            item(Struct, "A", None),
            item(Field, "x", Some("A")),
            item(Method, "g", Some("A")),
            item(Field, "x", None),
            item(Method, "g", None),
            item(Module, "N", None),
            item(Struct, "B", Some("N")),
            item(Field, "y", Some("B")),
            item(Field, "y", Some("N")),
            item(Variable, "pi", Some("N")),
            item(Function, "f", None),
        ]
    );
}

/// Each target assigned a function is a function, among several targets
/// and with more values than targets, and its siblings stay variables.
#[test]
fn lua_targets_assigned_functions() {
    use OutlineKind::*;
    let text = "\
local M = {}
local g, h = function() end, 1
k, M.f = 2, function() end
local p = function() end, 3
local q, r = 4
s, t = function() end
local u, v = 5, (function() end)
";
    assert_eq!(
        outline_of("lua", text),
        [
            item(Module, "M", None),
            item(Function, "g", None),
            item(Function, "f", None),
            item(Function, "p", None),
            item(Function, "s", None),
            item(Function, "v", None),
        ]
    );
    assert_eq!(
        symbols_of("lua", text),
        [
            item(Module, "M", None),
            item(Function, "g", None),
            item(Variable, "h", None),
            item(Variable, "k", None),
            item(Function, "f", None),
            item(Function, "p", None),
            item(Variable, "q", None),
            item(Variable, "r", None),
            item(Function, "s", None),
            item(Variable, "t", None),
            item(Variable, "u", None),
            item(Function, "v", None),
        ]
    );
}

/// Every name a destructuring pattern binds, at any depth, at the top of
/// the file or in a type, but not in a function, nor a key or a constant
/// matched against.
#[test]
fn nested_destructuring_binds_every_name() {
    use OutlineKind::*;
    let scala = "\
val (a, (b, c)) = pair
val (A, i) = pair
val all @ (j, _) = pair
object O {
  val (d, (e, f: Int)) = pair
  var Some((g, h)) = opt
  def m = { val (l1, (l2, l3)) = p; 1 }
}
";
    assert_eq!(
        symbols_of("scala", scala),
        [
            item(Variable, "a", None),
            item(Variable, "b", None),
            item(Variable, "c", None),
            item(Variable, "i", None),
            item(Variable, "all", None),
            item(Variable, "j", None),
            item(Module, "O", None),
            item(Field, "d", Some("O")),
            item(Field, "e", Some("O")),
            item(Field, "f", Some("O")),
            item(Field, "g", Some("O")),
            item(Field, "h", Some("O")),
            item(Method, "m", Some("O")),
        ]
    );
    assert_eq!(
        symbols_of("scala", "package p {\n  val (a, (b, c)) = pair\n}\n"),
        [
            item(Module, "p", None),
            item(Variable, "a", Some("p")),
            item(Variable, "b", Some("p")),
            item(Variable, "c", Some("p")),
        ]
    );

    let swift = "\
let (a, (b, c)) = pair
struct S {
    var (d, (e, _)) = pair
    func m() { let (l1, (l2, l3)) = p }
}
enum E {
    case one
    static let (x, (y, z)) = pair
}
";
    assert_eq!(
        symbols_of("swift", swift),
        [
            item(Variable, "a", None),
            item(Variable, "b", None),
            item(Variable, "c", None),
            item(Struct, "S", None),
            item(Field, "d", Some("S")),
            item(Field, "e", Some("S")),
            item(Method, "m", Some("S")),
            item(Enum, "E", None),
            item(Variant, "one", Some("E")),
            item(Field, "x", Some("E")),
            item(Field, "y", Some("E")),
            item(Field, "z", Some("E")),
        ]
    );

    let php = "\
<?php
list($a, list($b, $c)) = $row;
[$key => $value, 'k' => [$d, $e]] = $row;
[$k2 /* a key */ => $v2] = $row;
list('x' => $g, 'y' => list($h)) = $row;
[[&$r]] = $rows;
function fn1() { [$l1, [$l2]] = $row; }
";
    assert_eq!(
        symbols_of("php", php),
        ["a", "b", "c", "value", "d", "e", "v2", "g", "h", "r"]
            .map(|name| item(Variable, name, None))
            .into_iter()
            .chain([item(Function, "fn1", None)])
            .collect::<Vec<_>>()
    );
    assert_eq!(
        symbols_of(
            "php",
            "<?php\nnamespace N {\n    [$a, [$k => $b]] = $row;\n    function f() { [$l, [$m]] = $row; }\n}\n"
        ),
        [
            item(Module, "N", None),
            item(Variable, "a", Some("N")),
            item(Variable, "b", Some("N")),
            item(Function, "f", Some("N")),
        ]
    );
}

/// A case class's constructor annotations don't hide its first parameter
/// list's fields.
#[test]
fn scala_annotated_case_class_constructors() {
    use OutlineKind::*;
    let text = "\
case class C @Inject() (x: Int)(ctx: String)
case class D[T] @Inject() private (y: T)(z: Int)
case class E @A protected (w: Int)
case class F[T] @Inject() (v: T)(u: Int)
";
    assert_eq!(
        symbols_of("scala", text),
        [
            item(Class, "C", None),
            item(Field, "x", Some("C")),
            item(Class, "D", None),
            item(Field, "y", Some("D")),
            item(Class, "E", None),
            item(Field, "w", Some("E")),
            item(Class, "F", None),
            item(Field, "v", Some("F")),
        ]
    );
}

/// Each constant of a chain of assignments is its own item, at the top
/// and in a class or module, also when the chain starts with several
/// targets.
#[test]
fn ruby_chained_constants() {
    use OutlineKind::*;
    let text = "\
A = B = 1
A2, B2 = C2 = [1, 2]
X::Y = Z = 3
class K
  D = E::F = G = 1
  H, I = J = 1, 2
end
module M
  P = Q = 1
  R, S = T = [1, 2]
end
def m
  local = U = 1
end
";
    let expected = [
        item(Constant, "A", None),
        item(Constant, "B", Some("A")),
        item(Constant, "A2", None),
        item(Constant, "B2", None),
        item(Constant, "C2", None),
        item(Constant, "Y", None),
        item(Constant, "Z", Some("Y")),
        item(Class, "K", None),
        item(Constant, "D", Some("K")),
        item(Constant, "F", Some("D")),
        item(Constant, "G", Some("F")),
        item(Constant, "H", Some("K")),
        item(Constant, "I", Some("K")),
        item(Constant, "J", Some("K")),
        item(Module, "M", None),
        item(Constant, "P", Some("M")),
        item(Constant, "Q", Some("P")),
        item(Constant, "R", Some("M")),
        item(Constant, "S", Some("M")),
        item(Constant, "T", Some("M")),
        item(Function, "m", None),
    ];
    assert_eq!(outline_of("ruby", text), expected);
    assert_eq!(symbols_of("ruby", text), expected);
}
