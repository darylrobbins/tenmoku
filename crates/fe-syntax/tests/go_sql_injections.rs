//! Go strings that read as a SQL statement inject the sql grammar, by content.

#![cfg(feature = "go")]

use tree_sitter::{Parser, Query, QueryCursor, StreamingIterator};

fn injected(go_src: &str) -> Vec<(String, String)> {
    let language: tree_sitter::Language = tree_sitter_go::LANGUAGE.into();
    let query = Query::new(&language, include_str!("../queries/go-injections.scm")).unwrap();
    let mut parser = Parser::new();
    parser.set_language(&language).unwrap();
    let tree = parser.parse(go_src, None).unwrap();
    let content = query.capture_index_for_name("injection.content").unwrap();
    let mut cursor = QueryCursor::new();
    let mut matches = cursor.matches(&query, tree.root_node(), go_src.as_bytes());
    let mut found = Vec::new();
    while let Some(m) = matches.next() {
        let lang = query
            .property_settings(m.pattern_index)
            .iter()
            .find(|p| &*p.key == "injection.language")
            .and_then(|p| p.value.as_deref())
            .unwrap_or("")
            .to_string();
        for capture in m.captures().iter().filter(|c| c.index == content) {
            let text = capture.node.utf8_text(go_src.as_bytes()).unwrap();
            found.push((text.to_string(), lang.clone()));
        }
    }
    found.sort();
    found
}

fn in_func(body: &str) -> String {
    format!("package p\nfunc f() {{\n\t{body}\n}}\n")
}

fn sql_only(found: &[(String, String)]) -> Vec<&str> {
    found
        .iter()
        .filter(|(_, l)| l == "sql")
        .map(|(t, _)| t.as_str())
        .collect()
}

const INSERT: &str = "\n\tINSERT INTO books (title, category) VALUES\n\t\t('Dune', 'Science Fiction'),\n\t\t('The Go Programming Language', 'Programming'),\n\t\t('Stew', 'Cookery'),\n\t\t('Uncategorised', '')";
const SELECT: &str =
    "\n\tSELECT c.name FROM books b JOIN categories c ON c.id = b.category_id\n\tWHERE b.title = ?";

fn screenshot() -> String {
    format!(
        "package p\nfunc f() {{\n\tif _, err := database.Exec(`{INSERT}`); err != nil {{\n\t\tt.Fatalf(\"seed pre-migration books: %v\", err)\n\t}}\n\tvar got string\n\terr := database.Get(&got, `{SELECT}`, tc.title)\n}}\n"
    )
}

#[test]
fn screenshot_statements_inject_sql() {
    let found = injected(&screenshot());
    let sql: Vec<_> = found.iter().filter(|(_, l)| l == "sql").collect();
    assert_eq!(sql.len(), 2, "{found:?}");
    let texts: Vec<&str> = sql.iter().map(|(t, _)| t.as_str()).collect();
    assert!(
        texts.contains(&INSERT) && texts.contains(&SELECT),
        "{found:?}"
    );
    // The Fatalf format string stays printf.
    assert!(
        found.contains(&("seed pre-migration books: %v".into(), "printf".into())),
        "{found:?}"
    );
}

const STATEMENTS: &[&str] = &[
    "SELECT a,\n\tb\nFROM t\nWHERE x = 1",
    "INSERT INTO t (a) VALUES (1)",
    "UPDATE t SET a = 1",
    "DELETE FROM t",
    "WITH x AS (SELECT 1) SELECT * FROM x",
    "WITH RECURSIVE x(n) AS (SELECT 1) SELECT n FROM x",
    "CREATE TABLE t (a int)",
    "CREATE UNIQUE INDEX i ON t (a)",
    "ALTER TABLE t ADD COLUMN b int",
    "DROP VIEW v",
    "SELECT 1",
    "SELECT count(*)",
    "select id from t",
    "-- name: GetUser :one\nSELECT id FROM users WHERE id = $1",
];

#[test]
fn every_statement_form_injects() {
    for stmt in STATEMENTS {
        let escape_free = !stmt.contains('\n');
        let mut forms = vec![format!("`{stmt}`")];
        if escape_free {
            forms.push(format!("\"{stmt}\""));
        }
        for q in forms {
            let want = vec![(q[1..q.len() - 1].to_string(), "sql".to_string())];
            let positions = [
                format!("db.Exec({q})"),
                format!("db.QueryRowContext(ctx, {q}, id)"),
                format!("db.Get(&x, {q}, a)"),
                format!("conn.Query(ctx, {q})"),
                format!("const c = {q}"),
                format!("var v = {q}"),
                format!("v := {q}"),
                format!("v = {q}"),
                format!("_ = T{{Query: {q}}}"),
                format!("_ = []string{{{q}}}"),
            ];
            for body in positions {
                assert_eq!(injected(&in_func(&body)), want, "{body}");
            }
            let ret = format!("package p\nfunc f() string {{\n\treturn {q}\n}}\n");
            assert_eq!(injected(&ret), want, "return {q}");
        }
    }
}

