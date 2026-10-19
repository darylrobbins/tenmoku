//! The detail after each outline item's name: the text up to its body or
//! value, whitespace collapsed, cut at [`MAX_DETAIL_CHARS`].

#![cfg(all(
    feature = "rust",
    feature = "python",
    feature = "typescript",
    feature = "go",
    feature = "java",
    feature = "c",
    feature = "markdown",
    feature = "json",
    feature = "scala",
    feature = "javascript",
    feature = "ruby",
    feature = "cpp"
))]

use fe_buffer::Buffer;
use fe_syntax::{Syntax, language_by_name, outline, symbols};

/// The most chars a detail holds.
const MAX_DETAIL_CHARS: usize = 80;

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

/// Each symbol's name and detail.
fn symbol_details(language: &str, text: &str) -> Vec<(String, String)> {
    let symbols = symbols(&parsed(language, text)).unwrap();
    (symbols.items.into_iter())
        .map(|item| (item.name, item.detail))
        .collect()
}

/// Each outline item's name and detail.
fn details(language: &str, text: &str) -> Vec<(String, String)> {
    let outline = outline(&parsed(language, text)).unwrap();
    outline
        .items
        .into_iter()
        .map(|item| (item.name, item.detail))
        .collect()
}

/// The detail of the outline item named `name`.
fn detail_of(language: &str, text: &str, name: &str) -> String {
    let found = details(language, text);
    let (_, detail) = (found.iter())
        .find(|(item, _)| item == name)
        .unwrap_or_else(|| panic!("no {name} in {found:?}"));
    detail.clone()
}

#[test]
fn rust_details_are_signatures_and_types() {
    let text = "struct Point { x: f32 }\nstruct Unit;\nstruct Pair(u32, u32);\n\
        impl Point {\n    pub fn new(x: f32) -> Self { Self { x } }\n    fn long(\n        a: u32,\n        b: u32,\n    ) -> u32 where u32: Copy {\n        a\n    }\n}\n\
        const MAX: usize = 3;\ntrait Shape: Clone { fn area(&self) -> f32; }\nenum E<T> { A(T) }\n";
    assert_eq!(
        details("rust", text),
        [
            ("Point", ""),
            ("Unit", ""),
            ("Pair", "(u32, u32)"),
            ("impl Point", ""),
            ("new", "(x: f32) -> Self"),
            ("long", "(a: u32, b: u32,) -> u32 where u32: Copy"),
            ("MAX", ": usize"),
            ("Shape", ": Clone"),
            ("area", "(&self) -> f32"),
            ("E", "<T>"),
        ]
        .map(|(name, detail)| (name.to_string(), detail.to_string()))
    );
}

#[test]
fn typescript_details_keep_default_values_in_parameters() {
    let text = "class Circle implements Shape {\n  constructor(private r: number) {}\n  area(): number { return 1; }\n}\n\
        function add(a: number, b = 2): number { return a + b; }\ntype Id = string;\n";
    assert_eq!(detail_of("typescript", text, "Circle"), "implements Shape");
    assert_eq!(
        detail_of("typescript", text, "constructor"),
        "(private r: number)"
    );
    assert_eq!(detail_of("typescript", text, "area"), "(): number");
    assert_eq!(
        detail_of("typescript", text, "add"),
        "(a: number, b = 2): number"
    );
    assert_eq!(detail_of("typescript", text, "Id"), "");
}

#[test]
fn python_details_stop_at_the_colon() {
    let text = "class A(Base):\n    def f(self, x: int = 3) -> int:\n        return x\n\ndef g(*args, **kw):\n    pass\n";
    assert_eq!(
        details("python", text),
        [
            ("A", "(Base)"),
            ("f", "(self, x: int = 3) -> int"),
            ("g", "(*args, **kw)")
        ]
        .map(|(name, detail)| (name.to_string(), detail.to_string()))
    );
}

#[test]
fn go_java_and_c_details_follow_the_name() {
    let go = "package main\n\ntype T struct { A int }\n\nfunc (t *T) Name(a int, b string) (int, error) {\n\treturn 0, nil\n}\n";
    assert_eq!(
        detail_of("go", go, "Name"),
        "(a int, b string) (int, error)"
    );
    assert_eq!(detail_of("go", go, "T"), "struct");
    let java = "public class Main<T> extends Base {\n  public static int add(int a, int b) { return a + b; }\n}\n";
    assert_eq!(detail_of("java", java, "Main"), "<T> extends Base");
    assert_eq!(detail_of("java", java, "add"), "(int a, int b)");
    let c = "struct point { int x; };\nint add(int a, int b) {\n  return a + b;\n}\n";
    assert_eq!(detail_of("c", c, "add"), "(int a, int b)");
    assert_eq!(detail_of("c", c, "point"), "");
}

