//! Rewrites reindent captures that span several lines.

use fe_structural::{MatchOptions, Pattern, Rewrite, apply_edits};

fn language(name: &str) -> &'static fe_syntax::Language {
    fe_syntax::language_by_name(name).expect("a default language of fe-syntax")
}

/// `source` rewritten with `pattern` and `template` in `language`.
fn rewritten(language_name: &str, pattern: &str, template: &str, source: &str) -> String {
    let language = language(language_name);
    let pattern = Pattern::compile(language, pattern).unwrap();
    let rewrite = Rewrite::compile(&pattern, template).unwrap();
    let tree = fe_syntax::parse_str(language, source, || false).unwrap();
    let edits = pattern
        .edits(
            &rewrite,
            source,
            &tree,
            &MatchOptions::default(),
            &mut || false,
        )
        .unwrap();
    assert!(edits.edits.iter().all(|e| !e.flagged), "{edits:?}");
    apply_edits(source, &edits.edits)
}

#[test]
fn multiline_capture_is_reindented_to_the_match_indentation() {
    // Spaces: the closure starts a line deeper than the call, so its later
    // lines lose that line's indent (8) and take the call's (4).
    let source = "fn f() {\n    call(\n        || {\n            a();\n        }\n    );\n}\n";
    assert_eq!(
        rewritten("rust", "call($A)", "run($A)", source),
        "fn f() {\n    run(|| {\n        a();\n    });\n}\n"
    );
    // Tabs, the same.
    let source = "fn f() {\n\tcall(\n\t\t|| {\n\t\t\ta();\n\t\t}\n\t);\n}\n";
    assert_eq!(
        rewritten("rust", "call($A)", "run($A)", source),
        "fn f() {\n\trun(|| {\n\t\ta();\n\t});\n}\n"
    );
    // Tabs in the capture, spaces at the match: the base indent is taken off
    // as the file spells it, deeper indents kept as they are.
    let source = "fn f() {\n    call(\n\t|| {\n\t\ta();\n\t}\n    );\n}\n";
    assert_eq!(
        rewritten("rust", "call($A)", "run($A)", source),
        "fn f() {\n    run(|| {\n    \ta();\n    });\n}\n"
    );
}

#[test]
fn a_capture_starting_on_the_match_line_keeps_its_lines() {
    let source = "fn f() {\n    let x = call(|| {\n        a();\n\n    });\n}\n";
    assert_eq!(
        rewritten("rust", "call($A)", "run($A)", source),
        "fn f() {\n    let x = run(|| {\n        a();\n\n    });\n}\n"
    );
}

#[test]
fn template_text_is_kept_and_a_capture_takes_its_template_line_s_indent() {
    // The template's later lines stay as written; the capture's later lines
    // take the indent of the template line it stands on (4), not the
    // closure's in the file (12).
    let source = "fn f() {\n    if x {\n        call(\n            || {\n                a();\n            }\n        );\n    }\n}\n";
    let template = "{\n    let g = $A;\n\n    g()\n}";
    assert_eq!(
        rewritten("rust", "call($A)", template, source),
        "fn f() {\n    if x {\n        {\n    let g = || {\n        a();\n    };\n\n    g()\n};\n    }\n}\n"
    );
}

#[test]
fn a_multiline_string_in_the_template_is_not_reindented() {
    // T711 F1: literal text is substituted into, never changed.
    let source = "fn f() {\n        call(x);\n}\n";
    let template = "log(\"one\n  two\n\n\", $A)";
    assert_eq!(
        rewritten("rust", "call($A)", template, source),
        "fn f() {\n        log(\"one\n  two\n\n\", x);\n}\n"
    );
}

#[test]
fn whitespace_before_a_capture_on_a_later_template_line_is_kept() {
    // T711 F2: the spaces ending a literal part belong to the capture's line.
    let source =
        "fn f() {\n    call(x);\n    call(\n        || {\n            a();\n        }\n    );\n}\n";
    let template = "{\n    $A\n}";
    assert_eq!(
        rewritten("rust", "call($A)", template, source),
        "fn f() {\n    {\n    x\n};\n    {\n    || {\n        a();\n    }\n};\n}\n"
    );
    // Python, where the indent is the syntax.
    let source = "def f():\n    call(x)\n";
    assert_eq!(
        rewritten("python", "call($A)", "g(\n    $A\n)", source),
        "def f():\n    g(\n    x\n)\n"
    );
}

#[test]
fn a_python_block_moves_to_its_new_depth() {
    let source = "def f():\n    with lock:\n        run(\n            [\n                1,\n                2,\n            ]\n        )\n";
    assert_eq!(
        rewritten("python", "run($A)", "go($A)", source),
        "def f():\n    with lock:\n        go([\n            1,\n            2,\n        ])\n"
    );
}
