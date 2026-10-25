//! The project tree's file operations reach language servers: creating,
//! renaming and trashing a file send `didCreateFiles`, `didRenameFiles` and
//! `didDeleteFiles`, to the servers whose filters match; a refused or
//! unmatched operation sends nothing. A rename asks the servers whose
//! `willRename` filters match for their edits first, and makes them.

use std::{fs, time::Duration};

use fe_command::{ContextFrame, ContextStack, Invocation};
use fe_gpui::{EditorView, commands::RunCommand};
use fe_lsp::fake::{InitializeReply, Record, Reply, Script, Scripted};
use fe_project::FsChange;
use gpui::{AppContext as _, Entity, Focusable as _, TestAppContext};
use serde_json::{Value, json};

use crate::tests::lsp::Lsp;

/// A rust-analyzer fake that wants `*.rs` files' creation, rename and
/// deletion.
fn script() -> Script {
    let mut result = fe_lsp::fake::initialize_result(None);
    let filters = json!({"filters": [{"scheme": "file", "pattern": {"glob": "**/*.rs"}}]});
    result.capabilities.workspace = Some(
        serde_json::from_value(json!({"fileOperations": {
            "didCreate": filters, "didRename": filters, "didDelete": filters,
        }}))
        .unwrap(),
    );
    Script {
        initialize: InitializeReply::Result(Box::new(result)),
        ..Script::default()
    }
}

/// Runs command `id` where focus is, as a key bound in the panel would.
fn run(s: &mut Lsp, id: &str) {
    let context = ContextStack::new()
        .with(ContextFrame::new("workbench"))
        .with(ContextFrame::new("project_panel"));
    let run = RunCommand(Invocation::of(id).unwrap().context(context));
    s.cx.update(|window, cx| window.dispatch_action(Box::new(run), cx));
    s.draw();
}

/// Selects `name` (relative to the root) in the tree, and focuses it.
fn select(s: &mut Lsp, name: &str) {
    let path = s.path(name);
    // Opening a file moves focus off the panel, which its keys need.
    let workbench = s.workbench.clone();
    s.cx.update(|window, cx| window.focus(&workbench.focus_handle(cx), cx));
    s.cx.simulate_keystrokes("cmd-shift-e");
    let panel = s.workbench.read_with(&s.cx, |w, _| w.panel().clone());
    s.cx.update(|_, cx| {
        panel.update(cx, |panel, cx| panel.reveal(path, cx));
    });
    s.draw();
}

/// The `files` of each `method` notification the fake got, in order.
fn notified(s: &Lsp, method: &str) -> Vec<Value> {
    (s.fake.records().into_iter())
        .filter_map(|record| match record {
            Record::Notification { method: m, params } if m == method => {
                Some(params["files"].clone())
            }
            _ => None,
        })
        .collect()
}

/// Types `name` in the name field, and runs it.
fn name(s: &mut Lsp, name: &str) {
    s.cx.simulate_input(name);
    s.cx.simulate_keystrokes("enter");
    s.draw();
}

/// Waits for the fake to have `count` of `method`.
fn wait_for(s: &mut Lsp, method: &str, count: usize) {
    let method = method.to_owned();
    s.fake.wait_for(&method.clone(), move |records| {
        let n = records
            .iter()
            .filter(|r| matches!(r, Record::Notification { method: m, .. } if *m == method))
            .count();
        n >= count
    });
}

/// Holds each save until the test ends.
struct HoldSaves;

impl fe_gpui::pane_item::SaveHook for HoldSaves {
    fn before_save(&self, _: &Entity<EditorView>, cx: &mut gpui::App) -> gpui::Task<()> {
        cx.background_spawn(std::future::pending())
    }
}

