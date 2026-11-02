//! Previewing workspace edits: fe-lsp's fake server over a temporary
//! project (the LSP tests' [`Lsp`]), its edits decoded by the workbench's
//! manager as a rename's answer is, then previewed.

use std::{
    cell::RefCell,
    fs,
    path::{Path, PathBuf},
};

use fe_buffer::{BufferId, HunkStatus};
use fe_lsp::{EditTarget, FileEdits, ServerId};
use gpui::TestAppContext;
use lsp_types::{Position, Range, TextEdit};
use serde_json::{Value, json};

use super::{PreviewError, RefactorPreview};
use crate::tests::lsp::Lsp;

/// `src/main.rs`, open on the server.
const MAIN: &str = "fn main() {\n    f();\n}\n";
/// `src/a.rs`, closed unless a test opens it.
const A: &str = "pub fn a() {}\n";
/// `src/utf8.rs`, closed.
const UTF8: &str = "// naïve\npub fn f() {}\n";

/// An edit replacing `start..end`, each `(line, character)`, with `text`.
fn edit(start: (u32, u32), end: (u32, u32), text: &str) -> TextEdit {
    TextEdit {
        range: Range {
            start: Position::new(start.0, start.1),
            end: Position::new(end.0, end.1),
        },
        new_text: text.into(),
    }
}

/// The project with its files, `src/main.rs` open on the server, that
/// server, and main.rs's buffer.
fn setup(cx: &mut TestAppContext) -> (Lsp, ServerId, BufferId) {
    let mut s = Lsp::new(cx);
    fs::write(s.path("src/main.rs"), MAIN).unwrap();
    fs::write(s.path("src/a.rs"), A).unwrap();
    fs::write(s.path("src/utf8.rs"), UTF8).unwrap();
    s.open("src/main.rs");
    (s.fake)
        .wait_for_document(&s.uri("src/main.rs"), "didOpen", |r| r.is_some())
        .unwrap();
    let server = s
        .active_server()
        .flatten()
        .expect("main.rs went to a server");
    let main = buffer_shown(&mut s);
    (s, server, main)
}

/// The buffer of the active editor.
fn buffer_shown(s: &mut Lsp) -> BufferId {
    let editor = (s.workbench)
        .read_with(&s.cx, |w, cx| w.active_editor(cx).cloned())
        .expect("a tab is shown");
    editor.read_with(&s.cx, |view, _| view.model().buffer().id())
}

/// The active editor's text.
fn text_shown(s: &mut Lsp) -> String {
    let editor = (s.workbench)
        .read_with(&s.cx, |w, cx| w.active_editor(cx).cloned())
        .expect("a tab is shown");
    editor.read_with(&s.cx, |view, _| view.model().text().to_string())
}

/// A `TextDocumentEdit` of `name` with no version.
fn edit_of(s: &Lsp, name: &str, edits: Vec<TextEdit>) -> Value {
    json!({
        "textDocument": { "uri": s.uri(name), "version": null },
        "edits": edits,
    })
}

/// `changes`, a `documentChanges` array, decoded against what `server` has
/// open.
fn decode(s: &Lsp, server: ServerId, changes: Value) -> Vec<FileEdits> {
    let edit = serde_json::from_value(json!({ "documentChanges": changes })).unwrap();
    (s.workbench)
        .read_with(&s.cx, |w, _| w.lsp_decode_edit(server, edit))
        .expect("decoded")
}

/// Each file's text as it is on disk.
fn on_disk(path: &Path, _: &EditTarget) -> Result<String, String> {
    fs::read_to_string(path).map_err(|error| format!("{}: {error}", path.display()))
}

/// `edits` previewed, each file's text read from disk, and the paths asked
/// for.
fn preview(edits: Vec<FileEdits>) -> (RefactorPreview, Vec<PathBuf>) {
    let asked = RefCell::new(Vec::new());
    let preview = RefactorPreview::new("Rename", edits, |path, target| {
        asked.borrow_mut().push(path.to_path_buf());
        on_disk(path, target)
    })
    .expect("previewed");
    (preview, asked.into_inner())
}

/// Each listed file's hunks, as `(old, new, status)`.
fn hunks(preview: &RefactorPreview, path: &Path) -> Vec<(String, String, HunkStatus)> {
    (preview.hunks(path).into_iter())
        .map(|hunk| {
            (
                hunk.old_text.to_string(),
                hunk.new_text.to_string(),
                hunk.status,
            )
        })
        .collect()
}

