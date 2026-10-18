//! A diagnostic's `relatedInformation` and `codeDescription` link are kept,
//! pushed and pulled; a related location in the published document is in
//! bytes of its version.

use std::path::PathBuf;

use fe_buffer::Buffer;
use lsp_types::{
    CodeDescription, DiagnosticOptions, DiagnosticRelatedInformation, DiagnosticServerCapabilities,
    Location, Position, PositionEncodingKind, Range, Url,
};
use serde_json::json;

use super::{next, over, position, publish};
use crate::fake::{InitializeReply, Reply, Script, Scripted, initialize_result};
use crate::tests::{ROOT, Started};

#[test]
fn related_information_and_code_description_are_kept() {
    for (encoding, utf16) in [
        (PositionEncodingKind::UTF8, false),
        (PositionEncodingKind::UTF16, true),
    ] {
        for pulled in [false, true] {
            let what = format!("{encoding:?}, pulled: {pulled}");
            let mut result = initialize_result(Some(encoding.clone()));
            if pulled {
                result.capabilities.diagnostic_provider =
                    Some(DiagnosticServerCapabilities::Options(DiagnosticOptions {
                        identifier: Some("fake".into()),
                        inter_file_dependencies: true,
                        workspace_diagnostics: false,
                        ..Default::default()
                    }));
            }
            let script = Script {
                initialize: InitializeReply::Result(Box::new(result)),
                ..Script::default()
            };
            let mut s = Started::new(script);
            let text = "let 🦀 = alpha + beta;\n";
            let href = Url::parse("https://example.com/E0425").unwrap();
            let here_start = text.find("beta").unwrap();
            let here = Range::new(
                position(text, here_start, utf16),
                position(text, here_start + 4, utf16),
            );
            let other_uri = Url::from_file_path(format!("{ROOT}/b.rs")).unwrap();
            let mut diagnostic = over(text, "alpha", utf16, "bad alpha");
            diagnostic.code_description = Some(CodeDescription { href: href.clone() });

            let mut buffer = Buffer::new(text);
            if pulled {
                // Set before the open, which the fake's pull answers.
                let related = |uri: &Url| {
                    vec![
                        DiagnosticRelatedInformation {
                            location: Location::new(uri.clone(), here),
                            message: "declared here".into(),
                        },
                        DiagnosticRelatedInformation {
                            location: Location::new(
                                other_uri.clone(),
                                Range::new(Position::new(7, 2), Position::new(7, 5)),
                            ),
                            message: "expected because of this".into(),
                        },
                    ]
                };
                let uri_of = Url::from_file_path(PathBuf::from(ROOT).join("a.rs")).unwrap();
                diagnostic.related_information = Some(related(&uri_of));
                s.fake.set_reply(
                    "textDocument/diagnostic",
                    Scripted::now(Reply::Result(json!({
                        "kind": "full",
                        "items": [diagnostic],
                    }))),
                );
                s.open_synced(&mut buffer, "a.rs");
            } else {
                let uri = s.open_synced(&mut buffer, "a.rs");
                diagnostic.related_information = Some(vec![
                    DiagnosticRelatedInformation {
                        location: Location::new(uri.clone(), here),
                        message: "declared here".into(),
                    },
                    DiagnosticRelatedInformation {
                        location: Location::new(
                            other_uri.clone(),
                            Range::new(Position::new(7, 2), Position::new(7, 5)),
                        ),
                        message: "expected because of this".into(),
                    },
                ]);
                publish(&s, &uri, Some(0), vec![diagnostic]);
            }

            let published = next(&s.events);
            assert_eq!(published.version, buffer.version(), "{what}");
            let [item] = &published.items[..] else {
                panic!("one item, {what}")
            };
            assert_eq!(item.code_description, Some(href), "{what}");
            let [same, other] = &item.related[..] else {
                panic!("two related, {what}")
            };
            assert_eq!(same.range, Some(here_start..here_start + 4), "{what}");
            assert_eq!(same.message, "declared here", "{what}");
            assert_eq!(same.row, 0, "{what}");
            assert_eq!(other.range, None, "{what}");
            assert_eq!(other.path, Some(PathBuf::from("/fake/root/b.rs")), "{what}");
            assert_eq!(other.row, 7, "{what}");
            assert_eq!(other.message, "expected because of this", "{what}");
        }
    }
}
