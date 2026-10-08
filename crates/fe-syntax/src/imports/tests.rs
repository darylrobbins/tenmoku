use fe_buffer::Buffer;

use crate::{
    ImportsError, Syntax, language_by_name, module_specifiers, module_specifiers_supported,
};

thread_local! {
    /// How many `arguments` nodes `is_module` has validated on this thread.
    pub(super) static ARGUMENT_VALIDATIONS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

fn syntax(language: &str, text: &str) -> Syntax {
    let buffer = Buffer::new(text);
    let mut syntax = Syntax::new(language_by_name(language).unwrap(), buffer.current());
    syntax.finish(syntax.parse_job().run());
    syntax
}

fn specifiers(language: &str, text: &str) -> Vec<String> {
    let syntax = syntax(language, text);
    let found = module_specifiers(&syntax, &mut || false).unwrap();
    found.into_iter().map(|r| text[r].to_string()).collect()
}

#[test]
fn specifiers_of_imports_exports_and_requires() {
    let shared = "import a from 'a';\nimport 'b';\nexport { c } from \"c\";\n\
                  export * as n from 'n';\nconst d = require('d');\nrequire('e');\n";
    let ignored = "import('x');\nimport.meta;\nrequire(y);\nrequire(`z`);\n\
                   // require('c1')\n/* import q from 'c2' */\nconst s = \"import q from 'c3'\";\n\
                   const t = `require('c4')`;\nconst r = /import a from 'c5'/;\n";
    let expected = ["a", "b", "c", "n", "d", "e"];
    for language in ["javascript", "typescript", "tsx"] {
        assert_eq!(specifiers(language, shared), expected, "{language}");
        assert_eq!(
            specifiers(language, ignored),
            Vec::<String>::new(),
            "{language}"
        );
    }
    let types = "import type { T } from 't';\nexport type { U } from 'u';\n\
                 import V = require('v');\nrequire('w');\n";
    for language in ["typescript", "tsx"] {
        assert_eq!(
            specifiers(language, types),
            ["t", "u", "v", "w"],
            "{language}"
        );
    }
    assert_eq!(
        specifiers(
            "tsx",
            "const e = <p>import a from 'jsx'</p>;\nimport b from 'b';"
        ),
        ["b"]
    );
    assert_eq!(
        specifiers("javascript", "const e = <p>import a from 'jsx'</p>;"),
        Vec::<String>::new()
    );
    // Escapes stay as written.
    assert_eq!(
        specifiers("javascript", r"import a from 'a\'b';"),
        [r"a\'b"]
    );

    assert!(module_specifiers_supported("tsx"));
    assert!(!module_specifiers_supported("rust"));
    let rust = syntax("rust", "fn main() {}");
    assert_eq!(
        module_specifiers(&rust, &mut || false),
        Err(ImportsError::Unsupported)
    );
    let buffer = Buffer::new("import a from 'a';");
    let unparsed = Syntax::new(language_by_name("javascript").unwrap(), buffer.current());
    assert_eq!(
        module_specifiers(&unparsed, &mut || false),
        Err(ImportsError::NotParsed)
    );
}

#[test]
fn a_cancel_stops_the_query() {
    let text = "require('m');\n".repeat(8 * 1024);
    let syntax = syntax("javascript", &text);
    let mut polls = 0;
    let result = module_specifiers(&syntax, &mut || {
        polls += 1;
        polls > 3
    });
    assert_eq!(result, Err(ImportsError::Cancelled));
    assert!(polls > 1, "polled {polls} times");
    assert!(polls < 8 * 1024, "polled {polls} times");
    // Never cancelled, every call is found.
    assert_eq!(
        module_specifiers(&syntax, &mut || false).unwrap().len(),
        8 * 1024
    );
}

#[test]
fn recovered_and_commented_forms() {
    let sources: &[(&str, &[&str])] = &[
        ("require(/* c */ 'm')", &["m"]),
        ("require('m' /* c */)", &["m"]),
        ("require(/* a */ 'm' // b\n)", &["m"]),
        ("require('m', n)", &[]),
        ("require(/* c */ n)", &[]),
        ("export {\n  w,\n}\nfrom './w'\nx", &["./w"]),
        ("export\n{ w } from './w'\nx", &["./w"]),
        (
            "export default class {}\n/;import a from \"util\"/.test(y);",
            &[],
        ),
        (
            "export default function () {}\n/;import a from \"util\"/.test(y);",
            &[],
        ),
        ("export default class {}\nimport a from 'b'", &["b"]),
        // Independent statements are not a re-export.
        (
            "const w = 1, from = 2; export { w }; from; './phantom';",
            &[],
        ),
        ("export { w };\nfrom\n'./phantom';", &[]),
        ("export { w }\nfrom;\n'./phantom';", &[]),
        (
            "export {\n w,\n} /* a */\n// b\nfrom /* c */\n'./w'",
            &["./w"],
        ),
        ("export /* a */\n{ w } /* b */\nfrom './w';", &["./w"]),
    ];
    for language in ["javascript", "typescript", "tsx"] {
        for (text, expected) in sources {
            assert_eq!(specifiers(language, text), *expected, "{language} {text:?}");
        }
    }
}

#[test]
fn a_jsx_attribute_named_extends_keeps_later_imports() {
    let text = "const el = <Foo extends=\"/\" />; import a from './a';\nimport b from './b';\n";
    assert_eq!(specifiers("tsx", text), ["./a", "./b"]);
    // Several, each with its own misread.
    let several = "const x = <A extends=\"/\" />; import a from './a';\n".repeat(3);
    assert_eq!(specifiers("tsx", &several), ["./a", "./a", "./a"]);
    assert_eq!(
        specifiers("tsx", "const el = <Foo extends />; import a from './a';"),
        ["./a"]
    );
}

#[test]
fn a_call_of_many_strings_is_read_once() {
    let text = format!("require({});\n", vec!["'m'"; 20_000].join(", "));
    // Each `arguments` node is validated once, not once per string.
    for language in ["javascript", "typescript", "tsx"] {
        let before = ARGUMENT_VALIDATIONS.with(|count| count.get());
        assert_eq!(specifiers(language, &text), Vec::<String>::new());
        let validated = ARGUMENT_VALIDATIONS.with(|count| count.get()) - before;
        assert_eq!(validated, 1, "{language}");
    }
}

#[test]
fn a_call_of_many_comments_and_strings_is_read_once() {
    let text = format!(
        "require({}'m', {});\n",
        "/* c */ ".repeat(40_000),
        vec!["'m'"; 40_000].join(", ")
    );
    let started = std::time::Instant::now();
    assert_eq!(specifiers("tsx", &text), Vec::<String>::new());
    assert!(started.elapsed() < std::time::Duration::from_secs(10));
}

#[test]
fn many_jsx_attributes_named_extends_keep_the_last_import() {
    let mut text = String::new();
    for i in 0..40 {
        text.push_str(&format!("const x{i} = <A extends=\"/\" />; "));
    }
    text.push_str("import a from './a';\n");
    assert_eq!(specifiers("tsx", &text), ["./a"]);
}

#[test]
fn a_great_many_jsx_attributes_named_extends_are_read_in_linear_time() {
    let n = 20_000;
    let mut text = "const x = <A extends=\"/\" />;\n".repeat(n);
    text.push_str("import a from './a';\n");
    let started = std::time::Instant::now();
    assert_eq!(specifiers("tsx", &text), ["./a"]);
    assert!(started.elapsed() < std::time::Duration::from_secs(10));
}

#[test]
fn a_cancel_stops_the_recovery() {
    let mut text = "const x = <A extends=\"/\" />;\n".repeat(2_000);
    text.push_str("import a from './a';\n");
    let syntax = syntax("tsx", &text);
    assert_eq!(module_specifiers(&syntax, &mut || false).unwrap().len(), 1);
    let mut polls = 0;
    let result = module_specifiers(&syntax, &mut || {
        polls += 1;
        polls > 2
    });
    assert_eq!(result, Err(ImportsError::Cancelled));
}
