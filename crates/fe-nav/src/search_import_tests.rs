//! Follow-ups to T136's review: lexical context in the import scan,
//! whole identifiers in named lists, and cancels polled by the work done.

use std::{cell::Cell, rc::Rc};

use super::{tests::imports, *};

#[test]
fn text_that_only_looks_like_an_import_reports_no_module() {
    let cases: &[&str] = &[
        r#"const r = /;import a from "util"/;"#,
        r#"const s = "\";import a from 'util';";"#,
        r#"const s = '\'; export * from "util"';"#,
        "/*\nimport a from 'util'\n*/",
        "const t = `\nimport a from 'util'\n`;",
        "const t = `a ${b}\n; import a from 'util'\n`;",
        "const t = `\\`\nimport a from 'util'\n`;",
        "x = a / b; y = /[/]import a from 'util'/g;",
        "if (x) return /;import a from 'util'/.test(y)",
        // A condition's `)` is followed by a statement, so a regex.
        "if (x) /;import a from \"util\"/.test(y);",
        "while (x) /;import a from 'util'/.test(y);",
        "for (;;) /;import a from 'util'/.test(y);",
        // A block's `}` too.
        "{ f() }\n/;import a from 'util'/.test(y);",
        "function f() {}\n/;import a from 'util'/.test(y);",
        // A string continued by a `\` at a line end, in both quotes.
        "const s = \"\\\n;import a from 'util';\";",
        "const s = '\\\n;import a from \"util\";';",
        "const s = \"a\\\nb\\\n;import a from 'util';\";",
        // Templates nest, and their substitutions are code.
        "const t = `outer ${`inner\nimport a from 'util'\n`}`;",
        "const t = `a ${ b /* ` */ } \nimport a from 'util'\n`;",
        "const t = `a ${ {x: 1}.x + \"`\" } \nimport a from 'util'\n`;",
        "const t = `a ${ b\n} \nimport a from 'util'\n`;",
        "const t = `${`${`x`}\nimport a from 'util'\n`}`;",
        // Declarations and statement blocks end before a statement.
        "class C {}\n/;import a from \"util\"/.test(y);",
        "class C extends f({}) {}\n/;import a from \"util\"/.test(y);",
        "try {} catch {}\n/;import a from \"util\"/.test(y);",
        "try {} catch (e) {}\n/;import a from \"util\"/.test(y);",
        "try {} finally {}\n/;import a from \"util\"/.test(y);",
        "if (x) {} else {}\n/;import a from \"util\"/.test(y);",
        "export default function () {}\n/;import a from \"util\"/.test(y);",
        "async function f(a = {}) {}\n/;import a from \"util\"/.test(y);",
        "async function f() { for await (const x of xs) /;import a from \"util\"/.test(y); }",
        "async function f() { for await (const x of xs) {} /;import a from \"util\"/.test(y); }",
        // Past 64 open brackets the kinds aren't kept, and nothing after
        // is taken for a statement.
        &deep_template(),
        &deep_condition(),
    ];
    for source in cases {
        assert_eq!(
            imports("javascript", source),
            Vec::<&str>::new(),
            "{source:?}"
        );
    }
    // What follows the context is still read, on the same line or after.
    let numbers = [
        "const n = 1 / 2; import a from './a';",
        "const n = 1.5 / 2; import a from './a';",
        "const n = .5 / 2; import a from './a';",
        "const n = 1e+5 / 2; import a from './a';",
        "const n = 0x1e+1 / 2; import a from './a';",
        "const n = 10n / 2n; import a from './a';",
        "const n = 1_000 / 2; import a from './a';",
        "const f = function () {} / 2; import a from './a';",
        "const f = function g() { return /x/; } / 2; import a from './a';",
        "const c = class {} / 2; import a from './a';",
        "const c = class extends B { m() {} } / 2; import a from './a';",
        "x = async function () {} / 2; import a from './a';",
        "f(function () {} / 2); import a from './a';",
    ];
    for source in numbers {
        assert_eq!(imports("typescript", source), ["./a"], "{source:?}");
    }
    let cases: &[(&str, &[&str])] = &[
        ("/*\n*/ import a from './a'", &["./a"]),
        ("const t = `\n`; import a from './a'", &["./a"]),
        ("const r = /x/g; import a from './a'", &["./a"]),
        ("x = a / b; import a from './a'", &["./a"]),
        ("const s = \"\\\"\"; import a from './a'", &["./a"]),
        ("/* a\n b */ x;\nimport b from './b'", &["./b"]),
        (
            "/* a\n b */ x;\nimport b from './b'\n/* c\nexport * from './d'",
            &["./b"],
        ),
        // A restart after a statement keeps the context of later lines.
        (
            "import a from './a'; /*\nimport b from './b'\n*/ import c from './c'",
            &["./a", "./c"],
        ),
        // A `/` after a value is a division, not a regex that swallows
        // the import after it.
        ("x++ / 2; import a from './a';", &["./a"]),
        ("x-- / 2; import a from './a';", &["./a"]),
        ("a[0] / 2; import a from './a';", &["./a"]),
        ("f(x) / 2; import a from './a';", &["./a"]),
        ("x = ({}) / 2; import a from './a';", &["./a"]),
        ("x = {} / 2; import a from './a';", &["./a"]),
        ("a.return / 2; import a from './a';", &["./a"]),
        ("a?.in / 2; import a from './a';", &["./a"]),
        ("`t` / 2; import a from './a';", &["./a"]),
        // The strings that continue over lines end, and the template
        // substitutions' braces nest.
        ("const s = \"a\\\nb\"; import a from './a'", &["./a"]),
        (
            "const t = `${ {a: {b: 1}} }`; import a from './a'",
            &["./a"],
        ),
        ("const t = `${ `x` }` + `y`;\nimport a from './a'", &["./a"]),
        ("const t = `a ${ b\n} c`;\nimport a from './a'", &["./a"]),
        ("x = a\n++b / 2; import a from './a'", &["./a"]),
        (
            "x = a ? /re/ : /;import b from 'b'/; import a from './a'",
            &["./a"],
        ),
        ("x = [.../r/.exec(s)]; import a from './a'", &["./a"]),
    ];
    for (source, modules) in cases {
        assert_eq!(&imports("typescript", source), modules, "{source:?}");
    }
}

