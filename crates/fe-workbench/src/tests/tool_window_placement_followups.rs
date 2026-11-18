//! What T1935's review found left over after tool windows moved between
//! the bottom panel and the sidebar: every caller that showed, hid or
//! looked at a panel finds it in the dock that holds it; the tool windows
//! made on first use come back from a saved layout in their saved place;
//! a moved tool window ends the window keys' way back; and the foot's
//! titles are read as drawn.

use std::{fs, path::Path, process::Command, rc::Rc, time::Duration};

use fe_buffer::Buffer;
use fe_command::{ContextFrame, ContextStack, Invocation, Value};
use fe_gpui::commands::RunCommand;
use gpui::{
    AppContext as _, Entity, FocusHandle, Focusable as _, Modifiers, TestAppContext,
    VisualTestContext,
};
use kiln_shell::{DockView as _, PanelId};

use super::open;
use crate::{Dock, DockPanel, Launch, Workbench, dock::Place};

const GIT_LOG: PanelId = PanelId("git_log");
const TERMINAL: PanelId = PanelId("terminal");
const PROBLEMS: PanelId = PanelId("problems");
const DEBUG: PanelId = PanelId(crate::debug_panel::PANEL);

fn opened(
    cx: &mut TestAppContext,
) -> (Entity<Workbench>, &mut VisualTestContext, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("a.rs");
    fs::write(&path, "fn a() {}\n").unwrap();
    fs::write(dir.path().join("b.txt"), "b\n").unwrap();
    let settings = fe_config::project_settings_path(dir.path());
    fs::create_dir_all(settings.parent().unwrap()).unwrap();
    fs::write(settings, "[lsp.servers.rust-analyzer]\nenabled = false\n").unwrap();
    let launch = Launch {
        root: Some(dir.path().to_owned()),
        file: Some((path, Some(Buffer::new("fn a() {}\n").into()))),
    };
    let (workbench, cx, _) = open(cx, launch);
    let terminal = workbench.read_with(cx, |workbench, _| workbench.terminal().clone());
    terminal.update(cx, |terminal, _| terminal.fake_sessions = true);
    cx.update(|window, _| window.activate_window());
    cx.update(|window, cx| window.focus(&workbench.focus_handle(cx), cx));
    draw(cx);
    (workbench, cx, dir)
}

fn draw(cx: &mut VisualTestContext) {
    for _ in 0..4 {
        cx.run_until_parked();
        cx.update(|window, cx| window.draw(cx).clear(cx));
        if cx.update(|window, cx| window.simulate_next_frame(cx)) == 0 {
            break;
        }
    }
    cx.run_until_parked();
}

