// SPDX-License-Identifier: LicenseRef-Proprietary

//! `#[kiln::command]` on a fn: one declaration gives a command its type,
//! ID, title, default key, menu item and handler, which `kiln::app` adds
//! from this integration test's own target with no keymap or menus of the
//! app's; `id` and `context` override the ID and context; contributed
//! menu items go by order, then by file and line, whatever added them; and
//! a handler sees its invocation and window, the invocation observers
//! hearing of each run once.

use std::{cell::RefCell, rc::Rc};

use gpui::{
    Action as _, App, AppContext as _, Context, Entity, FocusHandle, Focusable, Global,
    InteractiveElement as _, IntoElement, Render, SharedString, Styled as _, Task, TestAppContext,
    Window, div,
};
use kiln::{
    Command as _, CommandSpec, Contributions, Cx, MenuSpec,
    command::{CommandId, ContextFrame, ContextPredicate, ContextStack, Invocation, Source, Value},
    current_invocation,
    platform::{
        command_actions::{dispatch_window, typed_command},
        commands::observe_invocations,
        menus,
    },
    prelude::*,
    shell::{
        PaneItem, PaneView as _, Workspace,
        command_palette::{COMMAND_PALETTE, commands_in},
    },
    testing::TestApp,
};

/// New Note
///
/// Opens an empty note in the active pane.
#[kiln::command(key = "secondary-n", menu = "File")]
fn new_note(workspace: &mut Workspace, cx: &mut Cx<Workspace>) {
    let note = cx.new(|cx| Note(cx.focus_handle()));
    workspace.add_item(Box::new(note), cx.window, cx.cx);
    cx.default_global::<Made>().0 += 1;
    let source = current_invocation(cx).map(|invocation| invocation.source);
    let here = dispatch_window(cx) == Some(cx.window.window_handle());
    log(
        cx,
        format!("new_note from {source:?}, in its window: {here}"),
    );
}

/// Save All
#[kiln::command(
    id = "workbench.save_all",
    context = "workspace",
    key = "secondary-alt-s",
    menu = "File"
)]
fn save_everything(_: &mut Workspace, cx: &mut Cx<Workspace>) {
    cx.default_global::<Saved>().0 += 1;
}

/// Close Note
#[kiln::command(menu = "File")]
pub(crate) fn close_note(_: &mut Workspace, cx: &mut Cx<Workspace>) {
    log(cx, "close_note".to_owned());
}

/// Twice
///
/// Runs `new_note` within it, then logs the command it sees as its own.
#[kiln::command]
fn twice(_: &mut Workspace, cx: &mut Cx<Workspace>) {
    let nested = Invocation::new(NewNote::id()).source(Source::Script);
    cx.window.run_command(nested, cx.cx).unwrap();
    let own = current_invocation(cx).map(|invocation| invocation.command.to_string());
    log(cx, format!("twice still sees {own:?}"));
}

/// Print
///
/// Declared last, but first in its menu by its order.
#[kiln::command(menu = "File", order = -1)]
fn print(_: &mut Workspace, _: &mut Cx<Workspace>) {}

/// What the handlers and notes did, in order.
#[derive(Default)]
struct Log(Vec<String>);

impl Global for Log {}

fn log(cx: &mut App, line: String) {
    cx.default_global::<Log>().0.push(line);
}

/// The notes `new_note` made.
#[derive(Default)]
struct Made(usize);

impl Global for Made {}

/// The times `save_everything` ran.
#[derive(Default)]
struct Saved(usize);

impl Global for Saved {}

struct Note(FocusHandle);

impl Render for Note {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        // A view on the focus path that handles `NewNote` too, then lets
        // it go on, and one that handles `CloseNote` itself.
        div()
            .track_focus(&self.0)
            .size_full()
            .on_action(|_: &NewNote, _, cx| {
                log(cx, "the note saw new_note".to_owned());
                cx.propagate();
            })
            .on_action(|_: &CloseNote, _, cx| log(cx, "the note closed".to_owned()))
    }
}

impl Focusable for Note {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.0.clone()
    }
}

impl PaneItem for Note {
    const KIND: &'static str = "test.macro_note";

    fn tab_title(&self, _: &App) -> SharedString {
        "Note".into()
    }

    fn restore(_: Value, _: &mut Window, cx: &mut App) -> Task<Option<Entity<Self>>> {
        Task::ready(Some(cx.new(|cx| Note(cx.focus_handle()))))
    }
}

