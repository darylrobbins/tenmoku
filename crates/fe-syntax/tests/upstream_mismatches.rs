//! Upstream grammar bugs that the fuzz harness found, kept as regression tests.
//!
//! Each test replays one input from `fuzz/regressions/syntax_edits`, decoded
//! by hand into plain text and edit batches so it doesn't depend on the fuzz
//! harness or `arbitrary`'s encoding. It asserts what the harness asserts:
//! after the edits, an incremental parse matches a fresh parse of the final
//! text, by tree for Bash and by highlights for Markdown. The causes are in
//! tree-sitter-bash and tree-sitter-md's inline grammar, so every test is
//! `#[ignore]`d and fails today. See `docs/decisions/khgdxezget-fuzzing.md`.
//!
//! Run them with
//! `cargo test -p fe-syntax --test upstream_mismatches -- --ignored`.
//! A test that starts to pass means upstream fixed that bug: un-ignore it.

#![cfg(all(feature = "bash", feature = "markdown"))]

use std::ops::Range;

use fe_buffer::{Buffer, Origin};
use fe_syntax::{HighlightSpan, Syntax, language_by_name};

type Batch<'a> = &'a [(Range<usize>, &'a str)];

/// Parses `text`, applies each batch of edits with a reparse after it, as the
/// editor does, and returns the incremental `Syntax` beside a fresh one for
/// the final text.
fn replay(language: &str, text: &str, batches: &[Batch]) -> (Syntax, Syntax) {
    let language = language_by_name(language).expect("the language is built in");
    let mut buffer = Buffer::new(text);
    let mut syntax = Syntax::new(language, buffer.current());
    syntax.finish(syntax.parse_job().run());
    for batch in batches {
        buffer.edit(batch.iter().cloned(), Origin::User);
        syntax.edit(buffer.current());
        syntax.finish(syntax.parse_job().run());
        highlights(&syntax, buffer.len());
    }
    let mut fresh = Syntax::new(language, buffer.current());
    fresh.finish(fresh.parse_job().run());
    (syntax, fresh)
}

/// Highlights the whole text, compiling any injected language it needs.
fn highlights(syntax: &Syntax, len: usize) -> Vec<HighlightSpan> {
    loop {
        let spans = syntax.highlights(0..len);
        let missing = syntax.take_missing_languages();
        if missing.is_empty() {
            return spans;
        }
        for name in missing {
            language_by_name(name);
        }
    }
}

fn final_len(text: &str, batches: &[Batch]) -> usize {
    let mut buffer = Buffer::new(text);
    for batch in batches {
        buffer.edit(batch.iter().cloned(), Origin::User);
    }
    buffer.len()
}

/// The incremental tree must equal a fresh parse's.
fn same_tree(language: &str, text: &str, batches: &[Batch]) {
    let (incremental, fresh) = replay(language, text, batches);
    assert!(!incremental.failed() && !fresh.failed());
    assert_eq!(
        incremental.tree().map(|tree| tree.root_node().to_sexp()),
        fresh.tree().map(|tree| tree.root_node().to_sexp()),
    );
}

/// The incremental highlights must equal a fresh parse's. The trees match in
/// every Markdown case: the difference is in the injected inline layer.
fn same_highlights(language: &str, text: &str, batches: &[Batch]) {
    let len = final_len(text, batches);
    let (incremental, fresh) = replay(language, text, batches);
    assert!(!incremental.failed() && !fresh.failed());
    assert_eq!(highlights(&incremental, len), highlights(&fresh, len));
}

// Bash: an unterminated heredoc start (`<<`) at the end of input.

/// Inserting a byte into `[ <word><<J` at end of input. From `fuzz/regressions/syntax_edits/bash-heredoc-in-test-incremental`.
#[test]
#[ignore = "upstream: tree-sitter-bash — an incremental reparse near an unterminated heredoc start at end of input builds a different tree than a fresh parse; see docs/decisions/khgdxezget-fuzzing.md"]
fn bash_heredoc_start_in_test() {
    same_tree("bash", "[\u{13}<<J", &[&[(2..2, "\0")]]);
}

/// Two batches of edits that end on a `<` heredoc start. From `fuzz/regressions/syntax_edits/bash-heredoc-0a96463b`.
#[test]
#[ignore = "upstream: tree-sitter-bash — an incremental reparse near an unterminated heredoc start at end of input builds a different tree than a fresh parse; see docs/decisions/khgdxezget-fuzzing.md"]
fn bash_heredoc_0a96463b() {
    same_tree(
        "bash",
        "[,{{\0\u{4}g++",
        &[
            &[
                (3..5, "\u{FFFD}+"),
                (5..5, "<\u{FFFD}9"),
                (3..8, "\u{FFFD}y"),
                (4..9, "<"),
            ],
            &[(3..7, "")],
        ],
    );
}

/// Deleting trailing bytes after `[Z\u{1}<<`. From `fuzz/regressions/syntax_edits/bash-heredoc-374eec01`.
#[test]
#[ignore = "upstream: tree-sitter-bash — an incremental reparse near an unterminated heredoc start at end of input builds a different tree than a fresh parse; see docs/decisions/khgdxezget-fuzzing.md"]
fn bash_heredoc_374eec01() {
    same_tree("bash", "[Z\u{1}<<\u{FFFD}\u{FFFD}", &[&[(8..11, "")]]);
}

/// Deleting a trailing byte after `[Z<<0`. From `fuzz/regressions/syntax_edits/bash-heredoc-8d746dea`.
#[test]
#[ignore = "upstream: tree-sitter-bash — an incremental reparse near an unterminated heredoc start at end of input builds a different tree than a fresh parse; see docs/decisions/khgdxezget-fuzzing.md"]
fn bash_heredoc_8d746dea() {
    same_tree("bash", "[Z<<0\u{FFFD}", &[&[(5..8, "")]]);
}

/// Deleting a byte inside `[..<<` at end of input. From `fuzz/regressions/syntax_edits/bash-heredoc-a2a6852d`.
#[test]
#[ignore = "upstream: tree-sitter-bash — an incremental reparse near an unterminated heredoc start at end of input builds a different tree than a fresh parse; see docs/decisions/khgdxezget-fuzzing.md"]
fn bash_heredoc_a2a6852d() {
    same_tree("bash", "[\u{FFFD}\u{FFFD}<<\u{FFFD}", &[&[(4..7, "")]]);
}

/// No `[`: a heredoc start at end of input after a deletion. From `fuzz/regressions/syntax_edits/bash-heredoc-d2f53688`.
#[test]
#[ignore = "upstream: tree-sitter-bash — an incremental reparse near an unterminated heredoc start at end of input builds a different tree than a fresh parse; see docs/decisions/khgdxezget-fuzzing.md"]
fn bash_heredoc_d2f53688() {
    same_tree(
        "bash",
        ".A\u{FFFD}\u{1}\u{FFFD}\u{b}<<\u{FFFD}\u{7}$\u{FFFD}",
        &[&[(17..20, "")]],
    );
}

// Markdown, inline grammar: the block trees match, but the injected inline
// layer's highlights differ. Each test is one symptom; the other inputs in
// `fuzz/regressions/syntax_edits/markdown-inline-*` show these same ones.

/// A strikethrough after `<` loses its delimiters. From `fuzz/regressions/syntax_edits/markdown-inline-strikethrough-incremental`.
#[test]
#[ignore = "upstream: tree-sitter-md inline grammar — strikethrough delimiters in `<~..~` are lost on incremental reparse; see docs/decisions/khgdxezget-fuzzing.md"]
fn markdown_inline_strikethrough_lost_after_lt() {
    same_highlights("markdown", "<~#\u{1}~1", &[&[], &[(5..6, "")]]);
}

/// A strikethrough with no `<` loses its delimiters. From `fuzz/regressions/syntax_edits/markdown-inline-b688980c`.
#[test]
#[ignore = "upstream: tree-sitter-md inline grammar — strikethrough delimiters in `.~.~` are lost on incremental reparse, with no `<`; see docs/decisions/khgdxezget-fuzzing.md"]
fn markdown_inline_strikethrough_lost_without_lt() {
    same_highlights("markdown", ".~.~", &[&[], &[(3..4, "~")]]);
}

/// A `~~` pair loses its delimiters. From `fuzz/regressions/syntax_edits/markdown-inline-172e910f`.
#[test]
#[ignore = "upstream: tree-sitter-md inline grammar — `~~` delimiters are lost on incremental reparse; see docs/decisions/khgdxezget-fuzzing.md"]
fn markdown_inline_double_tilde_lost() {
    same_highlights("markdown", "\u{FFFD}\u{3}~~%'~''", &[&[], &[(0..3, "")]]);
}

/// A `*` pair loses its delimiters. From `fuzz/regressions/syntax_edits/markdown-inline-1e1068b7`.
#[test]
#[ignore = "upstream: tree-sitter-md inline grammar — `*` emphasis delimiters are lost on incremental reparse; see docs/decisions/khgdxezget-fuzzing.md"]
fn markdown_inline_star_emphasis_lost() {
    same_highlights("markdown", "(*;\u{e}**", &[&[], &[(3..5, "")]]);
}

/// A `__` pair after an escape loses its delimiters. From `fuzz/regressions/syntax_edits/markdown-inline-69859c09`.
#[test]
#[ignore = "upstream: tree-sitter-md inline grammar — `__` emphasis delimiters after a backslash escape are lost on incremental reparse; see docs/decisions/khgdxezget-fuzzing.md"]
fn markdown_inline_underscore_emphasis_lost() {
    same_highlights(
        "markdown",
        "\u{FFFD}__",
        &[&[(0..0, "g__\\[")], &[(5..8, "")]],
    );
}

/// Stale `_` delimiters stay highlighted. From `fuzz/regressions/syntax_edits/markdown-inline-7dd0cecd`.
#[test]
#[ignore = "upstream: tree-sitter-md inline grammar — `_` emphasis delimiters stay highlighted incrementally that a fresh parse drops; see docs/decisions/khgdxezget-fuzzing.md"]
fn markdown_inline_underscore_emphasis_stale() {
    same_highlights("markdown", "A", &[&[(1..1, "+___!__+")], &[(7..9, "")]]);
}

/// A NUL splits the layer; `_` delimiters are lost. From `fuzz/regressions/syntax_edits/markdown-inline-38651f2b`.
#[test]
#[ignore = "upstream: tree-sitter-md inline grammar — a NUL splits the inline layer and `_` delimiters are lost on incremental reparse; see docs/decisions/khgdxezget-fuzzing.md"]
fn markdown_inline_split_layer_emphasis_lost() {
    same_highlights("markdown", "\0\u{FFFD}__?_", &[&[], &[(0..1, "")]]);
}

/// A NUL splits the layer; stale `_` delimiters stay. From `fuzz/regressions/syntax_edits/markdown-inline-53d5ff20`.
#[test]
#[ignore = "upstream: tree-sitter-md inline grammar — a NUL splits the inline layer and stale `_` delimiters stay highlighted; see docs/decisions/khgdxezget-fuzzing.md"]
fn markdown_inline_split_layer_emphasis_stale() {
    same_highlights(
        "markdown",
        "\u{FFFD}____^_\0\u{FFFD}",
        &[&[], &[(10..13, "<")]],
    );
}
