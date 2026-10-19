//! Third-party provenance: THIRD_PARTY.md rows and file headers agree, every
//! highlight, outline and imports query says where it came from, and nothing
//! is sourced from the editor's own crates beyond GPUI (CLEAN_ROOM.md).
//!
//! The checks are functions over `(path, text)` pairs, run over the
//! repository and over in-memory fixtures.

use std::collections::BTreeMap;
use std::path::Path;

/// Repository path (with `/`) to the file's text; `.rs` and `.scm` files
/// under `crates/` and `kiln/`.
type Files = BTreeMap<String, String>;

/// A THIRD_PARTY.md and the directory its paths are relative to ("" or
/// "kiln/").
struct Table<'a> {
    prefix: &'a str,
    text: &'a str,
}

const HEADER_LINES: usize = 15;
const QUERY_HEADER_LINES: usize = 5;

fn repo_root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("crates/fastedit is two levels below the repository root")
}

/// Adds every `.rs`, `.scm` and `.toml` file under `dir` to `files`; whatever can't be
/// read is named in `errors`, so the checks never pass over it silently.
fn collect(dir: &Path, root: &Path, files: &mut Files, errors: &mut Vec<String>) {
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(e) => {
            errors.push(format!("{}: can't read the directory: {e}", dir.display()));
            return;
        }
    };
    for entry in entries {
        let entry = match entry {
            Ok(entry) => entry,
            Err(e) => {
                errors.push(format!("{}: can't read an entry: {e}", dir.display()));
                continue;
            }
        };
        let path = entry.path();
        let name = entry.file_name();
        if path.is_dir() {
            if name != "target" && name != ".git" && name != "node_modules" {
                collect(&path, root, files, errors);
            }
        } else if matches!(
            path.extension().and_then(|e| e.to_str()),
            Some("rs" | "scm" | "toml")
        ) {
            match std::fs::read_to_string(&path) {
                Ok(text) => {
                    let rel = path.strip_prefix(root).unwrap();
                    let rel: Vec<_> = rel.iter().map(|p| p.to_string_lossy()).collect();
                    files.insert(rel.join("/"), text);
                }
                Err(e) => errors.push(format!("{}: can't read the file: {e}", path.display())),
            }
        }
    }
}

/// The repository's files; panics if any can't be read or too few are found.
fn repo_files() -> Files {
    let root = repo_root();
    let mut files = Files::new();
    let mut errors = Vec::new();
    collect(&root.join("crates"), root, &mut files, &mut errors);
    collect(&root.join("kiln"), root, &mut files, &mut errors);
    assert!(errors.is_empty(), "\n{}", errors.join("\n"));
    assert!(files.len() > 100, "found only {} files", files.len());
    files
}

fn first_lines(text: &str, n: usize) -> String {
    text.lines().take(n).collect::<Vec<_>>().join("\n")
}

/// A header's text without comment markers, on one line, so a phrase wrapped
/// across lines still matches.
fn flatten(header: &str) -> String {
    let words: Vec<&str> = header
        .lines()
        .flat_map(|line| {
            line.trim_start_matches(|c: char| c.is_whitespace() || matches!(c, '/' | '!' | ';'))
                .split_whitespace()
        })
        .collect();
    words.join(" ")
}

fn is_word_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

/// Whether `word` occurs in `text` as a whole word, any case.
fn has_word(text: &str, word: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    let word = word.to_ascii_lowercase();
    let bytes = lower.as_bytes();
    lower.match_indices(&word).any(|(i, _)| {
        let before = i == 0 || !is_word_byte(bytes[i - 1]);
        let after = bytes.get(i + word.len()).is_none_or(|b| !is_word_byte(*b));
        before && after
    })
}

struct Row {
    path: String,
    commit: String,
    license: String,
}

fn backticked(cell: &str) -> Vec<&str> {
    cell.split('`').skip(1).step_by(2).collect()
}