/// An app with no keymap or menus of its own, its window a workspace
/// that opens notes.
fn open(cx: &mut TestAppContext) -> TestApp {
    TestApp::open(
        kiln::app("dev.kiln.MacroCommand", "Kiln Macro Command")
            .restore_layout(false)
            .item::<Note>(),
        cx,
    )
}

/// The log since last taken.
fn take_log(app: &mut TestApp) -> Vec<String> {
    app.cx()
        .update(|_, cx| std::mem::take(&mut cx.default_global::<Log>().0))
}

/// The IDs of the invocations observed from now on.
fn observe(app: &mut TestApp) -> Rc<RefCell<Vec<String>>> {
    let observed = Rc::new(RefCell::new(Vec::new()));
    let record = observed.clone();
    app.cx().update(|_, cx| {
        observe_invocations(cx, move |invocation| {
            record.borrow_mut().push(invocation.command.to_string());
        });
    });
    observed
}

fn made(app: &mut TestApp) -> usize {
    app.cx()
        .read(|cx| cx.try_global::<Made>().map_or(0, |made| made.0))
}

fn saved(app: &mut TestApp) -> usize {
    app.cx()
        .read(|cx| cx.try_global::<Saved>().map_or(0, |saved| saved.0))
}

/// The items in the workspace's active pane.
fn notes(app: &mut TestApp) -> usize {
    let workspace = app.workspace();
    app.cx().read(|cx| {
        let pane = workspace.read(cx).active_pane().clone();
        pane.read(cx).pane().items().len()
    })
}

/// The File menu's items, by title.
fn file_menu(app: &mut TestApp) -> Vec<String> {
    app.cx().read(|cx| {
        let bar = menus::installed(cx).expect("menus installed");
        let file = bar.menu("File").expect("a File menu");
        (file.items.iter())
            .filter_map(|item| item.command().map(|command| command.title.clone()))
            .collect()
    })
}

#[gpui::test]
fn the_fn_makes_a_typed_command_titled_by_its_doc_line(cx: &mut TestAppContext) {
    let mut app = open(cx);
    assert_eq!(NewNote::id().as_str(), "macro_command.new_note");
    assert_eq!(NewNote.name(), "macro_command::NewNote");
    assert_eq!(
        NewNote::documentation(),
        Some("New Note\n\nOpens an empty note in the active pane.")
    );
    let typed = app
        .cx()
        .read(|cx| typed_command(&NewNote::id(), cx))
        .expect("registered");
    assert_eq!(typed.title, "New Note");
    assert_eq!(typed.context, None);
    let spec = app.cx().read(|cx| {
        Contributions::commands(cx)
            .iter()
            .find(|spec| spec.id == NewNote::id())
            .cloned()
            .expect("contributed")
    });
    assert_eq!(spec.keys, "secondary-n".into());
    assert_eq!(spec.menu.map(|menu| menu.menu), Some("File".into()));
}

#[gpui::test]
fn the_command_runs_from_its_key_the_palette_and_its_menu_item(cx: &mut TestAppContext) {
    let mut app = open(cx);
    assert_eq!(notes(&mut app), 0);

    app.press("secondary-n");
    assert_eq!(made(&mut app), 1);
    assert_eq!(notes(&mut app), 1);
    assert!(app.active_item::<Note>().is_some());

    app.press("secondary-shift-p");
    let shell = app
        .workspace()
        .read_with(app.cx(), |w, _| w.shell().clone());
    let modal = shell.read_with(app.cx(), |shell, cx| shell.modal(cx));
    assert_eq!(modal, Some(COMMAND_PALETTE));
    app.cx().simulate_input("new note");
    app.press("enter");
    assert_eq!(shell.read_with(app.cx(), |shell, cx| shell.modal(cx)), None);
    assert_eq!(made(&mut app), 2);
    assert_eq!(notes(&mut app), 2);

    app.menu("File", "New Note");
    assert_eq!(made(&mut app), 3);
    assert_eq!(notes(&mut app), 3);

    app.run_command("macro_command.new_note");
    assert_eq!(made(&mut app), 4);
}

