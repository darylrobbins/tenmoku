// SPDX-License-Identifier: Apache-2.0

//! The pseudo-locale `en-XA`, generated from en-US when a catalogue is built
//! for it (i18n design § 8): each message's text accented and padded about
//! 35% longer, with placeables and selectors as they were. The catalogue wraps
//! each formatted message in `⟦ ⟧` ([`bracket`]), once, so a message that
//! references another or a term has no brackets in the middle.
//! Text drawn outside the catalogue shows plain, a clipped layout cuts the
//! closing bracket off, and a message assembled from fragments shows brackets
//! in the middle. It is never shipped: the tag is not in
//! [`shipped_locales`](crate::shipped_locales), and only a request for it
//! (`TENMOKU_PSEUDO_LOCALE=1`, read by `kiln-platform`) builds it.

use fluent_syntax::ast::{Entry, Expression, InlineExpression, Pattern, PatternElement, Resource};
use fluent_syntax::{parser, serializer};

/// The pseudo-locale's tag.
pub const PSEUDO_LOCALE: &str = "en-XA";

/// Pads text by this fraction of its length.
const GROWTH_PERCENT: usize = 35;
const PAD: char = '·';

fn accent(c: char) -> char {
    match c {
        'a' => 'å',
        'b' => 'ƀ',
        'c' => 'ç',
        'd' => 'ð',
        'e' => 'é',
        'f' => 'ƒ',
        'g' => 'ĝ',
        'h' => 'ĥ',
        'i' => 'î',
        'j' => 'ĵ',
        'k' => 'ķ',
        'l' => 'ļ',
        'n' => 'ñ',
        'o' => 'ö',
        'p' => 'þ',
        'r' => 'ŕ',
        's' => 'š',
        't' => 'ţ',
        'u' => 'ü',
        'w' => 'ŵ',
        'y' => 'ý',
        'z' => 'ž',
        'A' => 'Å',
        'B' => 'Ɓ',
        'C' => 'Ç',
        'D' => 'Ð',
        'E' => 'É',
        'G' => 'Ĝ',
        'H' => 'Ĥ',
        'I' => 'Î',
        'J' => 'Ĵ',
        'K' => 'Ķ',
        'L' => 'Ļ',
        'N' => 'Ñ',
        'O' => 'Ö',
        'P' => 'Þ',
        'R' => 'Ŕ',
        'S' => 'Š',
        'T' => 'Ţ',
        'U' => 'Ü',
        'W' => 'Ŵ',
        'Y' => 'Ý',
        'Z' => 'Ž',
        other => other,
    }
}

/// `source`, an en-US `.ftl` file, as its pseudo-locale file.
pub fn pseudo_source(source: &str) -> String {
    let mut resource: Resource<String> = match parser::parse(source.to_owned()) {
        Ok(resource) => resource,
        Err((resource, _)) => resource,
    };
    for entry in &mut resource.body {
        match entry {
            Entry::Message(message) => {
                if let Some(value) = &mut message.value {
                    pseudo_pattern(value);
                }
                for attribute in &mut message.attributes {
                    pseudo_pattern(&mut attribute.value);
                }
            }
            Entry::Term(term) => {
                pseudo_pattern(&mut term.value);
                for attribute in &mut term.attributes {
                    pseudo_pattern(&mut attribute.value);
                }
            }
            _ => {}
        }
    }
    serializer::serialize(&resource)
}

/// `text`, a message formatted in the pseudo-locale, in `⟦ ⟧`.
pub(crate) fn bracket(text: &str) -> String {
    format!("⟦{text}⟧")
}

/// Accents `pattern`'s text and pads it.
fn pseudo_pattern(pattern: &mut Pattern<String>) {
    let mut chars = 0;
    for element in &mut pattern.elements {
        match element {
            PatternElement::TextElement { value } => {
                *value = value.chars().map(accent).collect();
                chars += value.chars().filter(|c| !c.is_whitespace()).count();
            }
            PatternElement::Placeable { expression } => pseudo_expression(expression),
        }
    }
    let padding: String = std::iter::repeat_n(PAD, chars * GROWTH_PERCENT / 100).collect();
    // The serializer puts a long pattern on lines of its own, where a leading
    // `*`, `[` or `.` would read as syntax: keep it as a string literal.
    if let Some(PatternElement::TextElement { value }) = pattern.elements.first_mut()
        && value.starts_with(['*', '[', '.'])
    {
        let rest = value.split_off(1);
        let literal = std::mem::replace(value, rest);
        pattern.elements.insert(
            0,
            PatternElement::Placeable {
                expression: Expression::Inline(InlineExpression::StringLiteral { value: literal }),
            },
        );
    }
    if !padding.is_empty() {
        match pattern.elements.last_mut() {
            Some(PatternElement::TextElement { value }) => value.push_str(&padding),
            _ => pattern
                .elements
                .push(PatternElement::TextElement { value: padding }),
        }
    }
}

fn pseudo_expression(expression: &mut Expression<String>) {
    match expression {
        Expression::Select { variants, .. } => {
            for variant in variants {
                pseudo_pattern(&mut variant.value);
            }
        }
        Expression::Inline(InlineExpression::Placeable { expression }) => {
            pseudo_expression(expression)
        }
        Expression::Inline(_) => {}
    }
}