/// A template with a substitution, in 64 parentheses: its text after the
/// substitution must stay text.
fn deep_template() -> String {
    format!(
        "const t = {}`outer ${{0}}\nimport a from 'util'\n`{};",
        "(".repeat(64),
        ")".repeat(64)
    )
}

/// An `if` condition inside 64 parentheses, so a statement follows its `)`.
fn deep_condition() -> String {
    format!(
        "{}if (x) /;import a from 'util'/.test(y){};",
        "(".repeat(64),
        ")".repeat(64)
    )
}

#[test]
fn imports_before_deep_nesting_are_kept() {
    let source = format!(
        "import a from './a';\n{}\nimport b from './b'\n",
        deep_template()
    );
    assert_eq!(imports("javascript", &source), ["./a"]);
    // Nesting that is closed again, within what is kept, doesn't matter.
    let shallow = format!(
        "f({}0{});\nimport b from './b'\n",
        "(".repeat(30),
        ")".repeat(30)
    );
    assert_eq!(imports("javascript", &shallow), ["./b"]);
}

#[test]
fn declarations_start_after_a_line_break_ends_a_statement() {
    // A line break after a value ends the statement (a semicolon is
    // inserted), so a declaration starts there, and a statement follows
    // its `}`.
    let after = [
        "const n=1",
        "x = f()",
        "x = a[0]",
        "x = 'a'",
        "x = {}",
        "x = /re/",
        "x = `t`",
        "x = a /* c */",
        "x = a /*\n*/",
        "x = a // c",
        "for (;;) break",
        "function g() { return",
        "function* g() { yield",
    ];
    let declarations = [
        "function f() {}",
        "async function f() {}",
        "function* f() {}",
        "class C {}",
        "class C extends B {}",
        "export function f() {}",
        "export default class {}",
        "export async function f() {}",
    ];
    for before in after {
        for declaration in declarations {
            // (Only a module's top level exports.)
            let close = if before.contains('{') { " }" } else { "" };
            if !close.is_empty() && declaration.starts_with("export") {
                continue;
            }
            let source =
                format!("{before}\n{declaration}\n/;import a from \"util\"/.test(y);{close}");
            assert_eq!(
                imports("typescript", &source),
                Vec::<&str>::new(),
                "{source:?}"
            );
        }
    }
    for source in [
        "@dec\nclass C {}\n/;import a from \"util\"/.test(y);",
        "x = 1\n@dec\nexport class C {}\n/;import a from \"util\"/.test(y);",
    ] {
        assert_eq!(
            imports("typescript", source),
            Vec::<&str>::new(),
            "{source:?}"
        );
    }
    // A line break after an operator, or where the expression must go on,
    // doesn't end it: the function or class there is a value, and a `/`
    // after it a division.
    let continued = [
        "const f = a ||\nfunction () {} / 2; import a from './a';",
        "const f = a\n|| function () {} / 2; import a from './a';",
        "const f =\nfunction () {} / 2; import a from './a';",
        "const f = (\nfunction () {}) / 2; import a from './a';",
        "const c = new\nclass {} / 2; import a from './a';",
        "const f = typeof\nfunction () {} / 2; import a from './a';",
        "const f = await\nasync function () {} / 2; import a from './a';",
        "const c = a ? b :\nclass {} / 2; import a from './a';",
        "const c = [a,\nclass {}] / 2; import a from './a';",
        "f(a,\nfunction () {} / 2); import a from './a';",
        "const f = function\n() {} / 2; import a from './a';",
        "const f = function\ng() {} / 2; import a from './a';",
        "const c = class\nC {} / 2; import a from './a';",
        "const c = class\nextends B {} / 2; import a from './a';",
    ];
    for source in continued {
        assert_eq!(imports("typescript", source), ["./a"], "{source:?}");
    }
}

