//! Inline Edit with AI over a scripted provider: no network.

use std::{fs, ops::Range, rc::Rc, sync::Arc};

use fe_buffer::{Origin, ResolvedSelection};
use fe_chat::StopReason;
use fe_chat::{BlockKind, ChatEvent, ChatProvider, ErrorKind, ProviderError, ScriptedProvider};
use fe_command::{ContextFrame, ContextStack, Invocation};
use fe_gpui::{EditorView, commands::RunCommand};
use gpui::{Entity, Focusable as _, TestAppContext, VisualTestContext};
use kiln::core::secrets::MemoryStore;
use serde_json::json;

use super::{InlineEdit, InlineEdits, Phase};
use crate::{
    Launch, Workbench,
    chat_panel::{ChatPanel, KeySource},
    tests::open,
};

const MAIN: &str = "func main() {\n    x := 1\n    y := 2\n}\n";
/// `x := 1`, the selection the tests edit.
const SELECTION: Range<usize> = 18..24;

/// A reply of `text`, streamed as one text block.
fn reply(text: &str) -> Vec<ChatEvent> {
    vec![
        ChatEvent::Started {
            model: "claude-opus-5-5".to_owned(),
            message_id: "msg".to_owned(),
        },
        ChatEvent::BlockStarted {
            index: 0,
            kind: BlockKind::Text,
        },
        ChatEvent::TextDelta {
            index: 0,
            text: text.to_owned(),
        },
        ChatEvent::BlockFinished {
            index: 0,
            block: json!({"type": "text", "text": text}),
        },
        ChatEvent::Finished {
            stop_reason: StopReason::EndTurn,
            stop_details: None,
        },
    ]
}

fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.run_until_parked();
}

struct Fixture {
    workbench: Entity<Workbench>,
    editor: Entity<EditorView>,
    provider: Arc<ScriptedProvider>,
    _dir: tempfile::TempDir,
}

/// `main.go` open with `SELECTION` selected and focused, over a provider
/// giving `replies`, with a key if `key`.
fn setup(
    cx: &mut TestAppContext,
    replies: Vec<Vec<ChatEvent>>,
    key: bool,
) -> (Fixture, &mut VisualTestContext) {
    let temp = fs::canonicalize(std::env::temp_dir()).unwrap();
    let dir = tempfile::tempdir_in(temp).unwrap();
    fs::write(dir.path().join("main.go"), MAIN).unwrap();
    let launch = Launch {
        root: Some(dir.path().to_owned()),
        file: None,
    };
    let (workbench, cx, _) = open(cx, launch);
    cx.update(|window, _| window.activate_window());
    let provider = Arc::new(ScriptedProvider::new(replies));
    let shared = provider.clone();
    cx.update(|_, cx| {
        ChatPanel::set_provider_factory(
            cx,
            Rc::new(move |_, _| shared.clone() as Arc<dyn ChatProvider>),
        );
        cx.set_global(KeySource {
            env: key.then(|| "key".to_owned()),
            store: Arc::new(MemoryStore::new()),
        });
    });
    draw(cx);
    let pane = workbench.read_with(cx, |workbench, _| workbench.pane().clone());
    let path = dir.path().join("main.go");
    cx.update(|window, cx| pane.update(cx, |pane, cx| pane.open(path, window, cx)));
    draw(cx);
    let editor = pane
        .read_with(cx, |pane, _| pane.active_editor().cloned())
        .expect("an editor");
    select(&editor, SELECTION, cx);
    cx.update(|window, cx| window.focus(&editor.focus_handle(cx), cx));
    draw(cx);
    let fixture = Fixture {
        workbench,
        editor,
        provider,
        _dir: dir,
    };
    (fixture, cx)
}

fn select(editor: &Entity<EditorView>, range: Range<usize>, cx: &mut VisualTestContext) {
    editor.update(cx, |editor, cx| {
        let selection = ResolvedSelection {
            range,
            reversed: false,
        };
        editor.model_mut().set_selections([selection]);
        cx.notify();
    });
}

fn edit(editor: &Entity<EditorView>, range: Range<usize>, text: &str, cx: &mut VisualTestContext) {
    editor.update(cx, |editor, cx| {
        let mut buffer = editor.model_mut().buffer_mut();
        buffer.edit([(range, text)], Origin::User);
        drop(buffer);
        cx.notify();
    });
    draw(cx);
}

/// Runs `id` as a key would in the frames `contexts`, at the focus; the
/// scheduler isn't run.
fn run(id: &str, contexts: &[&str], cx: &mut VisualTestContext) {
    let stack = (contexts.iter()).fold(ContextStack::new(), |stack, context| {
        stack.with(ContextFrame::new(*context))
    });
    let run = RunCommand(Invocation::of(id).unwrap().context(stack));
    cx.update(|window, cx| window.dispatch_action(Box::new(run), cx));
}

