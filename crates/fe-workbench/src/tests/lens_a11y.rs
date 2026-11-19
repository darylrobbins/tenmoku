//! Lens items as screen-reader buttons (T1964), for each provider in a
//! headless workbench: the test lens's ▷ Run test and Debug, a language
//! server's lens and a merge conflict's actions are Buttons named for their
//! titles, and a screen reader's Click on one runs it.

use std::{cell::RefCell, rc::Rc, time::Duration};

use fe_buffer::Buffer;
use fe_command::Invocation;
use fe_gpui::{EditorView, Lens};
use fe_lsp::fake::{InitializeReply, Record, Reply, Script, Scripted, initialize_result};
use gpui::{Entity, Focusable as _, TestAppContext, VisualTestContext, accesskit::Action};
use kiln_l10n::tr;
use lsp_types::{CodeLensOptions, ExecuteCommandOptions, PositionEncodingKind, ServerCapabilities};
use serde_json::{Value, json};

use crate::{
    Launch,
    test_commands::ConsumedTests,
    test_lens_tests::{LIB, project},
    test_runnables::TestFramework,
    test_runner::set_history_dir,
    tests::{lsp::Lsp, open},
};

/// The lenses every provider of `editor` answers for its text.
fn lenses(editor: &Entity<EditorView>, cx: &mut VisualTestContext) -> Vec<Lens> {
    editor.update(cx, |editor, _| {
        let snapshot = editor.model().snapshot().clone();
        (editor.lens_providers().iter())
            .flat_map(|provider| provider.borrow_mut().lenses(&snapshot))
            .collect()
    })
}

/// Draws `editor` afresh, then the role and name of the node over item
/// `index` of the lens row above `row`, if there is one.
fn node(
    editor: &Entity<EditorView>,
    row: u32,
    index: usize,
    cx: &mut VisualTestContext,
) -> Option<(fe_ui::Role, String)> {
    editor.update(cx, |_, cx| cx.notify());
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    let id = EditorView::lens_item_id(row, index);
    let nodes = cx.update(|window, cx| fe_ui::nodes(window, cx));
    (nodes.into_iter())
        .find(|node| node.id == id)
        .map(|node| (node.role, node.label.to_string()))
}

/// A screen reader's Click on item `index` of the lens row above `row`.
fn click(row: u32, index: usize, cx: &mut VisualTestContext) {
    let id = EditorView::lens_item_id(row, index);
    let registered = cx.update(|window, cx| fe_ui::actions(window, cx));
    let actions = (registered.iter()).find(|node| node.id == id);
    assert_eq!(
        actions.map(|node| &node.actions[..]),
        Some(&[Action::Click][..])
    );
    assert!(cx.update(|window, cx| fe_ui::perform(window, &id, Action::Click, cx)));
}

fn button(name: impl Into<String>) -> Option<(fe_ui::Role, String)> {
    Some((fe_ui::Role::Button, name.into()))
}

/// A test lens's title as a screen reader names it: without its ▷.
fn named(title: &str) -> String {
    title.trim_start_matches('\u{25b7}').trim_start().to_owned()
}