#[test]
fn blocks_and_type_declarations_end_before_a_statement() {
    let cases = [
        // A `{` after a line break that ends a statement opens a block.
        "x = a\n{ b }\n/;import a from \"util\"/.test(y);",
        "x = {}\n{}\n/;import a from \"util\"/.test(y);",
        // TypeScript's declarations have bodies that aren't values.
        "interface I {}\n/;import a from \"util\"/.test(y);",
        "interface I extends J<{}> { a: {} }\n/;import a from \"util\"/.test(y);",
        "enum E { A }\n/;import a from \"util\"/.test(y);",
        "const enum E { A }\n/;import a from \"util\"/.test(y);",
        "namespace N {}\n/;import a from \"util\"/.test(y);",
        "namespace N.M {}\n/;import a from \"util\"/.test(y);",
        "export namespace N {}\n/;import a from \"util\"/.test(y);",
        "module M {}\n/;import a from \"util\"/.test(y);",
        "declare module 'm' {}\n/;import a from \"util\"/.test(y);",
        "declare global {}\n/;import a from \"util\"/.test(y);",
        "x = 1\ninterface I {}\n/;import a from \"util\"/.test(y);",
        // A header whose bracket closed before its body is over: a `{`
        // after it is a block's again.
        "f({function: 1, class: 2})\n{ { try {} finally {}\n/;import a from \"util\"/.test(y) } }",
    ];
    for source in cases {
        assert_eq!(
            imports("typescript", source),
            Vec::<&str>::new(),
            "{source:?}"
        );
    }
    // The words are names elsewhere.
    let values = [
        "const namespace = {} / 2; import a from './a';",
        "x = {module: {}} / 2; import a from './a';",
        "module.exports = {} / 2; import a from './a';",
        "f(global, {}) / 2; import a from './a';",
    ];
    for source in values {
        assert_eq!(imports("typescript", source), ["./a"], "{source:?}");
    }
}

#[test]
fn statements_start_where_javascript_starts_them() {
    let phantoms = [
        // A return type before a body.
        "function f(): void {}\n/;import a from \"util\"/.test(y);",
        "class C { m(): void { function g() {}\n/;import a from \"util\"/.test(y) } }",
        "class C { m(): Promise<T> { function g() {}\n/;import a from \"util\"/.test(y) } }",
        // A line break ends a `break`, `continue` or `debugger`, label and all.
        "debugger\n/;import a from \"util\"/.test(y);",
        "for (;;) { break\n/;import a from \"util\"/.test(y) }",
        "l: for (;;) { continue l\n/;import a from \"util\"/.test(y) }",
        // After a label, or a case's `:`, a statement starts.
        "a: {}\n/;import a from \"util\"/.test(y);",
        "a: function f() {}\n/;import a from \"util\"/.test(y);",
        "switch (x) {\ncase 1: {}\n/;import a from \"util\"/.test(y)\n}",
        "switch (x) {\ndefault: {}\n/;import a from \"util\"/.test(y)\n}",
        "switch (x) {\ncase a ? b : c: {}\n/;import a from \"util\"/.test(y)\n}",
        "switch (x) {\ncase f({a: 1}): {}\n/;import a from \"util\"/.test(y)\n}",
        "switch (x) {\ncase a ? b : c + 1: {}\n/;import a from \"util\"/.test(y)\n}",
        "switch (x) {\ncase a ? b ? c : d : e + 1: {}\n/;import a from \"util\"/.test(y)\n}",
        // A regular expression not found on one line is still looked for
        // on the next.
        "x = /[\n/;import a from \"util\"/.test(y);",
        // Values that TypeScript's types end with.
        "const a = [1] as const\nfunction f() {}\n/;import a from \"util\"/.test(y)",
        "type H = (e: E) => void\nexport function f() {}\n/;import a from \"util\"/.test(y)",
        "let cb: Fn | void\nclass C {}\n/;import a from \"util\"/.test(y)",
        "switch (x) { case 1: y = z as const\ncase 2: {}\n/;import a from \"util\"/.test(y) }",
        "const m = 1 << 2\nfunction f(): Promise<void> {}\n/;import a from \"util\"/.test(y)",
        "enum E { A = 1 << 0, B = 1 << 1 }\nfunction f(): Promise<void> {}\n/;import a from \"util\"/.test(y)",
        "function f(): Promise<\n  Array<X>\n> {}\n/;import a from \"util\"/.test(y)",
        "function f(x): x is {a: 1} {}\n/;import a from \"util\"/.test(y)",
        // An object's method, with a return type.
        "const o = { m(): number { function g() {}\n/;import a from \"util\"/.test(y)\nreturn 1 } }",
        // A TypeScript declaration's keyword with no name on its line is a
        // name.
        "export default module\nfunction f() {}\n/;import a from \"util\"/.test(y)",
        "global\nclass C {}\n/;import a from \"util\"/.test(y)",
        // A line break after a function's or class's keyword doesn't end it.
        "export default function\nf() {}\n/;import a from \"util\"/.test(y);",
        "export default class\nC {}\n/;import a from \"util\"/.test(y);",
        "switch (x) { case 1: function f() {}\n/;import a from \"util\"/.test(y) }",
        // A class's static block holds statements.
        "class C {\n  static {\n    function f() {}\n    /;import a from \"util\"/.test(y)\n  }\n}",
        // Decorators come before a declaration.
        "@dec class C {}\n/;import a from \"util\"/.test(y);",
        "@dec export class C {}\n/;import a from \"util\"/.test(y);",
        "@a.b(c) @d class C {}\n/;import a from \"util\"/.test(y);",
        "@(a) class C {}\n/;import a from \"util\"/.test(y);",
    ];
    for source in phantoms {
        assert_eq!(
            imports("typescript", source),
            Vec::<&str>::new(),
            "{source:?}"
        );
    }
    let kept = [
        "a: {}\n/'/.test(s); import a from './a';",
        // TypeScript's postfix `!` keeps a value.
        "const v = map.get(k)! / 2; import a from './a';",
        "x = a != b / 2; import a from './a';",
        "x = y ? !/;import b from 'b'/.test(z) : 0; import a from './a';",
        "x = a\n!/;import b from 'b'/.test(z); import a from './a';",
        // `of` is a name but after a `for`'s binding.
        "const of = 4; x = of / 2; import a from './a';",
        "for (const x of /;import b from 'b'/.exec(s)) {} import a from './a';",
        // Private names are names.
        "class C { #default = 1; m() { return this.#default / 2 } } import a from './a';",
        "class C { #in = 1; m() { return this.#in / 2 } } import a from './a';",
        // Not labels: a field's type, an object's key, a conditional.
        "class C { x: {a: 1} = {} / 2; } import a from './a';",
        "x = {a: {} / 2}; import a from './a';",
        "x = a ? b : {} / 2; import a from './a';",
        "x = <T>{} / 2; import a from './a';",
        // Comparisons and type operators aren't the end of a type that a
        // body follows.
        "x = a > {} / 2; import a from './a';",
        "x = a >> {} / 2; import a from './a';",
        "x = a < b; y = c > {} / 2; import a from './a';",
        "x = 1 << 2\ny = c > {} / 2; import a from './a';",
        "x = a <= b\ny = c > {} / 2; import a from './a';",
        "const v = x as {a: number} / 2; import a from './a';",
        "const v = x satisfies {a: number} / 2; import a from './a';",
        "const of = 4, y = 1\ny\nof / 2; import a from './a';",
    ];
    for source in kept {
        assert_eq!(imports("typescript", source), ["./a"], "{source:?}");
    }
    assert_eq!(
        imports("javascript", "/* require('m') */ import a from './a'"),
        ["m", "./a"]
    );
    assert_eq!(
        imports("javascript", "/* c */ x = require(/* m */ 'm')"),
        ["m"]
    );
}