#[gpui::test]
fn tree_operations_notify_servers(cx: &mut TestAppContext) {
    let mut s = Lsp::with_scripts(cx, script(), Script::default());
    s.set_trust(true);
    s.open("src/main.rs");
    let main = s.uri("src/main.rs");
    s.fake.wait_for_document(&main, "didOpen", |r| r.is_some());

    // A file the filter doesn't match: nothing is sent.
    select(&mut s, "src");
    select(&mut s, "tool.py");
    run(&mut s, "project_panel.new_file");
    name(&mut s, "notes.txt");
    assert!(s.path("notes.txt").is_file());

    // A new `src/new.rs`.
    select(&mut s, "src");
    run(&mut s, "project_panel.new_file");
    name(&mut s, "new.rs");
    assert!(s.path("src/new.rs").is_file());
    wait_for(&mut s, "workspace/didCreateFiles", 1);
    let new = s.uri("src/new.rs");
    assert_eq!(
        notified(&s, "workspace/didCreateFiles"),
        [json!([{"uri": new.as_str()}])]
    );

    // Renamed.
    select(&mut s, "src/new.rs");
    run(&mut s, "project_panel.rename");
    name(&mut s, "renamed");
    assert!(s.path("src/renamed.rs").is_file());
    wait_for(&mut s, "workspace/didRenameFiles", 1);
    let renamed = s.uri("src/renamed.rs");
    assert_eq!(
        notified(&s, "workspace/didRenameFiles"),
        [json!([{"oldUri": new.as_str(), "newUri": renamed.as_str()}])]
    );

    // A rename refused while the file is being saved.
    let editor = s.workbench.read_with(&s.cx, |w, cx| w.open_editors(cx));
    let editor = editor.last().unwrap().clone();
    s.cx.simulate_input("x");
    let saving = s.cx.update(|window, cx| {
        editor.update(cx, |editor, cx| {
            editor.set_save_hook(Some(std::rc::Rc::new(HoldSaves)));
            editor.save(window, cx)
        })
    });
    s.draw();
    select(&mut s, "src/renamed.rs");
    run(&mut s, "project_panel.rename");
    name(&mut s, "again");
    assert!(s.path("src/renamed.rs").is_file());
    assert!(!s.path("src/again.rs").exists());
    drop(saving);

    // Trashed.
    s.cx.simulate_keystrokes("escape");
    select(&mut s, "src/renamed.rs");
    run(&mut s, "project_panel.delete");
    s.cx.simulate_prompt_answer("Move to Trash");
    s.draw();
    assert!(!s.path("src/renamed.rs").exists());
    wait_for(&mut s, "workspace/didDeleteFiles", 1);
    assert_eq!(
        notified(&s, "workspace/didDeleteFiles"),
        [json!([{"uri": renamed.as_str()}])]
    );
    assert_eq!(notified(&s, "workspace/didCreateFiles").len(), 1);
    assert_eq!(notified(&s, "workspace/didRenameFiles").len(), 1);
    let _ = fs::remove_file(s.path("notes.txt"));
}

/// [`script`]'s fake, also asked for its edits before a `*.rs` file's
/// rename; its answer is the test's to set.
fn participant() -> Script {
    let mut result = fe_lsp::fake::initialize_result(None);
    let filters = json!({"filters": [{"scheme": "file", "pattern": {"glob": "**/*.rs"}}]});
    result.capabilities.workspace = Some(
        serde_json::from_value(json!({"fileOperations": {
            "didCreate": filters, "didRename": filters, "didDelete": filters,
            "willRename": filters,
        }}))
        .unwrap(),
    );
    Script {
        initialize: InitializeReply::Result(Box::new(result)),
        ..Script::default()
    }
}

/// Has the fake answer `willRenameFiles` with `reply`.
fn answer(s: &Lsp, reply: Reply, delay: Duration) {
    s.fake
        .set_reply("workspace/willRenameFiles", Scripted { reply, delay });
}

/// What the participant answers renaming `src/util.rs`: `mod util;` to
/// `mod helpers;` in `src/main.rs` and `src/lib.rs`.
fn rename_edits(s: &Lsp) -> Value {
    let edit = json!([{"range": {"start": {"line": 0, "character": 4},
        "end": {"line": 0, "character": 8}}, "newText": "helpers"}]);
    json!({"changes": {
        s.uri("src/main.rs").as_str(): edit,
        s.uri("src/lib.rs").as_str(): edit,
    }})
}

const MAIN: &str = "mod util;\nfn main() {}\n";
const LIB: &str = "mod util;\npub fn f() {}\n";

/// `src/util.rs`, which `src/main.rs`, open and focused, and `src/lib.rs`,
/// closed, declare; `main.rs`'s editor.
fn modules(s: &mut Lsp) -> Entity<EditorView> {
    s.set_trust(true);
    fs::write(s.path("src/main.rs"), MAIN).unwrap();
    fs::write(s.path("src/lib.rs"), LIB).unwrap();
    fs::write(s.path("src/util.rs"), "pub fn h() {}\n").unwrap();
    let panel = s.workbench.read_with(&s.cx, |w, _| w.panel().clone());
    let util = FsChange::Path(s.path("src/util.rs"));
    s.cx.update(|_, cx| panel.update(cx, |panel, cx| panel.apply_fs_changes(&[util], cx)));
    s.open("src/main.rs");
    let main = s.uri("src/main.rs");
    s.fake.wait_for_document(&main, "didOpen", |r| r.is_some());
    let editor = s
        .workbench
        .read_with(&s.cx, |w, cx| w.active_editor(cx).cloned());
    editor.unwrap()
}

