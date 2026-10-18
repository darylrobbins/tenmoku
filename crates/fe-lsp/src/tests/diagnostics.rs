//! Published diagnostics (§4.1, "Diagnostics"): decoded against the ring's
//! text for the version they name, or dropped, and cleared once their
//! incarnation has been retired.

use std::sync::{Arc, Mutex};

use fe_buffer::{Buffer, Version};
use fe_text::Text;
use lsp_types::notification::PublishDiagnostics;
use lsp_types::{
    DiagnosticSeverity, NumberOrString, Position, PositionEncodingKind, PublishDiagnosticsParams,
    Range, Url,
};

use super::{Events, Started};
use crate::fake::{InitializeReply, Record, Script, initialize_result};
use crate::{Diagnostic, DiagnosticTag, IncarnationId, LspEvent, SessionId, Severity};

mod related;
mod uri_scheme;
mod uri_spelling;

/// What a [`LspEvent::Diagnostics`] carried.
#[derive(Debug)]
struct Published {
    buffer: fe_buffer::BufferId,
    identity: (SessionId, IncarnationId),
    version: Version,
    items: Vec<Diagnostic>,
}

/// The next diagnostics the sink gets.
fn next(events: &Events) -> Published {
    events.until("diagnostics", |event| match event {
        LspEvent::Diagnostics {
            buffer,
            session,
            incarnation,
            version,
            items,
            ..
        } => Some(Published {
            buffer,
            identity: (session, incarnation),
            version,
            items,
        }),
        _ => None,
    })
}

/// The LSP position of byte `offset` of `text`, counted as the protocol
/// says, not with fe-lsp's converter.
fn position(text: &str, offset: usize, utf16: bool) -> Position {
    let before = &text[..offset];
    let line = before.matches('\n').count();
    let column = &before[before.rfind('\n').map_or(0, |at| at + 1)..];
    let character = if utf16 {
        column.encode_utf16().count()
    } else {
        column.len()
    };
    Position::new(line as u32, character as u32)
}

/// A diagnostic over the first `word` in `text`, carrying `message`.
fn over(text: &str, word: &str, utf16: bool, message: &str) -> lsp_types::Diagnostic {
    let start = text.find(word).expect("the word is in the text");
    lsp_types::Diagnostic {
        range: Range::new(
            position(text, start, utf16),
            position(text, start + word.len(), utf16),
        ),
        message: message.into(),
        ..Default::default()
    }
}

fn publish(s: &Started, uri: &Url, version: Option<i32>, diagnostics: Vec<lsp_types::Diagnostic>) {
    s.fake
        .notify::<PublishDiagnostics>(PublishDiagnosticsParams {
            uri: uri.clone(),
            diagnostics,
            version,
        });
}

/// Publishes a marker for `uri` at `version` and asserts it's the next
/// diagnostics the sink gets: whatever was published before it was
/// dropped.
fn marker(s: &Started, uri: &Url, version: i32, text: &str, after: &str) {
    let message = format!("marker after {after}");
    publish(
        s,
        uri,
        Some(version),
        vec![over(text, &text[..1], false, &message)],
    );
    let next = next(&s.events);
    assert_eq!(next.items.len(), 1, "after {after}");
    assert_eq!(next.items[0].message, message, "{after} wasn't dropped");
}

fn script(encoding: PositionEncodingKind) -> Script {
    Script {
        initialize: InitializeReply::Result(Box::new(initialize_result(Some(encoding)))),
        ..Script::default()
    }
}