#[test]
fn review_round_four_contexts() {
    let phantoms = [
        // F3: a `for` header's binding goes on over a line break.
        "for (const x\nof /;import a from \"util\"/.exec(y)) {}",
        "for (const x /* c */\nof /;import a from \"util\"/.exec(y)) {}",
        "async function f() { for await (const x\nof /;import a from \"util\"/.exec(y)) {} }",
        // F4: return types through unions and type operators.
        "function f(): number | void {}\n/;import a from \"util\"/.test(y);",
        "function f(): A | B<C> {}\n/;import a from \"util\"/.test(y);",
        "function f(): keyof\nT {}\n/;import a from \"util\"/.test(y);",
        "function f(): Map<A, B> | void {}\n/;import a from \"util\"/.test(y);",
        "class C { m(): readonly\nT[] { function g() {}\n/;import a from \"util\"/.test(y) } }",
        // F8: a case inside a case's expression.
        "switch(x) { case (() => { switch(y) { case 1: break; } })(): {} /;import a from \"util\"/.test(y); }",
        "switch(x) { case a ? (() => { switch(y) { case 1: break; } })() : b: {} /;import a from \"util\"/.test(y); }",
        // TypeScript's optional members aren't conditionals.
        "class C { m?(): void { function g() {}\n/;import a from \"util\"/.test(y) } }",
        "class C { a?: void\n m(): void { function g() {}\n/;import a from \"util\"/.test(y) } }",
    ];
    for source in phantoms {
        assert_eq!(
            imports("typescript", source),
            Vec::<&str>::new(),
            "{source:?}"
        );
    }
    // F6: every JavaScript line terminator ends a line.
    for terminator in ["\u{2028}", "\u{2029}", "\r"] {
        for source in [
            format!(
                "const n=1{terminator}function f() {{}}{terminator}/;import a from \"util\"/.test(y);"
            ),
            format!(
                "const n=1 /*{terminator}*/ function f() {{}}{terminator}/;import a from \"util\"/.test(y);"
            ),
            format!(
                "const n=1 // c{terminator}function f() {{}}{terminator}/;import a from \"util\"/.test(y);"
            ),
        ] {
            assert_eq!(
                imports("javascript", &source),
                Vec::<&str>::new(),
                "{source:?}"
            );
        }
        for source in [
            format!("x = 1{terminator}import a from './a'"),
            format!("// c{terminator}import a from './a'"),
            format!("/* c */{terminator}import a from './a'"),
        ] {
            assert_eq!(imports("javascript", &source), ["./a"], "{source:?}");
        }
    }
    let kept = [
        // F2: comparisons aren't type arguments.
        "x = a < b && c > {} / 2; import a from './a';",
        "x = (a < b) > {} / 2; import a from './a';",
        "module.exports = a < b && c > {} / 2; import a from './a';",
        // F4.
        "const f = function(): keyof\nT {} / 2; import a from './a';",
        "const f = function(): readonly\nT[] {} / 2; import a from './a';",
        "const f = function(): A | void {} / 2; import a from './a';",
        // A ternary's `:` after a `)` isn't a return type's.
        "x = a ? (b) : readonly / 2; import a from './a';",
        "x = a ? (b) : void {} / 2; import a from './a';",
        // A return type ends with its list item.
        "x = { a: (x): T => x, b: c < d && e > {} / 2 }; import a from './a';",
        // TypeScript's optional marks aren't conditionals.
        "interface I { a?: number; m?(): void }\nx = 1 ? 2 : 3; import a from './a';",
        "class C { a?: void\n b = 1 }\nx = 1 ? 2 : 3; import a from './a';",
    ];
    for source in kept {
        assert_eq!(imports("typescript", source), ["./a"], "{source:?}");
    }
}

