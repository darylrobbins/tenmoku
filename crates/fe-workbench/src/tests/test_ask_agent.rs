//! `Ask an agent` on a failed test in the Runs tool window
//! ([`crate::test_run_view`]): the chat's composer prefilled, nothing sent.

use std::{fs, rc::Rc, sync::Arc};

use fe_chat::{ChatProvider, ContextItem, ContextKind, ScriptedProvider};
use fe_command::{ContextFrame, ContextStack, Invocation};
use fe_gpui::commands::RunCommand;
use fe_runlog::TestId;
use gpui::{Entity, Focusable as _, TestAppContext, VisualTestContext};
use kiln::core::secrets::MemoryStore;
use kiln_l10n::tr;

use crate::{
    Launch, Workbench,
    chat_panel::{ChatPanel, KeySource, chat_context::MAX_FILE},
    test_run_view::{ASK_AGENT_SELECTOR, SLOT_SELECTOR, TestRunView},
    test_runner::{TestOutcome, TestResult, Tracks, set_history_dir},
    tests::open,
};

const SOURCE: &str = "pub fn add(a: u32, b: u32) -> u32 {\n    a + b\n}\n\n#[cfg(test)]\nmod tests {\n    #[test]\n    fn adds() {\n        assert_eq!(1, 2);\n    }\n}\n";

const ASSERTION: &str = "thread 'tests::adds' panicked at src/lib.rs:9:9:\nassertion `left == right` failed: sums differ\n  left: 1\n right: 2\n";

fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.run_until_parked();
}

/// A workbench on package `p` showing the Runs tool window with the failed
/// test `tests::adds`, whose output is `output`, selected.
fn project<'a>(
    cx: &'a mut TestAppContext,
    output: &str,
    with_source: bool,
    trusted: bool,
) -> (
    Entity<Workbench>,
    &'a mut VisualTestContext,
    tempfile::TempDir,
    tempfile::TempDir,
) {
    cx.executor().allow_parking();
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("Cargo.toml"), "[package]\nname = \"p\"\n").unwrap();
    fs::create_dir(dir.path().join("src")).unwrap();
    if with_source {
        fs::write(dir.path().join("src/lib.rs"), SOURCE).unwrap();
    }
    let settings = fe_config::project_settings_path(dir.path());
    fs::create_dir_all(settings.parent().unwrap()).unwrap();
    fs::write(settings, "[lsp.servers.rust-analyzer]\nenabled = false\n").unwrap();
    let state = tempfile::tempdir().unwrap();
    cx.update(|cx| set_history_dir(cx, state.path().to_owned()));
    let launch = Launch {
        root: Some(dunce::canonicalize(dir.path()).unwrap()),
        file: None,
    };
    let (workbench, cx, _) = open(cx, launch);
    cx.update(|window, cx| {
        window.activate_window();
        ChatPanel::set_provider_factory(
            cx,
            Rc::new(|_, _| Arc::new(ScriptedProvider::new([])) as Arc<dyn ChatProvider>),
        );
        cx.set_global(KeySource {
            env: Some("key".to_owned()),
            store: Arc::new(MemoryStore::new()),
        });
    });
    cx.update(|window, cx| window.focus(&workbench.focus_handle(cx), cx));
    workbench.update(cx, |workbench, cx| workbench.set_root_trust(trusted, cx));
    draw(cx);
    let context = ContextStack::new().with(ContextFrame::new("workbench"));
    let run = RunCommand(Invocation::of("runs.show").unwrap().context(context));
    cx.update(|window, cx| window.dispatch_action(Box::new(run), cx));
    draw(cx);
    let result = TestResult {
        id: TestId::new("p", "tests::adds"),
        outcome: TestOutcome::Failed,
        output: output.to_owned(),
        secs: Some(0.01),
    };
    workbench.update(cx, |workbench, cx| {
        workbench.record_test_run("cargo test", vec![result], Tracks::default(), cx)
    });
    draw(cx);
    let panel = workbench.read_with(cx, |wb, _| wb.runs_panel().clone());
    panel.update(cx, |panel, cx| {
        panel.list().update(cx, |list, cx| list.select(0, cx))
    });
    draw(cx);
    let view: Entity<TestRunView> = panel.read_with(cx, |panel, _| panel.detail().clone());
    let tree = view.read_with(cx, |view, _| view.tree().clone());
    let at = tree.read_with(cx, |tree, _| {
        let rows = tree.delegate().rows();
        rows.iter().position(|row| row.label == "adds")
    });
    tree.update(cx, |tree, cx| tree.select(at.expect("a row"), cx));
    draw(cx);
    (workbench, cx, dir, state)
}