#[test]
fn versioned_publications_decode_against_the_ring() {
    for (encoding, utf16) in [
        (PositionEncodingKind::UTF8, false),
        (PositionEncodingKind::UTF16, true),
    ] {
        let mut s = Started::new(script(encoding.clone()));
        // The client's capabilities.
        let init = s
            .fake
            .records()
            .into_iter()
            .find_map(|record| match record {
                Record::Request { method, params, .. } if method == "initialize" => Some(params),
                _ => None,
            })
            .unwrap();
        let publish_caps = &init["capabilities"]["textDocument"]["publishDiagnostics"];
        assert_eq!(publish_caps["versionSupport"], true);
        assert_eq!(publish_caps["relatedInformation"], true);
        assert_eq!(publish_caps["codeDescriptionSupport"], true);
        assert_eq!(
            publish_caps["tagSupport"]["valueSet"],
            serde_json::json!([1, 2])
        );

        // Version 0, then two edits: one before the words on their line,
        // one replacing the second word.
        let v0 = "let 🦀 = alpha + beta;\n";
        let mut buffer = Buffer::new(v0);
        let at_v0 = buffer.version();
        let uri = s.open_synced(&mut buffer, "a.rs");
        s.edit(&mut buffer, &uri, (0..0, "🎉 "), 1);
        let beta = buffer.text().to_string().find("beta").unwrap();
        s.edit(&mut buffer, &uri, (beta..beta + 4, "gamma"), 2);
        let now = buffer.text().to_string();
        assert_eq!(now, "🎉 let 🦀 = alpha + gamma;\n");

        // Published for version 0, two edits old.
        let mut alpha = over(v0, "alpha", utf16, "unused alpha");
        alpha.severity = Some(DiagnosticSeverity::WARNING);
        alpha.source = Some("rustc".into());
        alpha.code = Some(NumberOrString::Number(42));
        alpha.tags = Some(vec![
            lsp_types::DiagnosticTag::UNNECESSARY,
            lsp_types::DiagnosticTag::DEPRECATED,
        ]);
        let mut beta = over(v0, "beta", utf16, "bad beta");
        beta.severity = Some(DiagnosticSeverity::ERROR);
        beta.code = Some(NumberOrString::String("E0425".into()));
        publish(&s, &uri, Some(0), vec![alpha, beta]);

        let published = next(&s.events);
        assert_eq!(published.buffer, buffer.id());
        assert_eq!(published.version, at_v0);
        assert_eq!(
            Some(published.identity),
            s.manager.active_identity(buffer.id(), s.server)
        );
        let text = Text::new(v0);
        let words: Vec<String> = (published.items.iter())
            .map(|item| text.slice_to_string(item.range.clone()))
            .collect();
        assert_eq!(words, ["alpha", "beta"], "{encoding:?}");
        let [alpha, beta] = &published.items[..] else {
            panic!("two items")
        };
        assert_eq!(alpha.severity, Some(Severity::Warning));
        assert_eq!(alpha.message, "unused alpha");
        assert_eq!(alpha.source.as_deref(), Some("rustc"));
        assert_eq!(alpha.code.as_deref(), Some("42"));
        assert_eq!(
            alpha.tags,
            [DiagnosticTag::Unnecessary, DiagnosticTag::Deprecated]
        );
        assert_eq!(beta.severity, Some(Severity::Error));
        assert_eq!(beta.message, "bad beta");
        assert_eq!(beta.source, None);
        assert_eq!(beta.code.as_deref(), Some("E0425"));
        assert!(beta.tags.is_empty());

        // The current version decodes against its own text.
        publish(&s, &uri, Some(2), vec![over(&now, "gamma", utf16, "gamma")]);
        let published = next(&s.events);
        assert_eq!(published.version, buffer.version());
        let range = published.items[0].range.clone();
        assert_eq!(&now[range], "gamma", "{encoding:?}");
    }
}

#[test]
fn dropped_publications() {
    let mut s = Started::new(Script::default());
    let mut buffer = Buffer::new("one two\n");
    let uri = s.open_synced(&mut buffer, "a.txt");
    let v0 = buffer.text().to_string();

    // Unversioned, before the first edit.
    publish(&s, &uri, None, vec![over(&v0, "two", false, "unversioned")]);
    marker(
        &s,
        &uri,
        0,
        &v0,
        "an unversioned publication before any edit",
    );

    s.edit(&mut buffer, &uri, (0..0, "zero "), 1);
    s.edit(&mut buffer, &uri, (0..0, "minus "), 2);
    let v2 = buffer.text().to_string();

    // Unversioned, after it.
    publish(&s, &uri, None, vec![over(&v2, "two", false, "unversioned")]);
    marker(&s, &uri, 2, &v2, "an unversioned publication after an edit");

    // Out of order: version 1 after version 2 was published.
    publish(&s, &uri, Some(1), vec![over(&v2, "two", false, "stale")]);
    marker(&s, &uri, 2, &v2, "an out-of-order publication");

    // Not in the ring: never sent.
    publish(&s, &uri, Some(7), vec![over(&v2, "two", false, "unsent")]);
    marker(&s, &uri, 2, &v2, "a version never sent");

    // Closed and reopened: the reopen goes on from version 3, and
    // publications for the old incarnation's versions are dropped.
    s.manager.close(buffer.id());
    s.fake
        .wait_for_document(&uri, "didClose", |r| r.is_some_and(|r| !r.open));
    // With no incarnation, ignored.
    publish(&s, &uri, Some(2), vec![over(&v2, "two", false, "closed")]);
    s.open_synced(&mut buffer, "a.txt");
    assert_eq!(s.fake.document(&uri).unwrap().version, 3);
    for old in [0, 1, 2] {
        publish(&s, &uri, Some(old), vec![over(&v2, "two", false, "old")]);
    }
    marker(&s, &uri, 3, &v2, "the old incarnation's versions");

    // An unversioned publication computed on the old incarnation's text,
    // sent after the new didOpen and before any didChange.
    s.manager.close(buffer.id());
    s.fake
        .wait_for_document(&uri, "didClose", |r| r.is_some_and(|r| !r.open));
    let old_text = buffer.text().to_string();
    buffer
        .edit([(0..0, "new ")], fe_buffer::Origin::User)
        .unwrap();
    let new_text = buffer.text().to_string();
    s.open_synced(&mut buffer, "a.txt");
    assert_eq!(s.fake.document(&uri).unwrap().text, new_text);
    publish(
        &s,
        &uri,
        None,
        vec![over(&old_text, "two", false, "old text")],
    );
    marker(
        &s,
        &uri,
        4,
        &new_text,
        "an unversioned publication across a reopen",
    );

    // A URI no document has open.
    let none = Url::from_file_path(format!("{}/none.txt", super::ROOT)).unwrap();
    publish(&s, &none, Some(0), vec![over(&v0, "one", false, "nobody")]);
    marker(&s, &uri, 4, &new_text, "a publication for a URI not open");
}

