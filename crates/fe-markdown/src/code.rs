//! Fenced code highlighting, by the fence's language.

use std::time::Duration;

use fe_buffer::Buffer;
use fe_syntax::{HighlightSpan, Syntax, fence_language, language_by_name};

/// How long a fence's code may take to parse. The preview parses off the UI
/// thread, so this only stops a pathological block holding it up.
const PARSE_BUDGET: Duration = Duration::from_secs(1);

/// How many times highlighting is retried for injected languages compiled
/// on the way, or for a query that ran out of time, before showing what it
/// has. Each retry gets twice the time, up to fe-syntax's limit.
const HIGHLIGHT_ATTEMPTS: usize = 5;

/// The language a fence's info string names and the highlights of its
/// `code`, which has `\n` line endings. An unknown or absent language gets
/// none.
pub(crate) fn highlight(info: &str, code: &str) -> (Option<&'static str>, Vec<HighlightSpan>) {
    let Some(language) = fence_language(info) else {
        return (None, Vec::new());
    };
    let buffer = Buffer::new(code);
    let mut syntax = Syntax::new(language, &buffer.snapshot());
    syntax.parse_within(PARSE_BUDGET);
    let mut spans = Vec::new();
    for _ in 0..HIGHLIGHT_ATTEMPTS {
        spans = syntax.highlights(0..code.len());
        let missing = syntax.take_missing_languages();
        if missing.is_empty() {
            break;
        }
        for name in missing {
            language_by_name(name);
        }
    }
    (Some(language.name()), spans)
}