/// The rows of the tables under the "Code copied or adapted" heading.
fn rows(md: &str) -> Vec<Row> {
    let mut rows = Vec::new();
    let mut in_section = false;
    for line in md.lines() {
        if let Some(heading) = line.strip_prefix("## ") {
            in_section = heading.starts_with("Code copied or adapted");
            continue;
        }
        if !in_section || !line.starts_with('|') {
            continue;
        }
        let cells: Vec<&str> = line
            .trim()
            .trim_matches('|')
            .split('|')
            .map(str::trim)
            .collect();
        if cells.len() < 4 || cells[0] == "Our path" || cells[0].starts_with("---") {
            continue;
        }
        // A path without backticks still counts, so a row can't hide a gap.
        let path = backticked(cells[0])
            .first()
            .map_or_else(|| cells[0].to_string(), |p| p.to_string());
        let commit = match backticked(cells[2]).last() {
            Some(token) => token.to_string(),
            None => cells[2].rsplit(' ').next().unwrap_or("").to_string(),
        };
        rows.push(Row {
            path,
            commit,
            license: cells[3].to_string(),
        });
    }
    rows
}

fn license_spellings(license: &str) -> Vec<String> {
    match license {
        "Apache-2.0" => vec!["Apache-2.0".into(), "Apache License, Version 2.0".into()],
        other => vec![other.to_string()],
    }
}

/// Rule `rows-have-files`.
fn check_rows(files: &Files, tables: &[Table]) -> Vec<String> {
    let mut failures = Vec::new();
    for table in tables {
        for row in rows(table.text) {
            let path = format!("{}{}", table.prefix, row.path);
            let Some(text) = files.get(&path) else {
                failures.push(format!(
                    "{path}: rule rows-have-files: the {}THIRD_PARTY.md row names a file that doesn't exist",
                    table.prefix
                ));
                continue;
            };
            let header = flatten(&first_lines(text, HEADER_LINES));
            if row.commit.is_empty() || !header.contains(&row.commit) {
                failures.push(format!(
                    "{path}: rule rows-have-files: the first {HEADER_LINES} lines don't give the row's commit or version, {:?}",
                    row.commit
                ));
            }
            if !license_spellings(&row.license)
                .iter()
                .any(|l| header.contains(l.as_str()))
            {
                failures.push(format!(
                    "{path}: rule rows-have-files: the first {HEADER_LINES} lines don't give the row's licence, {:?}",
                    row.license
                ));
            }
        }
    }
    failures
}

fn says_copied(path: &str, text: &str) -> bool {
    let header = flatten(&first_lines(text, HEADER_LINES)).to_ascii_lowercase();
    if header.contains("adapted from") || header.contains("copied from") {
        return true;
    }
    // `; From tree-sitter-ruby 0.23.1, ...`
    path.ends_with(".scm")
        && first_lines(text, HEADER_LINES).lines().any(|line| {
            let line = flatten(line);
            let mut words = line.split(' ');
            words.next() == Some("From")
                && words.next().is_some()
                && words
                    .next()
                    .is_some_and(|v| v.starts_with(|c: char| c.is_ascii_digit()))
        })
}

/// The ways a THIRD_PARTY.md may name `path`: from the repository root, from
/// kiln's root, and from its crate.
fn names_for(path: &str) -> Vec<String> {
    let mut names = vec![path.to_string()];
    let from_kiln = path.strip_prefix("kiln/").unwrap_or(path);
    names.push(from_kiln.to_string());
    let mut parts = from_kiln.splitn(3, '/');
    if parts.next() == Some("crates")
        && let (Some(_krate), Some(rest)) = (parts.next(), parts.next())
    {
        names.push(rest.to_string());
    }
    names
}

/// Rule `copied-files-have-rows`.
fn check_copied_have_rows(files: &Files, tables: &[Table]) -> Vec<String> {
    let mut failures = Vec::new();
    for (path, text) in files {
        if !says_copied(path, text) {
            continue;
        }
        let named = names_for(path).iter().any(|name| {
            let quoted = format!("`{name}`");
            tables.iter().any(|t| t.text.contains(&quoted))
        });
        if !named {
            failures.push(format!(
                "{path}: rule copied-files-have-rows: the header says adapted or copied, but no THIRD_PARTY.md names the file in backticks"
            ));
        }
    }
    failures
}

/// Whether `header` names a source: `from` and then a name, as in
/// `From tree-sitter-ruby 0.23.1` or `adapted from nvim-treesitter`.
fn names_a_source(header: &str) -> bool {
    let words: Vec<&str> = header
        .split_whitespace()
        .map(|w| w.trim_matches(|c: char| !c.is_alphanumeric()))
        .collect();
    words.windows(2).any(|w| {
        w[0].eq_ignore_ascii_case("from")
            && w[1].starts_with(|c: char| c.is_alphabetic())
            && !["the", "a", "an"].contains(&w[1].to_ascii_lowercase().as_str())
    })
}

