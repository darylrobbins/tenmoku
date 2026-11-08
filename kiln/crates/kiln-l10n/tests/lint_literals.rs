// SPDX-License-Identifier: Apache-2.0

//! The literal lint's fixtures (layer 2 of section 8): every row of the
//! design's "reported / not reported" table, then each rule's edges.

use kiln_l10n::lint::{Finding, Rule, scan_source};

fn findings(source: &str) -> Vec<Finding> {
    scan_source(source).unwrap_or_else(|e| panic!("fixture doesn't parse: {e}\n{source}"))
}

fn texts(source: &str) -> Vec<String> {
    findings(source).into_iter().map(|f| f.text).collect()
}

#[track_caller]
fn reported(source: &str, text: &str, rule: Rule) {
    let found = findings(source);
    assert!(
        found
            .iter()
            .any(|f| f.text == text && f.rules.contains(&rule)),
        "expected {text:?} under {rule:?}, found {found:#?}\nin:\n{source}"
    );
}

#[track_caller]
fn not_reported(source: &str) {
    let found = findings(source);
    assert!(
        found.is_empty(),
        "expected nothing, found {found:#?}\nin:\n{source}"
    );
}

// Reported, as the table lists them.

#[test]
fn a_capitalised_const_button_array_is_reported() {
    let source = r#"const BUTTONS: [&str; 2] = ["Allow", "Don't Allow"];"#;
    reported(source, "Allow", Rule::R1);
    reported(source, "Don't Allow", Rule::R1);
}

