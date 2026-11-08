// SPDX-License-Identifier: Apache-2.0

//! Bidi isolation of arguments, at interpolation only.
//!
//! Fluent's own isolation (FSI/PDI around every placeable) is off. Instead
//! each bundle gets a formatter, which Fluent calls only when it writes a
//! value into the text; a selector on the same argument still sees the raw
//! value, so `[male]` matches `male`. A string value is isolated when the
//! bundle's locale is right-to-left, or when the value itself holds a
//! strongly right-to-left character. Numbers are never isolated.

use std::borrow::Cow;

use fluent_bundle::FluentValue;
use unicode_bidi::{BidiClass, bidi_class};

/// FIRST STRONG ISOLATE: opens an isolated argument.
pub const FSI: char = '\u{2068}';
/// POP DIRECTIONAL ISOLATE: closes it.
pub const PDI: char = '\u{2069}';

fn has_strong_rtl(text: &str) -> bool {
    text.chars()
        .any(|c| matches!(bidi_class(c), BidiClass::R | BidiClass::AL))
}

fn isolated(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 6);
    out.push(FSI);
    out.push_str(text);
    out.push(PDI);
    out
}

/// The formatter for a left-to-right locale's bundle.
pub(crate) fn format_ltr<M>(value: &FluentValue, _: &M) -> Option<String> {
    match value {
        FluentValue::String(s) if has_strong_rtl(s) => Some(isolated(s)),
        _ => None,
    }
}

/// The formatter for a right-to-left locale's bundle.
pub(crate) fn format_rtl<M>(value: &FluentValue, _: &M) -> Option<String> {
    match value {
        FluentValue::String(s) => Some(isolated(s)),
        _ => None,
    }
}

/// `text` without bidi isolates (U+2066..U+2069), for text that leaves the
/// UI: the clipboard, an editor buffer, a log line, the command line.
pub fn strip_isolates(text: &str) -> Cow<'_, str> {
    let isolate = |c: char| matches!(c, '\u{2066}'..='\u{2069}');
    if text.contains(isolate) {
        Cow::Owned(text.chars().filter(|&c| !isolate(c)).collect())
    } else {
        Cow::Borrowed(text)
    }
}
