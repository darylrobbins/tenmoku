// SPDX-License-Identifier: LicenseRef-Proprietary

//! `#[kiln::command(context = "..")]` on a view's impl: each marked
//! method is a command, its ID `<crate>.<method>` unless `id`, its context
//! the impl's unless `context`, which runs from its key and menu item, and
//! which the palette lists, only while focus is inside the view; the view's
//! `ViewCommands::attach` sets its key context, whether a handwritten
//! `Render` calls it or `kiln::__private::attach!` does as the view macros
//! will, and leaves a view without commands as it is.

use gpui::{
    Action as _, App, AppContext as _, Context, Entity, FocusHandle, Focusable, Global,
    InteractiveElement as _, IntoElement, ParentElement as _, Render, SharedString, Styled as _,
    Task, TestAppContext, Window, div,
};
use kiln::{
    Command as _, Contributions, Cx, ViewCommands,
    command::{CommandId, ContextFrame, ContextPredicate, ContextStack, Value},
    platform::{command_actions::typed_command, keys, menus},
    shell::{PaneItem, command_palette::commands_in},
    testing::TestApp,
};

/// A note, hosted as the view macros will host it:
/// `kiln::__private::attach!` attaches its commands.
struct NoteView {
    focus: FocusHandle,
    title: String,
}

#[kiln::command(context = "note")]
impl NoteView {
    /// Rename Note
    ///
    /// Renames the focused note.
    #[command(key = "f2", menu = "Edit")]
    fn rename(&mut self, cx: &mut Cx<Self>) {
        self.title = format!("{} (renamed)", self.title);
        log(cx, format!("rename {}", self.title));
        cx.notify();
    }

    /// Pin Note
    #[command(id = "notes.pin", context = "workspace", menu = "Edit")]
    pub(crate) fn pin_it(&mut self, cx: &mut Cx<Self>) {
        log(cx, format!("pin {}", self.title));
    }

    /// Unmarked: no command of its own.
    fn shout(&mut self, cx: &mut Cx<Self>) {
        log(cx, format!("shout {}", self.title));
    }
}

impl Render for NoteView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let element = div()
            .track_focus(&self.focus)
            .size_full()
            .child(self.title.clone());
        kiln::__private::attach!(NoteView, element, cx)
    }
}

/// A sketch, whose handwritten `Render` attaches its commands itself.
struct SketchView {
    focus: FocusHandle,
}

#[kiln::command(context = "sketch")]
impl SketchView {
    /// Clear Sketch
    #[command(key = "f4", menu = "Edit")]
    fn clear(&mut self, cx: &mut Cx<Self>) {
        log(cx, "clear".to_owned());
    }
}

impl Render for SketchView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        Self::attach(div(), cx).track_focus(&self.focus).size_full()
    }
}

/// A view with no commands, put through `kiln::__private::attach!` too.
struct PlainView {
    focus: FocusHandle,
}

impl Render for PlainView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let element = div().track_focus(&self.focus).size_full();
        kiln::__private::attach!(PlainView, element, cx)
    }
}

macro_rules! pane_item {
    ($view:ty, $kind:literal) => {
        impl Focusable for $view {
            fn focus_handle(&self, _: &App) -> FocusHandle {
                self.focus.clone()
            }
        }

        impl PaneItem for $view {
            const KIND: &'static str = $kind;

            fn tab_title(&self, _: &App) -> SharedString {
                $kind.into()
            }

            fn restore(_: Value, _: &mut Window, _: &mut App) -> Task<Option<Entity<Self>>> {
                Task::ready(None)
            }
        }
    };
}

pane_item!(NoteView, "test.view_note");
pane_item!(SketchView, "test.view_sketch");
pane_item!(PlainView, "test.view_plain");

/// What the commands did, in order.
#[derive(Default)]
struct Log(Vec<String>);

impl Global for Log {}

fn log(cx: &mut App, line: String) {
    cx.default_global::<Log>().0.push(line);
}

fn take_log(app: &mut TestApp) -> Vec<String> {
    app.cx()
        .update(|_, cx| std::mem::take(&mut cx.default_global::<Log>().0))
}

fn open(cx: &mut TestAppContext) -> TestApp {
    TestApp::open(
        kiln::app("dev.kiln.MacroViewCommands", "Kiln Macro View Commands")
            .restore_layout(false)
            .item::<NoteView>()
            .item::<SketchView>()
            .item::<PlainView>(),
        cx,
    )
}