fn open_input(cx: &mut VisualTestContext) {
    run("ai.inline_edit", &["workbench", "editor"], cx);
    draw(cx);
}

fn submit(cx: &mut VisualTestContext) {
    run(
        "inline_edit.submit",
        &["workbench", "editor", "inline_edit", "text_input"],
        cx,
    );
}

fn escape(cx: &mut VisualTestContext) {
    run(
        "inline_edit.cancel",
        &["workbench", "editor", "inline_edit"],
        cx,
    );
}

fn input_of(f: &Fixture, cx: &mut VisualTestContext) -> Option<Entity<InlineEdit>> {
    let id = f.editor.entity_id();
    cx.update(|_, cx| {
        cx.try_global::<InlineEdits>()
            .and_then(|edits| edits.0.get(&id).cloned())
    })
}

fn type_text(open: &Entity<InlineEdit>, text: &str, cx: &mut VisualTestContext) {
    let input = open.read_with(cx, |open, _| open.input().clone());
    input.update(cx, |input, cx| input.set_text(text, cx));
}

fn toasts(f: &Fixture, cx: &mut VisualTestContext) -> Vec<String> {
    let layer = f
        .workbench
        .read_with(cx, |workbench, _| workbench.toasts().clone());
    let shown = layer.read_with(cx, |layer, _| {
        (layer.queue().shown().iter())
            .map(|entry| entry.toast.message.clone())
            .collect()
    });
    layer.update(cx, |layer, cx| layer.dismiss_all(cx));
    shown
}

fn text(f: &Fixture, cx: &mut VisualTestContext) -> String {
    f.editor
        .read_with(cx, |editor, _| editor.model().text().to_string())
}

fn hunks(f: &Fixture, cx: &mut VisualTestContext) -> Vec<(String, String)> {
    f.editor.read_with(cx, |editor, _| {
        let snapshot = editor.model().buffer().snapshot();
        (editor.proposals().iter())
            .flat_map(|proposal| {
                proposal
                    .borrow()
                    .hunks(&snapshot)
                    .into_iter()
                    .map(|hunk| (hunk.old_text.to_string(), hunk.new_text.to_string()))
                    .collect::<Vec<_>>()
            })
            .collect()
    })
}

fn focused(open: &Entity<InlineEdit>, cx: &mut VisualTestContext) -> bool {
    cx.update(|window, cx| open.read(cx).input().focus_handle(cx).is_focused(window))
}

#[gpui::test]
fn command_opens_input_under_selection(cx: &mut TestAppContext) {
    let (f, cx) = setup(cx, vec![], true);
    open_input(cx);
    let open = input_of(&f, cx).expect("an input");
    assert!(cx.debug_bounds("inline-edit-input").is_some());
    assert!(focused(&open, cx));
    assert_eq!(
        open.read_with(cx, |open, cx| open.current_range(cx)),
        Some(SELECTION)
    );
    let row = f
        .editor
        .read_with(cx, |editor, _| editor.overlay_anchor_row());
    assert_eq!(row, Some(1));
    // A caret anchors where it is.
    escape(cx);
    draw(cx);
    assert!(input_of(&f, cx).is_none());
    assert!(cx.debug_bounds("inline-edit-input").is_none());
    select(&f.editor, 5..5, cx);
    open_input(cx);
    let open = input_of(&f, cx).expect("an input");
    assert_eq!(
        open.read_with(cx, |open, cx| open.current_range(cx)),
        Some(5..5)
    );
}

#[gpui::test]
fn input_is_anchored_through_edits(cx: &mut TestAppContext) {
    let (f, cx) = setup(cx, vec![], true);
    open_input(cx);
    let open = input_of(&f, cx).unwrap();
    edit(&f.editor, 0..0, "// top\n", cx);
    assert_eq!(
        open.read_with(cx, |open, cx| open.current_range(cx)),
        Some(SELECTION.start + 7..SELECTION.end + 7)
    );
    let row = f
        .editor
        .read_with(cx, |editor, _| editor.overlay_anchor_row());
    assert_eq!(row, Some(2));
}

#[gpui::test]
fn read_only_editor_refuses_with_toast(cx: &mut TestAppContext) {
    let (f, cx) = setup(cx, vec![], true);
    f.editor
        .update(cx, |editor, cx| editor.set_read_only(true, cx));
    open_input(cx);
    assert!(input_of(&f, cx).is_none());
    assert_eq!(toasts(&f, cx), ["This editor is read-only"]);
}