#[test]
fn jsx_isnt_read_as_code() {
    let kept = [
        "const el = <div></div>; import a from './a';",
        "const el = <div/>; import a from './a';",
        "const el = <a.b-c x=\"/\" y='/' z={1 / 2} {...p}>t/x {a / b} </a.b-c>; import a from './a';",
        "const el = <>\n  <p>it's / here</p>\n  {x ? <b>/</b> : null}\n</>\nimport a from './a'",
        "const el = (\n  <div\n    a=\"1\"\n  >\n    text / more\n  </div>\n)\nimport a from './a'",
        "function f() { return <div>{`/${x}`}</div> }\nimport a from './a'",
        "const f = <T,>(x: T) => x / 2; import a from './a';",
        "const el = <p a=\"/>\" />; import a from './a';",
        "const el = <a><b/>t</a>; import a from './a';",
        "const e = <Button\n  // don't\n  b=\"x\"\n>t</Button>\nimport a from './a'\n",
        "const e = <a /* it's */ b={1}>x</a>\nimport a from './a'\n",
        "const e = <a /* it's\n */ b={1}>x</a>\nimport a from './a'\n",
        "const e = <Foo<string> a={1} />\nimport a from './a'\n",
        "const e = <Foo<Map<A, B>>>x</Foo>\nimport a from './a'\n",
        "const f = <const T,>(x: T) => x\nimport a from './a'\n",
        "x = a < b; y = c > d / 2; import a from './a';",
    ];
    for source in kept {
        for language in ["tsx", "javascript"] {
            assert_eq!(imports(language, source), ["./a"], "{language} {source:?}");
        }
    }
    let phantoms = [
        "const el = <p>\nimport a from 'util'\n</p>;",
        "const el = <p>{'x'}\n; import a from 'util'\n</p>;",
        "const el = <p a=\"\n\">;import a from 'util'</p>;",
    ];
    for source in phantoms {
        assert_eq!(imports("tsx", source), Vec::<&str>::new(), "{source:?}");
    }
    // In TypeScript proper `<T>x` is an assertion.
    assert_eq!(
        imports("typescript", "x = <T>{} / 2; import a from './a';"),
        ["./a"]
    );
}

#[test]
fn review_round_five_contexts() {
    let kept = [
        // F10: an arrow's body isn't its return type.
        "const f = (x): boolean => x < y && z > {} / 2; import a from './a';",
        "const f = (x): T => (y): U => a < b && c > {} / 2; import a from './a';",
        // (A function type in a return type keeps it going.)
        "const f = function (): (a: A) => void {} / 2; import a from './a';",
        // F14: deep conditionals don't push out a class expression's header.
        &format!(
            "const c = class extends (x ? {{}} : {{}}) {{}} / 2; const d = class extends ({}1{}) {{}} / 2; import a from './a';",
            (0..20).map(|i| format!("a{i} ? ")).collect::<String>(),
            (0..20).map(|i| format!(" : {i}")).collect::<String>(),
        ),
        &format!(
            "const d = class extends {}1{} {{}} / 2; import a from './a';",
            (0..20).map(|i| format!("a{i} ? ")).collect::<String>(),
            (0..20).map(|i| format!(" : {i}")).collect::<String>(),
        ),
        // F16: a `for` header's `;` separates expressions.
        "for (; function () {} / 2; ) {} import a from './a';",
        "for (let i = 0; class {} / 2; i++) {} import a from './a';",
    ];
    for source in kept {
        assert_eq!(imports("typescript", source), ["./a"], "{source:?}");
    }
    let phantoms = [
        // F13: an interface's name after a comment or a line break.
        // F10: a function type in a return type isn't an arrow's body.
        "function f(): (a: A) => void {}\n/;import a from \"util\"/.test(y);",
        "interface /*c*/ I<T> {}\n/;import a from \"util\"/.test(y);",
        "interface\nI<T> {}\n/;import a from \"util\"/.test(y);",
        "enum\nE {}\n/;import a from \"util\"/.test(y);",
        "namespace /*c*/ N {}\n/;import a from \"util\"/.test(y);",
        // An optional method's `?`, past spaces, comments and type
        // parameters.
        "class C {\n  m?<T>(): Promise<T> { function g() {}\n/;import a from \"util\"/.test(y) }\n}",
        "class C {\n  m? (): Promise<void> { function g() {}\n/;import a from \"util\"/.test(y) }\n}",
        "class C {\n  m?/* c */(): Promise<void> { function g() {}\n/;import a from \"util\"/.test(y) }\n}",
        // `<<` opening type arguments and a function type's parameters.
        "function f(): Array<<T>(x: T) => T> {}\n/;import a from \"util\"/.test(y)",
        // F14: deep conditionals don't push out a case.
        &format!(
            "switch (x) {{ case {}1{}: {{}}\n/;import a from \"util\"/.test(y) }}",
            (0..20).map(|i| format!("a{i} ? ")).collect::<String>(),
            (0..20).map(|i| format!(" : {i}")).collect::<String>(),
        ),
    ];
    for source in phantoms {
        assert_eq!(
            imports("typescript", source),
            Vec::<&str>::new(),
            "{source:?}"
        );
    }
    let jsx = [
        // F11: type arguments over lines, with arrows and strings.
        "const el = <Foo<Map<\n A, B>> />; import a from './a';",
        "const el = <Foo<(x: number) => number> />; import a from './a';",
        "const el = <Foo<\">\"> />; import a from './a';",
        // F12: generic arrows with defaults, comments and line breaks.
        "const f = <T = unknown>(x: T) => x; import a from './a';",
        "const f = <T /*c*/,>(x: T) => x; import a from './a';",
        "const f = <T\n extends U>(x: T) => x; import a from './a';",
        "const f = <T // c\n,>(x: T) => x; import a from './a';",
        "const f = <const T = unknown,>(x: T) => x\nimport a from './a'\n",
        "const f = <const T = unknown>(x: T) => x\nimport a from './a'\n",
        // Comments and escapes in type arguments.
        "const e = <Foo<\n  // a > b\n  Item\n> a={1} />\nimport a from './a'\n",
        "const e = <Foo</* > */ string> a={1} />\nimport a from './a'\n",
        "const e = <Foo</* >\n > */ string> a={1} />\nimport a from './a'\n",
        "const e = <Foo<'it\\'s'> a=\"x\" />\nimport a from './a'\n",
        // (A JSX attribute may be named `extends`.)
        "const el = <Foo extends />; import a from './a';",
        "const el = <Foo extends=\"/\" />; import a from './a';",
    ];
    for source in jsx {
        assert_eq!(imports("tsx", source), ["./a"], "{source:?}");
    }
    // F15: a comment in a JSX tag ends at any line terminator.
    for terminator in ["\u{2028}", "\u{2029}", "\r"] {
        let source = format!("const e = <A // c{terminator}/>; import a from './a';");
        assert_eq!(imports("tsx", &source), ["./a"], "{source:?}");
    }
}