#[test]
fn english_strings_stay_plain() {
    for s in [
        "update the cache",
        "delete failed",
        "select a file",
        "Select a file from the list",
        "Update the cache and set the flag",
        "insert coin",
        "with care as always",
        "drop it",
        "Create table later",
    ] {
        for body in [
            format!("log.Println(\"{s}\")"),
            format!("errors.New(\"{s}\")"),
            format!("const c = \"{s}\""),
        ] {
            let f = injected(&in_func(&body));
            assert!(f.is_empty(), "{body}: {f:?}");
        }
    }
}

#[test]
fn printf_format_strings_stay_printf() {
    for (call, text) in [
        (
            "fmt.Sprintf(\"SELECT * FROM %s WHERE id = ?\", t)",
            "SELECT * FROM %s WHERE id = ?",
        ),
        ("fmt.Sprintf(`SELECT a FROM %s`, t)", "SELECT a FROM %s"),
        ("fmt.Fprintf(w, \"DELETE FROM %s\", t)", "DELETE FROM %s"),
    ] {
        let found = injected(&in_func(call));
        assert_eq!(
            found,
            vec![(text.to_string(), "printf".to_string())],
            "{call}"
        );
        assert!(sql_only(&found).is_empty());
    }
}

#[test]
fn escaped_strings_stay_plain() {
    for body in [
        "db.Exec(\"SELECT a\\nFROM t\")",
        "v := \"SELECT a\\nFROM t\"",
        "v := \"SELECT a \" + \"FROM t\"",
        "db.Exec(\"SELECT a \" + \"FROM t\")",
    ] {
        assert!(injected(&in_func(body)).is_empty(), "{body}");
    }
}

const NAMED: &str = "\n\t\t\tINSERT INTO books (\n\t\t\t\ttitle, isbn10, isbn13, publisher, published_year, format, series,\n\t\t\t\tseries_index, page_count, cover_path, reading_status,\n\t\t\t\tlending_status, condition, rating, started_at, finished_at, notes,\n\t\t\t\tlocation_id, language, edition, acquired_source, acquired_date, acquired_price\n\t\t\t) VALUES (\n\t\t\t\t:title, :isbn10, :isbn13, :publisher, :published_year, :format, :series,\n\t\t\t\t:series_index, :page_count, :cover_path, :reading_status,\n\t\t\t\t:lending_status, :condition, :rating, :started_at, :finished_at, :notes,\n\t\t\t\t:location_id, :language, :edition, :acquired_source, :acquired_date, :acquired_price\n\t\t\t)";

fn named_fixture() -> String {
    format!(
        "package p\n\nfunc (r *Repo) Create(ctx context.Context, b *Book) error {{\n\tif b.LendingStatus == LendingOnLoan {{\n\t\treturn fmt.Errorf(\"cannot create a book with lending status %q: a create has no patron to lend it to\", LendingOnLoan)\n\t}}\n\treturn r.inTx(ctx, func(tx *sqlx.Tx) error {{\n\t\tres, err := tx.NamedExecContext(ctx, `{NAMED}`, b)\n\t\tif err != nil {{\n\t\t\treturn fmt.Errorf(\"insert book: %w\", err)\n\t\t}}\n\t\treturn nil\n\t}})\n}}\n"
    )
}

#[test]
fn named_parameter_fixture_injects_sql_only_for_the_statement() {
    let found = injected(&named_fixture());
    assert_eq!(sql_only(&found), vec![NAMED], "{found:?}");
    assert_eq!(found.len(), 3, "{found:?}");
    assert!(
        found.contains(&("insert book: %w".into(), "printf".into())),
        "{found:?}"
    );
}

#[cfg(feature = "sql")]
mod highlighted {
    use super::*;
    use fe_buffer::Buffer;
    use fe_syntax::{Syntax, language_by_name};

    fn spans(text: &str) -> Vec<(String, &'static str)> {
        let mut syntax = Syntax::new(language_by_name("go").unwrap(), Buffer::new(text).current());
        syntax.finish(syntax.parse_job().run());
        loop {
            let spans = syntax.highlights(0..text.len());
            let missing = syntax.take_missing_languages();
            if missing.is_empty() {
                return spans
                    .into_iter()
                    .map(|span| (text[span.range].to_string(), span.highlight.name()))
                    .collect();
            }
            for name in missing {
                language_by_name(name);
            }
        }
    }