#[gpui::test]
fn enter_streams_and_shows_hunks(cx: &mut TestAppContext) {
    let (f, cx) = setup(cx, vec![reply("```go\nx := 10\n```")], true);
    open_input(cx);
    let open = input_of(&f, cx).unwrap();
    type_text(&open, "make it ten", cx);
    submit(cx);
    assert_eq!(
        open.read_with(cx, |open, _| open.phase()),
        Phase::Generating
    );
    draw(cx);
    assert!(input_of(&f, cx).is_none(), "{:?}", toasts(&f, cx));
    assert!(cx.debug_bounds("inline-edit-input").is_none());
    let hunks = hunks(&f, cx);
    assert_eq!(hunks.len(), 1);
    assert!(hunks[0].1.contains("x := 10"), "{hunks:?}");
    let request = f.provider.requests().remove(0);
    let user = request.messages[0].content.last().unwrap()["text"]
        .as_str()
        .unwrap()
        .to_owned();
    assert_eq!(user, "make it ten");
}

#[gpui::test]
fn no_key_toasts(cx: &mut TestAppContext) {
    let (f, cx) = setup(cx, vec![reply("```go\nx\n```")], false);
    open_input(cx);
    let open = input_of(&f, cx).unwrap();
    type_text(&open, "make it ten", cx);
    submit(cx);
    draw(cx);
    assert_eq!(toasts(&f, cx), ["Set an API key in the chat"]);
    assert!(input_of(&f, cx).is_some());
    assert!(f.provider.requests().is_empty());
}

#[gpui::test]
fn failed_keeps_instruction(cx: &mut TestAppContext) {
    let failure = ChatEvent::Failed(ProviderError::new(ErrorKind::InvalidRequest, "overloaded"));
    let (f, cx) = setup(cx, vec![vec![failure]], true);
    open_input(cx);
    let open = input_of(&f, cx).unwrap();
    type_text(&open, "make it ten", cx);
    submit(cx);
    draw(cx);
    assert_eq!(open.read_with(cx, |open, _| open.phase()), Phase::Asking);
    assert_eq!(
        open.read_with(cx, |open, _| open.message().map(str::to_owned)),
        Some("overloaded".to_owned())
    );
    assert_eq!(
        open.read_with(cx, |open, cx| open.input().read(cx).text().to_owned()),
        "make it ten"
    );
    assert!(hunks(&f, cx).is_empty());
}

#[gpui::test]
fn no_code_reply_message(cx: &mut TestAppContext) {
    let (f, cx) = setup(cx, vec![reply("Sure, I will do that.")], true);
    open_input(cx);
    let open = input_of(&f, cx).unwrap();
    type_text(&open, "make it ten", cx);
    submit(cx);
    draw(cx);
    let message = open.read_with(cx, |open, _| open.message().map(str::to_owned));
    assert_eq!(message.as_deref(), Some("The reply had no code to apply"));
    assert_eq!(
        open.read_with(cx, |open, cx| open.input().read(cx).text().to_owned()),
        "make it ten"
    );
}

#[gpui::test]
fn escape_cancels_the_provider(cx: &mut TestAppContext) {
    let (f, cx) = setup(cx, vec![reply("```go\nx := 10\n```")], true);
    open_input(cx);
    let open = input_of(&f, cx).unwrap();
    type_text(&open, "make it ten", cx);
    submit(cx);
    assert_eq!(
        open.read_with(cx, |open, _| open.phase()),
        Phase::Generating
    );
    escape(cx);
    draw(cx);
    assert_eq!(f.provider.dropped(), 1);
    assert_eq!(open.read_with(cx, |open, _| open.phase()), Phase::Asking);
    assert!(hunks(&f, cx).is_empty());
    assert_eq!(text(&f, cx), MAIN);
    // Escape in the input closes it.
    escape(cx);
    draw(cx);
    assert!(input_of(&f, cx).is_none());
}

#[gpui::test]
fn buffer_untouched_until_accept(cx: &mut TestAppContext) {
    let (f, cx) = setup(cx, vec![reply("```go\nx := 10\n```")], true);
    open_input(cx);
    let open = input_of(&f, cx).unwrap();
    type_text(&open, "make it ten", cx);
    submit(cx);
    draw(cx);
    assert_eq!(text(&f, cx), MAIN);
    assert_eq!(hunks(&f, cx).len(), 1);
}

#[gpui::test]
fn deleted_range_discards_result(cx: &mut TestAppContext) {
    let (f, cx) = setup(cx, vec![reply("```go\nx := 10\n```")], true);
    open_input(cx);
    let open = input_of(&f, cx).unwrap();
    type_text(&open, "make it ten", cx);
    submit(cx);
    edit(&f.editor, 14..MAIN.len() - 2, "", cx);
    draw(cx);
    assert!(hunks(&f, cx).is_empty());
    assert!(input_of(&f, cx).is_none());
    assert_eq!(toasts(&f, cx).len(), 1);
}