#[test]
fn a_lowercase_label_in_child_is_reported() {
    reported(r#"fn f() { div().child("reload"); }"#, "reload", Rule::R2);
}

#[test]
fn the_same_through_a_const_is_reported() {
    let source = r#"
        const RELOAD: &str = "reload";
        fn f() { div().child(RELOAD); }
    "#;
    reported(source, "reload", Rule::R2);
    let found = findings(source);
    assert_eq!(found.len(), 1, "{found:#?}");
    assert_eq!(found[0].via.as_deref(), Some("`RELOAD`"));
    assert_eq!(found[0].line, 3, "reported where the sink uses it");
}

#[test]
fn toast_info_of_a_const_label_is_reported() {
    let source = r#"
        const LABEL: &str = "Saved";
        fn f() { Toast::info(LABEL); }
    "#;
    let found = findings(source);
    assert!(
        found
            .iter()
            .any(|f| f.text == "Saved" && f.rules.contains(&Rule::R2) && f.line == 3),
        "{found:#?}"
    );
    assert!(
        found
            .iter()
            .any(|f| f.text == "Saved" && f.rules.contains(&Rule::R1) && f.line == 2),
        "{found:#?}"
    );
}

#[test]
fn a_format_string_with_words_is_reported() {
    reported(
        r#"fn f(chars: usize) -> String { format!("({chars} selected)") }"#,
        "({chars} selected)",
        Rule::R3,
    );
}

#[test]
fn eprintln_of_a_cli_error_is_reported() {
    reported(
        r#"fn f(flag: &str) { eprintln!("tenmoku: unknown flag {flag}"); }"#,
        "tenmoku: unknown flag {flag}",
        Rule::R4,
    );
}

#[test]
fn an_error_attribute_is_reported() {
    let source = r#"
        #[derive(thiserror::Error, Debug)]
        enum ExError {
            #[error("E492: Not an editor command: {0}")]
            NotACommand(String),
        }
    "#;
    reported(source, "E492: Not an editor command: {0}", Rule::R1);
    reported(source, "E492: Not an editor command: {0}", Rule::R3);
}

// Not reported, as the table lists them.

#[test]
fn a_log_line_is_not_reported() {
    not_reported(r#"fn f(word: &str) { log::warn!("can't add {word} to the dictionary"); }"#);
    not_reported(r#"fn f() { warn!("Can't add it"); tracing::info!("Started up"); }"#);
}

#[test]
fn a_matcher_on_external_text_is_not_reported() {
    not_reported(r#"fn f(stderr: &str) -> bool { stderr.contains("Authentication failed") }"#);
}

#[test]
fn subprocess_arguments_are_not_reported() {
    not_reported(
        r#"fn f() { Command::new("git").arg("Status").args(["Log Here"]).env("LANG", "C Locale"); }"#,
    );
}

#[test]
fn text_inside_an_impl_debug_is_not_reported() {
    not_reported(
        r#"
        impl fmt::Debug for Thing {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.debug_struct("Thing Here").field("Some Field", &self.0).finish()?;
                write!(f, "Thing with {} items", self.0.len())
            }
        }
    "#,
    );
}

#[test]
fn a_serde_rename_is_not_reported() {
    not_reported(
        r#"
        #[derive(Deserialize)]
        struct Settings {
            #[serde(rename = "Foo")]
            foo: bool,
        }
    "#,
    );
}

#[test]
fn verbatim_is_not_reported() {
    not_reported(
        r#"fn f() { div().child(verbatim!("Cargo.toml")); let _ = verbatim!("Read Me"); }"#,
    );
}

// Each rule's edges.

#[test]
fn r1_reports_spaced_or_capitalised_text_anywhere() {
    assert_eq!(
        texts(
            r#"fn f() { let a = "two words"; let b = "Title"; let c = "id"; let d = "ID"; let e = "a.b"; let g = "3 4"; }"#
        ),
        ["two words", "Title"]
    );
}

#[test]
fn r2_reports_any_word_in_a_sink_through_branches_and_conversions() {
    let source = r#"
        fn f(on: bool) {
            div().tooltip(if on { "on" } else { "off" });
            div().children(["left", "right"]);
            div().aria_label("close".to_string());
            input().placeholder(Some("search"));
            Button::new("ok");
            PromptButton::cancel("no");
            Picker::new(d, "files", t, cx);
            SharedString::from("label");
            SharedString::new_static("static");
            registry.register("app.quit", "quit");
            window.prompt(level, "question", None, &[], cx);
            StatusItem { text: "ln", tooltip: Some("col".into()) };
            div().child("3");
        }
    "#;
    let found = texts(source);
    for text in [
        "on", "off", "left", "right", "close", "search", "ok", "no", "files", "label", "static",
        "app.quit", "quit", "question", "ln", "col",
    ] {
        assert!(
            found.contains(&text.to_string()),
            "{text:?} missing from {found:?}"
        );
    }
    assert!(!found.contains(&"3".to_string()), "no letter, not reported");
}

#[test]
fn r2_follows_a_local_bound_to_a_literal() {
    let source = r#"
        fn f() {
            let label = "reload";
            div().child(label);
        }
        fn g(label: &str) { div().child(label); }
    "#;
    let found = findings(source);
    assert_eq!(found.len(), 1, "{found:#?}");
    assert_eq!(found[0].via.as_deref(), Some("local `label`"));
    assert_eq!(found[0].line, 4);
}

#[test]
fn r2_doesnt_report_literals_nested_in_other_calls() {
    not_reported(
        r#"fn f() { div().child(Icon::new("chevron-right")); div().child(tr!("kiln-ui-close")); }"#,
    );
}

#[test]
fn r3_reports_words_outside_placeholders_only() {
    let source = r#"
        fn f(n: usize, register: char) -> String {
            let a = format!("recording @{register}");
            let b = format!("{n}:{}", n);
            let c = format!("{{{n}}}");
            let d = format!("{n:>width$}", width = 4);
            let e = write!(out, "{n}ms");
            let g = format_args!("{n}x");
            String::new()
        }
    "#;
    assert_eq!(texts(source), ["recording @{register}", "{n}ms"]);
}

#[test]
fn r4_reports_printed_words() {
    let source = r#"fn f() { println!("usage: tenmoku [file]"); print!("{}", 1); eprint!("x"); }"#;
    assert_eq!(texts(source), ["usage: tenmoku [file]"]);
}

#[test]
fn panics_assertions_and_expectations_are_not_reported() {
    not_reported(
        r#"
        fn f(x: Option<u8>) {
            x.expect("Value must be set");
            assert!(true, "Never fails here");
            assert_eq!(1, 1, "Equal values");
            debug_assert!(true, "Still true");
            unreachable!("Can't get here");
            panic!("Bad state");
            let _ = concat!("Two ", "Words");
            let _ = env!("CARGO_PKG_NAME");
            let _ = include_str!("Some File.txt");
            let _ = matches!(x, Some(_));
        }
    "#,
    );
}

#[test]
fn patterns_and_comparisons_are_not_reported() {
    not_reported(
        r#"
        fn f(s: &str) -> u8 {
            if s == "Hello World" || "Other Thing" != s { return 0; }
            if let "Some Text" = s { return 1; }
            match s {
                "Java" | "Dockerfile" => 2,
                _ => 3,
            }
        }
    "#,
    );
    not_reported(
        r#"fn f(s: &str) { s.starts_with("Error:"); s.ends_with(" Done"); s.strip_prefix("Re: "); s.find("Some Word"); }"#,
    );
}

#[test]
fn keys_and_tr_or_keys_are_not_reported_but_their_arguments_are() {
    not_reported(
        r#"fn f(p: &str) { tr!("workbench-git-pull"); text!("kiln-ui-cancel", path = p); tr_or("settings-x", p); }"#,
    );
    reported(
        r#"fn f() { tr!("workbench-open", path = "Read Me"); }"#,
        "Read Me",
        Rule::R1,
    );
}

#[test]
fn test_code_and_doc_comments_are_not_reported() {
    not_reported(
        r#"
        /// Shows "Some Text" in the bar.
        pub fn f() {}

        #[cfg(test)]
        mod tests {
            #[test]
            fn shows() { div().child("Hello World"); }
        }

        #[test]
        fn alone() { let _ = "Hello World"; }

        #[cfg(all(test, unix))]
        fn helper() { let _ = "Hello World"; }
    "#,
    );
    // `any(test, …)` can hold outside tests, so it is checked.
    reported(
        r#"#[cfg(any(test, feature = "x"))] fn f() { let _ = "Hello World"; }"#,
        "Hello World",
        Rule::R1,
    );
}

#[test]
fn other_macros_are_checked_as_expressions() {
    let source =
        r#"fn f() { let v = vec![div().child("inner")]; let w = some_macro!{ "Bare Text" ; x }; }"#;
    let found = texts(source);
    assert!(found.contains(&"inner".to_string()), "{found:?}");
    assert!(found.contains(&"Bare Text".to_string()), "{found:?}");
}

#[test]
fn a_finding_reads_as_file_line_column_rule_and_text() {
    let found = findings("fn f() {\n    div().child(\"Hello World\");\n}\n");
    assert_eq!(found.len(), 1);
    assert_eq!(
        found[0].to_string(),
        r#"src/lib.rs:2:17: R1+R2 "Hello World""#
    );
}