#[gpui::test]
fn id_and_context_override_the_commands(cx: &mut TestAppContext) {
    let mut app = open(cx);
    let id = CommandId::parse("workbench.save_all").unwrap();
    assert_eq!(SaveEverything::id(), id);
    assert_eq!(SaveEverything.name(), "workbench::SaveAll");
    let typed = app
        .cx()
        .read(|cx| typed_command(&id, cx))
        .expect("registered");
    assert_eq!(typed.title, "Save All");
    assert_eq!(
        typed.context,
        Some(ContextPredicate::parse("workspace").unwrap())
    );

    // Listed only where its context holds.
    let listed = |stack: ContextStack, app: &mut TestApp| {
        app.cx()
            .read(|cx| commands_in(&stack, cx).iter().any(|entry| entry.id == id))
    };
    assert!(!listed(ContextStack::new(), &mut app));
    let workspace: ContextStack = [ContextFrame::new("workspace")].into_iter().collect();
    assert!(listed(workspace, &mut app));

    // Its key holds in the workspace, where focus is; so does its ID.
    app.press("secondary-alt-s");
    assert_eq!(saved(&mut app), 1);
    app.run_command("workbench.save_all");
    assert_eq!(saved(&mut app), 2);
    app.menu("File", "Save All");
    assert_eq!(saved(&mut app), 3);
}

#[gpui::test]
fn menu_items_go_by_order_then_file_and_line(cx: &mut TestAppContext) {
    let mut app = open(cx);
    assert_eq!(
        file_menu(&mut app),
        ["Print", "New Note", "Save All", "Close Note"]
    );
}

/// Commands added by hand, as a module would.
mod other {
    kiln::commands!(
        other,
        [
            /// Alpha
            Alpha,
            /// Zeta
            Zeta,
        ]
    );
}

/// Two menu items added by hand after the app's own, as modules would,
/// the one declared later first.
fn declared_elsewhere(cx: &mut App) {
    let item = |file| MenuSpec {
        menu: "File".into(),
        order: 0,
        declared: Some((file, 1)),
    };
    Contributions::add_commands(
        cx,
        [
            CommandSpec {
                menu: Some(item("z.rs")),
                ..CommandSpec::new::<other::Zeta>("Zeta")
            },
            CommandSpec {
                menu: Some(item("a.rs")),
                ..CommandSpec::new::<other::Alpha>("Alpha")
            },
        ],
    );
}

#[gpui::test]
fn menu_items_go_by_where_declared_whatever_added_them(cx: &mut TestAppContext) {
    let app = kiln::app("dev.kiln.MacroCommand", "Kiln Macro Command")
        .restore_layout(false)
        .item::<Note>()
        .contributions(declared_elsewhere);
    let mut app = TestApp::open(app, cx);
    let items: Vec<String> = file_menu(&mut app);
    assert_eq!(
        items,
        [
            "Print",
            "Alpha",
            "New Note",
            "Save All",
            "Close Note",
            "Zeta"
        ]
    );
}

#[gpui::test]
fn a_handler_sees_its_invocation_and_window(cx: &mut TestAppContext) {
    let mut app = open(cx);
    app.press("secondary-n");
    app.press("secondary-shift-p");
    app.cx().simulate_input("new note");
    app.press("enter");
    app.menu("File", "New Note");
    let ran: Vec<String> = (take_log(&mut app).into_iter())
        .filter(|line| line.starts_with("new_note"))
        .collect();
    assert_eq!(
        ran,
        [
            "new_note from Some(Keyboard), in its window: true",
            "new_note from Some(Palette), in its window: true",
            "new_note from Some(Menu), in its window: true",
        ]
    );

    // A command dispatched within the handler sees its own invocation, and
    // the handler its own again after.
    app.run_command("macro_command.twice");
    assert_eq!(
        take_log(&mut app),
        [
            "the note saw new_note",
            "twice still sees Some(\"macro_command.twice\")",
            "new_note from Some(Script), in its window: true",
        ]
    );
}

#[gpui::test]
fn observers_hear_of_each_run_once(cx: &mut TestAppContext) {
    let mut app = open(cx);
    let observed = observe(&mut app);

    // Handled by the command's handler alone: no view has it.
    app.press("secondary-n");
    assert_eq!(*observed.borrow(), ["macro_command.new_note"]);
    // Handled by the note too, which lets it go on to the handler.
    app.press("secondary-n");
    assert_eq!(made(&mut app), 2);
    assert_eq!(observed.borrow().len(), 2, "{:?}", observed.borrow());
    // Handled by the note alone, which keeps it from the handler.
    app.menu("File", "Close Note");
    assert_eq!(
        *observed.borrow(),
        [
            "macro_command.new_note",
            "macro_command.new_note",
            "macro_command.close_note"
        ]
    );
    assert_eq!(
        take_log(&mut app)
            .into_iter()
            .filter(|line| !line.starts_with("new_note"))
            .collect::<Vec<_>>(),
        ["the note saw new_note", "the note closed"]
    );
}
