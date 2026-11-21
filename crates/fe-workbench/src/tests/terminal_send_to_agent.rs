//! Sending a failed terminal command to the agent
//! (`docs/design/design-system.md#terminal`, T1597): a block that exited
//! non-zero has a *Send to an agent* control after its prompt line, and
//! `terminal.send_block_to_agent` does the same for the block under the
//! cursor or selection. The chat gets the command line and the last 200 rows
//! of output, unsent, and the exit code in its prompt. An agent's own
//! terminal offers neither.

use std::{
    fs, thread,
    time::{Duration, Instant},
};

use fe_buffer::Buffer;
use fe_command::{ContextFrame, ContextStack, Invocation};
use fe_editor::lane::LaneId;
use fe_gpui::commands::RunCommand;
use fe_terminal_view::{TerminalOptions, TerminalView};
use gpui::{Entity, Focusable as _, TestAppContext, VisualTestContext};
use kiln_l10n::tr;

use crate::tests::open;
use crate::{Launch, TerminalPanel, Workbench};

fn opened(
    cx: &mut TestAppContext,
) -> (Entity<Workbench>, &mut VisualTestContext, tempfile::TempDir) {
    cx.executor().allow_parking();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("a.txt");
    fs::write(&path, "a\n").unwrap();
    let launch = Launch {
        root: Some(dir.path().to_owned()),
        file: Some((path, Some(Buffer::new("a\n").into()))),
    };
    let (workbench, cx, _) = open(cx, launch);
    cx.update(|window, _| window.activate_window());
    cx.update(|window, cx| window.focus(&workbench.focus_handle(cx), cx));
    draw(cx);
    (workbench, cx, dir)
}

fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.run_until_parked();
}

fn terminal(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Entity<TerminalPanel> {
    workbench.read_with(cx, |workbench, _| workbench.terminal().clone())
}

const READY: &str = "ready";
const PROMPT: &str = "\x1b]133;A\x07$ \x1b]133;B\x07";

fn command(line: &str, output: &str, end: &str) -> String {
    format!("{PROMPT}{line}\r\n\x1b]133;C\x07{output}\x1b]133;D{end}\x07")
}

fn wait_ready(view: &Entity<TerminalView>, cx: &mut VisualTestContext) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while !view.read_with(cx, |view, _| {
        view.terminal()
            .is_some_and(|terminal| terminal.snapshot().text().contains(READY))
    }) {
        assert!(
            Instant::now() < deadline,
            "timed out waiting for the session"
        );
        thread::sleep(Duration::from_millis(10));
        draw(cx);
    }
    draw(cx);
}

/// Opens a session that writes `blocks` and then a live prompt, and shows
/// the terminal.
fn session(
    workbench: &Entity<Workbench>,
    blocks: &str,
    cx: &mut VisualTestContext,
) -> Entity<TerminalView> {
    let bytes = format!("{blocks}\x1b]133;A\x07{READY} $ ");
    let panel = terminal(workbench, cx);
    cx.update(|window, cx| {
        panel.update(cx, |panel, cx| panel.open_with_session(&bytes, window, cx))
    });
    let stack = ContextStack::new().with(ContextFrame::new(crate::KEY_CONTEXT));
    let toggle = RunCommand(Invocation::of("terminal.toggle").unwrap().context(stack));
    cx.update(|window, cx| window.dispatch_action(Box::new(toggle), cx));
    draw(cx);
    let view = panel.read_with(cx, |panel, _| panel.view().unwrap().clone());
    wait_ready(&view, cx);
    view
}

