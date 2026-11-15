//! JSON with comments, as VS Code writes its settings, snippets and
//! `launch.json`: [`strip_jsonc`] turns it into plain JSON for `serde_json`,
//! keeping every byte where it was, so an error's line and column still
//! point into the file as written.

/// `text` with its `//` and `/* */` comments, and any comma before a
/// closing `}` or `]`, turned to spaces, keeping every line where it was.
/// Strings are kept whole.
pub fn strip_jsonc(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    // A comma written with nothing but space and comments after it yet.
    let mut comma: Option<usize> = None;
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        let next = bytes.get(i + 1).copied();
        match b {
            b'"' => {
                comma = None;
                out.push(b);
                i += 1;
                while i < bytes.len() {
                    let c = bytes[i];
                    out.push(c);
                    i += 1;
                    if c == b'\\' && i < bytes.len() {
                        out.push(bytes[i]);
                        i += 1;
                    } else if c == b'"' || c == b'\n' {
                        break;
                    }
                }
            }
            b'/' if next == Some(b'/') => {
                while i < bytes.len() && bytes[i] != b'\n' {
                    out.push(blank(bytes[i]));
                    i += 1;
                }
            }
            b'/' if next == Some(b'*') => {
                out.extend_from_slice(b"  ");
                i += 2;
                while i < bytes.len() && !(bytes[i] == b'*' && bytes.get(i + 1) == Some(&b'/')) {
                    out.push(blank(bytes[i]));
                    i += 1;
                }
                if i < bytes.len() {
                    out.extend_from_slice(b"  ");
                    i += 2;
                }
            }
            b',' => {
                comma = Some(out.len());
                out.push(b);
                i += 1;
            }
            b'}' | b']' => {
                if let Some(at) = comma.take() {
                    out[at] = b' ';
                }
                out.push(b);
                i += 1;
            }
            b' ' | b'\t' | b'\n' | b'\r' => {
                out.push(b);
                i += 1;
            }
            _ => {
                comma = None;
                out.push(b);
                i += 1;
            }
        }
    }
    // Strings are copied whole, and only ASCII is written in place of
    // comments' bytes, so the text stays UTF-8.
    String::from_utf8(out).unwrap_or_default()
}

/// A comment's byte as it's blanked: line breaks kept, the rest spaces.
fn blank(b: u8) -> u8 {
    match b {
        b'\n' | b'\r' => b,
        _ => b' ',
    }
}
