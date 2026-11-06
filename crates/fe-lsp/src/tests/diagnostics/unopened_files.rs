//! A publication for a file the session has no document for is reported as
//! sent, for the host to decode against the file's text.

use std::path::PathBuf;

use fe_buffer::Buffer;
use fe_text::Text;
use lsp_types::{DiagnosticSeverity, NumberOrString, PositionEncodingKind, Url};

use super::{over, publish, script};
use crate::tests::Started;
use crate::{
    DiagnosticTag, FileDiagnostic, LspEvent, PositionEncoding, Severity, decode_file_diagnostics,
    from_lsp,
};

/// The next `FileDiagnostics` the sink gets.
fn next_file(s: &Started) -> (Url, Option<PathBuf>, PositionEncoding, Vec<FileDiagnostic>) {
    s.events.until("file diagnostics", |event| match event {
        LspEvent::FileDiagnostics {
            uri,
            path,
            encoding,
            items,
            ..
        } => Some((uri, path, encoding, items)),
        _ => None,
    })
}

#[test]
fn unopened_files_diagnostics_are_reported_raw() {
    for (kind, encoding, utf16) in [
        (PositionEncodingKind::UTF8, PositionEncoding::Utf8, false),
        (PositionEncodingKind::UTF16, PositionEncoding::Utf16, true),
    ] {
        let mut s = Started::new(script(kind));
        let mut open = Buffer::new("fn main() {}\n");
        s.open_synced(&mut open, "a.rs");

        let text = "let a = 1;\nlet 🦀 = beta;\n";
        let mut first = over(text, "a", utf16, "unused a");
        first.severity = Some(DiagnosticSeverity::WARNING);
        first.source = Some("rustc".into());
        first.code = Some(NumberOrString::Number(7));
        first.tags = Some(vec![lsp_types::DiagnosticTag::UNNECESSARY]);
        let second = over(text, "beta", utf16, "bad beta");
        let uri = Url::from_file_path(crate::fake::fake_path("b.rs")).unwrap();
        publish(&s, &uri, None, vec![first.clone(), second.clone()]);

        let (got, path, got_encoding, items) = next_file(&s);
        assert_eq!(got, uri);
        assert_eq!(path, Some(crate::fake::fake_path("b.rs")));
        assert_eq!(got_encoding, encoding);
        let [one, two] = &items[..] else {
            panic!("two items")
        };
        assert_eq!(one.range, first.range);
        assert_eq!(two.range, second.range);
        assert_eq!(one.severity, Some(Severity::Warning));
        assert_eq!(one.source.as_deref(), Some("rustc"));
        assert_eq!(one.code.as_deref(), Some("7"));
        assert_eq!(one.tags, [DiagnosticTag::Unnecessary]);
        assert_eq!(two.severity, None);

        let decoded = decode_file_diagnostics(&Text::new(text), &items, encoding);
        let beta = text.find("beta").unwrap();
        assert_eq!(decoded[1].range, beta..beta + 4);
        assert_eq!(
            decoded[1].range.start,
            from_lsp(&Text::new(text), second.range.start, encoding)
        );
        assert_eq!(decoded[0].message, "unused a");
        assert_eq!(decoded[0].code.as_deref(), Some("7"));

        publish(&s, &uri, Some(3), vec![]);
        let (got, _, _, items) = next_file(&s);
        assert_eq!(got, uri);
        assert!(items.is_empty());

        let custom = Url::parse("custom:///fake/root/c.rs").unwrap();
        publish(&s, &custom, None, vec![second]);
        let (got, path, _, items) = next_file(&s);
        assert_eq!(got, custom);
        assert_eq!(path, None);
        assert_eq!(items.len(), 1);
    }
}

#[test]
fn open_documents_are_unchanged() {
    let mut s = Started::new(script(PositionEncodingKind::UTF16));
    let text = "one two\n";
    let mut buffer = Buffer::new(text);
    let uri = s.open_synced(&mut buffer, "a.rs");
    publish(&s, &uri, Some(0), vec![over(text, "two", true, "two")]);
    let saw_file = s.events.until("diagnostics", |event| match event {
        LspEvent::FileDiagnostics { .. } => Some(true),
        LspEvent::Diagnostics { .. } => Some(false),
        _ => None,
    });
    assert!(!saw_file, "an open document gets Diagnostics");
}