#[gpui::test]
fn the_test_lens_run_and_debug_are_buttons(cx: &mut TestAppContext) {
    let state = tempfile::tempdir().unwrap();
    cx.update(|cx| set_history_dir(cx, state.path().to_owned()));
    let project = project(cx, "src/lib.rs", LIB, true);
    // A build the Debug starts runs nothing.
    let terminal = (project.workbench).read_with(project.cx, |w, _| w.terminal().clone());
    terminal.update(project.cx, |terminal, _| terminal.fake_sessions = true);
    let (editor, cx) = (project.editor.clone(), project.cx);
    let shown = lenses(&editor, cx);
    let test = shown
        .iter()
        .find(|lens| lens.items.len() > 1)
        .expect("a test's lens");
    let row = test.row;

    let run = named(&tr!("workbench-lens-run-test"));
    assert_eq!(run, "Run test");
    assert_eq!(node(&editor, row, 0, cx), button(run));
    let debug = tr!("workbench-debug-test-lens").to_string();
    assert_eq!(node(&editor, row, 1, cx), button(debug));
    // The module's lens, above it: Run tests.
    let module = shown
        .iter()
        .find(|lens| lens.row < row)
        .expect("the module's");
    let run_tests = named(&tr!("workbench-lens-run-tests"));
    assert_eq!(node(&editor, module.row, 0, cx), button(run_tests));

    // Debug's Click runs T1856's debug invocation for the test's row.
    let seen = Rc::new(RefCell::new(Vec::<Invocation>::new()));
    let sink = seen.clone();
    cx.update(|_, cx| {
        fe_gpui::commands::observe_invocations(cx, move |invocation| {
            sink.borrow_mut().push(invocation.clone())
        })
    });
    click(row, 1, cx);
    for _ in 0..50 {
        cx.run_until_parked();
        cx.update(|window, cx| window.draw(cx).clear(cx));
        let consumed = cx.update(|_, cx| cx.try_global::<ConsumedTests>().map(|c| c.0.clone()));
        if consumed.is_some_and(|consumed| !consumed.is_empty()) {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    let ran: Vec<_> = (seen.borrow().iter())
        .map(|invocation| {
            let row = invocation.args.get("row").and_then(Value::as_u64);
            (invocation.command.to_string(), row)
        })
        .collect();
    assert_eq!(
        ran,
        [("debug.test_at_cursor".to_owned(), Some(u64::from(row)))]
    );
    let consumed = cx.update(|_, cx| cx.try_global::<ConsumedTests>().map(|c| c.0.clone()));
    let consumed = consumed.unwrap_or_default();
    assert!(
        matches!(
            &consumed[..],
            [("debug", test)] if matches!(&test.framework,
                TestFramework::Cargo { name, .. } if name == "tests::adds")
        ),
        "{consumed:?}"
    );
}

const EXECUTE: &str = "workspace/executeCommand";

/// rust-analyzer's way: a lens with data only, resolved to `▶︎ Run Test`, a
/// command the server lists.
fn lsp_script() -> Script {
    let mut init = initialize_result(Some(PositionEncodingKind::UTF8));
    init.capabilities = ServerCapabilities {
        code_lens_provider: Some(CodeLensOptions {
            resolve_provider: Some(true),
        }),
        execute_command_provider: Some(ExecuteCommandOptions {
            commands: vec!["rust-analyzer.runSingle".into()],
            ..Default::default()
        }),
        ..init.capabilities
    };
    Script {
        initialize: InitializeReply::Result(Box::new(init)),
        ..Script::default()
    }
}

#[gpui::test]
fn an_lsp_lens_is_a_button_that_runs_its_command(cx: &mut TestAppContext) {
    let mut s = Lsp::with_fake_clock(cx, lsp_script(), Script::default());
    let range =
        json!({ "start": { "line": 1, "character": 0 }, "end": { "line": 1, "character": 9 } });
    s.fake.set_reply(
        "textDocument/codeLens",
        Scripted::now(Reply::Result(
            json!([{ "range": range, "data": { "id": 1 } }]),
        )),
    );
    let title = "\u{25b6}\u{fe0e} Run Test";
    s.fake.set_reply(
        "codeLens/resolve",
        Scripted::now(Reply::Result(json!({
            "range": range, "data": { "id": 1 },
            "command": { "title": title, "command": "rust-analyzer.runSingle",
                         "arguments": [{ "n": 1 }] },
        }))),
    );
    s.fake
        .set_reply(EXECUTE, Scripted::now(Reply::Result(Value::Null)));
    s.set_trust(true);
    std::fs::write(s.path("src/main.rs"), "fn a() {}\nfn b() {}\n").unwrap();
    s.open("src/main.rs");
    let editor = (s.workbench)
        .read_with(&s.cx, |w, cx| w.active_editor(cx).cloned())
        .expect("main.rs is shown");
    s.cx.update(|window, cx| window.focus(&editor.focus_handle(cx), cx));
    s.until("the resolved lens", |s| {
        s.clock.advance(Duration::from_millis(20));
        let shown = lenses(&editor, &mut s.cx);
        shown
            .iter()
            .any(|lens| lens.items.iter().any(|item| item.title == title))
    });

    assert_eq!(node(&editor, 1, 0, &mut s.cx), button("Run Test"));
    click(1, 0, &mut s.cx);
    let sent = |s: &Lsp| -> Vec<Value> {
        (s.fake.records().iter())
            .filter_map(|record| match record {
                Record::Request { method, params, .. } if method == EXECUTE => Some(params.clone()),
                _ => None,
            })
            .collect()
    };
    s.until("the command", |s| {
        s.clock.advance(Duration::from_millis(20));
        !sent(s).is_empty()
    });
    let sent = sent(&s);
    assert_eq!(sent.len(), 1, "{sent:?}");
    assert_eq!(sent[0]["command"], "rust-analyzer.runSingle");
    assert_eq!(sent[0]["arguments"], json!([{ "n": 1 }]));
}

/// A workbench showing a two-way conflict over rows 0 to 4, and its editor.
fn conflict(
    cx: &mut TestAppContext,
) -> (
    tempfile::TempDir,
    Entity<EditorView>,
    &mut VisualTestContext,
) {
    let text = "<<<<<<< HEAD\na\n=======\nb\n>>>>>>> other\nmid\n";
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("f.txt");
    std::fs::write(&path, text).unwrap();
    let launch = Launch {
        root: Some(dir.path().to_owned()),
        file: Some((path, Some(Buffer::new(text).into()))),
    };
    let (workbench, cx, _) = open(cx, launch);
    cx.update(|window, cx| window.focus(&workbench.focus_handle(cx), cx));
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    let editor = workbench
        .read_with(cx, |workbench, cx| workbench.active_editor(cx).cloned())
        .expect("an editor shows");
    (dir, editor, cx)
}

#[gpui::test]
fn merge_conflict_lenses_are_buttons(cx: &mut TestAppContext) {
    let (_dir, editor, cx) = conflict(cx);

    let names = [
        tr!("workbench-git-conflict-accept-current"),
        tr!("workbench-git-conflict-accept-incoming"),
        tr!("workbench-git-conflict-accept-both"),
        tr!("workbench-git-conflict-compare"),
    ]
    .map(|name| name.to_string());
    assert_eq!(
        names,
        [
            "Accept Current Change",
            "Accept Incoming Change",
            "Accept Both Changes",
            "Compare Changes",
        ]
    );
    for (index, name) in names.into_iter().enumerate() {
        assert_eq!(node(&editor, 0, index, cx), button(name));
    }
    let incoming = &lenses(&editor, cx)[0].items[1];
    let invocation = incoming.invocation.as_ref().expect("a command");
    assert_eq!(invocation.command.as_str(), "git.accept_incoming");
    assert_eq!(invocation.args.get("row").and_then(Value::as_u64), Some(0));

    click(0, 1, cx);
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    let now = editor.read_with(cx, |editor, _| editor.model().text().to_string());
    assert_eq!(now, "b\nmid\n");
    assert_eq!(
        node(&editor, 0, 0, cx),
        None,
        "the conflict and its lens are gone"
    );
}

/// The pointer's way to the same item (the context fix in
/// fe-gpui's `run_lens`): before, a click ran it in the editor's frame
/// alone, where the workbench's `git.accept_incoming` isn't found.
#[gpui::test]
fn a_pointer_click_on_a_conflict_lens_runs_it(cx: &mut TestAppContext) {
    let (_dir, editor, cx) = conflict(cx);
    let drawn = editor.read_with(cx, |editor, _| {
        editor.drawn_annotations().expect("drawn").lenses.clone()
    });
    let incoming = tr!("workbench-git-conflict-accept-incoming");
    let item = (drawn.iter())
        .find(|text| text.text == incoming)
        .expect("drawn");
    let at = item.bounds.center();
    cx.simulate_mouse_down(at, gpui::MouseButton::Left, gpui::Modifiers::none());
    cx.simulate_mouse_up(at, gpui::MouseButton::Left, gpui::Modifiers::none());
    cx.run_until_parked();
    let now = editor.read_with(cx, |editor, _| editor.model().text().to_string());
    assert_eq!(now, "b\nmid\n");
}
