//! Applying workspace edits: fe-lsp's fake server over a temporary
//! project (the LSP tests' [`Lsp`]), its edits decoded by the workbench's
//! manager as a rename's answer is, or pushed with `workspace/applyEdit`.

use std::{
    collections::HashMap,
    fs,
    path::Path,
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

use fe_command::{CommandId, ContextPredicate};
use fe_gpui::EditorView;
use fe_lsp::{FileEdits, ServerId, fake::Script};
use gpui::{Entity, Focusable as _, TestAppContext};
use kiln::platform::command_actions::typed_command;
use lsp_types::{
    ApplyWorkspaceEditParams, Position, Range, TextEdit, Url, WorkspaceEdit,
    request::ApplyWorkspaceEdit,
};
use serde_json::{Value, json};

use super::AppliedEdits;
use crate::tests::lsp::Lsp;

/// `src/main.rs` as the tests open it.
const MAIN: &str = "fn main() {\n    f();\n}\n";
/// A closed UTF-8 file.
const UTF8: &str = "// naïve\npub fn f() {}\n";
/// A closed windows-1252 file with CRLF line endings: "// café".
const LATIN: &[u8] = b"// caf\xE9\r\npub fn f() {}\r\n";

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

/// The project with the closed files, `src/main.rs` open on the
/// trusted root's server, and that server.
fn setup(cx: &mut TestAppContext) -> (Lsp, ServerId) {
    prepare(Lsp::new(cx))
}

/// [`setup`] for `s`, its root not trusted yet.
fn prepare(mut s: Lsp) -> (Lsp, ServerId) {
    s.set_trust(true);
    fs::write(s.path("src/main.rs"), MAIN).unwrap();
    fs::write(s.path("src/utf8.rs"), UTF8).unwrap();
    fs::write(s.path("src/latin.rs"), LATIN).unwrap();
    s.open("src/main.rs");
    s.fake
        .wait_for_document(&s.uri("src/main.rs"), "didOpen", |r| r.is_some())
        .unwrap();
    let server = s
        .active_server()
        .flatten()
        .expect("main.rs went to a server");
    (s, server)
}

impl Lsp {
    /// `changes` pushed by the server with `workspace/applyEdit`, from a
    /// thread of its own that ends with the answer.
    fn push_edit(&mut self, changes: Vec<(&str, Vec<TextEdit>)>) -> JoinHandle<Value> {
        let changes: HashMap<Url, Vec<TextEdit>> = (changes.into_iter())
            .map(|(name, edits)| (self.uri(name), edits))
            .collect();
        let params = ApplyWorkspaceEditParams {
            label: Some("Rename".into()),
            edit: WorkspaceEdit::new(changes),
        };
        let fake = self.fake.clone();
        // The fake's usual 10 s would race the tests' own 10 s waits for the
        // edit to reach the workbench, under load.
        thread::spawn(move || {
            let launch = fake.launches();
            (fake.request_within::<ApplyWorkspaceEdit>(launch, Duration::from_secs(120), params))
                .unwrap()
        })
    }

    /// `changes`, each a file's edits, decoded against what `server` has
    /// open.
    fn decode(&mut self, server: ServerId, changes: Vec<(&str, Vec<TextEdit>)>) -> Vec<FileEdits> {
        let changes: HashMap<Url, Vec<TextEdit>> = (changes.into_iter())
            .map(|(name, edits)| (self.uri(name), edits))
            .collect();
        let edit = WorkspaceEdit::new(changes);
        self.workbench
            .read_with(&self.cx, |w, _| w.lsp_decode_edit(server, edit))
            .expect("decoded")
    }

    fn apply(&mut self, edits: Vec<FileEdits>) -> Result<AppliedEdits, String> {
        let workbench = self.workbench.clone();
        let applied = workbench.update(&mut self.cx, |w, cx| w.apply_file_edits(edits, cx));
        self.draw();
        applied
    }

    fn editor(&mut self) -> Entity<EditorView> {
        self.workbench
            .read_with(&self.cx, |w, cx| w.active_editor(cx).cloned())
            .expect("a tab is shown")
    }

    fn text(&mut self, editor: &Entity<EditorView>) -> String {
        editor.read_with(&self.cx, |view, _| view.model().text().to_string())
    }

    fn bytes(&self, name: &str) -> Vec<u8> {
        fs::read(self.path(name)).unwrap()
    }

    /// Waits for the server to have `name` as `text`.
    fn server_has(&mut self, name: &str, text: &str) {
        self.draw();
        let uri = self.uri(name);
        (self.fake)
            .wait_for_document(&uri, "didChange", |r| r.is_some_and(|r| r.text == text))
            .expect("the server caught up");
    }
}

#[gpui::test]
fn open_and_closed_files(cx: &mut TestAppContext) {
    let (mut s, server) = setup(cx);
    let main = s.editor();
    // The caret before the closing brace.
    let brace = MAIN.rfind('}').unwrap();
    s.cx.update(|_, cx| main.update(cx, |view, _| view.model_mut().set_cursor(brace)));

    let edits = s.decode(
        server,
        vec![
            (
                "src/main.rs",
                vec![
                    edit((0, 3), (0, 7), "start"),
                    edit((1, 4), (1, 5), "renamed"),
                ],
            ),
            ("src/utf8.rs", vec![edit((1, 7), (1, 8), "renamed")]),
            (
                "src/latin.rs",
                vec![
                    edit((0, 0), (0, 3), "// crème "),
                    edit((1, 7), (1, 8), "renamed"),
                ],
            ),
        ],
    );
    assert_eq!(s.apply(edits), Ok(AppliedEdits { files: 3, edits: 5 }));

    // The buffer, its caret anchored.
    let edited = "fn start() {\n    renamed();\n}\n";
    assert_eq!(s.text(&main), edited);
    let caret = main.read_with(&s.cx, |view, _| view.model().primary_selection().head());
    assert_eq!(caret, edited.rfind('}').unwrap());
    // Not saved: the file is as it was.
    assert_eq!(s.bytes("src/main.rs"), MAIN.as_bytes());
    // The closed files, each in its own encoding and line endings.
    assert_eq!(
        s.bytes("src/utf8.rs"),
        b"// na\xC3\xAFve\npub fn renamed() {}\n"
    );
    assert_eq!(
        s.bytes("src/latin.rs"),
        b"// cr\xE8me caf\xE9\r\npub fn renamed() {}\r\n"
    );
    // No temporary file left beside them.
    let left: Vec<_> = (fs::read_dir(s.path("src")).unwrap())
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|name| name.contains("fastedit-save"))
        .collect();
    assert!(left.is_empty(), "{left:?}");

    // One transaction of the user: cmd-z in the editor takes it all back.
    s.cx.update(|window, cx| window.focus(&main.focus_handle(cx), cx));
    s.cx.simulate_keystrokes("cmd-z");
    s.draw();
    assert_eq!(s.text(&main), MAIN);
    s.cx.simulate_keystrokes("cmd-shift-z");
    s.draw();
    assert_eq!(s.text(&main), edited);
}