/// Rule `queries-say-where`.
fn check_queries(files: &Files) -> Vec<String> {
    let mut failures = Vec::new();
    for (path, text) in files {
        if !path.starts_with("crates/fe-syntax/queries/") || !path.ends_with(".scm") {
            continue;
        }
        let header = flatten(&first_lines(text, QUERY_HEADER_LINES));
        let ours = has_word(&header, "ours");
        let licensed = header.contains("MIT") || header.contains("Apache-2.0");
        if !ours && !(licensed && names_a_source(&header)) {
            failures.push(format!(
                "{path}: rule queries-say-where: the first {QUERY_HEADER_LINES} lines say neither `ours` nor `from <source>` with an MIT or Apache-2.0 licence"
            ));
        }
    }
    failures
}

fn tokens(line: &str) -> impl Iterator<Item = &str> {
    line.split(|c: char| !(c.is_ascii_alphanumeric() || matches!(c, '_' | '-')))
        .filter(|t| !t.is_empty())
}

fn is_gpui_source(name: &str) -> bool {
    name.eq_ignore_ascii_case("gpui") || name.starts_with("gpui-pre") || name == "sum_tree"
}

/// Whether a line naming Zed is allowed. Strict and line-based, not a parser
/// of source clauses: it passes only if it names a GPUI source (`gpui`,
/// `gpui-pre*`, `sum_tree`, GPUI's examples) and nothing else: no `editor`,
/// highlight queries or GPL licence, no `crates/<name>` path other than a
/// GPUI crate, and no second source (`and`, `&`, `+`, a second `from`). A
/// false failure is fixed by giving each source a line of its own.
fn zed_line_allowed(line: &str) -> bool {
    let words: Vec<String> = tokens(line).map(str::to_ascii_lowercase).collect();
    let word = |w: &str| words.iter().any(|t| t == w);
    let gpl = words
        .iter()
        .any(|t| t.starts_with("gpl") || t.starts_with("agpl"));
    let queries = words
        .iter()
        .any(|t| t.starts_with("highlight") || t.starts_with("quer"));
    let crate_paths_ok = line.match_indices("crates/").all(|(i, m)| {
        tokens(&line[i + m.len()..])
            .next()
            .is_some_and(is_gpui_source)
    });
    let second_source = word("and")
        || line.contains(['&', '+'])
        || words.iter().filter(|t| *t == "from").count() > 1;
    words.iter().any(|t| is_gpui_source(t))
        && !word("editor")
        && !gpl
        && !queries
        && crate_paths_ok
        && !second_source
}

/// Rule `no-zed-beyond-gpui`.
fn check_no_zed(files: &Files) -> Vec<String> {
    let mut failures = Vec::new();
    for (path, text) in files {
        for line in text.lines().take(HEADER_LINES) {
            if !has_word(line, "zed") && !line.contains("zed-industries/zed") {
                continue;
            }
            if !zed_line_allowed(line) {
                failures.push(format!(
                    "{path}: rule no-zed-beyond-gpui: the header names Zed as a source other than GPUI: {}",
                    line.trim()
                ));
            }
        }
    }
    failures
}

fn read(path: &str) -> String {
    std::fs::read_to_string(repo_root().join(path)).unwrap_or_else(|e| panic!("{path}: {e}"))
}

fn assert_clean(failures: Vec<String>) {
    assert!(failures.is_empty(), "\n{}", failures.join("\n"));
}

#[test]
fn copied_files_and_rows_agree() {
    let (main, kiln) = (read("THIRD_PARTY.md"), read("kiln/THIRD_PARTY.md"));
    let tables = [
        Table {
            prefix: "",
            text: &main,
        },
        Table {
            prefix: "kiln/",
            text: &kiln,
        },
    ];
    let files = repo_files();
    assert!(files.len() > 100, "found only {} files", files.len());
    assert!(
        rows(&main).len() > 10 && !rows(&kiln).is_empty(),
        "no rows found"
    );
    let mut failures = check_rows(&files, &tables);
    failures.extend(check_copied_have_rows(&files, &tables));
    assert_clean(failures);
}

#[test]
fn queries_say_where_they_came_from() {
    let files = repo_files();
    assert!(
        files
            .keys()
            .filter(|p| p.starts_with("crates/fe-syntax/queries/"))
            .count()
            > 20,
        "found too few queries"
    );
    assert_clean(check_queries(&files));
}