fn click_ask(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) {
    let bounds = cx.debug_bounds(ASK_AGENT_SELECTOR).expect("drawn");
    let slot = cx.debug_bounds(SLOT_SELECTOR).expect("drawn");
    assert!(
        slot.contains(&bounds.center()),
        "{bounds:?} is in the slot, {slot:?}"
    );
    cx.simulate_click(bounds.center(), Default::default());
    // The function is found off the main thread.
    for _ in 0..1000 {
        draw(cx);
        if !items(workbench, cx).0.is_empty() {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
}

fn chat(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Entity<ChatPanel> {
    workbench.read_with(cx, |workbench, cx| {
        workbench.dock().read(cx).chat().cloned().expect("a chat")
    })
}

fn items(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> (String, Vec<ContextItem>) {
    let view = chat(workbench, cx).read_with(cx, |panel, _| panel.view().clone());
    view.read_with(cx, |view, cx| (view.composer_text(cx), view.attachments()))
}

#[gpui::test]
fn the_button_prefills_the_chat_with_assertion_and_output_cut_to_the_item_limit(
    cx: &mut TestAppContext,
) {
    let lines = "x".repeat(99) + "\n";
    let output = format!("{}{ASSERTION}", lines.repeat(MAX_FILE / 100 + 10));
    let (workbench, cx, _dir, _state) = project(cx, &output, false, true);
    click_ask(&workbench, cx);
    let (prompt, attached) = items(&workbench, cx);
    assert_eq!(
        prompt,
        tr!("workbench-test-runs-ask-agent-prompt", name = "tests::adds").to_string()
    );
    let [item] = attached.as_slice() else {
        panic!("one item, got {attached:?}");
    };
    assert_eq!(
        item.label,
        tr!("workbench-test-runs-ask-agent-item").to_string()
    );
    let head = tr!(
        "workbench-test-runs-ask-agent-assertion",
        left = "1",
        right = "2"
    )
    .to_string();
    assert!(item.text.starts_with(&head), "{}", &item.text[..80]);
    // The end of the output is kept, the start cut.
    assert!(item.text.ends_with(ASSERTION));
    assert!(item.text.len() <= MAX_FILE);
}

#[gpui::test]
fn it_attaches_the_test_function_when_in_the_root(cx: &mut TestAppContext) {
    let (workbench, cx, _dir, _state) = project(cx, ASSERTION, true, true);
    click_ask(&workbench, cx);
    let (_, attached) = items(&workbench, cx);
    let function = attached
        .iter()
        .find(|item| matches!(&item.kind, ContextKind::File { path, rows: Some(_) } if path == "src/lib.rs"))
        .unwrap_or_else(|| panic!("the function, got {attached:?}"));
    assert_eq!(
        function.text,
        "    fn adds() {\n        assert_eq!(1, 2);\n    }\n"
    );
    assert_eq!(attached.len(), 2);
}

#[gpui::test]
fn it_is_disabled_when_chat_is_unavailable(cx: &mut TestAppContext) {
    let (workbench, cx, _dir, _state) = project(cx, ASSERTION, true, false);
    click_ask(&workbench, cx);
    let has_chat = workbench.read_with(cx, |workbench, cx| {
        workbench
            .dock()
            .read(cx)
            .chat()
            .is_some_and(|panel| !panel.read(cx).view().read(cx).composer_text(cx).is_empty())
    });
    assert!(!has_chat, "nothing was prefilled");
}