#[gpui::test]
fn all_or_nothing(cx: &mut TestAppContext) {
    let (mut s, server) = setup(cx);
    let main = s.editor();
    let changes = || {
        vec![
            ("src/latin.rs", vec![edit((1, 7), (1, 8), "renamed")]),
            ("src/main.rs", vec![edit((1, 4), (1, 5), "renamed")]),
            ("src/utf8.rs", vec![edit((1, 7), (1, 8), "renamed")]),
        ]
    };

    // The buffer edited past the version the edits are for: refused, and
    // the closed file checked before it isn't written.
    let edits = s.decode(server, changes());
    s.cx.update(|_, cx| main.update(cx, |view, _| view.model_mut().insert("// ")));
    let typed = format!("// {MAIN}");
    assert_eq!(s.text(&main), typed);
    let error = s.apply(edits).unwrap_err();
    assert!(
        error.contains("main.rs") && error.contains("changed"),
        "{error}"
    );
    assert_eq!(s.text(&main), typed);
    assert_eq!(s.bytes("src/latin.rs"), LATIN);
    assert_eq!(s.bytes("src/utf8.rs"), UTF8.as_bytes());

    // A closed file opened since: refused, and the open buffer, checked
    // before it and still at its version, isn't edited.
    s.server_has("src/main.rs", &typed);
    let edits = s.decode(server, changes());
    s.open("src/utf8.rs");
    let error = s.apply(edits).unwrap_err();
    assert!(
        error.contains("utf8.rs") && error.contains("opened"),
        "{error}"
    );
    assert_eq!(s.text(&main), typed);
    assert_eq!(s.bytes("src/latin.rs"), LATIN);
    assert_eq!(s.bytes("src/utf8.rs"), UTF8.as_bytes());

    // A closed file its encoding can't hold the edit in: refused too.
    let edits = s.decode(
        server,
        vec![
            ("src/latin.rs", vec![edit((1, 7), (1, 8), "😀")]),
            ("src/main.rs", vec![edit((1, 4), (1, 5), "renamed")]),
        ],
    );
    let error = s.apply(edits).unwrap_err();
    assert!(error.contains("latin.rs"), "{error}");
    assert_eq!(s.bytes("src/latin.rs"), LATIN);
    assert_eq!(s.text(&main), typed);
}

