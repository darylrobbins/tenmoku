//! The anchored diagnostic set (§4.1, "Delivery to the UI").

use std::ops::Range;

use fe_buffer::{Bias, Buffer, BufferSnapshot, Origin};

use crate::diagnostics::{Diagnostic, DiagnosticSet, Severity};

/// Servers and identities, as a host might key them.
type Set = DiagnosticSet<u32, (u64, u64)>;

const A: (u64, u64) = (1, 1);
const B: (u64, u64) = (1, 2);

fn item(text: &str, word: &str, message: &str) -> (Range<usize>, Diagnostic) {
    let start = text.find(word).unwrap();
    (
        start..start + word.len(),
        Diagnostic::new(Severity::Error, message),
    )
}

/// What `server`'s diagnostics select in `snapshot`, with their messages.
fn shown(set: &Set, snapshot: &BufferSnapshot, server: u32) -> Vec<(String, String)> {
    set.server_entries(snapshot, server)
        .into_iter()
        .map(|entry| {
            (
                snapshot.text().slice_to_string(entry.range.clone()),
                entry.message.clone(),
            )
        })
        .collect()
}

fn edit(buffer: &mut Buffer, at: usize, text: &str) {
    buffer.edit([(at..at, text)], Origin::User).unwrap();
}

#[test]
fn anchored_ranges_rebase() {
    let mut buffer = Buffer::new("let alpha = beta;\n");
    let published = buffer.snapshot();
    let v = published.version();
    let text = published.text().to_string();
    let items = vec![item(&text, "alpha", "a"), item(&text, "beta", "b")];

    // Two edits after the version they were computed on.
    edit(&mut buffer, 0, "// one\n");
    let at = buffer.text().to_string().find("alpha").unwrap();
    edit(&mut buffer, at, "mut ");
    let now = buffer.snapshot();
    let mut set = Set::new();
    assert!(set.apply(&now, 1, A, v, items.clone(), Some(A)));
    let entries = set.server_entries(&now, 1);
    for (entry, (range, _)) in entries.iter().zip(&items) {
        assert_eq!(
            entry.range,
            now.map_offset(range.start, Bias::Right, v)..now.map_offset(range.end, Bias::Left, v)
        );
    }
    let words = |set: &Set, snapshot: &BufferSnapshot| {
        shown(set, snapshot, 1)
            .into_iter()
            .map(|(word, _)| word)
            .collect::<Vec<_>>()
    };
    assert_eq!(words(&set, &now), ["alpha", "beta"]);

    // They move with further edits, refreshed or not.
    edit(&mut buffer, 0, "  ");
    let at = buffer.text().to_string().find("beta").unwrap();
    edit(&mut buffer, at, "(");
    let later = buffer.snapshot();
    assert_eq!(words(&set, &later), ["alpha", "beta"]);
    set.refresh(&later);
    edit(&mut buffer, 0, "\n");
    assert_eq!(words(&set, &buffer.snapshot()), ["alpha", "beta"]);

    // A version below the edit log's floor changes nothing. (Anchors the
    // log is compacted past can't be resolved, so refresh first, as a model
    // does each frame.)
    let latest = buffer.snapshot();
    set.refresh(&latest);
    buffer.compact_log(latest.version());
    let compacted = buffer.snapshot();
    let below = vec![item(&text, "let", "stale")];
    assert!(!set.apply(&compacted, 1, B, later.version(), below, Some(B)));
    assert_eq!(words(&set, &compacted), ["alpha", "beta"]);
    assert_eq!(set.identity(1), Some(A));

    // Two servers on one buffer keep their own.
    let text = compacted.text().to_string();
    let v = compacted.version();
    assert!(set.apply(
        &compacted,
        2,
        A,
        v,
        vec![item(&text, "let", "two")],
        Some(A)
    ));
    assert_eq!(words(&set, &compacted), ["alpha", "beta"]);
    assert_eq!(shown(&set, &compacted, 2), [("let".into(), "two".into())]);
    assert!(set.clear(1, A));
    assert!(shown(&set, &compacted, 1).is_empty());
    assert_eq!(shown(&set, &compacted, 2), [("let".into(), "two".into())]);
    let all: Vec<_> = set
        .entries(&compacted)
        .into_iter()
        .map(|e| e.message.clone())
        .collect();
    assert_eq!(all, ["two"]);
}

#[test]
fn identity_checks() {
    let buffer = Buffer::new("one two\n");
    let snapshot = buffer.snapshot();
    let v = snapshot.version();
    let items = || vec![item("one two\n", "two", "message")];
    let mut set = Set::new();

    // Not the active identity, or none active.
    assert!(!set.apply(&snapshot, 1, A, v, items(), Some(B)));
    assert!(!set.apply(&snapshot, 1, A, v, items(), None));
    assert!(set.is_empty());

    // The active one.
    assert!(set.apply(&snapshot, 1, A, v, items(), Some(A)));
    assert_eq!(set.len(), 1);

    // Cleared for another identity: kept, but that one is now rejected.
    assert!(!set.clear(1, B));
    assert_eq!(set.len(), 1);
    assert!(!set.apply(&snapshot, 1, B, v, items(), Some(B)));
    assert_eq!(set.identity(1), Some(A));

    // Cleared for its own: emptied, and a later apply from it rejected.
    assert!(set.clear(1, A));
    assert!(set.is_empty());
    assert!(!set.apply(&snapshot, 1, A, v, items(), Some(A)));
    assert!(set.is_empty());
    assert_eq!(set.identity(1), None);
}
