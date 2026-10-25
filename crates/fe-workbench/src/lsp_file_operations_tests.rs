//! `joined` joins a file's text edits only within a run between resource
//! operations: edits to a file either side of an operation that replaces it
//! are both made, in order.

use std::{fs, time::Duration};

use fe_command::{ContextFrame, ContextStack, Invocation};
use fe_gpui::commands::RunCommand;
use fe_lsp::fake::{InitializeReply, Reply, Script, Scripted};
use gpui::TestAppContext;
use serde_json::json;

use crate::tests::lsp::Lsp;

/// A rust-analyzer fake that is asked for its edits before a `*.rs`
/// file's rename.
fn participant() -> Script {
    let mut result = fe_lsp::fake::initialize_result(None);
    let filters = json!({"filters": [{"scheme": "file", "pattern": {"glob": "**/*.rs"}}]});
    result.capabilities.workspace =
        Some(serde_json::from_value(json!({"fileOperations": {"willRename": filters}})).unwrap());
    Script {
        initialize: InitializeReply::Result(Box::new(result)),
        ..Script::default()
    }
}

#[gpui::test]
fn will_rename_edits_around_an_overwrite_create_file(cx: &mut TestAppContext) {
    let mut s = Lsp::with_scripts(cx, participant(), Script::default());
    s.set_trust(true);
    fs::write(s.path("src/lib.rs"), "mod util;\npub fn f() {}\n").unwrap();
    fs::write(s.path("src/util.rs"), "pub fn h() {}\n").unwrap();
    s.open("src/main.rs");
    let main = s.uri("src/main.rs");
    s.fake.wait_for_document(&main, "didOpen", |r| r.is_some());
    let panel = s.workbench.read_with(&s.cx, |w, _| w.panel().clone());
    let util = fe_project::FsChange::Path(s.path("src/util.rs"));
    s.cx.update(|_, cx| panel.update(cx, |panel, cx| panel.apply_fs_changes(&[util], cx)));

    // An edit to lib.rs, lib.rs replaced, then an insert into the new file.
    let lib = s.uri("src/lib.rs");
    let at = |line: u32, start: u32, end: u32, text: &str| {
        json!({"range": {"start": {"line": line, "character": start},
            "end": {"line": line, "character": end}}, "newText": text})
    };
    let document = || json!({"uri": lib.as_str(), "version": null});
    let edits = json!({"documentChanges": [
        {"textDocument": document(), "edits": [at(0, 4, 8, "helpers")]},
        {"kind": "create", "uri": lib.as_str(), "options": {"overwrite": true}},
        {"textDocument": document(), "edits": [at(0, 0, 0, "// new\n")]},
    ]});
    s.fake.set_reply(
        "workspace/willRenameFiles",
        Scripted {
            reply: Reply::Result(edits),
            delay: Duration::ZERO,
        },
    );

    // Focus the tree on util.rs, as the project panel's own tests do.
    let workbench = s.workbench.clone();
    s.cx.update(|window, cx| {
        use gpui::Focusable as _;
        window.focus(&workbench.focus_handle(cx), cx)
    });
    s.cx.simulate_keystrokes("cmd-shift-e");
    let path = s.path("src/util.rs");
    s.cx.update(|_, cx| panel.update(cx, |panel, cx| panel.reveal(path, cx)));
    s.draw();
    let context = ContextStack::new()
        .with(ContextFrame::new("workbench"))
        .with(ContextFrame::new("project_panel"));
    let run = RunCommand(
        Invocation::of("project_panel.rename")
            .unwrap()
            .context(context),
    );
    s.cx.update(|window, cx| window.dispatch_action(Box::new(run), cx));
    s.draw();
    s.cx.simulate_input("helpers");
    s.cx.simulate_keystrokes("enter");
    s.draw();

    s.until("the rename", |s| s.path("src/helpers.rs").is_file());
    assert_eq!(
        fs::read_to_string(s.path("src/lib.rs")).unwrap(),
        "// new\n"
    );
}