#[test]
fn headings_and_data_keys_have_no_detail() {
    for (name, detail) in details("markdown", "# Title: with colon\n\nText\n\n## Sub\n")
        .into_iter()
        .chain(details("json", "{\"a\": 1, \"b\": {\"c\": true}}\n"))
    {
        assert_eq!(detail, "", "{name}");
    }
}

#[test]
fn long_details_are_cut() {
    let params: Vec<String> = (0..30).map(|i| format!("argument_{i}: u32")).collect();
    let text = format!("fn wide({}) {{}}\n", params.join(", "));
    let detail = detail_of("rust", &text, "wide");
    // Cut at the cap, less a trailing comma or space the cut leaves.
    let chars = detail.chars().count();
    assert!(
        (MAX_DETAIL_CHARS - 2..=MAX_DETAIL_CHARS).contains(&chars),
        "{detail:?}"
    );
    assert!(text.contains(&detail));
}

#[test]
fn symbols_carry_details_too() {
    let text = "struct Point {\n    x: f32,\n    y: f32,\n}\nenum E { A(u8) }\n";
    assert_eq!(
        symbol_details("rust", text),
        [
            ("Point", ""),
            ("x", ": f32"),
            ("y", ": f32"),
            ("E", ""),
            ("A", "(u8)")
        ]
        .map(|(name, detail)| (name.to_string(), detail.to_string()))
    );
}

/// Pairs of names and details, as owned strings.
fn owned<const N: usize>(pairs: [(&str, &str); N]) -> Vec<(String, String)> {
    pairs
        .map(|(name, detail)| (name.to_string(), detail.to_string()))
        .to_vec()
}

#[test]
fn brackets_in_strings_and_comments_dont_count() {
    let python = "def f(s=\"(\"):\n    return 42\n\ndef g(s=\")\", n=1):\n    pass\n\n\
        def h(x=(1,), y=[2]):  # a comment (\n    pass\n";
    assert_eq!(
        details("python", python),
        owned([
            ("f", "(s=\"(\")"),
            ("g", "(s=\")\", n=1)"),
            ("h", "(x=(1,), y=[2])")
        ])
    );
    let rust = "fn h(s: &str /* ) { */, t: u8) -> u8 { 0 }\nconst C: &str = \"{\";\n";
    assert_eq!(
        details("rust", rust),
        owned([("h", "(s: &str, t: u8) -> u8"), ("C", ": &str")])
    );
    let typescript = "class C { m(a = \"{\"): void {} }\n";
    assert_eq!(
        detail_of("typescript", typescript, "m"),
        "(a = \"{\"): void"
    );
}

#[test]
fn commas_are_kept() {
    let rust = "fn f(x: (u32,)) {}\nfn g() -> (u32,) {}\n";
    assert_eq!(
        details("rust", rust),
        owned([("f", "(x: (u32,))"), ("g", "() -> (u32,)")])
    );
    assert_eq!(
        detail_of("python", "def h(x=(1,)):\n    pass\n", "h"),
        "(x=(1,))"
    );
}

#[test]
fn types_and_generics_dont_end_the_detail() {
    let typescript = "function f(): { x: number } { return { x: 1 }; }\n\
        function g<T extends { a: string } = { a: string }>(t: T): T { return t; }\n";
    assert_eq!(
        details("typescript", typescript),
        owned([
            ("f", "(): { x: number }"),
            ("g", "<T extends { a: string } = { a: string }>(t: T): T"),
        ])
    );
    assert_eq!(
        detail_of("rust", "struct S<T = u8> { x: T }\n", "S"),
        "<T = u8>"
    );
}

#[test]
fn parenthesized_c_declarators_reach_their_parameters() {
    let c = "int (twice)(int x) { return x; }\ntypedef int (*fp)(int);\n";
    assert_eq!(
        details("c", c),
        owned([("twice", "(int x)"), ("fp", "(int)")])
    );
}