#[gpui::test]
fn server_apply_edit(cx: &mut TestAppContext) {
    let (mut s, _) = setup(cx);
    let main = s.editor();
    let ask = |s: &mut Lsp, changes: Vec<(&str, Vec<TextEdit>)>| {
        let asked = s.push_edit(changes);
        s.until("the answer", |_| asked.is_finished());
        asked.join().unwrap()
    };

    // Applied, and answered so.
    let answer = ask(
        &mut s,
        vec![
            ("src/main.rs", vec![edit((1, 4), (1, 5), "renamed")]),
            ("src/utf8.rs", vec![edit((1, 7), (1, 8), "renamed")]),
        ],
    );
    assert_eq!(answer, json!({ "applied": true }));
    assert_eq!(s.text(&main), "fn main() {\n    renamed();\n}\n");
    assert_eq!(
        s.bytes("src/utf8.rs"),
        b"// na\xC3\xAFve\npub fn renamed() {}\n"
    );

    // Refused by the workbench, a closed file that isn't there: answered
    // failed with why, and nothing changed.
    s.server_has("src/main.rs", "fn main() {\n    renamed();\n}\n");
    let answer = ask(
        &mut s,
        vec![
            ("src/main.rs", vec![edit((0, 3), (0, 7), "start")]),
            ("src/missing.rs", vec![edit((0, 0), (0, 0), "// new\n")]),
        ],
    );
    assert_eq!(answer["applied"], json!(false));
    let reason = answer["failureReason"].as_str().unwrap();
    assert!(reason.contains("missing.rs"), "{answer}");
    assert_eq!(s.text(&main), "fn main() {\n    renamed();\n}\n");
    assert!(!Path::new(&s.path("src/missing.rs")).exists());

    // Delivered only after fe-lsp answered it not applied, as it does when
    // the workbench hasn't answered in 5 s: dropped, changing nothing.
    let asked = s.push_edit(vec![("src/utf8.rs", vec![edit((1, 7), (1, 14), "f")])]);
    let deadline = Instant::now() + Duration::from_secs(10);
    while s.workbench.read_with(&s.cx, |w, _| w.lsp_queued()) == 0 {
        assert!(
            Instant::now() < deadline,
            "the edit never reached the workbench"
        );
        thread::sleep(Duration::from_millis(5));
    }
    s.clock.wait_for_sleep(Duration::from_secs(5));
    s.clock.advance(Duration::from_secs(5));
    let answer = asked.join().unwrap();
    assert_eq!(answer["applied"], json!(false), "{answer}");
    s.draw();
    assert_eq!(s.workbench.read_with(&s.cx, |w, _| w.lsp_queued()), 0);
    assert_eq!(
        s.bytes("src/utf8.rs"),
        b"// na\xC3\xAFve\npub fn renamed() {}\n"
    );
}

