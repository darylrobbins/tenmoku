//! Golden highlight tests. Each sample in `tests/golden/`, named after its
//! language, is highlighted whole and compared with the spans saved next to
//! it in `<sample>.golden`. They catch changes in output when a grammar or
//! query crate is bumped, or when precedence (`earlier_patterns_win`) changes.
//!
//! Each line of a `.golden` file is one span, `row:col-col highlight "text"`,
//! with 1-based rows and byte columns and the end column exclusive. A span
//! over several rows is written `row:col-row:col`.
//!
//! `UPDATE_GOLDEN=1 cargo nextest run -p fe-syntax golden` rewrites them;
//! review the diff before committing it.

use std::{fmt::Write as _, fs, path::PathBuf};

use fe_buffer::Buffer;

use crate::{Language, Syntax, language_by_name, language_for_path, language_names};

const DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/golden");
const SUFFIX: &str = ".golden";

#[test]
fn highlights_match_golden_files() {
    let update = std::env::var_os("UPDATE_GOLDEN").is_some();
    let mut samples: Vec<PathBuf> = fs::read_dir(DIR)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| !path.to_string_lossy().ends_with(SUFFIX))
        // The directory's own `.gitattributes` is detected too, but no sample.
        .filter(|path| !path.file_name().unwrap().to_string_lossy().starts_with('.'))
        .collect();
    samples.sort();
    let mut covered = Vec::new();
    let mut failures = Vec::new();
    for sample in &samples {
        // Some languages' paths can't be a language's name with an extension
        // (`gitcommit`, whose files have none). Samples for languages left out
        // of this build are skipped.
        let by_stem = || language_by_name(sample.file_stem()?.to_str()?);
        let Some(language) = language_for_path(sample).or_else(by_stem) else {
            continue;
        };
        let file_name = sample.file_name().unwrap().to_string_lossy();
        assert_eq!(
            sample.file_stem().unwrap().to_string_lossy(),
            language.name(),
            "{file_name} is named after its language"
        );
        covered.push(language.name());
        let actual = render(language, &fs::read_to_string(sample).unwrap(), &file_name);
        let golden = sample.with_file_name(format!("{file_name}{SUFFIX}"));
        if update {
            fs::write(&golden, &actual).unwrap();
            continue;
        }
        let expected = fs::read_to_string(&golden).unwrap_or_default();
        if expected != actual {
            failures.push(format!(
                "tests/golden/{file_name}{SUFFIX}:\n{}",
                diff(&expected, &actual)
            ));
        }
    }
    let mut all: Vec<_> = language_names().collect();
    all.sort_unstable();
    covered.sort_unstable();
    assert_eq!(covered, all, "every language has a sample in {DIR}");
    assert!(
        failures.is_empty(),
        "highlights differ from the golden files (- expected, + actual). If the change \
         is intended, rerun with UPDATE_GOLDEN=1 and review the diff.\n\n{}",
        failures.join("\n")
    );
}

/// One line per highlight span of `text`, in the golden file format.
fn render(language: &'static Language, text: &str, file_name: &str) -> String {
    let buffer = Buffer::new(text);
    let mut syntax = Syntax::new(language, buffer.current());
    syntax.finish(syntax.parse_job().run());
    assert!(
        !syntax.tree().unwrap().root_node().has_error(),
        "{file_name} parses without errors"
    );
    let point = |offset| {
        let point = buffer.text().offset_to_point(offset);
        (point.row + 1, point.column + 1)
    };
    let mut out = String::new();
    for span in syntax.highlights_compiling(0..text.len()) {
        let (row, column) = point(span.range.start);
        let (end_row, end_column) = point(span.range.end);
        let end = if end_row == row {
            end_column.to_string()
        } else {
            format!("{end_row}:{end_column}")
        };
        let name = span.highlight.name();
        let text = &text[span.range];
        writeln!(out, "{row}:{column}-{end} {name} {text:?}").unwrap();
    }
    out
}

/// A line diff of `expected` against `actual`: changed lines marked `-` and
/// `+`, with a few unchanged lines around each change.
fn diff(expected: &str, actual: &str) -> String {
    const CONTEXT: usize = 3;
    let a: Vec<&str> = expected.lines().collect();
    let b: Vec<&str> = actual.lines().collect();
    // `common[i][j]`: the longest common subsequence of `a[i..]` and `b[j..]`.
    let mut common = vec![vec![0u32; b.len() + 1]; a.len() + 1];
    for i in (0..a.len()).rev() {
        for j in (0..b.len()).rev() {
            common[i][j] = if a[i] == b[j] {
                common[i + 1][j + 1] + 1
            } else {
                common[i + 1][j].max(common[i][j + 1])
            };
        }
    }
    let mut lines = Vec::new();
    let (mut i, mut j) = (0, 0);
    while i < a.len() || j < b.len() {
        if i < a.len() && j < b.len() && a[i] == b[j] {
            lines.push((' ', a[i]));
            (i, j) = (i + 1, j + 1);
        } else if i < a.len() && (j == b.len() || common[i + 1][j] >= common[i][j + 1]) {
            lines.push(('-', a[i]));
            i += 1;
        } else {
            lines.push(('+', b[j]));
            j += 1;
        }
    }
    let mut shown = vec![false; lines.len()];
    for (ix, (mark, _)) in lines.iter().enumerate() {
        if *mark != ' ' {
            let end = (ix + CONTEXT + 1).min(lines.len());
            shown[ix.saturating_sub(CONTEXT)..end].fill(true);
        }
    }
    let mut out = String::new();
    let mut previous = None;
    for (ix, (mark, line)) in lines.iter().enumerate().filter(|&(ix, _)| shown[ix]) {
        if previous.is_some_and(|previous| previous + 1 != ix) {
            out.push_str("  ...\n");
        }
        writeln!(out, "{mark} {line}").unwrap();
        previous = Some(ix);
    }
    out
}

#[test]
fn a_mismatch_shows_a_line_diff() {
    let numbered = |changed: Option<usize>| {
        (1..=20)
            .map(|n| {
                if Some(n) == changed {
                    format!("x{n}\n")
                } else {
                    format!("l{n}\n")
                }
            })
            .collect::<String>()
    };
    let expected = numbered(None);
    let actual = numbered(Some(10));
    assert_eq!(
        diff(&expected, &actual),
        "  l7\n  l8\n  l9\n- l10\n+ x10\n  l11\n  l12\n  l13\n"
    );
    // Two distant changes give two hunks with `...` between them.
    let actual = actual.replace("l18", "x18").replace("l1\n", "x1\n");
    assert_eq!(
        diff(&expected, &actual),
        "- l1\n+ x1\n  l2\n  l3\n  l4\n  ...\n  l7\n  l8\n  l9\n- l10\n+ x10\n  l11\n  l12\n  l13\n  ...\n  l15\n  l16\n  l17\n- l18\n+ x18\n  l19\n  l20\n"
    );
}