#[test]
fn headers_without_bodies_dont_end_the_scan() {
    let cases = [
        // Typed arrow functions in a list, each with a return type.
        "export const api = {\n".to_string()
            + &(0..40)
                .map(|i| format!("  a{i}: (x: number): number => x,\n"))
                .collect::<String>()
            + "}\n",
        "f(\n".to_string()
            + &(0..40)
                .map(|i| format!("  (x): Map<A, B{i}> => x,\n"))
                .collect::<String>()
            + ")\n",
        (0..20)
            .map(|i| format!("global.a{i} = {i}\n"))
            .collect::<String>(),
        (0..20).map(|i| format!("module.a{i} = {i}\n")).collect(),
        (0..20)
            .map(|i| format!("export function f(a: N{i}): void\n"))
            .collect::<String>()
            + "export function f(a) {}\n",
        (0..20)
            .map(|i| format!("declare module '*.e{i}'\n"))
            .collect(),
        (0..20)
            .map(|i| format!("function f{i}(): void {{}}\n"))
            .collect(),
        (0..20)
            .map(|i| format!("x = {{f{i}: function () {{}}}}\n"))
            .collect(),
    ];
    // More conditionals nested than are kept.
    let deep = (0..40).map(|i| format!("a{i} ? ")).collect::<String>()
        + "1"
        + &(0..40).map(|i| format!(" : {i}")).collect::<String>();
    let cases = cases.into_iter().chain([format!("x = {deep}\n")]);
    for source in cases {
        let source = source + "export * from './y'\n";
        assert_eq!(imports("typescript", &source), ["./y"], "{source:?}");
    }
}

#[test]
fn nested_functions_and_classes_keep_the_outer_one_heading_for_its_body() {
    let values = [
        "const f = function(a = function(){}) {} / 2; import a from './a';",
        "const f = function(a = class {}) {} / 2; import a from './a';",
        "const f = function(a = () => function () {}) {} / 2; import a from './a';",
        "const f = function(a = [function () {}]) {} / 2; import a from './a';",
        "const f = function(a = {b: class {}}) {} / 2; import a from './a';",
        "const c = class extends (class {}) {} / 2; import a from './a';",
        "const c = class extends (function () {}) {} / 2; import a from './a';",
        "const c = class extends f(class {}) {} / 2; import a from './a';",
        "const c = class extends class {} {} / 2; import a from './a';",
        "const c = class extends class extends class {} {} {} / 2; import a from './a';",
        "const c = class extends function () {} {} / 2; import a from './a';",
        "const c = class extends {} {} / 2; import a from './a';",
        // Type parameters and annotations aren't bodies.
        "const f = function <T extends {}>() {} / 2; import a from './a';",
        "const f = function (): {a: 1} {} / 2; import a from './a';",
        "const f = function (): A<{a: 1}> {} / 2; import a from './a';",
        "const c = class <T extends {}> {} / 2; import a from './a';",
        "const c = class extends B<{}> {} / 2; import a from './a';",
        // A name that is a keyword, in an object, doesn't head a body.
        "const o = {class: 1, b: {}} / 2; import a from './a';",
        "const o = {function: 1}; const p = {} / 2; import a from './a';",
    ];
    for source in values {
        assert_eq!(imports("typescript", source), ["./a"], "{source:?}");
    }
    let declarations = [
        "function f(a = function(){}) {}\n/;import a from \"util\"/.test(y);",
        "function f(a = class {}) {}\n/;import a from \"util\"/.test(y);",
        "class C extends (class {}) {}\n/;import a from \"util\"/.test(y);",
        "class C extends class {} {}\n/;import a from \"util\"/.test(y);",
        "class C extends function () {} {}\n/;import a from \"util\"/.test(y);",
        "class C<T extends {}> {}\n/;import a from \"util\"/.test(y);",
        "function f<T extends {}>(): {a: 1} {}\n/;import a from \"util\"/.test(y);",
    ];
    for source in declarations {
        assert_eq!(
            imports("typescript", source),
            Vec::<&str>::new(),
            "{source:?}"
        );
    }
}

