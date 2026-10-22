//! Workspace edits that create, rename and delete files, pushed with
//! `workspace/applyEdit`: applied in order with their text edits, the
//! first that fails stopping the rest.

use std::{
    fs,
    os::unix::fs::MetadataExt as _,
    path::{Path, PathBuf},
    thread::{self, JoinHandle},
    time::Duration,
};

use lsp_types::{ApplyWorkspaceEditParams, request::ApplyWorkspaceEdit};
use serde_json::{Value, json};

use super::{edit, setup};
use crate::tests::lsp::Lsp;

impl Lsp {
    /// `changes`, a `documentChanges` array, pushed by the server with
    /// `workspace/applyEdit`; the answer, once it's in.
    fn push_changes(&mut self, changes: Value) -> Value {
        let params = ApplyWorkspaceEditParams {
            label: Some("Move".into()),
            edit: serde_json::from_value(json!({ "documentChanges": changes })).unwrap(),
        };
        let fake = self.fake.clone();
        let asked: JoinHandle<Value> = thread::spawn(move || {
            let launch = fake.launches();
            (fake.request_within::<ApplyWorkspaceEdit>(launch, Duration::from_secs(120), params))
                .unwrap()
        });
        self.until("the answer", |_| asked.is_finished());
        asked.join().unwrap()
    }

    /// A text edit of `name`, which the edit makes, so has no version.
    fn edit_of(&self, name: &str, edits: Vec<lsp_types::TextEdit>) -> Value {
        json!({
            "textDocument": { "uri": self.uri(name), "version": null },
            "edits": edits,
        })
    }
}

/// Where the Trash keeps what's moved there: the home trash, and the ones
/// at the top of `path`'s mount.
fn trashes(path: &Path) -> Vec<PathBuf> {
    let home = match std::env::var_os("XDG_DATA_HOME").filter(|dir| !dir.is_empty()) {
        Some(dir) => PathBuf::from(dir).join("Trash"),
        None => Path::new(&std::env::var_os("HOME").unwrap_or_default()).join(".local/share/Trash"),
    };
    let device = fs::metadata(path).unwrap().dev();
    let mut top = path;
    while let Some(up) = top
        .parent()
        .filter(|up| fs::metadata(up).unwrap().dev() == device)
    {
        top = up;
    }
    // SAFETY: getuid has no preconditions and can't fail.
    let uid = unsafe { libc::getuid() };
    vec![
        home,
        top.join(format!(".Trash-{uid}")),
        top.join(".Trash").join(uid.to_string()),
    ]
}

/// Removes `path`, moved to the Trash, from it, where it can: each entry
/// whose `.trashinfo` records it.
fn purge(root: &Path, path: &Path) {
    let rel = path.strip_prefix(root).unwrap();
    let ending = Path::new(root.file_name().unwrap()).join(rel);
    for trash in trashes(root) {
        let Ok(infos) = fs::read_dir(trash.join("info")) else {
            continue;
        };
        for info in infos.flatten() {
            let recorded = fs::read_to_string(info.path()).unwrap_or_default();
            let ours = (recorded.lines())
                .filter_map(|line| line.strip_prefix("Path="))
                .any(|recorded| Path::new(recorded).ends_with(&ending));
            if !ours {
                continue;
            }
            if let Some(stem) = info.path().file_stem() {
                let _ = fs::remove_file(trash.join("files").join(stem));
            }
            let _ = fs::remove_file(info.path());
        }
    }
}