fn listed(preview: &mut RefactorPreview) -> Vec<PathBuf> {
    preview.files().into_iter().map(|file| file.path).collect()
}

#[gpui::test]
fn previewed_only_beyond_the_source_buffer(cx: &mut TestAppContext) {
    let (mut s, server, main) = setup(cx);
    fs::write(s.path("src/b.rs"), "pub fn b() {}\n").unwrap();
    s.open("src/b.rs");
    (s.fake)
        .wait_for_document(&s.uri("src/b.rs"), "didOpen", |r| r.is_some())
        .unwrap();
    let b = buffer_shown(&mut s);
    assert_ne!(b, main);

    // Only main.rs: applied at once, as before.
    let only_main = decode(
        &s,
        server,
        json!([edit_of(
            &s,
            "src/main.rs",
            vec![edit((0, 3), (0, 7), "start"), edit((1, 4), (1, 5), "g")]
        )]),
    );
    assert!(matches!(only_main[0].target, EditTarget::Open { buffer, .. } if buffer == main));
    assert!(!RefactorPreview::needs_preview(&only_main, main));
    assert!(!RefactorPreview::needs_preview(&[], main));
    // Asked from b.rs, the same edit changes another buffer.
    assert!(RefactorPreview::needs_preview(&only_main, b));

    // Another open buffer.
    let open_other = decode(
        &s,
        server,
        json!([
            edit_of(&s, "src/main.rs", vec![edit((0, 3), (0, 7), "start")]),
            edit_of(&s, "src/b.rs", vec![edit((0, 7), (0, 8), "start")]),
        ]),
    );
    assert!(matches!(open_other[1].target, EditTarget::Open { buffer, .. } if buffer == b));
    assert!(RefactorPreview::needs_preview(&open_other, main));

    // A closed file.
    let closed = decode(
        &s,
        server,
        json!([edit_of(&s, "src/utf8.rs", vec![edit((1, 7), (1, 8), "g")])]),
    );
    assert!(matches!(closed[0].target, EditTarget::Closed { .. }));
    assert!(RefactorPreview::needs_preview(&closed, main));

    // Edits that depend on a create that may be skipped.
    let depends = decode(
        &s,
        server,
        json!([
            // Made, main.rs is emptied first; skipped, it's main.rs's buffer.
            { "kind": "create", "uri": s.uri("src/main.rs"), "options": { "ignoreIfExists": true } },
            edit_of(&s, "src/main.rs", vec![edit((0, 0), (0, 0), "// x\n")]),
        ]),
    );
    assert!(matches!(depends[1].target, EditTarget::Depends { .. }));
    assert!(RefactorPreview::needs_preview(&depends[1..], main));

    // Each file operation, alone or with edits to main.rs only.
    for operation in [
        json!({ "kind": "create", "uri": s.uri("src/c.rs") }),
        json!({ "kind": "rename", "oldUri": s.uri("src/a.rs"), "newUri": s.uri("src/d.rs") }),
        json!({ "kind": "delete", "uri": s.uri("src/a.rs") }),
    ] {
        let alone = decode(&s, server, json!([operation.clone()]));
        assert!(alone[0].operation.is_some());
        assert!(RefactorPreview::needs_preview(&alone, main), "{operation}");
        let with_main = decode(
            &s,
            server,
            json!([
                edit_of(&s, "src/main.rs", vec![edit((0, 3), (0, 7), "start")]),
                operation.clone()
            ]),
        );
        assert!(
            RefactorPreview::needs_preview(&with_main, main),
            "{operation}"
        );
    }
}