/// An edit handed to the workbench whose session ends before it's
/// delivered, its server still shutting down: answered failed by fe-lsp,
/// and dropped.
#[gpui::test]
fn an_edit_from_an_ended_session(cx: &mut TestAppContext) {
    // A stop waits for `shutdown`, on a clock that doesn't move.
    let script = Script {
        answer_shutdown: false,
        ..Script::default()
    };
    let (mut s, server) = prepare(Lsp::with_scripts(cx, script, Script::default()));
    let asked = s.push_edit(vec![("src/utf8.rs", vec![edit((1, 7), (1, 8), "renamed")])]);
    let deadline = Instant::now() + Duration::from_secs(10);
    while s.workbench.read_with(&s.cx, |w, _| w.lsp_queued()) == 0 {
        assert!(
            Instant::now() < deadline,
            "the edit never reached the workbench"
        );
        thread::sleep(Duration::from_millis(5));
    }
    s.workbench.read_with(&s.cx, |w, _| w.lsp_stop(server));
    assert_eq!(
        asked.join().unwrap(),
        json!({ "applied": false, "failureReason": "the server's session ended" })
    );
    s.draw();
    assert_eq!(s.workbench.read_with(&s.cx, |w, _| w.lsp_queued()), 0);
    assert_eq!(s.bytes("src/utf8.rs"), UTF8.as_bytes());
}

#[gpui::test]
#[cfg(unix)]
fn aliases_and_reads(cx: &mut TestAppContext) {
    let (mut s, server) = setup(cx);
    let main = s.editor();
    std::os::unix::fs::symlink(s.path("src/main.rs"), s.path("src/alias.rs")).unwrap();
    std::os::unix::fs::symlink(s.path("src/utf8.rs"), s.path("src/utf8_alias.rs")).unwrap();
    s.cx.update(|_, cx| main.update(cx, |view, _| view.model_mut().insert("// ")));
    let typed = format!("// {MAIN}");
    s.server_has("src/main.rs", &typed);

    // A symlink to the open, edited file, which the server has as a
    // closed file of its own: refused, the file left as it is.
    let edits = s.decode(
        server,
        vec![
            ("src/alias.rs", vec![edit((0, 0), (0, 0), "// theirs\n")]),
            ("src/latin.rs", vec![edit((1, 7), (1, 8), "renamed")]),
        ],
    );
    let error = s.apply(edits).unwrap_err();
    assert!(
        error.contains("alias.rs") && error.contains("opened"),
        "{error}"
    );
    assert_eq!(s.bytes("src/main.rs"), MAIN.as_bytes());
    assert_eq!(s.text(&main), typed);
    assert_eq!(s.bytes("src/latin.rs"), LATIN);

    // One closed file by two paths: refused, neither written.
    let edits = s.decode(
        server,
        vec![
            ("src/utf8.rs", vec![edit((1, 7), (1, 8), "renamed")]),
            ("src/utf8_alias.rs", vec![edit((1, 7), (1, 8), "other")]),
        ],
    );
    let error = s.apply(edits).unwrap_err();
    assert!(error.contains("twice"), "{error}");
    assert_eq!(s.bytes("src/utf8.rs"), UTF8.as_bytes());

    // A file being read into a pane, not shown yet: refused, so the tab
    // it lands in never shows the text from before the edit. An older
    // read, superseded by a newer one, still lands, so it counts too.
    let edits = s.decode(
        server,
        vec![
            ("src/latin.rs", vec![edit((1, 7), (1, 8), "renamed")]),
            ("src/utf8.rs", vec![edit((1, 7), (1, 8), "renamed")]),
        ],
    );
    let pane = s.workbench.read_with(&s.cx, |w, _| w.pane().clone());
    let (utf8, lib) = (s.path("src/utf8.rs"), s.path("src/lib.rs"));
    s.cx.update(|window, cx| {
        pane.update(cx, |pane, cx| {
            pane.open(utf8, window, cx);
            pane.open(lib, window, cx);
        })
    });
    let applied = {
        let workbench = s.workbench.clone();
        workbench.update(&mut s.cx, |w, cx| w.apply_file_edits(edits, cx))
    };
    let error = applied.unwrap_err();
    assert!(
        error.contains("utf8.rs") && error.contains("opened"),
        "{error}"
    );
    assert_eq!(s.bytes("src/latin.rs"), LATIN);
    assert_eq!(s.bytes("src/utf8.rs"), UTF8.as_bytes());
}