#[gpui::test]
fn renames_creates_and_deletes_apply_in_order(cx: &mut gpui::TestAppContext) {
    let (mut s, _) = setup(cx);
    fs::write(s.path("src/a.rs"), "pub fn a() {}\n").unwrap();
    fs::write(s.path("src/d.rs"), "pub fn d() {}\n").unwrap();
    s.open("src/a.rs");
    (s.fake)
        .wait_for_document(&s.uri("src/a.rs"), "didOpen", |r| r.is_some())
        .unwrap();
    let a = s.editor();

    let changes = json!([
        { "kind": "rename", "oldUri": s.uri("src/a.rs"), "newUri": s.uri("src/b.rs") },
        s.edit_of("src/b.rs", vec![edit((0, 7), (0, 8), "b")]),
        { "kind": "create", "uri": s.uri("src/c.rs") },
        s.edit_of("src/c.rs", vec![edit((0, 0), (0, 0), "pub fn c() {}\n")]),
        { "kind": "delete", "uri": s.uri("src/d.rs") },
    ]);
    let answer = s.push_changes(changes);
    let trashed = !s.path("src/d.rs").exists();
    purge(&s.root, &s.path("src/d.rs"));
    assert_eq!(answer, json!({ "applied": true }));

    // a.rs's editor followed it to b.rs, and took the edit there.
    let shown = a.read_with(&s.cx, |view, _| view.model().path().map(Path::to_owned));
    assert_eq!(shown, Some(s.path("src/b.rs")));
    assert_eq!(s.text(&a), "pub fn b() {}\n");
    assert!(!s.path("src/a.rs").exists());
    assert_eq!(s.bytes("src/b.rs"), b"pub fn a() {}\n");
    // Made, with its text; and moved to the Trash.
    assert_eq!(s.bytes("src/c.rs"), b"pub fn c() {}\n");
    assert!(trashed, "src/d.rs is still there");
}

#[gpui::test]
fn a_failing_operation_aborts_the_rest(cx: &mut gpui::TestAppContext) {
    let (mut s, _) = setup(cx);
    let lib = s.bytes("src/lib.rs");
    let other = s.bytes("src/other.rs");

    // src/other.rs is there, and nothing says to overwrite or skip it.
    let changes = json!([
        { "kind": "create", "uri": s.uri("src/other.rs") },
        s.edit_of("src/lib.rs", vec![edit((0, 7), (0, 8), "renamed")]),
    ]);
    let answer = s.push_changes(changes);
    assert_eq!(answer["applied"], json!(false), "{answer}");
    let reason = answer["failureReason"].as_str().unwrap();
    assert!(reason.contains("other.rs already exists"), "{answer}");
    assert_eq!(s.bytes("src/other.rs"), other);
    assert_eq!(s.bytes("src/lib.rs"), lib);
}

impl Lsp {
    /// `changes`, a `documentChanges` array, decoded against what `server`
    /// has open, then applied.
    fn apply_changes(
        &mut self,
        server: fe_lsp::ServerId,
        changes: Value,
    ) -> Result<super::AppliedEdits, String> {
        let edit = serde_json::from_value(json!({ "documentChanges": changes })).unwrap();
        let decoded = (self.workbench)
            .read_with(&self.cx, |w, _| w.lsp_decode_edit(server, edit))
            .expect("decoded");
        self.apply(decoded)
    }
}

/// An open file edited before and after an operation: the second run's
/// edits, for the same version as the first's, move past them.
#[gpui::test]
fn an_open_file_edited_around_an_operation(cx: &mut gpui::TestAppContext) {
    let (mut s, server) = setup(cx);
    let main = s.editor();
    let changes = json!([
        s.edit_of("src/main.rs", vec![edit((0, 3), (0, 7), "start")]),
        { "kind": "create", "uri": s.uri("src/x.rs") },
        s.edit_of("src/main.rs", vec![edit((1, 4), (1, 5), "g"), edit((2, 1), (2, 1), "// end")]),
    ]);
    s.apply_changes(server, changes).unwrap();
    assert_eq!(s.text(&main), "fn start() {\n    g();\n}// end\n");
    assert!(s.path("src/x.rs").is_file());

    // Into what the first run replaced: refused, after the create.
    let changes = json!([
        s.edit_of("src/main.rs", vec![edit((0, 3), (0, 8), "main")]),
        { "kind": "create", "uri": s.uri("src/y.rs") },
        s.edit_of("src/main.rs", vec![edit((0, 5), (0, 6), "x")]),
    ]);
    let error = s.apply_changes(server, changes).unwrap_err();
    assert!(error.contains("overlap"), "{error}");
    assert_eq!(s.text(&main), "fn main() {\n    g();\n}// end\n");
}

