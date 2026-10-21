//! The detail of an item captured as its name alone whose suffix lies in its
//! own declaration or pattern: Swift's properties and enum payloads, Scala's
//! typed patterns and C's and C++'s bit-fields. Each reads its own suffix,
//! never a sibling's.

#![cfg(all(feature = "swift", feature = "scala", feature = "c", feature = "cpp"))]

use fe_buffer::Buffer;
use fe_syntax::{Syntax, language_by_name, symbols};

/// Each symbol's name and detail in `text`, parsed whole as `language`.
fn symbol_details(language: &str, text: &str) -> Vec<(String, String)> {
    let buffer = Buffer::new(text);
    let mut syntax = Syntax::new(language_by_name(language).unwrap(), buffer.current());
    syntax.finish(syntax.parse_job().run());
    assert!(
        !syntax.tree().unwrap().root_node().has_error(),
        "the {language} text parses without errors"
    );
    (symbols(&syntax).unwrap().items.into_iter())
        .map(|item| (item.name, item.detail))
        .collect()
}

/// Pairs of names and details, as owned strings.
fn owned<const N: usize>(pairs: [(&str, &str); N]) -> Vec<(String, String)> {
    pairs
        .map(|(name, detail)| (name.to_string(), detail.to_string()))
        .to_vec()
}

#[test]
fn swift_properties_keep_their_types() {
    let swift = "struct S {\n  var x: Int\n  var a: Int, b: String = \"\"\n  static var z = 3\n  \
        var area: Double { 1.0 }\n}\nprotocol P { var p: Int { get } }\nlet top: Int = 1\n";
    assert_eq!(
        symbol_details("swift", swift),
        owned([
            ("S", ""),
            ("x", ": Int"),
            ("a", ": Int"),
            ("b", ": String"),
            ("z", ""),
            ("area", ": Double"),
            ("P", ""),
            ("p", ": Int"),
            ("top", ": Int"),
        ])
    );
}

#[test]
fn swift_enum_cases_keep_their_payloads() {
    let swift =
        "enum E {\n  case a(Int)\n  case b(x: Int, y: String), c(Int)\n  case f, g(Int)\n}\n";
    assert_eq!(
        symbol_details("swift", swift),
        owned([
            ("E", ""),
            ("a", "(Int)"),
            ("b", "(x: Int, y: String)"),
            ("c", "(Int)"),
            ("f", ""),
            ("g", "(Int)"),
        ])
    );
}

#[test]
fn swift_raw_values_and_tuple_names_dont_take_a_siblings_suffix() {
    let swift =
        "enum R: Int {\n  case d = 1, e\n}\nstruct T {\n  let (p, q): (Int, Int) = (1, 2)\n}\n";
    assert_eq!(
        symbol_details("swift", swift),
        owned([
            ("R", ": Int"),
            ("d", ""),
            ("e", ""),
            ("T", ""),
            ("p", ""),
            ("q", ""),
        ])
    );
}

#[test]
fn scala_typed_patterns_keep_their_types() {
    let scala = "val Some(x: Int) = opt\nval (a: Map[Int, String], b) = pair\n\
        class A {\n  val Some(y: String) = o\n}\n";
    assert_eq!(
        symbol_details("scala", scala),
        owned([
            ("x", ": Int"),
            ("a", ": Map[Int, String]"),
            ("b", ""),
            ("A", ""),
            ("y", ": String"),
        ])
    );
}

#[test]
fn c_bit_fields_keep_their_widths() {
    let c = "struct S { unsigned x:3; int a:1, b:2; int *p, q : 4; int e; int y[4][5]; };\n";
    assert_eq!(
        symbol_details("c", c),
        owned([
            ("S", ""),
            ("x", ":3"),
            ("a", ":1"),
            ("b", ":2"),
            ("p", ""),
            ("q", ": 4"),
            ("e", ""),
            ("y", "[4][5]"),
        ])
    );
    // A declarator's array sizes stay its own, not its sibling's.
    assert_eq!(
        symbol_details("c", "struct S { int x, y[4][5]; };\n"),
        owned([("S", ""), ("x", ""), ("y", "[4][5]")])
    );
}

#[test]
fn cpp_bit_fields_keep_their_widths() {
    let cpp = "struct S {\n  unsigned x:3;\n  int a:1, b:2;\n  int e = 3;\n  int f{4};\n  \
        int g() const;\n};\n";
    assert_eq!(
        symbol_details("cpp", cpp),
        owned([
            ("S", ""),
            ("x", ":3"),
            ("a", ":1"),
            ("b", ":2"),
            ("e", ""),
            ("f", ""),
            ("g", "() const"),
        ])
    );
}

/// The task's regression test, named for this file: one name-only item of
/// each kind keeps its own suffix and leaves its sibling's alone.
#[test]
fn outline_detail_suffix() {
    for (language, text, expected) in [
        (
            "swift",
            "struct S { var x: Int, y: String }\n",
            [("x", ": Int"), ("y", ": String")],
        ),
        (
            "swift",
            "enum E { case a(Int), b }\n",
            [("a", "(Int)"), ("b", "")],
        ),
        (
            "scala",
            "val (x: Int, y) = pair\n",
            [("x", ": Int"), ("y", "")],
        ),
        ("c", "struct S { int x:3, y; };\n", [("x", ":3"), ("y", "")]),
        (
            "cpp",
            "struct S { int x, y:3; };\n",
            [("x", ""), ("y", ":3")],
        ),
    ] {
        let found = symbol_details(language, text);
        for (name, detail) in expected {
            assert!(
                found.contains(&(name.to_string(), detail.to_string())),
                "{language} {name}: {detail:?} in {found:?}"
            );
        }
    }
}
