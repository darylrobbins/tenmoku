//! The project tree's file operations reach language servers: creating,
//! renaming and trashing a file send `didCreateFiles`, `didRenameFiles` and
//! `didDeleteFiles`, to the servers whose filters match; a refused or
//! unmatched operation sends nothing.

use std::fs;

use fe_command::{ContextFrame, ContextStack, Invocation};
use fe_gpui::{EditorView, commands::RunCommand};
use fe_lsp::fake::{InitializeReply, Record, Script};
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