#[gpui::test]
fn each_file_is_listed_once_with_a_hunk_per_edit(cx: &mut TestAppContext) {
    let (mut s, server, _) = setup(cx);
    let edits = decode(
        &s,
        server,
        json!([
            // Out of text order, two inserts at one place, and a no-op.
            edit_of(
                &s,
                "src/utf8.rs",
                vec![
                    edit((1, 7), (1, 8), "g"),
                    edit((0, 0), (0, 0), "// a\n"),
                    edit((0, 0), (0, 0), "// b\n"),
                    edit((1, 0), (1, 3), "pub"),
                ]
            ),
            edit_of(
                &s,
                "src/main.rs",
                vec![
                    edit((0, 3), (0, 7), "start"),
                    edit((1, 4), (1, 5), "f"),
                    edit((2, 1), (2, 1), "// end"),
                ]
            ),
        ]),
    );
    assert!(matches!(edits[0].target, EditTarget::Closed { .. }));
    assert!(matches!(edits[1].target, EditTarget::Open { .. }));
    let (mut preview, asked) = preview(edits);
    assert_eq!(asked, [s.path("src/utf8.rs"), s.path("src/main.rs")]);
    assert!(preview.is_decidable());
    assert_eq!(preview.title(), "Rename");
    assert_eq!(
        listed(&mut preview),
        [s.path("src/utf8.rs"), s.path("src/main.rs")]
    );

    let pending = HunkStatus::Pending;
    let own = |old: &str, new: &str| (old.to_owned(), new.to_owned(), pending);
    assert_eq!(
        hunks(&preview, &s.path("src/utf8.rs")),
        [own("", "// a\n// b\n"), own("pub", "pub"), own("f", "g")]
    );
    assert_eq!(
        hunks(&preview, &s.path("src/main.rs")),
        [own("main", "start"), own("f", "f"), own("", "// end")]
    );
    let summary = preview.summary();
    assert_eq!(
        (summary.files, summary.counts.hunks, summary.counts.pending),
        (2, 6, 6)
    );
    let utf8 = preview
        .buffer(&s.path("src/utf8.rs"))
        .expect("a scratch copy");
    assert_eq!(utf8.text().to_string(), UTF8);

    // Keeping everything changes the scratch copies, not the files or the
    // open buffer.
    let accepted = preview.accept_all().unwrap();
    assert!(
        accepted.iter().all(|(_, result)| result.is_ok()),
        "{accepted:?}"
    );
    let main = preview
        .buffer(&s.path("src/main.rs"))
        .expect("a scratch copy");
    assert_eq!(main.text().to_string(), "fn start() {\n    f();\n}// end\n");
    assert_eq!(preview.summary().counts.accepted, 6);
    assert!(preview.changeset().is_resolved());
    s.draw();
    assert_eq!(text_shown(&mut s), MAIN);
    assert_eq!(fs::read_to_string(s.path("src/main.rs")).unwrap(), MAIN);
    assert_eq!(fs::read_to_string(s.path("src/utf8.rs")).unwrap(), UTF8);
}

#[gpui::test]
fn edits_that_dont_fit_the_text_are_refused(cx: &mut TestAppContext) {
    let (s, server, _) = setup(cx);
    let edits = decode(
        &s,
        server,
        json!([edit_of(&s, "src/main.rs", vec![edit((2, 0), (2, 1), "")])]),
    );
    // The buffer moved on: shorter than the edits.
    let short = RefactorPreview::new("Rename", edits.clone(), |_, _| Ok("fn".into()));
    let error = short.err().expect("refused");
    assert!(error.contains("doesn't fit"), "{error}");
    let unread = RefactorPreview::new("Rename", edits, |_, _| Err("gone".into()));
    assert_eq!(unread.err().as_deref(), Some("gone"));
}

