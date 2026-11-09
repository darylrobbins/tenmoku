//! Writing Fluent source whose messages format back to their text byte for
//! byte, for the generated catalogue files.

/// Appends message `key` with `value` and `attributes` to `out`.
pub(crate) fn message(out: &mut String, key: &str, value: &str, attributes: &[(String, String)]) {
    out.push_str(key);
    out.push_str(" =");
    if !value.is_empty() || attributes.is_empty() {
        push_pattern(out, value, "    ");
    }
    for (name, text) in attributes {
        out.push_str("    .");
        out.push_str(name);
        out.push_str(" =");
        push_pattern(out, text, "        ");
    }
}

/// Appends ` <text>` on one line, or a newline and each line of `text`
/// indented by `indent`, then a newline.
fn push_pattern(out: &mut String, text: &str, indent: &str) {
    let lines: Vec<&str> = text.split('\n').collect();
    let last = lines.len() - 1;
    if lines.len() == 1 {
        out.push(' ');
        out.push_str(&line(lines[0], true));
        out.push('\n');
        return;
    }
    out.push('\n');
    for (index, text) in lines.into_iter().enumerate() {
        let escaped = line(text, index == 0 || index == last);
        if !escaped.is_empty() {
            out.push_str(indent);
            out.push_str(&escaped);
        }
        out.push('\n');
    }
}

/// `text` as one line of a pattern. What Fluent's syntax would read or
/// trim is a string literal placeable: `{` and `}`; leading and trailing
/// spaces, which indentation and the end of a pattern take; a first `[`,
/// `*` or `.`, which start a variant or an attribute; and an empty line at
/// either end of the pattern, which is trimmed.
fn line(text: &str, edge: bool) -> String {
    if text.is_empty() {
        return if edge { literal("") } else { String::new() };
    }
    let body = text.trim_start_matches(' ');
    let lead = &text[..text.len() - body.len()];
    let inner = body.trim_end_matches(' ');
    let trail = &body[inner.len()..];
    let mut out = String::new();
    if !lead.is_empty() {
        out.push_str(&literal(lead));
    }
    for (at, c) in inner.char_indices() {
        match c {
            '{' | '}' => out.push_str(&literal(&c.to_string())),
            '[' | '*' | '.' if at == 0 && lead.is_empty() => out.push_str(&literal(&c.to_string())),
            _ => out.push(c),
        }
    }
    if !trail.is_empty() {
        out.push_str(&literal(trail));
    }
    out
}

/// `{"text"}`.
fn literal(text: &str) -> String {
    let mut out = String::from("{\"");
    for c in text.chars() {
        if matches!(c, '"' | '\\') {
            out.push('\\');
        }
        out.push(c);
    }
    out.push_str("\"}");
    out
}

/// Whether `name` can follow `settings-` in a key: ASCII letters, digits
/// and `_`, so no two paths make one key.
pub(crate) fn is_key_segment(name: &str) -> bool {
    !name.is_empty() && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
}

/// Whether `name` is a Fluent attribute name.
pub(crate) fn is_identifier(name: &str) -> bool {
    let mut bytes = name.bytes();
    bytes.next().is_some_and(|b| b.is_ascii_alphabetic())
        && bytes.all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}