#[test]
fn fields_captured_by_name_read_on_in_their_declaration() {
    let go = "package main\n\ntype T struct {\n\tX, Y int\n\tZ string // a comment\n}\n";
    assert_eq!(
        symbol_details("go", go),
        owned([("T", "struct"), ("X", "int"), ("Y", "int"), ("Z", "string")])
    );
    let c = "struct p { int x; int y[4]; };\n";
    assert_eq!(
        symbol_details("c", c),
        owned([("p", ""), ("x", ""), ("y", "[4]")])
    );
    let scala =
        "class A {\n  val x: Int = 3\n  var y: String = \"\"\n  def f(a: Int): Int = a\n}\n";
    assert_eq!(
        symbol_details("scala", scala),
        owned([
            ("A", ""),
            ("x", ": Int"),
            ("y", ": String"),
            ("f", "(a: Int): Int")
        ])
    );
}

#[test]
fn scala_names_declared_together_share_their_type() {
    let scala =
        "class A {\n  val x, y: Int = 3\n  var a, b: String = \"\"\n}\nval top, other: Int = 1\n";
    assert_eq!(
        symbol_details("scala", scala),
        owned([
            ("A", ""),
            ("x", ": Int"),
            ("y", ": Int"),
            ("a", ": String"),
            ("b", ": String"),
            ("top", ": Int"),
            ("other", ": Int"),
        ])
    );
}

#[test]
fn names_alone_dont_read_into_their_lists() {
    // An enum's last variant, without a value.
    assert_eq!(
        symbol_details("rust", "enum E { A }\n"),
        owned([("E", ""), ("A", "")])
    );
    assert_eq!(
        symbol_details("c", "enum E { A };\n"),
        owned([("E", ""), ("A", "")])
    );
    assert_eq!(
        symbol_details("typescript", "enum E { A }\n"),
        owned([("E", ""), ("A", "")])
    );
    // Destructured names.
    assert_eq!(
        symbol_details("javascript", "const { a, b } = o;\nconst [c, d] = o;\n"),
        owned([("a", ""), ("b", ""), ("c", ""), ("d", "")])
    );
    // A C declarator's array sizes are its own, not its sibling's.
    assert_eq!(
        symbol_details("c", "struct S { int x, y[4][5]; };\n"),
        owned([("S", ""), ("x", ""), ("y", "[4][5]")])
    );
}

#[test]
fn commas_after_a_whole_item_are_its_own() {
    // Ruby's parameters without parentheses.
    let ruby = "class A < B\n  def f a, b\n    1\n  end\n  def g(x) = x\nend\n";
    assert_eq!(
        details("ruby", ruby),
        owned([("A", "< B"), ("f", "a, b"), ("g", "(x)")])
    );
}

#[test]
fn a_go_fields_tag_isnt_its_type() {
    let go =
        "package main\n\ntype T struct {\n\tX int `json:\"x\"`\n\tY, Z string `json:\"y\"`\n}\n";
    assert_eq!(
        symbol_details("go", go),
        owned([
            ("T", "struct"),
            ("X", "int"),
            ("Y", "string"),
            ("Z", "string")
        ])
    );
}

#[test]
fn cpp_details_keep_qualifiers() {
    let cpp = "class A : public B {\n public:\n  virtual int f(int a) const = 0;\n  int g() const { return 1; }\n};\n\
        template <typename T> T id(T t) { return t; }\n";
    assert_eq!(
        details("cpp", cpp),
        owned([
            ("A", ": public B"),
            ("f", "(int a) const"),
            ("g", "() const"),
            ("id", "(T t)")
        ])
    );
}

#[test]
fn a_deeply_nested_default_doesnt_overflow_the_stack() {
    // Each + nests the one before it, 100,000 deep.
    let text = format!("def f(x=1{}):\n    pass\n", "+1".repeat(100_000));
    let detail = detail_of("python", &text, "f");
    assert_eq!(detail.chars().count(), MAX_DETAIL_CHARS);
    assert!(detail.starts_with("(x=1+1+1+"), "{detail}");
}

#[test]
fn a_comment_between_tokens_keeps_them_apart() {
    let rust = "fn f() -> impl /*note*/Trait { loop {} }\n\
        fn g() -> impl/*note*/Trait { loop {} }\n\
        fn h(a: u32 /* first */, b: u32) {}\n";
    assert_eq!(
        details("rust", rust),
        owned([
            ("f", "() -> impl Trait"),
            ("g", "() -> impl Trait"),
            ("h", "(a: u32, b: u32)"),
        ])
    );
}

#[test]
fn a_line_break_around_or_in_a_comment_ends_the_detail() {
    let rust = "fn f() /* a\n b */ -> u32 { 0 }\nfn g() // note\n    -> u32 { 0 }\n";
    assert_eq!(details("rust", rust), owned([("f", "()"), ("g", "()")]));
}