#[gpui::test]
fn second_invocation_focuses_existing(cx: &mut TestAppContext) {
    let (f, cx) = setup(cx, vec![reply("```go\nx\n```")], true);
    open_input(cx);
    let open = input_of(&f, cx).unwrap();
    type_text(&open, "go", cx);
    submit(cx);
    cx.update(|window, cx| window.focus(&f.editor.focus_handle(cx), cx));
    assert!(!focused(&open, cx));
    // The reply hasn't landed: the scheduler isn't run.
    run("ai.inline_edit", &["workbench", "editor"], cx);
    let again = input_of(&f, cx).unwrap();
    assert_eq!(again.entity_id(), open.entity_id());
    assert!(focused(&open, cx));
}

#[gpui::test]
fn the_editors_keys_do_not_reach_through_the_input(cx: &mut TestAppContext) {
    let (f, cx) = setup(cx, vec![], true);
    open_input(cx);
    let open = input_of(&f, cx).unwrap();
    for id in ["editor.delete", "editor.cut", "editor.select_all"] {
        run(
            id,
            &["workbench", "editor", "inline_edit", "text_input"],
            cx,
        );
        draw(cx);
    }
    assert_eq!(text(&f, cx), MAIN);
    assert_eq!(
        f.editor
            .read_with(cx, |e, _| e.model().primary_selection().range),
        SELECTION
    );
    // Keys the keymap binds, and typed text, reach the input alone.
    cx.simulate_keystrokes("delete");
    cx.simulate_input("ab");
    draw(cx);
    assert_eq!(text(&f, cx), MAIN);
    assert_eq!(
        open.read_with(cx, |open, cx| open.input().read(cx).text().to_owned()),
        "ab"
    );
}

#[gpui::test]
fn replacing_the_selection_discards_the_result(cx: &mut TestAppContext) {
    let (f, cx) = setup(cx, vec![reply("```go\nx := 10\n```")], true);
    open_input(cx);
    let open = input_of(&f, cx).unwrap();
    type_text(&open, "make it ten", cx);
    submit(cx);
    edit(&f.editor, SELECTION, "z := 3", cx);
    assert!(hunks(&f, cx).is_empty());
    assert!(input_of(&f, cx).is_none());
    assert_eq!(toasts(&f, cx).len(), 1);
}

#[gpui::test]
fn the_input_stays_under_the_target_when_text_is_inserted_at_its_end(cx: &mut TestAppContext) {
    let (f, cx) = setup(cx, vec![], true);
    open_input(cx);
    let open = input_of(&f, cx).unwrap();
    edit(&f.editor, SELECTION.end..SELECTION.end, "\n// a\n// b", cx);
    assert_eq!(
        open.read_with(cx, |open, cx| open.current_range(cx)),
        Some(SELECTION)
    );
    let row = f
        .editor
        .read_with(cx, |editor, _| editor.overlay_anchor_row());
    assert_eq!(row, Some(1));
    // And a caret.
    escape(cx);
    draw(cx);
    select(&f.editor, 5..5, cx);
    open_input(cx);
    edit(&f.editor, 5..5, "a\nb\n", cx);
    let row = f
        .editor
        .read_with(cx, |editor, _| editor.overlay_anchor_row());
    assert_eq!(row, Some(0));
}

#[gpui::test]
fn typing_with_vim_on_only_types(cx: &mut TestAppContext) {
    let (f, cx) = setup(cx, vec![], true);
    cx.update(|_, cx| fe_gpui::set_vim_enabled(cx, true));
    draw(cx);
    open_input(cx);
    let open = input_of(&f, cx).unwrap();
    assert!(focused(&open, cx));
    cx.simulate_input("dd x:q");
    draw(cx);
    assert_eq!(text(&f, cx), MAIN);
    assert_eq!(
        open.read_with(cx, |open, cx| open.input().read(cx).text().to_owned()),
        "dd x:q"
    );
}

#[gpui::test]
fn vim_chords_only_type_in_the_input(cx: &mut TestAppContext) {
    let (f, cx) = setup(cx, vec![], true);
    cx.update(|_, cx| fe_gpui::set_vim_enabled(cx, true));
    draw(cx);
    open_input(cx);
    let open = input_of(&f, cx).unwrap();
    cx.simulate_keystrokes("g d g r a [ d");
    draw(cx);
    assert_eq!(text(&f, cx), MAIN);
    assert!(input_of(&f, cx).is_some());
    assert_eq!(
        open.read_with(cx, |open, cx| open.input().read(cx).text().to_owned()),
        "gdgra[d"
    );
}

#[gpui::test]
fn option_text_is_typed(cx: &mut TestAppContext) {
    let (f, cx) = setup(cx, vec![], true);
    open_input(cx);
    let open = input_of(&f, cx).unwrap();
    cx.simulate_input("™");
    draw(cx);
    assert_eq!(
        open.read_with(cx, |open, cx| open.input().read(cx).text().to_owned()),
        "™"
    );
}