/// Edits after renaming an open file are for its buffer at the version the
/// server had: typed into since, they're refused, though the rename stands.
#[gpui::test]
fn a_stale_edit_after_a_rename_is_refused(cx: &mut gpui::TestAppContext) {
    let (mut s, server) = setup(cx);
    fs::write(s.path("src/a.rs"), "pub fn a() {}\n").unwrap();
    s.open("src/a.rs");
    (s.fake)
        .wait_for_document(&s.uri("src/a.rs"), "didOpen", |r| r.is_some())
        .unwrap();
    let a = s.editor();
    let changes = json!([
        { "kind": "rename", "oldUri": s.uri("src/a.rs"), "newUri": s.uri("src/b.rs") },
        s.edit_of("src/b.rs", vec![edit((0, 7), (0, 8), "b")]),
    ]);
    let edit = serde_json::from_value(json!({ "documentChanges": changes })).unwrap();
    let decoded = (s.workbench)
        .read_with(&s.cx, |w, _| w.lsp_decode_edit(server, edit))
        .unwrap();
    s.cx.update(|_, cx| {
        a.update(cx, |view, _| {
            (view.model_mut().buffer_mut()).edit(vec![(0..0, "// x\n")], fe_buffer::Origin::User)
        })
    });
    let error = s.apply(decoded).unwrap_err();
    assert!(error.contains("b.rs has changed since"), "{error}");
    let shown = a.read_with(&s.cx, |view, _| view.model().path().map(Path::to_owned));
    assert_eq!(shown, Some(s.path("src/b.rs")));
    assert_eq!(s.text(&a), "// x\npub fn a() {}\n");
}

/// A rename onto a file an editor shows is refused, whether the file is
/// there or not.
#[gpui::test]
fn a_rename_onto_an_open_file_is_refused(cx: &mut gpui::TestAppContext) {
    let (mut s, server) = setup(cx);
    fs::write(s.path("src/a.rs"), "pub fn a() {}\n").unwrap();
    fs::write(s.path("src/b.rs"), "pub fn b() {}\n").unwrap();
    s.open("src/b.rs");
    fs::remove_file(s.path("src/b.rs")).unwrap();
    let changes = json!([
        { "kind": "rename", "oldUri": s.uri("src/a.rs"), "newUri": s.uri("src/b.rs") },
    ]);
    let error = s.apply_changes(server, changes).unwrap_err();
    assert!(error.contains("b.rs is open"), "{error}");
    assert!(s.path("src/a.rs").is_file());
    assert!(!s.path("src/b.rs").exists());
}

/// `ignoreIfExists` skips the operation for an open file there, and the
/// edits after it go to its buffer.
#[gpui::test]
fn an_operation_skipped_for_an_open_file_keeps_its_buffer(cx: &mut gpui::TestAppContext) {
    let (mut s, _) = setup(cx);
    let main = s.editor();
    fs::write(s.path("src/p.rs"), "pub fn p() {}\n").unwrap();
    let changes = json!([
        { "kind": "create", "uri": s.uri("src/main.rs"), "options": { "ignoreIfExists": true } },
        s.edit_of("src/main.rs", vec![edit((0, 3), (0, 7), "start")]),
        { "kind": "rename", "oldUri": s.uri("src/p.rs"), "newUri": s.uri("src/main.rs"),
          "options": { "ignoreIfExists": true } },
        s.edit_of("src/main.rs", vec![edit((1, 4), (1, 5), "g")]),
    ]);
    assert_eq!(s.push_changes(changes), json!({ "applied": true }));
    assert_eq!(s.text(&main), "fn start() {\n    g();\n}\n");
    assert_eq!(s.bytes("src/main.rs"), b"fn main() {\n    f();\n}\n");
    assert!(s.path("src/p.rs").is_file());
}