/// Adds the item `make` makes to the active pane, focused, then draws.
fn add<T: PaneItem>(app: &mut TestApp, make: impl FnOnce(FocusHandle) -> T) -> Entity<T> {
    let workspace = app.workspace();
    let item = app.cx().update(|window, cx| {
        let item = cx.new(|cx| make(cx.focus_handle()));
        workspace.update(cx, |workspace, cx| {
            workspace.add_item(Box::new(item.clone()), window, cx);
        });
        let focus = item.read(cx).focus_handle(cx);
        window.focus(&focus, cx);
        item
    });
    app.draw();
    item
}

fn note(app: &mut TestApp, title: &str) -> Entity<NoteView> {
    add(app, |focus| NoteView {
        focus,
        title: title.to_owned(),
    })
}

/// The context stack where focus is.
fn stack(app: &mut TestApp) -> ContextStack {
    app.cx()
        .update(|window, cx| keys::context_stack(&window.context_stack(), cx))
}

fn holds(predicate: &str, app: &mut TestApp) -> bool {
    ContextPredicate::parse(predicate)
        .unwrap()
        .eval(&stack(app))
}

/// Whether focus is in `context`, set innermost, within the workspace's.
fn innermost(context: &str, app: &mut TestApp) -> bool {
    let stack = stack(app);
    let frames = stack.frames();
    frames.last().is_some_and(|frame| frame.has(context))
        && frames.iter().any(|frame| frame.has("workspace"))
}

/// Whether the palette, opened where focus is, would list `id`.
fn listed(id: &str, app: &mut TestApp) -> bool {
    let stack = stack(app);
    let id = CommandId::parse(id).unwrap();
    app.cx()
        .read(|cx| commands_in(&stack, cx).iter().any(|entry| entry.id == id))
}

/// The Edit menu's items, by title.
fn edit_menu(app: &mut TestApp) -> Vec<String> {
    app.cx().read(|cx| {
        let bar = menus::installed(cx).expect("menus installed");
        let edit = bar.menu("Edit").expect("an Edit menu");
        (edit.items.iter())
            .filter_map(|item| item.command().map(|command| command.title.clone()))
            .collect()
    })
}

#[gpui::test]
fn a_method_command_runs_from_its_key_and_menu_only_with_focus_in_the_view(
    cx: &mut TestAppContext,
) {
    let mut app = open(cx);

    // Focus in a plain view: the note's commands neither run nor list.
    add(&mut app, |focus| PlainView { focus });
    app.press("f2");
    app.menu("Edit", "Rename Note");
    assert_eq!(take_log(&mut app), Vec::<String>::new());
    assert!(!listed("macro_view_commands.rename", &mut app));

    // Focus in the note: they do, on that note.
    let first = note(&mut app, "first");
    assert!(listed("macro_view_commands.rename", &mut app));
    app.press("f2");
    app.menu("Edit", "Rename Note");
    app.run_command("macro_view_commands.rename");
    assert_eq!(
        take_log(&mut app),
        [
            "rename first (renamed)",
            "rename first (renamed) (renamed)",
            "rename first (renamed) (renamed) (renamed)",
        ]
    );

    // On whichever note focus is in.
    note(&mut app, "second");
    app.press("f2");
    assert_eq!(take_log(&mut app), ["rename second (renamed)"]);
    assert_eq!(
        app.cx().read(|cx| first.read(cx).title.clone()),
        "first (renamed) (renamed) (renamed)"
    );

    // With focus nowhere, they're gone again.
    app.cx().update(|window, cx| window.blur(cx));
    app.draw();
    assert!(!listed("macro_view_commands.rename", &mut app));
    app.press("f2");
    app.menu("Edit", "Rename Note");
    assert_eq!(take_log(&mut app), Vec::<String>::new());
}