fn failed(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> usize {
    let panel = terminal(workbench, cx);
    panel.read_with(cx, |panel, cx| panel.failed_blocks(cx).1.len())
}

fn control(id: u64, cx: &mut VisualTestContext) -> Option<gpui::Bounds<gpui::Pixels>> {
    cx.debug_bounds(Box::leak(
        format!("terminal-send-block-{id}").into_boxed_str(),
    ))
}

fn send_command(cx: &mut VisualTestContext) {
    let stack = ContextStack::new().with(ContextFrame::new(fe_terminal_view::KEY_CONTEXT));
    let run = RunCommand(
        Invocation::of("terminal.send_block_to_agent")
            .unwrap()
            .context(stack),
    );
    cx.update(|window, cx| window.dispatch_action(Box::new(run), cx));
    draw(cx);
}

fn chat(
    workbench: &Entity<Workbench>,
    cx: &mut VisualTestContext,
) -> Option<Entity<crate::chat_panel::ChatPanel>> {
    workbench.read_with(cx, |workbench, cx| {
        workbench.dock().read(cx).chat().cloned()
    })
}

fn attached_and_composer(
    workbench: &Entity<Workbench>,
    cx: &mut VisualTestContext,
) -> (Vec<fe_chat::ContextItem>, String) {
    let panel = chat(workbench, cx).expect("the chat opened");
    let view = panel.read_with(cx, |panel, _| panel.view().clone());
    view.read_with(cx, |view, cx| (view.attachments(), view.composer_text(cx)))
}

#[gpui::test]
#[cfg_attr(windows, ignore = "runs a /bin/sh script in the terminal")]
fn a_failed_block_offers_the_control_and_a_passed_one_does_not(cx: &mut TestAppContext) {
    let (workbench, cx, _dir) = opened(cx);
    let bytes = [
        command("true", "", ";0"),
        command("false", "oops\r\n", ";1"),
    ]
    .concat();
    let view = session(&workbench, &bytes, cx);
    let blocks: Vec<_> = view
        .read_with(cx, |view, _| view.terminal().unwrap().command_blocks())
        .into_iter()
        .filter(|block| block.exit_code.is_some())
        .collect();
    assert_eq!(blocks.len(), 2);
    assert_eq!(failed(&workbench, cx), 1);
    assert!(control(blocks[0].id, cx).is_none(), "a passed block");
    let bounds = control(blocks[1].id, cx).expect("the failed block's control");
    // Clicking it asks the agent, unsent.
    cx.simulate_click(bounds.center(), gpui::Modifiers::none());
    draw(cx);
    let (items, composer) = attached_and_composer(&workbench, cx);
    assert_eq!(items.len(), 1);
    assert_eq!(
        composer,
        tr!(
            "workbench-terminal-send-prompt",
            command = "false",
            code = 1
        )
        .to_string()
    );
}

#[gpui::test]
#[cfg_attr(windows, ignore = "runs a /bin/sh script in the terminal")]
fn the_command_and_last_200_rows_are_attached(cx: &mut TestAppContext) {
    let (workbench, cx, _dir) = opened(cx);
    let output: String = (1..=250).map(|n| format!("row{n}\r\n")).collect();
    let bytes = command("make all", &output, ";2");
    session(&workbench, &bytes, cx);
    send_command(cx);
    let (items, composer) = attached_and_composer(&workbench, cx);
    assert_eq!(
        composer,
        tr!(
            "workbench-terminal-send-prompt",
            command = "make all",
            code = 2
        )
        .to_string()
    );
    assert_eq!(items.len(), 1);
    let text = &items[0].text;
    assert!(text.starts_with("$ make all\n"), "{text}");
    assert!(!text.contains("row50\n"), "row 50 is past the last 200");
    assert!(text.contains("row51\n"), "the 200th from the end");
    assert!(text.ends_with("row250"), "{text}");
}

#[gpui::test]
#[cfg_attr(windows, ignore = "runs a /bin/sh script in the terminal")]
fn an_agents_own_terminal_does_not_offer_it(cx: &mut TestAppContext) {
    let (workbench, cx, _dir) = opened(cx);
    let bytes = format!(
        "{}\x1b]133;A\x07{READY}",
        command("false", "oops\r\n", ";1")
    );
    let options = TerminalOptions {
        shell: Some((
            "/bin/sh".to_owned(),
            vec![
                "-c".to_owned(),
                "printf '%s' \"$1\"; exec cat >/dev/null".to_owned(),
                "sh".to_owned(),
                bytes,
            ],
        )),
        ..TerminalOptions::new(std::env::temp_dir())
    };
    let user = session(&workbench, "", cx);
    let panel = terminal(&workbench, cx);
    let lane = LaneId::from_number(2).unwrap();
    cx.update(|window, cx| {
        panel.update(cx, |panel, cx| {
            panel.add_lane_session(lane, true, "agent", options, window, cx)
        })
    });
    draw(cx);
    let agent = panel.read_with(cx, |panel, _| panel.view().unwrap().clone());
    assert_ne!(agent, user);
    wait_ready(&agent, cx);
    let id = agent.read_with(cx, |view, _| {
        view.terminal().unwrap().command_blocks()[0].id
    });
    assert_eq!(failed(&workbench, cx), 0);
    assert!(control(id, cx).is_none());
    send_command(cx);
    // The chat may exist; nothing was put in it.
    if chat(&workbench, cx).is_some() {
        let (items, composer) = attached_and_composer(&workbench, cx);
        assert!(items.is_empty() && composer.is_empty(), "nothing was asked");
    }
}
