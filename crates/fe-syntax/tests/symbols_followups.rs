//! Definition symbols the first `queries/definitions/` missed: C
//! declarations behind preprocessor conditionals, attributes, calling
//! conventions and assembly labels; JavaScript destructuring, function
//! initializers and named class expressions; Python chained and unpacking
//! assignments; Java annotation-type constants.

#![cfg(all(
    feature = "python",
    feature = "javascript",
    feature = "typescript",
    feature = "java",
    feature = "c"
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

/// `text`'s symbols as `language`, checked to nest as an outline does.
fn symbols_of(language: &str, text: &str) -> Outline {
    nested(symbols(&parsed(language, text)).unwrap())
}

/// `symbols`, after checking that each item's ancestors are exactly the
/// earlier items whose ranges contain its own, as [`Outline::item_at`]
/// relies on.
fn nested(symbols: Outline) -> Outline {
    for (ix, item) in symbols.items.iter().enumerate() {
        let mut ancestors = Vec::new();
        let mut parent = item.parent;
        while let Some(p) = parent {
            ancestors.push(p as usize);
            parent = symbols.items[p as usize].parent;
        }
        ancestors.reverse();
        let containing: Vec<usize> = (0..ix)
            .filter(|&other| {
                let range = &symbols.items[other].range;
                range.start <= item.range.start && item.range.end <= range.end
            })
            .collect();
        assert_eq!(ancestors, containing, "the ancestors of {:?}", item.name);
    }
    symbols
}

/// An item's kind, name and parent's kind and name.
type Listed = (OutlineKind, String, Option<(OutlineKind, String)>);

/// Each item's kind, name and parent, in order.
fn listed(symbols: &Outline) -> Vec<Listed> {
    (symbols.items.iter())
        .map(|item| {
            let parent = item.parent.map(|ix| {
                let parent = &symbols.items[ix as usize];
                (parent.kind, parent.name.clone())
            });
            (item.kind, item.name.clone(), parent)
        })
        .collect()
}

/// The items of the given kinds, with their parents.
fn of_kinds(symbols: &Outline, kinds: &[OutlineKind]) -> Vec<Listed> {
    let mut items = listed(symbols);
    items.retain(|(kind, _, _)| kinds.contains(kind));
    items
}

fn item(kind: OutlineKind, name: &str, parent: Option<(OutlineKind, &str)>) -> Listed {
    let parent = parent.map(|(kind, name)| (kind, name.to_owned()));
    (kind, name.to_owned(), parent)
}

/// Top-level variables named `names`.
fn variables(names: &[&str]) -> Vec<Listed> {
    (names.iter())
        .map(|name| item(OutlineKind::Variable, name, None))
        .collect()
}

const JS_LANGUAGES: [&str; 3] = ["javascript", "typescript", "tsx"];

#[test]
fn c_definitions_behind_preprocessor_branches() {
    use OutlineKind::*;
    let text = "\
#ifdef A
int a1;
#elif defined(B)
int b1;
#elifdef D
int d1;
#else
int c1;
#endif
#if X
int x1;
#elif Y
int y1;
#else
int z1;
#endif
#ifndef GUARD
#define GUARD
#ifdef H
#if Z
int nested;
#else
int nested_else;
#endif
#endif
#endif
struct s {
#ifdef A
  int fa;
#elif B
  int fb;
#else
  int fc;
#if Z
  int deep;
#endif
#endif
  int plain;
};
union u {
#if X
  int ua;
#else
  int ub;
#endif
};
struct {
#ifdef A
  int anon;
#endif
  int anon2;
} v;
struct outer {
  struct {
#ifdef A
    int inner;
#endif
  } m;
};
void f(void) {
#ifdef A
  int local;
#else
  int other;
#endif
  int local2;
}
";
    let symbols = symbols_of("c", text);
    let struct_s = Some((Struct, "s"));
    let union_u = Some((Struct, "u"));
    assert_eq!(
        of_kinds(&symbols, &[Field, Variable]),
        [
            item(Variable, "a1", None),
            item(Variable, "b1", None),
            item(Variable, "d1", None),
            item(Variable, "c1", None),
            item(Variable, "x1", None),
            item(Variable, "y1", None),
            item(Variable, "z1", None),
            item(Variable, "nested", None),
            item(Variable, "nested_else", None),
            item(Field, "fa", struct_s),
            item(Field, "fb", struct_s),
            item(Field, "fc", struct_s),
            item(Field, "deep", struct_s),
            item(Field, "plain", struct_s),
            item(Field, "ua", union_u),
            item(Field, "ub", union_u),
            item(Variable, "v", None),
            item(Field, "m", Some((Struct, "outer"))),
        ]
    );
}

#[test]
fn js_destructuring_binds_each_name() {
    let text = "\
const { a, b: c, ...rest } = o;
export const [x, , y = 1, ...zs] = arr;
let { p = q, r: { s }, [k]: kv } = o;
export var { t: [u, { w }] } = o;
const plain = 1;
function g() {
  const { local } = o;
  let [l2] = arr;
}
";
    for language in JS_LANGUAGES {
        let symbols = symbols_of(language, text);
        assert_eq!(
            of_kinds(&symbols, &[OutlineKind::Variable]),
            variables(&[
                "a", "c", "rest", "x", "y", "zs", "p", "s", "kv", "u", "w", "plain"
            ]),
            "{language}"
        );
    }
}