fn text(s: &Lsp, editor: &Entity<EditorView>) -> String {
    editor.read_with(&s.cx, |view, _| view.model().text().to_string())
}

/// The name field's note, while it's open.
fn note(s: &Lsp) -> Option<&'static str> {
    let panel = s.workbench.read_with(&s.cx, |w, _| w.panel().clone());
    panel.read_with(&s.cx, |panel, _| panel.name_field_note())
}

/// Where in what the fake got its first `method` is.
fn first(s: &Lsp, method: &str) -> usize {
    (s.fake.records().iter())
        .position(|record| record.method() == method)
        .unwrap_or_else(|| panic!("no {method}"))
}

#[gpui::test]
fn will_rename_edits_land_before_the_rename(cx: &mut TestAppContext) {
    let mut s = Lsp::with_scripts(cx, participant(), Script::default());
    answer(&s, Reply::Result(rename_edits(&s)), Duration::ZERO);
    let editor = modules(&mut s);

    select(&mut s, "src/util.rs");
    run(&mut s, "project_panel.rename");
    name(&mut s, "helpers");
    s.until("the rename", |s| s.path("src/helpers.rs").is_file());
    assert!(!s.path("src/util.rs").exists());
    assert_eq!(text(&s, &editor), "mod helpers;\nfn main() {}\n");
    assert_eq!(
        fs::read_to_string(s.path("src/lib.rs")).unwrap(),
        "mod helpers;\npub fn f() {}\n"
    );
    wait_for(&mut s, "workspace/didRenameFiles", 1);
    assert!(first(&s, "workspace/willRenameFiles") < first(&s, "workspace/didRenameFiles"));
    let (util, helpers) = (s.uri("src/util.rs"), s.uri("src/helpers.rs"));
    let request = (s.fake.records().into_iter()).find_map(|record| match record {
        Record::Request { method, params, .. } if method == "workspace/willRenameFiles" => {
            Some(params)
        }
        _ => None,
    });
    assert_eq!(
        request,
        Some(json!({"files": [{"oldUri": util.as_str(), "newUri": helpers.as_str()}]}))
    );

    // One undo step.
    editor.update(&mut s.cx, |view, cx| {
        view.model_mut().undo().unwrap();
        cx.notify();
    });
    assert_eq!(text(&s, &editor), MAIN);
}

#[gpui::test]
fn a_slow_participant_can_be_skipped(cx: &mut TestAppContext) {
    let mut s = Lsp::with_fake_clock(cx, participant(), Script::default());
    let editor = modules(&mut s);

    // Held: the field says so, and the file stays.
    answer(&s, Reply::Result(rename_edits(&s)), Duration::from_secs(5));
    select(&mut s, "src/util.rs");
    run(&mut s, "project_panel.rename");
    name(&mut s, "helpers");
    s.fake.wait_for("willRenameFiles", |records| {
        records
            .iter()
            .any(|record| record.method() == "workspace/willRenameFiles")
    });
    s.draw();
    assert_eq!(note(&s), Some("Renaming…"));
    assert!(s.path("src/util.rs").is_file());
    // Enter again renames nothing more.
    s.cx.simulate_keystrokes("enter");
    s.draw();
    assert_eq!(note(&s), Some("Renaming…"));

    // Escape renames at once, without the edits.
    s.cx.simulate_keystrokes("escape");
    assert!(s.path("src/helpers.rs").is_file());
    assert!(!s.path("src/util.rs").exists());
    s.draw();
    assert_eq!(note(&s), None);
    s.until("the request cancelled", |s| {
        (s.fake.records().iter()).any(|record| record.is_notification("$/cancelRequest"))
    });
    // The late answer isn't made.
    s.clock.advance(Duration::from_secs(5));
    s.draw();
    assert_eq!(text(&s, &editor), MAIN);
    assert_eq!(fs::read_to_string(s.path("src/lib.rs")).unwrap(), LIB);

    // A server's error: renamed without edits.
    let error = Reply::Error {
        code: -32603,
        message: "no".into(),
    };
    answer(&s, error, Duration::ZERO);
    select(&mut s, "src/helpers.rs");
    run(&mut s, "project_panel.rename");
    name(&mut s, "again");
    s.until("the rename", |s| s.path("src/again.rs").is_file());
    let asked = (s.fake.records().iter())
        .filter(|record| {
            matches!(record, Record::Request { method, .. } if method == "workspace/willRenameFiles")
        })
        .count();
    assert_eq!(asked, 2);
    assert_eq!(text(&s, &editor), MAIN);
    assert_eq!(fs::read_to_string(s.path("src/lib.rs")).unwrap(), LIB);
}