#[test]
fn no_zed_sources_beyond_gpui() {
    assert_clean(check_no_zed(&repo_files()));
}

fn fixture(files: &[(&str, &str)]) -> Files {
    files
        .iter()
        .map(|(p, t)| (p.to_string(), t.to_string()))
        .collect()
}

const ROW_MD: &str =
    "## Code copied or adapted\n\n| Our path | Source | Commit | License |\n|---|---|---|---|\n";

fn all_checks(files: &Files, md: &str) -> Vec<String> {
    let tables = [Table {
        prefix: "",
        text: md,
    }];
    let mut failures = check_rows(files, &tables);
    failures.extend(check_copied_have_rows(files, &tables));
    failures.extend(check_queries(files));
    failures.extend(check_no_zed(files));
    failures
}

fn assert_reports(failures: &[String], path: &str, rule: &str) {
    assert!(
        failures
            .iter()
            .any(|f| f.starts_with(path) && f.contains(&format!("rule {rule}:"))),
        "expected {path} under {rule}, got {failures:#?}"
    );
}

#[test]
fn checks_catch_gaps() {
    let good = "// Adapted from foo 1.2.3; MIT\n";
    let row = |path: &str| format!("{ROW_MD}| `{path}` | foo | version 1.2.3 | MIT |\n");

    // A consistent set reports nothing.
    let files = fixture(&[
        ("crates/a/src/ok.rs", good),
        ("crates/fe-syntax/queries/ours.scm", ";; query, ours\n"),
        (
            "crates/a/src/gpui_ex.rs",
            "// Adapted from gpui-pre 0.3.7 (zed@1a28cff); MIT\n",
        ),
    ]);
    let md = format!(
        "{}| `crates/a/src/gpui_ex.rs` | gpui-pre | zed `1a28cff` | MIT |\n",
        row("crates/a/src/ok.rs")
    );
    assert_eq!(all_checks(&files, &md), Vec::<String>::new());

    // An adapted file with no row.
    let files = fixture(&[("crates/a/src/unlisted.rs", good)]);
    assert_reports(
        &all_checks(&files, ROW_MD),
        "crates/a/src/unlisted.rs",
        "copied-files-have-rows",
    );

    // A `From <name> <version>` query with no row, named from its crate.
    let from = "; From tree-sitter-x 1.0, path queries/highlights.scm, license MIT\n";
    let files = fixture(&[("crates/fe-syntax/queries/x.scm", from)]);
    assert_reports(
        &all_checks(&files, ROW_MD),
        "crates/fe-syntax/queries/x.scm",
        "copied-files-have-rows",
    );
    let md = row("queries/x.scm");
    assert_eq!(
        check_copied_have_rows(
            &files,
            &[Table {
                prefix: "",
                text: &md
            }]
        ),
        Vec::<String>::new()
    );

    // A row whose path isn't in backticks is still checked.
    let md = format!("{ROW_MD}| crates/a/src/plain_gone.rs | foo | version 1.2.3 | MIT |\n");
    let failures = all_checks(&Files::new(), &md);
    assert_reports(&failures, "crates/a/src/plain_gone.rs", "rows-have-files");

    // A row whose file is missing.
    let failures = all_checks(&Files::new(), &row("crates/a/src/gone.rs"));
    assert_reports(&failures, "crates/a/src/gone.rs", "rows-have-files");

    // A header without the row's licence, and one without its commit.
    let files = fixture(&[("crates/a/src/nolicense.rs", "// Adapted from foo 1.2.3\n")]);
    let failures = all_checks(&files, &row("crates/a/src/nolicense.rs"));
    assert_reports(&failures, "crates/a/src/nolicense.rs", "rows-have-files");
    assert!(
        failures.iter().any(|f| f.contains("licence")),
        "{failures:#?}"
    );
    let files = fixture(&[("crates/a/src/nover.rs", "// Adapted from foo; MIT\n")]);
    let failures = all_checks(&files, &row("crates/a/src/nover.rs"));
    assert!(
        failures.iter().any(|f| f.contains("commit or version")),
        "{failures:#?}"
    );

    // A query that says nothing about where it came from.
    let files = fixture(&[("crates/fe-syntax/queries/imports/bare.scm", "(a) @b\n")]);
    assert_reports(
        &all_checks(&files, ROW_MD),
        "crates/fe-syntax/queries/imports/bare.scm",
        "queries-say-where",
    );

    // A licence with no source, and a source with no licence.
    let files = fixture(&[
        (
            "crates/fe-syntax/queries/licence_only.scm",
            ";; MIT\n(a) @b\n",
        ),
        (
            "crates/fe-syntax/queries/source_only.scm",
            ";; From tree-sitter-x 1.0\n(a) @b\n",
        ),
    ]);
    let failures = check_queries(&files);
    assert_reports(
        &failures,
        "crates/fe-syntax/queries/licence_only.scm",
        "queries-say-where",
    );
    assert_reports(
        &failures,
        "crates/fe-syntax/queries/source_only.scm",
        "queries-say-where",
    );

    // A source needn't be hyphenated.
    let files = fixture(&[(
        "crates/fe-syntax/queries/lapce.scm",
        ";; From Lapce 0.4.0; Apache-2.0\n(a) @b\n",
    )]);
    assert_eq!(check_queries(&files), Vec::<String>::new());

    // A directory that can't be read is reported, not skipped.
    let (mut found, mut errors) = (Files::new(), Vec::new());
    let gone = repo_root().join("no-such-dir");
    collect(&gone, repo_root(), &mut found, &mut errors);
    assert!(
        errors.len() == 1 && errors[0].contains("no-such-dir"),
        "{errors:#?}"
    );

    // A header naming a Zed editor crate, and one naming Zed with no GPUI.
    let files = fixture(&[
        (
            "crates/a/src/gpl.rs",
            "// Adapted from zed crates/editor/src/editor.rs; GPL-3.0\n",
        ),
        (
            "crates/a/src/gpl_gpui.rs",
            "// Copied from gpui, zed crates/editor/src/x.rs\n",
        ),
        ("crates/a/src/plain.rs", "// Based on zed-industries/zed\n"),
        (
            "crates/a/src/hl.rs",
            "// Copied from Zed highlight queries; used with GPUI; Apache-2.0\n",
        ),
        (
            "crates/a/src/lookalike.rs",
            "// Adapted from zed crates/gpui_editor/src/x.rs\n",
        ),
        (
            "crates/a/src/gpl_lic.rs",
            "// Adapted from zed gpui-pre 0.3.7; GPL-3.0\n",
        ),
        (
            "crates/a/src/editor_gpui.rs",
            "// Copied from Zed editor; used with GPUI; Apache-2.0\n",
        ),
        (
            "crates/a/src/two_sources.rs",
            "// Copied from GPUI and Zed editor; Apache-2.0\n",
        ),
        (
            "crates/a/src/override.rs",
            "// Copied from Zed editor; helper adapted from crates/gpui/src/x.rs; Apache-2.0\n",
        ),
        (
            "crates/a/src/agpl.rs",
            "// Adapted from zed gpui 0.3; AGPL-3.0\n",
        ),
    ]);
    let failures = all_checks(&files, ROW_MD);
    for path in [
        "crates/a/src/gpl.rs",
        "crates/a/src/gpl_gpui.rs",
        "crates/a/src/plain.rs",
        "crates/a/src/hl.rs",
        "crates/a/src/lookalike.rs",
        "crates/a/src/gpl_lic.rs",
        "crates/a/src/editor_gpui.rs",
        "crates/a/src/agpl.rs",
        "crates/a/src/two_sources.rs",
        "crates/a/src/override.rs",
    ] {
        assert_reports(&failures, path, "no-zed-beyond-gpui");
    }
    assert!(
        !failures.iter().any(|f| f.contains("sized")),
        "{failures:#?}"
    );
    assert!(check_no_zed(&fixture(&[("crates/a/src/s.rs", "// a sized buffer\n")])).is_empty());
    let allowed = fixture(&[
        (
            "crates/a/src/a.rs",
            "// Adapted from zed sum_tree; Apache-2.0\n",
        ),
        (
            "crates/a/src/b.rs",
            "// Adapted from GPUI examples (zed@1a28cff); Apache-2.0\n",
        ),
        (
            "crates/a/src/c.rs",
            "// Adapted from zed crates/gpui/src/x.rs; Apache-2.0\n",
        ),
        (
            "crates/a/src/d.rs",
            "// Copied from Zed's GPUI examples; Apache-2.0\n",
        ),
        (
            "kiln/crates/kiln-ui/src/text_field.rs",
            "// Adapted from gpui-pre 0.3.7 examples/input.rs (zed@1a28cff); Apache-2.0\n",
        ),
    ]);
    assert_eq!(check_no_zed(&allowed), Vec::<String>::new());
}