/// Another window's editors count as this one's: what they show isn't
/// overwritten, renamed onto or written under them.
#[gpui::test]
fn another_windows_editors_are_kept(cx: &mut gpui::TestAppContext) {
    let (mut s, server) = setup(cx);
    fs::write(s.path("src/notes.txt"), "notes\n").unwrap();
    let root = s.root.clone();
    let mut other = s.another_rooted(&root);
    other.open("src/notes.txt");

    let create = json!([
        { "kind": "create", "uri": s.uri("src/notes.txt"), "options": { "overwrite": true } },
    ]);
    let error = s.apply_changes(server, create).unwrap_err();
    assert!(error.contains("notes.txt is open"), "{error}");
    let rename = json!([
        { "kind": "rename", "oldUri": s.uri("src/other.rs"), "newUri": s.uri("src/notes.txt"),
          "options": { "overwrite": true } },
    ]);
    let error = s.apply_changes(server, rename).unwrap_err();
    assert!(error.contains("notes.txt is open"), "{error}");
    let text = json!([s.edit_of("src/notes.txt", vec![edit((0, 0), (0, 0), "x")])]);
    let error = s.apply_changes(server, text).unwrap_err();
    assert!(error.contains("notes.txt has been opened since"), "{error}");
    assert_eq!(s.bytes("src/notes.txt"), b"notes\n");
    assert!(s.path("src/other.rs").is_file());
}

/// A create over an existing file empties it, and the edits after it
/// write it.
#[gpui::test]
fn a_create_over_a_file_empties_it(cx: &mut gpui::TestAppContext) {
    let (mut s, server) = setup(cx);
    let changes = json!([
        { "kind": "create", "uri": s.uri("src/lib.rs"), "options": { "overwrite": true } },
        s.edit_of("src/lib.rs", vec![edit((0, 0), (0, 0), "// new\n")]),
    ]);
    s.apply_changes(server, changes).unwrap();
    assert_eq!(s.bytes("src/lib.rs"), b"// new\n");
}

/// Sets extended attribute `name` of `path` to `value`: false where its
/// file system has none.
fn set_xattr(path: &Path, name: &str, value: &[u8]) -> bool {
    use std::{ffi::CString, os::unix::ffi::OsStrExt as _};

    let (path, name) = (
        CString::new(path.as_os_str().as_bytes()).unwrap(),
        CString::new(name).unwrap(),
    );
    // SAFETY: `path` and `name` are NUL-terminated and `value` is valid for
    // reads of its length.
    #[cfg(target_os = "linux")]
    let set = unsafe {
        libc::setxattr(
            path.as_ptr(),
            name.as_ptr(),
            value.as_ptr().cast(),
            value.len(),
            0,
        )
    };
    // SAFETY: as above.
    #[cfg(target_vendor = "apple")]
    let set = unsafe {
        libc::setxattr(
            path.as_ptr(),
            name.as_ptr(),
            value.as_ptr().cast(),
            value.len(),
            0,
            0,
        )
    };
    if set == 0 {
        return true;
    }
    let error = std::io::Error::last_os_error();
    assert_eq!(error.raw_os_error(), Some(libc::ENOTSUP), "{error}");
    false
}

/// Extended attribute `name` of `path`, if it has one.
fn xattr(path: &Path, name: &str) -> Option<Vec<u8>> {
    use std::{ffi::CString, os::unix::ffi::OsStrExt as _};

    let (path, name) = (
        CString::new(path.as_os_str().as_bytes()).unwrap(),
        CString::new(name).unwrap(),
    );
    let mut value = vec![0u8; 256];
    // SAFETY: `path` and `name` are NUL-terminated and `value` is valid for
    // writes of its length.
    #[cfg(target_os = "linux")]
    let len = unsafe {
        libc::getxattr(
            path.as_ptr(),
            name.as_ptr(),
            value.as_mut_ptr().cast(),
            value.len(),
        )
    };
    // SAFETY: as above.
    #[cfg(target_vendor = "apple")]
    let len = unsafe {
        libc::getxattr(
            path.as_ptr(),
            name.as_ptr(),
            value.as_mut_ptr().cast(),
            value.len(),
            0,
            0,
        )
    };
    value.truncate(usize::try_from(len).ok()?);
    Some(value)
}