#[gpui::test]
fn selected_leaves_out_what_was_rejected(cx: &mut TestAppContext) {
    let (s, server, _) = setup(cx);
    let edits = decode(
        &s,
        server,
        json!([
            edit_of(
                &s,
                "src/main.rs",
                vec![
                    edit((0, 3), (0, 7), "start"),
                    edit((1, 4), (1, 5), "g"),
                    edit((2, 1), (2, 1), "// end"),
                ]
            ),
            edit_of(&s, "src/a.rs", vec![edit((0, 7), (0, 8), "g")]),
            edit_of(
                &s,
                "src/utf8.rs",
                vec![
                    edit((1, 7), (1, 8), "g"),
                    edit((0, 0), (0, 0), "// a\n"),
                    edit((1, 0), (1, 3), "pub(crate)"),
                    edit((0, 0), (0, 0), "// b\n"),
                ]
            ),
        ]),
    );
    let (mut preview, _) = preview(edits.clone());
    assert_eq!(preview.selected(), edits);
    // Kept, pending or accepted alike.
    let main = s.path("src/main.rs");
    let first = preview.hunks(&main)[0].id;
    preview.accept_hunk(&main, first).unwrap();
    assert_eq!(preview.selected(), edits);

    // main.rs's second edit, a.rs whole, and utf8.rs's inserts (one hunk)
    // and its last edit.
    let second = preview.hunks(&main)[1].id;
    preview.reject_hunk(&main, second).unwrap();
    preview.reject_file(&s.path("src/a.rs")).unwrap();
    let utf8 = s.path("src/utf8.rs");
    let [inserts, _, last] = &preview.hunks(&utf8)[..] else {
        panic!("three hunks in utf8.rs");
    };
    assert_eq!(&*inserts.new_text, "// a\n// b\n");
    preview.reject_hunk(&utf8, inserts.id).unwrap();
    preview.reject_hunk(&utf8, last.id).unwrap();
    // Decided once only.
    assert!(preview.reject_hunk(&utf8, last.id).is_err());

    let mut expected = vec![edits[0].clone(), edits[2].clone()];
    let EditTarget::Open {
        edits: main_edits, ..
    } = &mut expected[0].target
    else {
        panic!("main.rs is open");
    };
    main_edits.remove(1);
    let EditTarget::Closed {
        edits: utf8_edits, ..
    } = &mut expected[1].target
    else {
        panic!("utf8.rs is closed");
    };
    // Its remaining edit.
    *utf8_edits = vec![edit((1, 0), (1, 3), "pub(crate)")];
    let selected = preview.selected();
    assert_eq!(selected, expected);
    // Buffer and version kept, and the encoding.
    let (
        EditTarget::Open {
            buffer, version, ..
        },
        EditTarget::Open {
            buffer: b,
            version: v,
            ..
        },
    ) = (&selected[0].target, &edits[0].target)
    else {
        panic!("main.rs is open");
    };
    assert_eq!((buffer, version), (b, v));
    let (EditTarget::Closed { encoding, .. }, EditTarget::Closed { encoding: e, .. }) =
        (&selected[1].target, &edits[2].target)
    else {
        panic!("utf8.rs is closed");
    };
    assert_eq!(encoding, e);

    // Every hunk still pending rejected: only main.rs's accepted one is
    // left.
    preview.reject_all().unwrap();
    let EditTarget::Open {
        edits: main_edits, ..
    } = &mut expected[0].target
    else {
        panic!("main.rs is open");
    };
    main_edits.truncate(1);
    assert_eq!(preview.selected(), expected[..1]);
}

#[gpui::test]
fn closed_edits_keep_their_order(cx: &mut TestAppContext) {
    let (s, server, _) = setup(cx);
    let edits = decode(
        &s,
        server,
        json!([edit_of(
            &s,
            "src/utf8.rs",
            vec![
                edit((1, 7), (1, 8), "g"),
                edit((0, 0), (0, 0), "// a\n"),
                edit((1, 0), (1, 3), "pub(crate)"),
            ]
        )]),
    );
    let (mut preview, _) = preview(edits.clone());
    let utf8 = s.path("src/utf8.rs");
    let views = preview.hunks(&utf8);
    assert_eq!(
        (views.iter())
            .map(|hunk| &*hunk.new_text)
            .collect::<Vec<_>>(),
        ["// a\n", "pub(crate)", "g"]
    );
    preview.reject_hunk(&utf8, views[1].id).unwrap();
    let selected = preview.selected();
    let (EditTarget::Closed { edits: kept, .. }, EditTarget::Closed { edits: all, .. }) =
        (&selected[0].target, &edits[0].target)
    else {
        panic!("utf8.rs is closed");
    };
    // The others, in the entry's order.
    let mut expected = all.clone();
    expected.retain(|edit| edit.new_text != "pub(crate)");
    assert_eq!(kept, &expected);
}