/// The edit is a transaction of its own, even with one of the user's open
/// (a Vim insert): neither joined nor closed. Inserts at one place keep
/// the server's order.
#[gpui::test]
fn its_own_transaction(cx: &mut TestAppContext) {
    let (mut s, server) = setup(cx);
    let main = s.editor();
    let edits = s.decode(
        server,
        vec![(
            "src/main.rs",
            vec![
                edit((1, 4), (1, 4), "a"),
                edit((1, 4), (1, 4), "b"),
                edit((1, 4), (1, 5), "c"),
            ],
        )],
    );
    // An insert the user has open, at the end, after the server's
    // version: the edit is for the version before it, so it goes first.
    let open = s.cx.update(|_, cx| {
        main.update(cx, |view, _| {
            let mut buffer = view.model_mut().buffer_mut();
            buffer.start_transaction(fe_buffer::Origin::User);
            buffer.edit([(MAIN.len()..MAIN.len(), "//")], fe_buffer::Origin::User);
            buffer.open_transaction()
        })
    });
    // Refused: the buffer moved on. Applied once the server has it.
    assert!(s.apply(edits).is_err());
    let typed = format!("{MAIN}//");
    s.server_has("src/main.rs", &typed);
    let edits = s.decode(
        server,
        vec![(
            "src/main.rs",
            vec![
                edit((1, 4), (1, 4), "a"),
                edit((1, 4), (1, 4), "b"),
                edit((1, 4), (1, 5), "c"),
            ],
        )],
    );
    assert_eq!(s.apply(edits), Ok(AppliedEdits { files: 1, edits: 3 }));
    let edited = "fn main() {\n    abc();\n}\n//";
    assert_eq!(s.text(&main), edited);
    // The user's insert is still open, and goes on.
    let still = s.cx.update(|_, cx| {
        main.update(cx, |view, _| {
            let mut buffer = view.model_mut().buffer_mut();
            let still = buffer.open_transaction();
            let len = buffer.len();
            buffer.edit([(len..len, " x")], fe_buffer::Origin::User);
            buffer.end_transaction();
            still
        })
    });
    assert_eq!(still, open);
    assert_eq!(s.text(&main), format!("{edited} x"));
    // Undo takes back one at a time: the server's edit, then the user's
    // insert, whole.
    s.cx.update(|window, cx| window.focus(&main.focus_handle(cx), cx));
    s.cx.simulate_keystrokes("cmd-z");
    s.draw();
    assert_eq!(s.text(&main), format!("{typed} x"));
    s.cx.simulate_keystrokes("cmd-z");
    s.draw();
    assert_eq!(s.text(&main), MAIN);
}