#[gpui::test]
fn attach_sets_the_key_context_around_the_view(cx: &mut TestAppContext) {
    let mut app = open(cx);
    assert_eq!(<NoteView as ViewCommands>::CONTEXT, "note");
    assert_eq!(<SketchView as ViewCommands>::CONTEXT, "sketch");

    // Through `kiln::__private::attach!`, as the view macros host a view.
    note(&mut app, "first");
    assert!(innermost("note", &mut app));
    assert!(!holds("sketch", &mut app));

    // Through a handwritten `Render`'s `Self::attach`: the same.
    add(&mut app, |focus| SketchView { focus });
    assert!(innermost("sketch", &mut app));
    assert!(!holds("note", &mut app));
    assert!(listed("macro_view_commands.clear", &mut app));
    assert!(!listed("macro_view_commands.rename", &mut app));
    app.press("f4");
    app.menu("Edit", "Clear Sketch");
    app.press("f2");
    assert_eq!(take_log(&mut app), ["clear", "clear"]);

    // A view without `ViewCommands` is left as it is: no key context of
    // its own, the workspace's frames all there is.
    let frames = |app: &mut TestApp| stack(app).frames().len();
    add(&mut app, |focus| SketchView { focus });
    let around_sketch = frames(&mut app);
    add(&mut app, |focus| PlainView { focus });
    assert_eq!(frames(&mut app), around_sketch - 1);
    assert!(!holds("note || sketch", &mut app));
    assert!(holds("workspace", &mut app));
}

#[gpui::test]
fn ids_default_to_the_crate_and_method_and_id_and_context_override_them(cx: &mut TestAppContext) {
    let mut app = open(cx);
    let typed = |id: &str, app: &mut TestApp| {
        let id = CommandId::parse(id).unwrap();
        app.cx()
            .read(|cx| typed_command(&id, cx))
            .unwrap_or_else(|| panic!("{id} registered"))
    };

    // `<crate>.<method>`, titled by its doc line, in the impl's context.
    assert_eq!(Rename::id().as_str(), "macro_view_commands.rename");
    assert_eq!(
        Rename::documentation(),
        Some("Rename Note\n\nRenames the focused note.")
    );
    let rename = typed("macro_view_commands.rename", &mut app);
    assert_eq!(rename.title, "Rename Note");
    assert_eq!(
        rename.context,
        Some(ContextPredicate::parse("note").unwrap())
    );
    let spec = app.cx().read(|cx| {
        Contributions::commands(cx)
            .iter()
            .find(|spec| spec.id == Rename::id())
            .cloned()
            .expect("contributed")
    });
    assert_eq!(spec.context, Some("note"));
    assert_eq!(spec.keys, "f2".into());
    assert_eq!(spec.menu.map(|menu| menu.menu), Some("Edit".into()));

    // `id` and `context` override them.
    assert_eq!(PinIt::id().as_str(), "notes.pin");
    let pin = typed("notes.pin", &mut app);
    assert_eq!(pin.title, "Pin Note");
    assert_eq!(
        pin.context,
        Some(ContextPredicate::parse("workspace").unwrap())
    );
    let workspace: ContextStack = [ContextFrame::new("workspace")].into_iter().collect();
    let listed_in_workspace = app.cx().read(|cx| {
        commands_in(&workspace, cx)
            .iter()
            .any(|entry| entry.id == PinIt::id())
    });
    assert!(listed_in_workspace);
    // Still handled by the note focus is in.
    note(&mut app, "first");
    app.run_command("notes.pin");
    assert_eq!(take_log(&mut app), ["pin first"]);

    // An unmarked method is left alone: a plain method, no command.
    let shout = CommandId::parse("macro_view_commands.shout").unwrap();
    assert!(app.cx().read(|cx| typed_command(&shout, cx)).is_none());
    let note = app.active_item::<NoteView>().unwrap();
    app.cx().update(|window, cx| {
        note.update(cx, |note, cx| note.shout(&mut Cx { window, cx }));
    });
    assert_eq!(take_log(&mut app), ["shout first"]);

    // Menu items go where the methods are declared, not the impl.
    let line = |command: CommandId, app: &mut TestApp| {
        app.cx().read(|cx| {
            let spec = Contributions::commands(cx)
                .iter()
                .find(|spec| spec.id == command)
                .cloned()
                .expect("contributed");
            spec.menu
                .and_then(|menu| menu.declared)
                .expect("declared")
                .1
        })
    };
    let (rename, pin) = (line(Rename::id(), &mut app), line(PinIt::id(), &mut app));
    assert!(rename < pin, "{rename} < {pin}");
    assert_eq!(
        edit_menu(&mut app),
        ["Rename Note", "Pin Note", "Clear Sketch"]
    );
}