/// A create over a file empties that file, rather than making another: it
/// keeps its inode, its permissions and its extended attributes.
#[gpui::test]
fn a_create_over_a_file_keeps_the_file(cx: &mut gpui::TestAppContext) {
    use std::os::unix::fs::PermissionsExt as _;

    let (mut s, server) = setup(cx);
    let notes = s.path("src/notes.txt");
    fs::write(&notes, "notes\n").unwrap();
    fs::set_permissions(&notes, fs::Permissions::from_mode(0o640)).unwrap();
    #[cfg(target_os = "linux")]
    let name = "user.fastedit.test";
    #[cfg(target_vendor = "apple")]
    let name = "com.example.fastedit";
    let tagged = set_xattr(&notes, name, b"kept");
    let before = fs::metadata(&notes).unwrap();
    let changes = json!([
        { "kind": "create", "uri": s.uri("src/notes.txt"), "options": { "overwrite": true } },
    ]);
    s.apply_changes(server, changes).unwrap();
    assert_eq!(s.bytes("src/notes.txt"), b"");
    let after = fs::metadata(&notes).unwrap();
    assert_eq!((after.dev(), after.ino()), (before.dev(), before.ino()));
    assert_eq!(after.permissions().mode() & 0o777, 0o640);
    assert_eq!(after.gid(), before.gid());
    if tagged {
        assert_eq!(xattr(&notes, name).as_deref(), Some(&b"kept"[..]));
    }
}

/// A create over a symlink moves the link to the Trash and makes a file in
/// its place, never emptying what it leads to.
#[gpui::test]
fn a_create_over_a_symlink_replaces_the_link(cx: &mut gpui::TestAppContext) {
    let (mut s, server) = setup(cx);
    let link = s.path("src/link.rs");
    std::os::unix::fs::symlink(s.path("src/main.rs"), &link).unwrap();
    let main = s.bytes("src/main.rs");
    let changes = json!([
        { "kind": "create", "uri": s.uri("src/link.rs"), "options": { "overwrite": true } },
    ]);
    let applied = s.apply_changes(server, changes);
    purge(&s.root, &link);
    applied.unwrap();
    assert!(fs::symlink_metadata(&link).unwrap().is_file());
    assert_eq!(s.bytes("src/link.rs"), b"");
    assert_eq!(s.bytes("src/main.rs"), main);
}

/// A later run's edits move past what the earlier run made, as made: a
/// CRLF inserted is one byte shorter as LF, and a replacement with the same
/// text is no edit at all, which the later run may edit inside.
#[gpui::test]
fn later_runs_follow_what_was_made(cx: &mut gpui::TestAppContext) {
    let (mut s, server) = setup(cx);
    let main = s.editor();
    let changes = json!([
        s.edit_of(
            "src/main.rs",
            vec![edit((0, 0), (0, 0), "// a\r\n"), edit((1, 4), (1, 5), "f")]
        ),
        { "kind": "create", "uri": s.uri("src/x.rs") },
        s.edit_of("src/main.rs", vec![edit((1, 4), (1, 5), "g"), edit((2, 0), (2, 1), "};")]),
    ]);
    s.apply_changes(server, changes).unwrap();
    assert_eq!(s.text(&main), "// a\nfn main() {\n    g();\n};\n");
}

/// A rename of a file shown only in another window, its server's edit
/// applied here: that window's editor follows it and takes the edits.
#[gpui::test]
fn another_windows_editor_follows_and_takes_the_edits(cx: &mut gpui::TestAppContext) {
    let (mut s, _) = setup(cx);
    fs::write(s.path("src/a.rs"), "pub fn a() {}\n").unwrap();
    let root = s.root.clone();
    let mut other = s.another_rooted(&root);
    other.open("src/a.rs");
    let a = other.editor();
    let theirs = other
        .active_server()
        .flatten()
        .expect("a.rs went to a server");
    let changes = json!([
        { "kind": "rename", "oldUri": s.uri("src/a.rs"), "newUri": s.uri("src/b.rs") },
        s.edit_of("src/b.rs", vec![edit((0, 7), (0, 8), "b")]),
    ]);
    let edit = serde_json::from_value(json!({ "documentChanges": changes })).unwrap();
    (other.fake)
        .wait_for_document(&other.uri("src/a.rs"), "didOpen", |r| r.is_some())
        .unwrap();
    let decoded = (other.workbench)
        .read_with(&other.cx, |w, _| w.lsp_decode_edit(theirs, edit))
        .unwrap();
    s.apply(decoded).unwrap();
    other.draw();
    let shown = a.read_with(&other.cx, |view, _| view.model().path().map(Path::to_owned));
    assert_eq!(shown, Some(s.path("src/b.rs")));
    assert_eq!(other.text(&a), "pub fn b() {}\n");
    assert_eq!(s.bytes("src/b.rs"), b"pub fn a() {}\n");
}