#[test]
fn named_lists_take_whole_identifiers() {
    let cases: &[(&str, &[&str])] = &[
        ("import { \\u0061 as helper } from './util'", &["./util"]),
        ("export { \\u0061 } from './util'", &["./util"]),
        ("import { \\u{61}b, c\\u0062 } from './util'", &["./util"]),
        ("import { a\u{301} as b\u{301} } from './util'", &["./util"]),
        ("export { a\u{200d}b } from './util'", &["./util"]),
        ("import \\u0061 from './util'", &["./util"]),
        // ID_Start and ID_Continue chars that aren't letters or digits.
        ("import { \u{2118} } from './util'", &["./util"]),
        ("import { a\u{2118}, b\u{b7}c } from './util'", &["./util"]),
        (
            "import { \u{1885} as \u{212e}x } from './util'",
            &["./util"],
        ),
        ("import { $a, _b, a$ } from './util'", &["./util"]),
        // Malformed lists are still given up on.
        ("import { \\u00 } from './util'", &[]),
        ("import { a ; } from './util'", &[]),
        ("export { a; import b from './b'", &["./b"]),
        ("import { a / b } from './util'", &[]),
        // Not identifiers: symbols, punctuation, escapes of them,
        // surrogates, out-of-range escapes, and a start that is a digit.
        ("import { a\u{1f4a5}b } from './util'", &[]),
        ("import { a\u{2014}b } from './util'", &[]),
        ("import { \\u003B } from './util'", &[]),
        ("import { a\\u003Bb } from './util'", &[]),
        ("import { \\ud800 } from './util'", &[]),
        ("import { \\u{110000} } from './util'", &[]),
        ("import { \\u{} } from './util'", &[]),
        ("import { \\u0030a } from './util'", &[]),
        ("import { \u{301}a } from './util'", &[]),
    ];
    for (source, modules) in cases {
        assert_eq!(&imports("typescript", source), modules, "{source:?}");
    }
}

/// Runs `f` with `hook` called as import steps work on this thread.
fn with_progress<R>(hook: impl FnMut(Work) + 'static, f: impl FnOnce() -> R) -> R {
    struct Clear;
    impl Drop for Clear {
        fn drop(&mut self) {
            PROGRESS.with(|hook| *hook.borrow_mut() = None);
        }
    }
    PROGRESS.with(|slot| *slot.borrow_mut() = Some(Box::new(hook)));
    let _clear = Clear;
    f()
}

/// A cancel flag, and a hook that sets it on the `nth` unit (from 0) of
/// work at `site`, counting in `after` the units of work done since.
fn cancel_at(site: Work, nth: usize) -> (Arc<AtomicBool>, Rc<Cell<usize>>, impl FnMut(Work)) {
    let cancel = Arc::new(AtomicBool::new(false));
    let after = Rc::new(Cell::new(0));
    let at_site = Cell::new(0);
    let hook = {
        let (cancel, after) = (cancel.clone(), after.clone());
        move |work| {
            if cancel.load(Ordering::Relaxed) {
                after.set(after.get() + 1);
            }
            if work == site {
                if at_site.get() == nth {
                    cancel.store(true, Ordering::Relaxed);
                }
                at_site.set(at_site.get() + 1);
            }
        }
    };
    (cancel, after, hook)
}

/// Asserts that scanning `source` for imports, cancelled at the `nth`
/// unit of `site`, stops within a poll interval of it.
fn assert_scan_stops(source: &str, site: Work, nth: usize) {
    assert!(import_lines("javascript", source, &AtomicBool::new(false)).is_ok());
    let (cancel, after, hook) = cancel_at(site, nth);
    let answer = with_progress(hook, || import_lines("javascript", source, &cancel));
    assert_eq!(answer, Err(NavError::Cancelled), "{site:?}");
    assert!(cancel.load(Ordering::Relaxed), "{site:?} reached");
    assert!(
        after.get() <= CANCEL_EVERY,
        "{site:?}: {} units after",
        after.get()
    );
}

#[test]
fn a_cancel_in_a_named_list_stops_its_lookahead() {
    let names = (0..8 * CANCEL_EVERY)
        .map(|ix| format!("n{ix}"))
        .collect::<Vec<_>>();
    let source = format!("import {{ {} }} from './util'\n", names.join(", "));
    // Well inside the one statement's list.
    assert_scan_stops(&source, Work::List, 3 * CANCEL_EVERY);
}

#[test]
fn a_cancel_in_the_require_loop_stops_it() {
    let source = "require('./a'); ".repeat(8 * CANCEL_EVERY);
    assert_scan_stops(&source, Work::Require, 10);
}

#[test]
fn a_cancel_while_reading_tokens_stops_the_scan() {
    // One long line, all of it read before any line is looked at.
    let source = "x = a + b; ".repeat(8 * CANCEL_EVERY);
    assert_scan_stops(&source, Work::Token, 3 * CANCEL_EVERY);
}

#[test]
fn a_cancel_while_scanning_lines_stops_the_scan() {
    let source = "import a from './a'\nx();\n".repeat(8 * CANCEL_EVERY);
    assert_scan_stops(&source, Work::Line, 10);
}