/// The host's read of the launch file still lands after another tab took
/// its place: the file counts as opened until then.
#[gpui::test]
fn the_launch_read(cx: &mut TestAppContext) {
    let (mut s, server) = setup(cx);
    let path = s.path("src/utf8.rs");
    let launch = crate::Launch {
        root: Some(s.root.clone()),
        file: Some((path.clone(), None)),
    };
    let (launched, mut cx) = crate::tests::lsp::window(&mut s.cx, launch);
    // The host has read it, and hasn't handed it over yet.
    let read = crate::loader::open(path.clone());
    let pane = launched.read_with(&cx, |w, _| w.pane().clone());
    let lib = s.path("src/lib.rs");
    cx.update(|window, cx| pane.update(cx, |pane, cx| pane.open(lib, window, cx)));
    cx.run_until_parked();

    let edits = s.decode(
        server,
        vec![("src/utf8.rs", vec![edit((1, 7), (1, 8), "renamed")])],
    );
    let applied = launched.update(&mut cx, |w, cx| w.apply_file_edits(edits, cx));
    let error = applied.unwrap_err();
    assert!(
        error.contains("utf8.rs") && error.contains("opened"),
        "{error}"
    );
    assert_eq!(s.bytes("src/utf8.rs"), UTF8.as_bytes());

    // It lands, as the file is.
    cx.update(|window, cx| {
        launched.update(cx, |w, cx| w.file_read(path.clone(), read, window, cx))
    });
    cx.run_until_parked();
    let texts: Vec<String> = cx.update(|_, cx| {
        let editors = launched.read(cx).open_editors(cx);
        (editors.iter())
            .filter(|editor| editor.read(cx).model().path() == Some(path.as_path()))
            .map(|editor| editor.read(cx).model().text().to_string())
            .collect()
    });
    assert_eq!(texts, [UTF8]);
}

/// An edit for a buffer saved under another name since is refused: it
/// was for the old file's document.
#[gpui::test]
fn an_edit_across_a_save_as(cx: &mut TestAppContext) {
    let (mut s, server) = setup(cx);
    let main = s.editor();
    let edits = s.decode(
        server,
        vec![("src/main.rs", vec![edit((1, 4), (1, 5), "renamed")])],
    );
    let moved = s.path("src/moved.rs");
    s.cx.update(|window, cx| {
        (s.workbench.clone()).update(cx, |w, cx| w.save_editor_as(&main, moved, window, cx))
    });
    s.draw();
    let error = s.apply(edits).unwrap_err();
    assert!(
        error.contains("main.rs") && error.contains("another name"),
        "{error}"
    );
    assert_eq!(s.text(&main), MAIN);
    assert_eq!(s.bytes("src/moved.rs"), MAIN.as_bytes());
}

/// A closed file a pending Save As is to write counts as opened: an edit
/// to it is refused, so the save doesn't write over it unseen.
#[gpui::test]
fn an_edit_to_a_pending_save_as(cx: &mut TestAppContext) {
    let (mut s, server) = setup(cx);
    let main = s.editor();
    let edits = s.decode(
        server,
        vec![("src/utf8.rs", vec![edit((1, 7), (1, 8), "renamed")])],
    );
    let utf8 = s.path("src/utf8.rs");
    let workbench = s.workbench.clone();
    // Held: the save doesn't run until the executor does.
    s.cx.update(|window, cx| {
        workbench.update(cx, |w, cx| w.save_editor_as(&main, utf8, window, cx))
    });
    let applied = workbench.update(&mut s.cx, |w, cx| w.apply_file_edits(edits, cx));
    let error = applied.unwrap_err();
    assert!(
        error.contains("utf8.rs") && error.contains("opened"),
        "{error}"
    );
    assert_eq!(s.bytes("src/utf8.rs"), UTF8.as_bytes());
    s.draw();
    assert_eq!(s.bytes("src/utf8.rs"), MAIN.as_bytes());
}