#[gpui::test]
fn file_operations_are_previewed_whole(cx: &mut TestAppContext) {
    let (s, server, _) = setup(cx);
    let edits = decode(
        &s,
        server,
        json!([
            edit_of(&s, "src/utf8.rs", vec![edit((1, 7), (1, 8), "g")]),
            { "kind": "create", "uri": s.uri("src/c.rs") },
            edit_of(&s, "src/c.rs", vec![edit((0, 0), (0, 0), "pub fn c() {}\n")]),
            { "kind": "rename", "oldUri": s.uri("src/a.rs"), "newUri": s.uri("src/b.rs") },
            edit_of(&s, "src/b.rs", vec![edit((0, 7), (0, 8), "b")]),
            { "kind": "delete", "uri": s.uri("src/utf8.rs") },
        ]),
    );
    let (mut preview, asked) = preview(edits.clone());
    // c.rs is made by the edit, and b.rs is a.rs renamed.
    assert_eq!(asked, [s.path("src/utf8.rs"), s.path("src/a.rs")]);
    assert!(!preview.is_decidable());
    let (utf8, c, b) = (
        s.path("src/utf8.rs"),
        s.path("src/c.rs"),
        s.path("src/b.rs"),
    );
    assert_eq!(listed(&mut preview), [utf8.clone(), c.clone(), b.clone()]);
    let pending = HunkStatus::Pending;
    assert_eq!(
        hunks(&preview, &c),
        [("".into(), "pub fn c() {}\n".into(), pending)]
    );
    assert_eq!(hunks(&preview, &b), [("a".into(), "b".into(), pending)]);

    let hunk = preview.hunks(&utf8)[0].id;
    let refused = Err(PreviewError::NotDecidable);
    assert_eq!(preview.accept_hunk(&utf8, hunk), refused);
    assert_eq!(preview.reject_hunk(&utf8, hunk), refused);
    assert_eq!(preview.accept_file(&utf8), refused);
    assert_eq!(preview.reject_file(&utf8), refused);
    assert_eq!(preview.accept_all().map(drop), refused);
    assert_eq!(preview.reject_all().map(drop), refused);
    assert_eq!(preview.summary().counts.pending, 3);
    assert_eq!(preview.buffer(&utf8).unwrap().text().to_string(), UTF8);
    assert_eq!(preview.selected(), edits);
    assert!(s.path("src/a.rs").exists() && !s.path("src/c.rs").exists());
}

#[gpui::test]
fn dependent_edits_are_previewed_whole(cx: &mut TestAppContext) {
    let (s, server, _) = setup(cx);
    let edits = decode(
        &s,
        server,
        json!([
            // Made, main.rs is emptied first; skipped, it's main.rs's buffer.
            { "kind": "create", "uri": s.uri("src/main.rs"), "options": { "ignoreIfExists": true } },
            edit_of(&s, "src/main.rs", vec![edit((0, 0), (0, 0), "// x\n")]),
        ]),
    );
    assert!(matches!(edits[1].target, EditTarget::Depends { .. }));
    let (mut preview, asked) = preview(edits.clone());
    assert!(!preview.is_decidable());
    // Shown as made: a new file, not read.
    assert!(asked.is_empty(), "{asked:?}");
    let new = s.path("src/main.rs");
    assert_eq!(listed(&mut preview), std::slice::from_ref(&new));
    let pending = HunkStatus::Pending;
    assert_eq!(
        hunks(&preview, &new),
        [("".into(), "// x\n".into(), pending)]
    );
    let hunk = preview.hunks(&new)[0].id;
    assert_eq!(
        preview.reject_hunk(&new, hunk),
        Err(PreviewError::NotDecidable)
    );
    assert_eq!(preview.selected(), edits);
}

#[gpui::test]
fn a_file_edited_around_an_operation_is_one_diff(cx: &mut TestAppContext) {
    let (s, server, _) = setup(cx);
    let edits = decode(
        &s,
        server,
        json!([
            edit_of(&s, "src/main.rs", vec![edit((0, 3), (0, 7), "start")]),
            { "kind": "create", "uri": s.uri("src/x.rs") },
            edit_of(&s, "src/main.rs", vec![edit((1, 4), (1, 5), "g"), edit((2, 1), (2, 1), "// end")]),
        ]),
    );
    let (mut preview, asked) = preview(edits.clone());
    let main = s.path("src/main.rs");
    assert_eq!(asked, std::slice::from_ref(&main));
    assert_eq!(listed(&mut preview), std::slice::from_ref(&main));
    // Line by line, from its text before to after both runs.
    let views = preview.hunks(&main);
    let old: String = views.iter().map(|hunk| &*hunk.old_text).collect();
    let new: String = views.iter().map(|hunk| &*hunk.new_text).collect();
    assert_eq!(old, MAIN);
    assert_eq!(new, "fn start() {\n    g();\n}// end\n");
    assert_eq!(preview.selected(), edits);
}

