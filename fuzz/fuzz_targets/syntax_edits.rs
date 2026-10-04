//! Arbitrary text in every language, edited batch by batch with a full
//! reparse and highlight after each, must end with the same tree and
//! highlights as a parse from scratch. Mostly here to run the grammars' C
//! parsers and external scanners under a sanitizer.

#![no_main]

use std::ops::Range;

use arbitrary::Arbitrary;
use fe_buffer::{Bias, Buffer, Origin, Text};
use fe_syntax::{HighlightSpan, Syntax, language_by_name, language_names};
use libfuzzer_sys::fuzz_target;

const MAX_TEXT: usize = 64 * 1024;
const MAX_BATCHES: usize = 32;

#[derive(Debug, Arbitrary)]
struct Input {
    language: u8,
    text: Vec<u8>,
    batches: Vec<Vec<RawEdit>>,
}

#[derive(Debug, Arbitrary)]
struct RawEdit {
    start: u16,
    end: u16,
    text: Vec<u8>,
}

fuzz_target!(|input: Input| run(input));

fn run(input: Input) {
    let names: Vec<_> = language_names().collect();
    let name = names[usize::from(input.language) % names.len()];
    let language = language_by_name(name).expect("listed languages exist");

    let mut buffer = Buffer::new(&decode(&input.text));
    let mut syntax = Syntax::new(language, buffer.current());
    syntax.finish(syntax.parse_job().run());

    for batch in input.batches.iter().take(MAX_BATCHES) {
        let text = buffer.text();
        let edits: Vec<_> = batch
            .iter()
            .map(|edit| {
                let start = boundary(text, edit.start);
                let end = boundary(text, edit.end);
                (start.min(end)..start.max(end), decode(&edit.text))
            })
            .collect();
        let added: usize = edits.iter().map(|(_, text)| text.len()).sum();
        if buffer.len() + added > MAX_TEXT {
            break;
        }
        buffer.edit(edits, Origin::User);
        syntax.edit(buffer.current());
        syntax.finish(syntax.parse_job().run());
        highlights(&syntax, 0..buffer.len());
    }

    let mut fresh = Syntax::new(language, buffer.current());
    fresh.finish(fresh.parse_job().run());
    if syntax.failed() || fresh.failed() {
        return;
    }
    let text = buffer.text().to_string();
    assert_eq!(
        syntax.tree().map(|tree| tree.root_node().to_sexp()),
        fresh.tree().map(|tree| tree.root_node().to_sexp()),
        "{name}: incremental tree differs from a fresh parse of {text:?}"
    );
    // Complete: fe-syntax's `no-query-budget` feature lifts the time limit.
    let incremental = highlights(&syntax, 0..buffer.len());
    let full = highlights(&fresh, 0..buffer.len());
    // Highlighting can panic too, which gives up on the buffer.
    if syntax.failed() || fresh.failed() {
        return;
    }
    assert_eq!(
        incremental, full,
        "{name}: incremental highlights differ from a fresh parse of {text:?}"
    );
}

/// Highlights `range`, compiling any injected language it needs.
fn highlights(syntax: &Syntax, range: Range<usize>) -> Vec<HighlightSpan> {
    loop {
        let spans = syntax.highlights(range.clone());
        let missing = syntax.missing_languages();
        if missing.is_empty() {
            return spans;
        }
        for name in missing {
            language_by_name(name);
        }
    }
}

/// `bytes` as UTF-8 with `\n` line endings, as a buffer holds it, cut to
/// `MAX_TEXT`.
fn decode(bytes: &[u8]) -> String {
    let mut text = String::from_utf8_lossy(bytes)
        .replace("\r\n", "\n")
        .replace('\r', "\n");
    let mut len = text.len().min(MAX_TEXT);
    while !text.is_char_boundary(len) {
        len -= 1;
    }
    text.truncate(len);
    text
}

/// An offset into `text` from a raw one, moved back to a char boundary.
fn boundary(text: &Text, raw: u16) -> usize {
    text.clip_offset(usize::from(raw) % (text.len() + 1), Bias::Left)
}