#[gpui::test]
fn servers_hear_of_operations_done_while_the_tree_reopens(cx: &mut TestAppContext) {
    let mut s = Lsp::with_scripts(cx, script(), Script::default());
    s.set_trust(true);
    s.open("src/main.rs");
    let main = s.uri("src/main.rs");
    s.fake.wait_for_document(&main, "didOpen", |r| r.is_some());
    let panel = s.workbench.read_with(&s.cx, |w, _| w.panel().clone());

    // Each duplicate lands while the tree is opened again, or once it's
    // open: either way servers hear of it.
    for copies in 1..=4 {
        select(&mut s, "src/other.rs");
        let context = ContextStack::new()
            .with(ContextFrame::new("workbench"))
            .with(ContextFrame::new("project_panel"));
        let duplicate = Invocation::of("project_panel.duplicate").unwrap();
        let run = RunCommand(duplicate.context(context));
        s.cx.update(|window, cx| window.dispatch_action(Box::new(run), cx));
        s.cx.update(|_, cx| {
            panel.update(cx, |panel, cx| {
                let shown = panel.lists_ignored();
                panel.set_show_ignored(!shown, cx);
            })
        });
        s.draw();
        wait_for(&mut s, "workspace/didCreateFiles", copies);
    }
    assert_eq!(notified(&s, "workspace/didCreateFiles").len(), 4);
}

#[gpui::test]
fn a_renamed_path_keeps_its_spelling(cx: &mut TestAppContext) {
    let mut s = Lsp::with_scripts(cx, script(), Script::default());
    s.set_trust(true);
    s.open("src/main.rs");
    let main = s.uri("src/main.rs");
    s.fake.wait_for_document(&main, "didOpen", |r| r.is_some());

    // As on a case-insensitive disk after `new.rs` became `NEW.rs`, the old
    // path still names the file, here by a symlink: it's sent as given.
    fs::write(s.path("src/NEW.rs"), "").unwrap();
    // A case-insensitive disk (macOS) already names the file by `new.rs`.
    if fs::symlink_metadata(s.path("src/new.rs")).is_err() {
        std::os::unix::fs::symlink("NEW.rs", s.path("src/new.rs")).unwrap();
    }
    let (from, to) = (s.path("src/new.rs"), s.path("src/NEW.rs"));
    (s.workbench).read_with(&s.cx, |w, _| w.lsp_files_renamed(&from, &to));
    wait_for(&mut s, "workspace/didRenameFiles", 1);
    let (old, new) = (s.uri("src/new.rs"), s.uri("src/NEW.rs"));
    assert_eq!(
        notified(&s, "workspace/didRenameFiles"),
        [json!([{"oldUri": old.as_str(), "newUri": new.as_str()}])]
    );
}

/// The name field's error, while it's open.
fn field_error(s: &Lsp) -> Option<String> {
    let panel = s.workbench.read_with(&s.cx, |w, _| w.panel().clone());
    panel.read_with(&s.cx, |panel, cx| panel.name_field(cx)?.1)
}

/// Starts renaming `src/util.rs` to `helpers.rs`, the answer held for 5 s
/// on the fake clock.
fn start_held_rename(s: &mut Lsp) {
    let asked = |records: &[Record]| {
        (records.iter())
            .filter(|record| {
                matches!(record, Record::Request { method, .. } if method == "workspace/willRenameFiles")
            })
            .count()
    };
    let before = asked(&s.fake.records());
    select(s, "src/util.rs");
    run(s, "project_panel.rename");
    name(s, "helpers");
    s.fake
        .wait_for("willRenameFiles", |records| asked(records) > before);
    s.clock.wait_for_sleep(Duration::from_secs(5));
    s.draw();
    assert_eq!(note(s), Some("Renaming…"));
}

/// Puts another file at `src/util.rs`, by renaming it over the one there,
/// so it's another inode.
fn replace_util(s: &Lsp) {
    fs::write(s.path("src/replacement"), "pub fn other() {}\n").unwrap();
    fs::rename(s.path("src/replacement"), s.path("src/util.rs")).unwrap();
}