/// Once the launch pane has closed, its read will be dropped, so the file
/// no longer counts as opened: an edit to it applies on disk.
#[gpui::test]
fn the_launch_read_after_its_pane_closed(cx: &mut TestAppContext) {
    let (mut s, server) = setup(cx);
    let path = s.path("src/utf8.rs");
    let launch = crate::Launch {
        root: Some(s.root.clone()),
        file: Some((path.clone(), None)),
    };
    let (launched, mut cx) = crate::tests::lsp::window(&mut s.cx, launch);
    let read = crate::loader::open(path.clone());
    // Another pane opens a file, and the launch pane closes while reading.
    cx.simulate_keystrokes("cmd-\\");
    let right = launched.read_with(&cx, |w, _| w.pane().clone());
    let lib = s.path("src/lib.rs");
    cx.update(|window, cx| right.update(cx, |pane, cx| pane.open(lib, window, cx)));
    cx.run_until_parked();
    cx.simulate_keystrokes("cmd-k cmd-left cmd-k cmd-w");
    cx.run_until_parked();
    let panes = launched.read_with(&cx, |w, cx| w.pane_group(cx).panes().len());
    assert_eq!(panes, 1);

    let edits = s.decode(
        server,
        vec![("src/utf8.rs", vec![edit((1, 7), (1, 8), "renamed")])],
    );
    let applied = launched.update(&mut cx, |w, cx| w.apply_file_edits(edits, cx));
    applied.unwrap();
    assert_ne!(s.bytes("src/utf8.rs"), UTF8.as_bytes());

    // The read lands, and is dropped.
    cx.update(|window, cx| {
        launched.update(cx, |w, cx| w.file_read(path.clone(), read, window, cx))
    });
    cx.run_until_parked();
    let shown = cx.update(|_, cx| {
        let editors = launched.read(cx).open_editors(cx);
        (editors.iter()).any(|editor| editor.read(cx).model().path() == Some(path.as_path()))
    });
    assert!(!shown);
}

#[gpui::test]
fn a_file_that_doesnt_read_cleanly(cx: &mut TestAppContext) {
    let (mut s, server) = setup(cx);
    let main = s.editor();
    // UTF-8 by its BOM, with a byte that isn't: an edit that takes away
    // what it read as U+FFFD would still rewrite what it couldn't read.
    let bad = b"\xEF\xBB\xBF// \xFF\npub fn f() {}\n";
    fs::write(s.path("src/bad.rs"), bad).unwrap();
    let edits = s.decode(
        server,
        vec![
            ("src/bad.rs", vec![edit((0, 0), (1, 0), "")]),
            ("src/main.rs", vec![edit((1, 4), (1, 5), "renamed")]),
        ],
    );
    let error = s.apply(edits).unwrap_err();
    assert!(
        error.contains("bad.rs") && error.contains("valid"),
        "{error}"
    );
    assert_eq!(s.bytes("src/bad.rs"), bad);
    assert_eq!(s.text(&main), MAIN);
}

#[gpui::test]
fn stub_commands_in_the_editor_context(cx: &mut TestAppContext) {
    cx.update(|cx| {
        fe_gpui::init(cx);
        crate::init(cx);
    });
    cx.read(|cx| {
        for (id, title) in [
            ("lsp.rename", "Rename Symbol…"),
            ("lsp.code_actions", "Code Actions…"),
            ("lsp.format_document", "Format Document"),
            ("lsp.format_selection", "Format Selection"),
        ] {
            let command = typed_command(&CommandId::parse(id).unwrap(), cx)
                .unwrap_or_else(|| panic!("{id} is registered"));
            assert_eq!(command.title, title);
            assert_eq!(
                command.context,
                Some(ContextPredicate::identifier(fe_editor::commands::CONTEXT)),
                "{id}"
            );
        }
    });
}

#[path = "workspace_edit_ops_tests.rs"]
mod ops;