/// A rename `ignoreIfExists` may skip goes the way the disk says: made
/// onto a missing file, the edits after it are its source's buffer's;
/// skipped for one that's there, the source stays and takes its own
/// edits, and the target's go to its file.
#[gpui::test]
fn a_skippable_rename_goes_either_way(cx: &mut gpui::TestAppContext) {
    let (mut s, _) = setup(cx);
    fs::write(s.path("src/a.rs"), "pub fn a() {}\n").unwrap();
    fs::write(s.path("src/t.rs"), "pub fn t() {}\n").unwrap();
    s.open("src/a.rs");
    (s.fake)
        .wait_for_document(&s.uri("src/a.rs"), "didOpen", |r| r.is_some())
        .unwrap();
    let a = s.editor();
    let rename = |s: &Lsp, to: &str| {
        json!({ "kind": "rename", "oldUri": s.uri("src/a.rs"), "newUri": s.uri(to),
                "options": { "ignoreIfExists": true } })
    };

    // Skipped: t.rs is there.
    let changes = json!([
        rename(&s, "src/t.rs"),
        s.edit_of("src/a.rs", vec![edit((0, 0), (0, 0), "// kept\n")]),
        s.edit_of("src/t.rs", vec![edit((0, 7), (0, 8), "u")]),
    ]);
    assert_eq!(s.push_changes(changes), json!({ "applied": true }));
    let shown = a.read_with(&s.cx, |view, _| view.model().path().map(Path::to_owned));
    assert_eq!(shown, Some(s.path("src/a.rs")));
    assert_eq!(s.text(&a), "// kept\npub fn a() {}\n");
    assert_eq!(s.bytes("src/t.rs"), b"pub fn u() {}\n");

    // Made: b.rs isn't there, so a.rs's editor follows and takes the edit.
    s.server_has("src/a.rs", "// kept\npub fn a() {}\n");
    let changes = json!([
        rename(&s, "src/b.rs"),
        s.edit_of("src/b.rs", vec![edit((1, 7), (1, 8), "b")]),
    ]);
    assert_eq!(s.push_changes(changes), json!({ "applied": true }));
    let shown = a.read_with(&s.cx, |view, _| view.model().path().map(Path::to_owned));
    assert_eq!(shown, Some(s.path("src/b.rs")));
    assert_eq!(s.text(&a), "// kept\npub fn b() {}\n");
}

/// A create over a file keeps its permissions.
#[gpui::test]
fn a_create_over_a_file_keeps_its_permissions(cx: &mut gpui::TestAppContext) {
    use std::os::unix::fs::PermissionsExt as _;

    let (mut s, server) = setup(cx);
    let tool = s.path("src/tool.sh");
    fs::write(&tool, "#!/bin/sh\n").unwrap();
    fs::set_permissions(&tool, fs::Permissions::from_mode(0o750)).unwrap();
    let changes = json!([
        { "kind": "create", "uri": s.uri("src/tool.sh"), "options": { "overwrite": true } },
        s.edit_of("src/tool.sh", vec![edit((0, 0), (0, 0), "#!/bin/bash\n")]),
    ]);
    s.apply_changes(server, changes).unwrap();
    assert_eq!(s.bytes("src/tool.sh"), b"#!/bin/bash\n");
    let mode = fs::metadata(&tool).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o750);
}