    fn has(found: &[(String, &'static str)], text: &str, highlight: &str) -> bool {
        found.iter().any(|(t, h)| t == text && *h == highlight)
    }

    #[test]
    fn screenshot_statements_highlight_as_sql() {
        let found = spans(&screenshot());
        for kw in [
            "INSERT", "INTO", "VALUES", "SELECT", "FROM", "JOIN", "ON", "WHERE",
        ] {
            assert!(has(&found, kw, "keyword"), "{kw}: {found:?}");
        }
        assert!(has(&found, "?", "variable.parameter"), "{found:?}");
        assert!(
            found
                .iter()
                .any(|(t, h)| t == "'Dune'" && h.starts_with("string")),
            "{found:?}"
        );
        let ann = spans("package p\nfunc f() {\n\tdb.Exec(`INSERT INTO t VALUES ('Ann')`)\n}\n");
        assert!(
            ann.iter()
                .any(|(t, h)| t == "'Ann'" && h.starts_with("string")),
            "{ann:?}"
        );
        for go in ["database", "Exec", "Get"] {
            assert!(
                found.iter().any(|(t, h)| t == go && *h != "string"),
                "{go}: {found:?}"
            );
        }
        assert!(
            found.iter().any(|(t, h)| t == "title" && *h != "string"),
            "tc.title: {found:?}"
        );
    }

    #[test]
    fn placeholders_keep_the_statement_highlighted() {
        for (tail, param) in [
            ("id = ? AND x = ?", Some("?")),
            ("id = $1 AND x = $2", Some("$1")),
            ("id = @p1 AND x = @name", None),
            ("id = :id AND x = :name", None),
        ] {
            let src =
                format!("package p\nfunc f() {{\n\tdb.Exec(`SELECT * FROM t WHERE {tail}`)\n}}\n");
            let found = spans(&src);
            for kw in ["SELECT", "FROM", "WHERE", "AND"] {
                assert!(has(&found, kw, "keyword"), "{tail}: {kw}: {found:?}");
            }
            if let Some(param) = param {
                assert!(
                    has(&found, param, "variable.parameter"),
                    "{tail}: {found:?}"
                );
            }
        }
    }

    #[test]
    fn named_parameters_do_not_break_the_statement() {
        let found = spans(&named_fixture());
        for kw in ["INSERT", "INTO", "VALUES"] {
            assert!(has(&found, kw, "keyword"), "{kw}: {found:?}");
        }
        // Each :name colours as its name token; sequel's own keywords
        // (format, language) colour as keywords, as in a .sql file.
        let values = NAMED.split("VALUES").nth(1).unwrap();
        let names: Vec<&str> = values
            .split(|c: char| !(c.is_alphanumeric() || c == '_' || c == ':'))
            .filter_map(|w| w.strip_prefix(':'))
            .collect();
        assert_eq!(names.len(), 23);
        let tail = &found[found.iter().position(|(t, _)| t == "VALUES").unwrap()..];
        for name in names {
            let h = if ["format", "language"].contains(&name) {
                "keyword"
            } else {
                "property"
            };
            assert!(has(tail, name, h), "{name}: {tail:?}");
        }
        // No parameter falls into an error run: the only brackets and
        // commas after VALUES are punctuation.
        assert!(
            !tail
                .iter()
                .any(|(t, h)| t.contains(':') && t.contains(',') && *h != "string"),
            "{tail:?}"
        );
    }
}

#[cfg(not(feature = "sql"))]
mod without_sql {
    use fe_buffer::Buffer;
    use fe_syntax::{Syntax, language_by_name};

    #[test]
    fn sql_strings_stay_strings_without_sql() {
        let text = super::screenshot();
        let mut syntax = Syntax::new(
            language_by_name("go").unwrap(),
            Buffer::new(&text).current(),
        );
        syntax.finish(syntax.parse_job().run());
        // Printf (the Fatalf format) loads on demand; sql is never asked for.
        let spans = loop {
            let spans = syntax.highlights(0..text.len());
            let missing = syntax.take_missing_languages();
            if missing.is_empty() {
                break spans;
            }
            assert!(!missing.contains(&"sql"), "{missing:?}");
            for name in missing {
                language_by_name(name);
            }
        };
        for raw in [super::INSERT, super::SELECT] {
            let at = text.find(raw).unwrap();
            let covering: Vec<_> = spans
                .iter()
                .filter(|s| s.range.start < at + raw.len() && s.range.end > at)
                .collect();
            assert_eq!(covering.len(), 1, "{raw}: {covering:?}");
            assert_eq!(covering[0].highlight.name(), "string");
            assert!(covering[0].range.start <= at && covering[0].range.end >= at + raw.len());
        }
    }
}