#[gpui::test]
fn a_file_moved_while_participants_answer_isnt_renamed(cx: &mut TestAppContext) {
    let mut s = Lsp::with_fake_clock(cx, participant(), Script::default());
    let editor = modules(&mut s);
    answer(&s, Reply::Result(rename_edits(&s)), Duration::from_secs(5));
    let moved = "“util.rs” was moved or replaced while it was being renamed. Nothing was renamed.";
    let unchanged = |s: &Lsp| {
        assert_eq!(text(s, &editor), MAIN);
        assert_eq!(fs::read_to_string(s.path("src/lib.rs")).unwrap(), LIB);
        assert!(!s.path("src/helpers.rs").exists());
    };

    // Removed, then the answer lands: no edits, and nothing renamed.
    start_held_rename(&mut s);
    fs::remove_file(s.path("src/util.rs")).unwrap();
    s.clock.advance(Duration::from_secs(5));
    s.until("the answer", |s| note(s).is_none());
    assert_eq!(field_error(&s).as_deref(), Some(moved));
    unchanged(&s);
    s.cx.simulate_keystrokes("escape");
    s.draw();

    // Replaced, then the answer lands: the other file stays as it is.
    fs::write(s.path("src/util.rs"), "pub fn h() {}\n").unwrap();
    start_held_rename(&mut s);
    replace_util(&s);
    s.clock.advance(Duration::from_secs(5));
    s.until("the answer", |s| note(s).is_none());
    assert_eq!(field_error(&s).as_deref(), Some(moved));
    unchanged(&s);
    let util = fs::read_to_string(s.path("src/util.rs")).unwrap();
    assert_eq!(util, "pub fn other() {}\n");
    s.cx.simulate_keystrokes("escape");
    s.draw();

    // Replaced, then escape: nothing renamed either.
    start_held_rename(&mut s);
    replace_util(&s);
    s.cx.simulate_keystrokes("escape");
    s.draw();
    assert_eq!(note(&s), None);
    assert_eq!(field_error(&s).as_deref(), Some(moved));
    unchanged(&s);
    assert!(s.path("src/util.rs").is_file());
}

#[gpui::test]
fn participants_edits_to_one_file_are_joined(cx: &mut TestAppContext) {
    let mut s = Lsp::with_scripts(cx, participant(), participant());
    let editor = modules(&mut s);
    s.open("tool.py");
    let tool = s.uri("tool.py");
    s.python
        .wait_for_document(&tool, "didOpen", |r| r.is_some());
    let lib = s.uri("src/lib.rs");
    let edit = |line: u32, start: u32, end: u32, text: &str| {
        json!({"range": {"start": {"line": line, "character": start},
            "end": {"line": line, "character": end}}, "newText": text})
    };

    // rust-analyzer's `mod` lines, and the other server's insert in the
    // closed lib.rs, apart from rust-analyzer's there.
    answer(&s, Reply::Result(rename_edits(&s)), Duration::ZERO);
    let other = json!({"changes": {
        lib.as_str(): [edit(1, 0, 0, "// moved\n")],
    }});
    s.python.set_reply(
        "workspace/willRenameFiles",
        Scripted::now(Reply::Result(other)),
    );
    select(&mut s, "src/util.rs");
    run(&mut s, "project_panel.rename");
    name(&mut s, "helpers");
    s.until("the rename", |s| s.path("src/helpers.rs").is_file());
    assert_eq!(
        fs::read_to_string(s.path("src/lib.rs")).unwrap(),
        "mod helpers;\n// moved\npub fn f() {}\n"
    );
    assert_eq!(text(&s, &editor), "mod helpers;\nfn main() {}\n");

    // The other server's edit overlapping rust-analyzer's is left out;
    // rust-analyzer's is made.
    let lib_edit = |text: &str| json!({"changes": {lib.as_str(): [edit(0, 4, 11, text)]}});
    s.fake.set_reply(
        "workspace/willRenameFiles",
        Scripted::now(Reply::Result(lib_edit("again"))),
    );
    s.python.set_reply(
        "workspace/willRenameFiles",
        Scripted::now(Reply::Result(lib_edit("other"))),
    );
    select(&mut s, "src/helpers.rs");
    run(&mut s, "project_panel.rename");
    name(&mut s, "again");
    s.until("the rename", |s| s.path("src/again.rs").is_file());
    assert_eq!(
        fs::read_to_string(s.path("src/lib.rs")).unwrap(),
        "mod again;\n// moved\npub fn f() {}\n"
    );
}

#[path = "../lsp_file_operations_tests.rs"]
mod lsp_file_operations_tests;