fn sidebar(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Entity<Dock> {
    workbench.read_with(cx, |workbench, _| workbench.dock().clone())
}

fn run(id: &str, cx: &mut VisualTestContext) {
    let stack = ContextStack::new().with(ContextFrame::new(crate::KEY_CONTEXT));
    let run = RunCommand(Invocation::of(id).unwrap().context(stack));
    cx.update(|window, cx| window.dispatch_action(Box::new(run), cx));
    draw(cx);
}

fn focused_in(focus: &FocusHandle, cx: &mut VisualTestContext) -> bool {
    cx.update(|window, cx| focus.contains_focused(window, cx))
}

fn editor_focus(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> FocusHandle {
    workbench.read_with(cx, |workbench, cx| workbench.focus_handle(cx))
}

/// Moves the tool window `panel` to `to`, shown and focused there.
fn move_to(workbench: &Entity<Workbench>, panel: PanelId, to: Place, cx: &mut VisualTestContext) {
    let moved = workbench.update_in(cx, |workbench, window, cx| {
        workbench.move_tool_window(panel, to, None, window, cx)
    });
    assert!(moved);
    draw(cx);
}

fn shown(workbench: &Entity<Workbench>, panel: PanelId, cx: &mut VisualTestContext) -> bool {
    workbench.read_with(cx, |workbench, cx| workbench.tool_shown(panel, cx))
}

fn place(workbench: &Entity<Workbench>, panel: PanelId, cx: &mut VisualTestContext) -> Place {
    workbench.read_with(cx, |workbench, cx| workbench.place_of(panel, cx))
}

/// Hides both docks.
fn hide_docks(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) {
    let (sidebar, bottom) = (
        sidebar(workbench, cx),
        workbench.read_with(cx, |workbench, _| workbench.terminal_dock().clone()),
    );
    sidebar.update(cx, |dock, cx| {
        dock.dock_mut().set_visible(false);
        cx.notify();
    });
    bottom.update(cx, |dock, cx| {
        dock.dock_mut().set_visible(false);
        cx.notify();
    });
    draw(cx);
}

fn bottom_visible(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> bool {
    workbench.read_with(cx, |workbench, cx| {
        workbench.terminal_dock().read(cx).dock().is_visible()
    })
}

/// Makes the Call Hierarchy panel, as its first use does.
fn made_hierarchy(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) {
    let weak = workbench.downgrade();
    let panel =
        cx.update(|_, cx| cx.new(|cx| crate::hierarchy_panel::HierarchyPanel::new(weak, cx)));
    sidebar(workbench, cx).update(cx, |dock, cx| dock.set_hierarchy(panel, cx));
    draw(cx);
}

/// The tooltip of the rail's foot button `id`, hovered.
fn tooltip(
    workbench: &Entity<Workbench>,
    id: PanelId,
    cx: &mut VisualTestContext,
) -> Option<String> {
    let dock = sidebar(workbench, cx);
    let away = workbench.read_with(cx, |workbench, cx| {
        workbench.pane().read(cx).bounds().center()
    });
    cx.simulate_mouse_move(away, None, Modifiers::none());
    draw(cx);
    let button = dock.read_with(cx, |dock, _| dock.state().drawn().rail_button_bounds(id))?;
    cx.simulate_mouse_move(button.center(), None, Modifiers::none());
    cx.executor().advance_clock(Duration::from_millis(600));
    draw(cx);
    dock.read_with(cx, |dock, _| dock.state().drawn().last_tooltip())
}

fn git(dir: &Path, args: &[&str]) {
    let output = Command::new("git")
        .args([
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@t",
            "-c",
            "commit.gpgsign=false",
            "-c",
            "init.defaultBranch=main",
        ])
        .args(args)
        .current_dir(dir)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .output()
        .expect("run git");
    assert!(output.status.success(), "git {args:?}");
}

/// The project's repository, `a.rs` committed.
fn repository(dir: &tempfile::TempDir) {
    git(dir.path(), &["init", "-q"]);
    git(dir.path(), &["add", "."]);
    git(dir.path(), &["commit", "-q", "-m", "init"]);
}

#[gpui::test]
fn references_results_reveal_where_references_is_placed(cx: &mut TestAppContext) {
    a_references_lens_reveals_it_in_the_bottom_panel(&mut cx.clone());
    let (workbench, cx, dir) = opened(cx);
    let references = DockPanel::References.id();
    move_to(&workbench, references, Place::Bottom, cx);
    assert!(shown(&workbench, references, cx));

    // Find Usages reveals it in the bottom panel, not the sidebar.
    hide_docks(&workbench, cx);
    assert!(!shown(&workbench, references, cx));
    workbench.update_in(cx, |workbench, window, cx| {
        workbench.find_references(window, cx)
    });
    draw(cx);
    assert!(shown(&workbench, references, cx));
    assert!(bottom_visible(&workbench, cx));
    let panel =
        sidebar(&workbench, cx).read_with(cx, |dock, cx| dock.references().focus_handle(cx));
    assert!(focused_in(&panel, cx));
    assert_ne!(
        sidebar(&workbench, cx).read_with(cx, |dock, _| dock.state().rail_highlighted()),
        Some(references)
    );

    // The show-op path reveals it there too.
    hide_docks(&workbench, cx);
    workbench.update_in(cx, |workbench, window, cx| {
        workbench.run_dock_op(crate::dock::DockOp::ShowReferences, window, cx)
    });
    draw(cx);
    assert!(shown(&workbench, references, cx));
    assert!(bottom_visible(&workbench, cx));

    // History, shown where it is placed, as a file's history is.
    repository(&dir);
    run("git.file_history", cx);
    let history = DockPanel::History.id();
    assert!(shown(&workbench, history, cx));
    move_to(&workbench, history, Place::Bottom, cx);
    hide_docks(&workbench, cx);
    run("git.file_history", cx);
    assert!(shown(&workbench, history, cx));
    assert!(bottom_visible(&workbench, cx));
}

#[gpui::test]
fn moved_panels_keep_their_callers_working(cx: &mut TestAppContext) {
    let (workbench, cx, dir) = opened(cx);

    // A Chat moved to the bottom refreshes its editor context.
    let chat = DockPanel::Chat.id();
    move_to(&workbench, chat, Place::Bottom, cx);
    assert!(shown(&workbench, chat, cx));
    workbench.update(cx, |workbench, cx| workbench.set_root_trust(true, cx));
    draw(cx);
    let context = |workbench: &Entity<Workbench>, cx: &mut VisualTestContext| {
        let panel = workbench.read_with(cx, |workbench, cx| {
            workbench.dock().read(cx).chat().cloned().unwrap()
        });
        cx.update(|_, cx| format!("{:?}", panel.read(cx).view().read(cx).context()))
    };
    let pane = workbench.read_with(cx, |workbench, _| workbench.pane().clone());
    let b = dir.path().join("b.txt");
    cx.update(|window, cx| pane.update(cx, |pane, cx| pane.open(b, window, cx)));
    draw(cx);
    assert!(
        context(&workbench, cx).contains("b.txt"),
        "{}",
        context(&workbench, cx)
    );
    hide_docks(&workbench, cx);

    // Quickfix, moved either way, closes and toggles.
    for to in [Place::Bottom, Place::Sidebar] {
        workbench.update_in(cx, |workbench, window, cx| {
            workbench.show_quickfix_panel(false, None, window, cx)
        });
        draw(cx);
        let quickfix = DockPanel::Quickfix.id();
        if place(&workbench, quickfix, cx) != to {
            move_to(&workbench, quickfix, to, cx);
        }
        assert!(shown(&workbench, quickfix, cx));
        workbench.update_in(cx, |workbench, window, cx| {
            workbench.close_quickfix_panel(false, window, cx)
        });
        draw(cx);
        assert!(!shown(&workbench, quickfix, cx), "{to:?} closes");
        workbench.update_in(cx, |workbench, window, cx| {
            workbench.toggle_quickfix_panel(false, window, cx)
        });
        draw(cx);
        assert!(shown(&workbench, quickfix, cx), "{to:?} opens");
        workbench.update_in(cx, |workbench, window, cx| {
            workbench.toggle_quickfix_panel(false, window, cx)
        });
        draw(cx);
        assert!(!shown(&workbench, quickfix, cx), "{to:?} toggles shut");
    }

    // The History and Git log quit handlers hide the dock holding them.
    repository(&dir);
    cx.update(|_, cx| fe_gpui::set_vim_enabled(cx, true));
    run("git.file_history", cx);
    let history = DockPanel::History.id();
    move_to(&workbench, history, Place::Bottom, cx);
    assert!(shown(&workbench, history, cx));
    cx.simulate_keystrokes("q");
    draw(cx);
    assert!(!shown(&workbench, history, cx));
    assert!(!bottom_visible(&workbench, cx));
    run("git.log", cx);
    move_to(&workbench, GIT_LOG, Place::Sidebar, cx);
    assert!(shown(&workbench, GIT_LOG, cx));
    cx.simulate_keystrokes("q");
    draw(cx);
    assert!(!shown(&workbench, GIT_LOG, cx));
    assert!(!sidebar(&workbench, cx).read_with(cx, |dock, _| dock.is_visible()));

    // Show Debugger with Debug shown in the sidebar leaves it.
    run("debug.show_panels", cx);
    move_to(&workbench, DEBUG, Place::Sidebar, cx);
    assert!(shown(&workbench, DEBUG, cx));
    cx.update(|window, cx| {
        let choose = Rc::new(|_: &mut gpui::Window, _: &mut gpui::App| {});
        crate::run_widget_debug::show_session_menu(&workbench, choose, window, cx);
    });
    draw(cx);
    cx.simulate_keystrokes("enter");
    draw(cx);
    assert!(shown(&workbench, DEBUG, cx));
}

#[gpui::test]
fn lazy_panels_restore_into_their_saved_dock(cx: &mut TestAppContext) {
    let mut app = cx.clone();
    let (first, cx, dir) = opened(cx);
    repository(&dir);
    let lazy = [
        DockPanel::History,
        DockPanel::CallHierarchy,
        DockPanel::Quickfix,
        DockPanel::AiChanges,
    ];
    // All four made, then placed in the bottom panel: History and Call
    // Hierarchy together, Quickfix alone, AI Changes active in a group of
    // its own.
    run("git.file_history", cx);
    first.update_in(cx, |workbench, window, cx| {
        workbench.show_quickfix_panel(false, None, window, cx);
        workbench.ai_changes_panel(cx);
        let _ = window;
    });
    made_hierarchy(&first, cx);
    draw(cx);
    for panel in lazy {
        move_to(&first, panel.id(), Place::Bottom, cx);
    }
    run("dock.split_panel_right", cx);
    let entry = first.read_with(cx, |workbench, cx| workbench.layout_entry(cx));
    let groups_of = |workbench: &Entity<Workbench>, cx: &mut VisualTestContext| {
        workbench.read_with(cx, |workbench, cx| {
            let dock = workbench.terminal_dock().read(cx).dock();
            lazy.map(|panel| dock.group_of(panel.id()))
        })
    };
    let saved = groups_of(&first, cx);

    // A fresh workbench, none of them made.
    let (second, cx, _dir) = opened(&mut app);
    assert!(second.read_with(cx, |workbench, cx| {
        workbench.dock().read(cx).history().is_none()
    }));
    second.update_in(cx, |workbench, window, cx| {
        workbench.restore_layout(&entry, window, cx)
    });
    draw(cx);
    for panel in lazy {
        assert_eq!(place(&second, panel.id(), cx), Place::Bottom, "{panel:?}");
        let foot = sidebar(&second, cx).read_with(cx, |dock, _| dock.state().action_ids());
        assert!(foot.contains(&panel.id().0), "{panel:?} {foot:?}");
        let tip = tooltip(&second, panel.id(), cx);
        assert!(tip.is_some_and(|tip| !tip.is_empty()), "{panel:?}");
    }
    // Made on first use, each joins its saved group.
    run("git.file_history", cx);
    second.update_in(cx, |workbench, window, cx| {
        workbench.show_quickfix_panel(false, None, window, cx);
        workbench.ai_changes_panel(cx);
        let _ = window;
    });
    made_hierarchy(&second, cx);
    draw(cx);
    for panel in lazy {
        second.update_in(cx, |workbench, window, cx| {
            workbench.show_bottom_panel(panel.id(), window, cx)
        });
    }
    draw(cx);
    assert_eq!(groups_of(&second, cx), saved);

    // A layout restored after it, with no groups, owes nothing to the
    // first's: a tool window made then takes the focused group.
    let (third, cx, _dir) = opened(&mut app);
    let default = third.read_with(cx, |workbench, cx| workbench.layout_entry(cx));
    third.update_in(cx, |workbench, window, cx| {
        workbench.restore_layout(&entry, window, cx)
    });
    third.update_in(cx, |workbench, window, cx| {
        workbench.restore_layout(&default, window, cx)
    });
    draw(cx);
    let panel = DockPanel::AiChanges.id();
    third.update_in(cx, |workbench, window, cx| {
        workbench.ai_changes_panel(cx);
        workbench.show_bottom_panel(panel, window, cx);
        let _ = window;
    });
    draw(cx);
    assert_eq!(place(&third, panel, cx), Place::Sidebar);
    move_to(&third, panel, Place::Bottom, cx);
    assert_eq!(
        third.read_with(cx, |workbench, cx| {
            workbench.terminal_dock().read(cx).dock().group_of(panel)
        }),
        Some(0)
    );
    assert_eq!(
        third.read_with(cx, |workbench, cx| {
            workbench.terminal_dock().read(cx).dock().group_count()
        }),
        1
    );
    let _ = (Value::Null, PROBLEMS, TERMINAL);
}

#[gpui::test]
fn relocating_a_focused_tool_clears_the_window_key_return(cx: &mut TestAppContext) {
    let (workbench, cx, _dir) = opened(cx);
    // Problems shown in the bottom panel, entered from the sidebar by a
    // window key.
    run("problems.show", cx);
    run("dock.show_project", cx);
    let project = workbench.read_with(cx, |workbench, cx| workbench.panel().focus_handle(cx));
    assert!(focused_in(&project, cx));
    run("dock.focus_down", cx);
    let problems =
        sidebar(&workbench, cx).read_with(cx, |dock, cx| dock.problems().focus_handle(cx));
    assert!(focused_in(&problems, cx));

    // Moved to the sidebar, then the terminal shown: up from it is the
    // editor, not the sidebar.
    move_to(&workbench, PROBLEMS, Place::Sidebar, cx);
    run("terminal.toggle", cx);
    let terminal = workbench.read_with(cx, |workbench, cx| workbench.terminal().focus_handle(cx));
    assert!(focused_in(&terminal, cx));
    run("dock.focus_up", cx);
    assert!(focused_in(&editor_focus(&workbench, cx), cx));
    assert!(!focused_in(&problems, cx));
    let _ = TERMINAL;
}

#[gpui::test]
fn foot_actions_read_titles_when_drawn(cx: &mut TestAppContext) {
    let (workbench, cx, dir) = opened(cx);
    repository(&dir);
    run("git.file_history", cx);
    let history = DockPanel::History.id();
    move_to(&workbench, history, Place::Bottom, cx);
    let title =
        |workbench: &Entity<Workbench>, cx: &mut VisualTestContext| tooltip(workbench, history, cx);
    let before = title(&workbench, cx).expect("a foot action");
    // The panel's title changes with the file it lists.
    let b = dir.path().join("b.txt");
    let pane = workbench.read_with(cx, |workbench, _| workbench.pane().clone());
    cx.update(|window, cx| pane.update(cx, |pane, cx| pane.open(b, window, cx)));
    draw(cx);
    run("git.file_history", cx);
    let after = title(&workbench, cx).unwrap();
    let panel_title = sidebar(&workbench, cx)
        .read_with(cx, |dock, cx| dock.history().unwrap().read(cx).title(cx));
    assert_ne!(before, after);
    assert!(
        after.contains(panel_title.as_ref()),
        "{after} {panel_title}"
    );
}

/// Show References, as a code lens's click runs it, with References in the
/// bottom panel and both docks hidden.
fn a_references_lens_reveals_it_in_the_bottom_panel(cx: &mut TestAppContext) {
    use crate::tests::lsp::Lsp;
    use fe_lsp::fake::{InitializeReply, Reply, Script, Scripted, initialize_result};
    use lsp_types::{CodeLensOptions, PositionEncodingKind, ServerCapabilities};
    use serde_json::json;

    let mut init = initialize_result(Some(PositionEncodingKind::UTF16));
    init.capabilities = ServerCapabilities {
        code_lens_provider: Some(CodeLensOptions {
            resolve_provider: Some(true),
        }),
        ..init.capabilities
    };
    let script = Script {
        initialize: InitializeReply::Result(Box::new(init)),
        ..Script::default()
    };
    let mut s = Lsp::with_fake_clock(cx, script, Script::default());
    s.set_trust(true);
    fs::write(
        s.path("src/z.rs"),
        "fn a() {}\nfn b() {}\nfn c() {}\nfn d() {}\n",
    )
    .unwrap();
    let main = s.uri("src/z.rs");
    let at = |line: u32, from: u32, to: u32| {
        json!({ "uri": main.to_string(), "range": {
            "start": { "line": line, "character": from },
            "end": { "line": line, "character": to } } })
    };
    s.fake.set_reply(
        "textDocument/codeLens",
        Scripted::now(Reply::Result(json!([{
            "range": { "start": { "line": 3, "character": 3 }, "end": { "line": 3, "character": 4 } },
            "command": {
                "title": "1 reference",
                "command": "editor.action.showReferences",
                "arguments": [main.to_string(), { "line": 3, "character": 3 }, [at(3, 3, 4)]],
            },
        }]))),
    );
    s.open("src/z.rs");
    s.fake
        .wait_for_document(&main, "didOpen", |r| r.is_some_and(|r| r.open));
    let editor = (s.workbench)
        .read_with(&s.cx, |w, cx| w.active_editor(cx).cloned())
        .expect("z.rs is shown");
    s.cx.update(|window, cx| window.focus(&editor.focus_handle(cx), cx));
    s.draw();

    let references = DockPanel::References.id();
    let workbench = s.workbench.clone();
    move_to(&workbench, references, Place::Bottom, &mut s.cx);
    hide_docks(&workbench, &mut s.cx);
    let lens = |s: &Lsp| {
        editor.read_with(&s.cx, |view, _| {
            let snapshot = view.model().snapshot().clone();
            (view.lens_providers().iter())
                .flat_map(|provider| provider.borrow_mut().lenses(&snapshot))
                .find_map(|lens| lens.items.first().and_then(|item| item.invocation.clone()))
        })
    };
    s.until("the lens", |s| {
        s.clock.advance(Duration::from_millis(20));
        lens(s).is_some()
    });
    let context = ContextStack::new()
        .with(ContextFrame::new(crate::KEY_CONTEXT))
        .with(ContextFrame::new(fe_editor::commands::CONTEXT));
    let run = RunCommand(
        lens(&s)
            .unwrap()
            .context(context)
            .source(fe_command::Source::Mouse),
    );
    s.cx.update(|window, cx| window.dispatch_action(Box::new(run), cx));
    s.draw();
    assert!(shown(&workbench, references, &mut s.cx));
    assert!(bottom_visible(&workbench, &mut s.cx));
    let panel = sidebar(&workbench, &mut s.cx)
        .read_with(&s.cx, |dock, cx| dock.references().focus_handle(cx));
    s.draw();
    assert!(focused_in(&panel, &mut s.cx));
    let sidebar_lit =
        sidebar(&workbench, &mut s.cx).read_with(&s.cx, |dock, _| dock.state().rail_highlighted());
    assert_ne!(sidebar_lit, Some(references));
}