#[test]
fn c_attributed_prototypes_are_not_variables() {
    let text = "\
int f [[deprecated]] (void);
int (g [[deprecated]])(void);
int *h [[deprecated]] (void);
int (*fp [[deprecated]])(void);
int (*(fq) [[deprecated]])(void);
int n [[deprecated]];
";
    let symbols = symbols_of("c", text);
    assert_eq!(
        of_kinds(&symbols, &[OutlineKind::Variable]),
        variables(&["fp", "fq", "n"])
    );
}

#[test]
fn js_function_initializers_are_not_variables() {
    use OutlineKind::*;
    let text = "\
const gen = function* () {};
export const f = (() => 1);
let h = ((function () {}));
export var k = function* named() {};
const plain = 1;
const arrow = () => 2;
export const fe = function () {};
";
    for language in JS_LANGUAGES {
        let syntax = parsed(language, text);
        let symbols = nested(symbols(&syntax).unwrap());
        assert_eq!(
            of_kinds(&symbols, &[Variable]),
            variables(&["plain"]),
            "{language}"
        );
        let outline = outline(&syntax).unwrap();
        assert_eq!(
            of_kinds(&symbols, &[Function]),
            of_kinds(&outline, &[Function]),
            "{language}"
        );
        assert_eq!(
            of_kinds(&outline, &[Function]),
            [item(Function, "arrow", None), item(Function, "fe", None)],
            "{language}"
        );
    }

    // TypeScript's type assertions wrap a function as parentheses do.
    let typescript = "\
const a = (() => 1) as F;
const b = (function* () {}) satisfies G;
const c = (() => 1)!;
const d = <F>(() => 1);
const e = 1 as number;
";
    let symbols = symbols_of("typescript", typescript);
    assert_eq!(listed(&symbols), variables(&["e"]));
}

#[test]
fn python_chained_and_unpacked_assignments() {
    use OutlineKind::*;
    let text = "\
a0, b0 = 1, 2
m = n = 1
[q0, *r0] = 1, 2
MAX = MIN = 3

class K:
    a, b = 1, 2
    x = y = z = 1
    (c, [d, *e]) = 1, (2, 3)
    self_attr.p, f = 1, 2
    g: int = 4

    def method(self):
        u, v = 1, 2
        w = t = 3

def fn():
    p = s = 1
    r, h = 1, 2

for i, j in []:
    pass
";
    let symbols = symbols_of("python", text);
    let k = Some((Class, "K"));
    assert_eq!(
        listed(&symbols),
        [
            item(Variable, "m", None),
            item(Variable, "n", None),
            item(Constant, "MAX", None),
            // The outline's MAX is the whole statement.
            item(Constant, "MIN", Some((Constant, "MAX"))),
            item(Class, "K", None),
            item(Field, "a", k),
            item(Field, "b", k),
            item(Field, "x", k),
            item(Field, "y", k),
            item(Field, "z", k),
            item(Field, "c", k),
            item(Field, "d", k),
            item(Field, "e", k),
            item(Field, "f", k),
            item(Field, "g", k),
            item(Method, "method", k),
            item(Function, "fn", None),
        ]
    );
}

#[test]
fn java_annotation_constants_are_fields() {
    use OutlineKind::*;
    let text = "\
@interface Marker {
  int LIMIT = 1;
  String NAME = \"n\", OTHER = \"o\";
  String value();
}
";
    let symbols = symbols_of("java", text);
    let marker = Some((Interface, "Marker"));
    assert_eq!(
        listed(&symbols),
        [
            item(Interface, "Marker", None),
            item(Field, "LIMIT", marker),
            item(Field, "NAME", marker),
            item(Field, "OTHER", marker),
            item(Method, "value", marker),
        ]
    );
}

#[test]
fn c_modifiers_and_asm_labels_name_nothing() {
    let text = "\
int __stdcall h(void);
extern int real __asm__(\"real\");
int (__stdcall *hp)(void);
int __cdecl k(int);
";
    let symbols = symbols_of("c", text);
    assert_eq!(listed(&symbols), variables(&["real", "hp"]));
}

#[test]
fn js_named_class_expression_fields() {
    use OutlineKind::*;
    let text = "\
const C = class Named { x = 1; static #y; m() {} };
const D = class { z = 1; };
class A { f = class N { g = 1; }; }
";
    for language in JS_LANGUAGES {
        let symbols = symbols_of(language, text);
        let c = Some((Variable, "C"));
        assert_eq!(
            listed(&symbols),
            [
                item(Variable, "C", None),
                item(OutlineKind::Field, "x", c),
                item(OutlineKind::Field, "#y", c),
                item(Method, "m", c),
                item(Variable, "D", None),
                item(Class, "A", None),
                item(OutlineKind::Field, "f", Some((Class, "A"))),
                item(OutlineKind::Field, "g", Some((OutlineKind::Field, "f"))),
            ],
            "{language}"
        );
    }
}