/// `path`'s text as the preview shows it once its hunks apply: its scratch
/// copy with each pending hunk's new text in place.
fn previewed_text(preview: &RefactorPreview, path: &Path) -> String {
    let base = preview.buffer(path).expect("listed").text().to_string();
    let mut out = String::new();
    let mut copied = 0;
    for hunk in preview.hunks(path) {
        assert_eq!(hunk.status, HunkStatus::Pending);
        out.push_str(&base[copied..hunk.range.start]);
        out.push_str(&hunk.new_text);
        copied = hunk.range.end;
    }
    out.push_str(&base[copied..]);
    out
}

#[gpui::test]
fn an_overwritten_file_is_shown_with_its_new_text(cx: &mut TestAppContext) {
    let (s, server, _) = setup(cx);
    let edits = decode(
        &s,
        server,
        json!([
            edit_of(&s, "src/utf8.rs", vec![edit((1, 7), (1, 8), "g")]),
            { "kind": "create", "uri": s.uri("src/utf8.rs"), "options": { "overwrite": true } },
            edit_of(&s, "src/utf8.rs", vec![edit((0, 0), (0, 0), "fresh\n")]),
        ]),
    );
    let (mut preview, asked) = preview(edits);
    let utf8 = s.path("src/utf8.rs");
    assert_eq!(asked, std::slice::from_ref(&utf8));
    assert_eq!(listed(&mut preview), std::slice::from_ref(&utf8));
    assert_eq!(preview.buffer(&utf8).unwrap().text().to_string(), UTF8);
    assert_eq!(previewed_text(&preview, &utf8), "fresh\n");
}

#[gpui::test]
fn a_deleted_and_recreated_file_starts_empty(cx: &mut TestAppContext) {
    let (s, server, _) = setup(cx);
    let edits = decode(
        &s,
        server,
        json!([
            { "kind": "delete", "uri": s.uri("src/a.rs") },
            { "kind": "create", "uri": s.uri("src/a.rs") },
            edit_of(&s, "src/a.rs", vec![edit((0, 0), (0, 0), "pub fn n() {}\n")]),
        ]),
    );
    let (mut preview, asked) = preview(edits);
    let a = s.path("src/a.rs");
    assert!(asked.is_empty(), "{asked:?}");
    assert_eq!(listed(&mut preview), std::slice::from_ref(&a));
    assert_eq!(preview.buffer(&a).unwrap().text().to_string(), "");
    assert_eq!(previewed_text(&preview, &a), "pub fn n() {}\n");
}

#[gpui::test]
fn a_rename_over_an_edited_file_brings_its_text(cx: &mut TestAppContext) {
    let (s, server, _) = setup(cx);
    let edits = decode(
        &s,
        server,
        json!([
            edit_of(&s, "src/utf8.rs", vec![edit((1, 7), (1, 8), "g")]),
            {
                "kind": "rename",
                "oldUri": s.uri("src/a.rs"),
                "newUri": s.uri("src/utf8.rs"),
                "options": { "overwrite": true },
            },
            edit_of(&s, "src/utf8.rs", vec![edit((0, 7), (0, 8), "b")]),
        ]),
    );
    let (mut preview, asked) = preview(edits);
    let utf8 = s.path("src/utf8.rs");
    assert_eq!(asked, [utf8.clone(), s.path("src/a.rs")]);
    assert_eq!(listed(&mut preview), std::slice::from_ref(&utf8));
    // From its own text to a.rs's, edited.
    assert_eq!(preview.buffer(&utf8).unwrap().text().to_string(), UTF8);
    assert_eq!(previewed_text(&preview, &utf8), "pub fn b() {}\n");
}

