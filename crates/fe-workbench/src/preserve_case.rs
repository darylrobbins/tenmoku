//! Preserve Case, as VS Code's find widget and Search view have it: a
//! replacement takes the case of the text it replaces.
//!
//! [`apply`] is VS Code's `buildReplaceStringWithCasePreserved`
//! (`src/vs/base/common/search.ts`), rule for rule. The find bar and the
//! search panel apply it to each match's replacement after its capture
//! groups are expanded, so `$1` takes the case of the whole match too,
//! where VS Code leaves a template with groups as it expands.

/// `replacement` in the case of `matched`, the text it replaces, checked in
/// this order:
///
/// - `matched` empty: `replacement` as written.
/// - Both hold `-` (or `_`, but not both separators) and split into as many
///   parts at it: part by part, each by these rules.
/// - `matched` unchanged by upper-casing (`FOO`, or `123`, which has no
///   letter, as in VS Code): `replacement` upper-cased.
/// - `matched` unchanged by lower-casing (`foo`): lower-cased.
/// - `matched`'s first char upper-case (`Foo`): `replacement`'s first char
///   upper-cased, the rest as written.
/// - `matched`'s first char lower-case (`fOo`): its first char lower-cased.
/// - Otherwise (a first char with no case, as `1Foo`): as written.
pub(crate) fn apply(matched: &str, replacement: &str) -> String {
    if matched.is_empty() {
        return replacement.to_owned();
    }
    let hyphens = splits_alike(matched, replacement, '-');
    let underscores = splits_alike(matched, replacement, '_');
    match (hyphens, underscores) {
        (true, false) => return by_part(matched, replacement, '-'),
        (false, true) => return by_part(matched, replacement, '_'),
        _ => {}
    }
    if matched.to_uppercase() == matched {
        return replacement.to_uppercase();
    }
    if matched.to_lowercase() == matched {
        return replacement.to_lowercase();
    }
    let first = matched.chars().next().expect("matched isn't empty");
    let mut chars = replacement.chars();
    let Some(head) = chars.next() else {
        return String::new();
    };
    let head: String = if first.to_lowercase().ne(Some(first)) {
        head.to_uppercase().collect()
    } else if first.to_uppercase().ne(Some(first)) {
        head.to_lowercase().collect()
    } else {
        return replacement.to_owned();
    };
    head + chars.as_str()
}

/// Whether `matched` and `replacement` both hold `separator` and split into
/// as many parts at it.
fn splits_alike(matched: &str, replacement: &str, separator: char) -> bool {
    matched.contains(separator)
        && replacement.contains(separator)
        && matched.split(separator).count() == replacement.split(separator).count()
}

/// `replacement`'s parts at `separator`, each in the case of `matched`'s
/// part in its place.
fn by_part(matched: &str, replacement: &str, separator: char) -> String {
    let parts: Vec<String> = (replacement.split(separator))
        .zip(matched.split(separator))
        .map(|(part, matched)| apply(matched, part))
        .collect();
    parts.join(&separator.to_string())
}

#[cfg(test)]
#[path = "preserve_case_tests.rs"]
mod tests;