#[test]
fn cleared_after_retirement() {
    let mut s = Started::new(Script::default());
    // What the table says as each cleared event is reported.
    let seen = Arc::new(Mutex::new(Vec::new()));
    {
        let (identities, seen) = (s.manager.identities().clone(), seen.clone());
        s.events.probe(move |event| {
            if let LspEvent::DiagnosticsCleared {
                buffer,
                session,
                incarnation,
                server,
            } = event
            {
                let active = identities.active(*buffer, *server);
                seen.lock()
                    .unwrap()
                    .push(((*session, *incarnation), active));
            }
        });
    }
    let cleared = |events: &Events| {
        events.until("diagnostics cleared", |event| match event {
            LspEvent::DiagnosticsCleared {
                buffer,
                session,
                incarnation,
                ..
            } => Some((buffer, (session, incarnation))),
            _ => None,
        })
    };
    s.manager.record_handed(s.server);

    // After didClose.
    let mut a = Buffer::new("a\n");
    let a_uri = s.open_synced(&mut a, "a.txt");
    let first = s.manager.active_identity(a.id(), s.server).unwrap();
    s.manager.close(a.id());
    assert_eq!(cleared(&s.events), (a.id(), first));
    assert!(
        (s.manager.handed(s.server).iter()).any(|handed| *handed == format!("didClose {a_uri}")),
        "cleared before didClose was sent"
    );
    assert_eq!(seen.lock().unwrap().pop(), Some((first, None)));

    // After the session crashes.
    let mut b = Buffer::new("b\n");
    s.open_synced(&mut b, "b.txt");
    let second = s.manager.active_identity(b.id(), s.server).unwrap();
    s.fake.crash();
    assert_eq!(cleared(&s.events), (b.id(), second));
    let (identity, active) = seen.lock().unwrap().pop().unwrap();
    assert_eq!(identity, second);
    assert_ne!(active, Some(second));
    assert_ne!(s.manager.active_identity(b.id(), s.server), Some(second));
}

#[test]
fn cleared_after_a_panic() {
    let mut s = Started::new(Script::default());
    let mut buffer = Buffer::new("one two\n");
    let uri = s.open_synced(&mut buffer, "a.txt");
    let text = buffer.text().to_string();
    publish(&s, &uri, Some(0), vec![over(&text, "two", false, "shown")]);
    let shown = next(&s.events);
    assert_eq!(shown.items[0].message, "shown");

    // The actor panics: what it had shown is reported cleared, its
    // identity retired by then.
    s.fake.notify::<crate::router::Panic>(());
    let cleared = s.events.until("diagnostics cleared", |event| match event {
        LspEvent::DiagnosticsCleared {
            buffer,
            session,
            incarnation,
            ..
        } => Some((buffer, (session, incarnation))),
        _ => None,
    });
    assert_eq!(cleared, (buffer.id(), shown.identity));
    assert_ne!(
        s.manager.active_identity(buffer.id(), s.server),
        Some(shown.identity)
    );
}