#[gpui::test]
fn a_renamed_open_buffer_keeps_its_edits(cx: &mut TestAppContext) {
    let (mut s, server, _) = setup(cx);
    s.open("src/a.rs");
    (s.fake)
        .wait_for_document(&s.uri("src/a.rs"), "didOpen", |r| r.is_some())
        .unwrap();
    let a_buffer = buffer_shown(&mut s);
    // Both runs are for the version the server has: the second moves past
    // the first.
    let edits = decode(
        &s,
        server,
        json!([
            edit_of(&s, "src/a.rs", vec![edit((0, 7), (0, 8), "xyz")]),
            { "kind": "rename", "oldUri": s.uri("src/a.rs"), "newUri": s.uri("src/b.rs") },
            edit_of(&s, "src/b.rs", vec![edit((0, 11), (0, 13), "{ 1 }")]),
        ]),
    );
    assert!(matches!(edits[2].target, EditTarget::Open { buffer, .. } if buffer == a_buffer));
    let (mut preview, asked) = preview(edits);
    let (a, b) = (s.path("src/a.rs"), s.path("src/b.rs"));
    assert_eq!(asked, std::slice::from_ref(&a));
    assert_eq!(listed(&mut preview), [a.clone(), b.clone()]);
    assert_eq!(previewed_text(&preview, &a), "pub fn xyz() {}\n");
    assert_eq!(
        preview.buffer(&b).unwrap().text().to_string(),
        "pub fn xyz() {}\n"
    );
    assert_eq!(previewed_text(&preview, &b), "pub fn xyz() { 1 }\n");
}

/// Inserts at one place with a `\r` and a `\n`: the preview's text is what
/// applying makes, a closed file's each normalized first, an open one's
/// joined first.
#[gpui::test]
fn line_endings_preview_as_they_apply(cx: &mut TestAppContext) {
    let (mut s, server, _) = setup(cx);
    let edits = decode(
        &s,
        server,
        json!([
            edit_of(
                &s,
                "src/utf8.rs",
                vec![edit((1, 0), (1, 0), "\r"), edit((1, 0), (1, 0), "\n")]
            ),
            edit_of(
                &s,
                "src/main.rs",
                vec![edit((1, 0), (1, 0), "\r"), edit((1, 0), (1, 0), "\n")]
            ),
        ]),
    );
    let (preview, _) = preview(edits.clone());
    let (utf8, main) = (s.path("src/utf8.rs"), s.path("src/main.rs"));
    let shown_utf8 = previewed_text(&preview, &utf8);
    let shown_main = previewed_text(&preview, &main);
    assert_eq!(shown_utf8, "// naïve\n\n\npub fn f() {}\n");
    assert_eq!(shown_main, "fn main() {\n\n    f();\n}\n");

    let workbench = s.workbench.clone();
    let applied = workbench.update(&mut s.cx, |w, cx| w.apply_file_edits(edits, cx));
    assert!(applied.is_ok(), "{applied:?}");
    s.draw();
    assert_eq!(fs::read_to_string(&utf8).unwrap(), shown_utf8);
    assert_eq!(text_shown(&mut s), shown_main);
}

/// A rename to the same path changes nothing, as applying makes nothing:
/// a file made or edited before it keeps its text, and isn't read again.
#[gpui::test]
fn a_rename_to_the_same_path_keeps_the_text(cx: &mut TestAppContext) {
    let (s, server, _) = setup(cx);
    let same =
        |name: &str| json!({ "kind": "rename", "oldUri": s.uri(name), "newUri": s.uri(name) });
    let edits = decode(
        &s,
        server,
        json!([
            { "kind": "create", "uri": s.uri("src/c.rs") },
            same("src/c.rs"),
            edit_of(&s, "src/c.rs", vec![edit((0, 0), (0, 0), "pub fn c() {}\n")]),
            edit_of(&s, "src/utf8.rs", vec![edit((1, 7), (1, 8), "g")]),
            same("src/utf8.rs"),
            edit_of(&s, "src/utf8.rs", vec![edit((1, 0), (1, 3), "pub(crate)")]),
            edit_of(&s, "src/a.rs", vec![edit((0, 7), (0, 8), "b")]),
            same("src"),
            edit_of(&s, "src/a.rs", vec![edit((0, 0), (0, 3), "pub(crate)")]),
        ]),
    );
    let (mut preview, asked) = preview(edits);
    let (c, utf8, a) = (
        s.path("src/c.rs"),
        s.path("src/utf8.rs"),
        s.path("src/a.rs"),
    );
    assert_eq!(asked, [utf8.clone(), a.clone()]);
    assert_eq!(listed(&mut preview), [c.clone(), utf8.clone(), a.clone()]);
    assert_eq!(previewed_text(&preview, &c), "pub fn c() {}\n");
    assert_eq!(
        previewed_text(&preview, &utf8),
        "// naïve\npub(crate) fn g() {}\n"
    );
    assert_eq!(previewed_text(&preview, &a), "pub(crate) fn b() {}\n");
}