/// A project of `files` TypeScript files, each defining `other`.
fn definers(root: &Path, files: usize) -> Arc<RwLock<SymbolIndex>> {
    let mut index = SymbolIndex::new(root);
    index.extend(
        (0..files)
            .map(|ix| FileSymbols {
                rel: PathBuf::from(format!("m{ix}.ts")),
                language: "typescript",
                len: 0,
                modified: None,
                symbols: vec![crate::Symbol {
                    name: "other".into(),
                    kind: OutlineKind::Function,
                    name_range: 0..5,
                    range: 0..5,
                    row: 0,
                    container: None,
                }],
                truncated: false,
            })
            .collect(),
    );
    index.finish();
    Arc::new(RwLock::new(index))
}

fn request_for(root: &Path, imports: &str) -> NavRequest {
    let text = format!("{imports}other();\n");
    NavRequest {
        root: root.to_path_buf(),
        rel: PathBuf::from("main.ts"),
        language: Some("typescript"),
        offset: text.len() - "();\n".len() - 1,
        text: Text::new(&text),
    }
}

/// A navigation over `index` whose request is cancelled by `hook`'s
/// cancel, with a long budget.
fn nav_over(root: &Path, index: Arc<RwLock<SymbolIndex>>) -> SearchNavigation {
    SearchNavigation::new(root, index).with_budget(Duration::from_secs(60))
}

#[test]
fn a_cancel_while_matching_imports_is_answered_within_a_poll() {
    let dir = tempfile::tempdir().unwrap();
    let files = 4 * CANCEL_EVERY;
    let index = definers(dir.path(), files);
    // Many imports, none of which any definition's module is among.
    let imports: String = (0..40)
        .map(|ix| format!("import a{ix} from './z{ix}'\n"))
        .collect();
    let req = request_for(dir.path(), &imports);
    let nav = nav_over(dir.path(), index);
    let uncancelled = nav.definitions(&req, &AtomicBool::new(false)).unwrap();
    assert_eq!(uncancelled.items.len(), MAX_ITEMS);

    let (cancel, after, hook) = cancel_at(Work::Compare, 5);
    let answer = with_progress(hook, || nav.definitions(&req, &cancel));
    assert_eq!(answer, Err(NavError::Cancelled));
    // Stopped soon after the cancel, not after the 160k comparisons.
    assert!(after.get() <= CANCEL_EVERY, "{} units after", after.get());
}

#[test]
fn a_cancel_while_looking_at_definitions_is_answered_within_a_poll() {
    let dir = tempfile::tempdir().unwrap();
    let files = 4 * CANCEL_EVERY;
    let nav = nav_over(dir.path(), definers(dir.path(), files));
    // No imports, so a definition costs no comparison.
    let req = request_for(dir.path(), "");
    let looked = Rc::new(Cell::new(0));
    let (cancel, _, mut hook) = cancel_at(Work::Definition, 5);
    let counter = looked.clone();
    let answer = with_progress(
        move |work| {
            hook(work);
            if work == Work::Definition {
                counter.set(counter.get() + 1);
            }
        },
        || nav.definitions(&req, &cancel),
    );
    assert_eq!(answer, Err(NavError::Cancelled));
    assert!(
        looked.get() <= 5 + CANCEL_EVERY,
        "{} looked at",
        looked.get()
    );
}

#[test]
fn a_cancel_while_scanning_a_request_text_is_answered() {
    let dir = tempfile::tempdir().unwrap();
    let nav = nav_over(dir.path(), definers(dir.path(), 3));
    let imports = "import a from './a'\nconst x = 1\n".repeat(4 * CANCEL_EVERY);
    let req = request_for(dir.path(), &imports);
    let (cancel, after, hook) = cancel_at(Work::Line, 10);
    let answer = with_progress(hook, || nav.definitions(&req, &cancel));
    assert_eq!(answer, Err(NavError::Cancelled));
    assert!(after.get() <= CANCEL_EVERY, "{} units after", after.get());
}

/// A navigation over many definitions that cancels its request when it
/// reaches `at`.
fn cancelling_at(at: Phase, root: &Path) -> (SearchNavigation, Arc<AtomicBool>) {
    let cancel = Arc::new(AtomicBool::new(false));
    let nav = nav_over(root, definers(root, 20)).with_on_phase({
        let cancel = cancel.clone();
        move |phase| {
            if phase == at {
                cancel.store(true, Ordering::Relaxed);
            }
        }
    });
    (nav, cancel)
}

#[test]
fn a_cancel_after_the_checks_before_ranking_is_answered() {
    let dir = tempfile::tempdir().unwrap();
    let req = request_for(dir.path(), "import { other } from './m3'\n");
    // The answer has definitions to rank.
    let (nav, cancel) = cancelling_at(Phase::Fuzzy, dir.path());
    let found = nav.definitions(&req, &cancel).unwrap();
    assert!(!found.items.is_empty());
    // At each step, including after the last check before the answer, which
    // only the final check can see.
    for at in [Phase::Imports, Phase::Rank, Phase::Ranked] {
        let (nav, cancel) = cancelling_at(at, dir.path());
        assert_eq!(
            nav.definitions(&req, &cancel),
            Err(NavError::Cancelled),
            "{at:?}"
        );
        assert!(cancel.load(Ordering::Relaxed), "{at:?} reached");
    }
}
