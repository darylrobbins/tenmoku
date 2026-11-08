// SPDX-License-Identifier: Apache-2.0

//! Kiln: an app framework on GPUI that hides the differences between
//! platforms and provides high-level widgets, in GPUI's own idioms. This
//! crate sets an app up ([`app()`], or [`application`] and [`init`] by
//! hand) and re-exports the others; see `docs/framework.md` in kiln.

mod app_builder;
mod app_commands;
mod config;
mod contributions;
mod cx;
mod ext;
pub mod extensions;
mod item;
pub mod l10n;
pub mod model;
mod settings;
mod setup;
mod tables;
#[cfg(feature = "test-support")]
pub mod testing;
mod view;

pub use gpui;
pub use kiln_command as command;
pub use kiln_core as core;
pub use kiln_platform as platform;
pub use kiln_shell as shell;
pub use kiln_ui as ui;

/// The function [`app()`] beside the module [`app`](mod@app) of app commands:
/// `kiln::app(..)` sets an app up, and `kiln::app::Quit` is a command.
pub use app_builder::{AppBuilder, AppWindow, Started, app};
pub use contributions::{AnyDockPanel, MakePanel, Module, PanelSpec, StatusSpec};
pub use contributions::{
    CONTRIBUTED_KEYS_PATH, CONTRIBUTED_MENUS_PATH, CommandSpec, Contributions, Keys, MenuSpec,
    RegisterCommand, recorded_catalog, workspace_handler,
};
pub use cx::{Cx, with_cx};
pub use ext::binding_label_for;
pub use item::{Item, WorkspaceExt};
/// serde, for an app's settings type without a serde dependency of its
/// own: `#[derive(Deserialize, Serialize)]` with
/// `#[serde(crate = "kiln::serde", default)]`.
pub use kiln_core::serde;
pub use kiln_core::{SettingsType, settings::SettingsEvent};
/// Makes a fn a command, with `key`, `menu`, `order`, `id` and `context`
/// optional:
///
/// ```
/// use kiln::{Cx, shell::Workspace};
///
/// /// New Note
/// #[kiln::command(key = "secondary-n", menu = "File")]
/// fn new_note(workspace: &mut Workspace, cx: &mut Cx<Workspace>) {
///     // Runs on the workspace of the window the command is dispatched in.
///     # let _ = (workspace, cx);
/// }
/// ```
///
/// - The fn stays as it is, and handles the command, from its keys, its
///   menu item, the palette or [`prelude::WindowExt::run_command`], on the
///   workspace of the window it's dispatched in, once the dispatch is over,
///   seeing its invocation ([`current_invocation`]); the invocation
///   observers hear of each run once ([`workspace_handler`]).
/// - `NewNote`, the fn's name in PascalCase, is the command's type, with
///   the fn's visibility: a unit-struct GPUI action written as
///   [`commands!`] writes one, so `NewNote::id()` is its ID and
///   `.on_action(..)` handles it in a view first.
/// - Its ID is `<crate>.new_note`, `<crate>` being `CARGO_CRATE_NAME`, or
///   `id = "workbench.save_all"`'s, whose last segment comes back from the
///   type it names (`SaveAll`).
/// - Its title is the fn's first doc line: a fn with none doesn't compile.
/// - `key = "secondary-n"` gives it default keys, `secondary` being cmd on
///   macOS and ctrl elsewhere, under the app's keymap and the user's;
///   `context = "editor"` limits it, and its keys, to a key context; and
///   `menu = "File"` puts its item in that menu, after the menu table's,
///   by `order = n` (`0` unless given), then by file and line.
///
/// The macro records the command with `inventory`. [`app()`] adds the
/// records compiled into the app's own binary, example or integration test
/// ([`Contributions::add_recorded`]); a library crate's go in through its
/// [`module!`], which the app passes to [`AppBuilder::module`], as a
/// library crate's would otherwise go missing when nothing referenced it.
/// In a debug build, [`app()`] warns of a library crate whose records are
/// linked in but no `module!` claimed, naming it.
///
/// It expands to the fn, then:
///
/// ```ignore
/// /// New Note
/// pub struct NewNote; // a GPUI action named `<crate>::NewNote`
/// fn register(cx: &mut App, info: CommandInfo) { /* NewNote, and new_note as its handler */ }
/// inventory::submit!(CommandRecord {
///     id: "<crate>.new_note", title: "New Note", keys: Some("secondary-n"),
///     menu: Some("File"), file: file!(), line: line!(), register, ..
/// });
/// ```
///
/// which is, by hand, in a crate named `notes`:
///
/// ```
/// use kiln::{CommandSpec, Contributions, Cx, gpui::App, shell::Workspace};
///
/// mod notes {
///     kiln::commands!(notes, [
///         /// New Note
///         NewNote,
///     ]);
/// }
///
/// fn new_note(workspace: &mut Workspace, cx: &mut Cx<Workspace>) {
///     # let _ = (workspace, cx);
/// }
///
/// fn module(cx: &mut App) {
///     Contributions::add_command(
///         cx,
///         CommandSpec::new::<notes::NewNote>("New Note")
///             .keys("secondary-n")
///             .menu("File", 0),
///     );
///     cx.on_action(kiln::workspace_handler::<notes::NewNote>(new_note));
/// }
/// # let _: kiln::Module = module;
/// ```
///
/// the macro's items also placed in their menu by file and line
/// ([`MenuSpec::declared`]).
///
/// A fn without a doc line doesn't compile:
///
/// ```compile_fail
/// use kiln::{Cx, shell::Workspace};
///
/// #[kiln::command]
/// fn new_note(_: &mut Workspace, _: &mut Cx<Workspace>) {}
/// ```
///
/// nor does an argument it doesn't know, the error on its name:
///
/// ```compile_fail
/// use kiln::{Cx, shell::Workspace};
///
/// /// New Note
/// #[kiln::command(shortcut = "secondary-n")]
/// fn new_note(_: &mut Workspace, _: &mut Cx<Workspace>) {}
/// ```
///
/// nor a fn whose signature isn't a handler's, the error on its name:
///
/// ```compile_fail,E0061
/// use kiln::shell::Workspace;
///
/// /// New Note
/// #[kiln::command]
/// fn new_note(_: &mut Workspace) {}
/// ```
///
/// # A view's own commands
///
/// On a view's `impl`, with the view's key context, it makes each method
/// marked `#[command(..)]` a command that runs on the view focus is in:
///
/// ```
/// use kiln::{
///     Cx, ViewCommands as _,
///     gpui::{Context, FocusHandle, IntoElement, Render, Window, div, prelude::*},
/// };
///
/// struct NoteView {
///     focus: FocusHandle,
///     title: String,
/// }
///
/// #[kiln::command(context = "note")]
/// impl NoteView {
///     /// Rename Note
///     #[command(key = "f2", menu = "Edit")]
///     fn rename(&mut self, cx: &mut Cx<Self>) {
///         self.title = "Renamed".into();
///         cx.notify();
///     }
///
///     /// Unmarked, so left alone.
///     fn title(&self) -> &str {
///         &self.title
///     }
/// }
///
/// impl Render for NoteView {
///     fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
///         // Sets the key context "note", with a handler of `Rename`.
///         Self::attach(div(), cx)
///             .track_focus(&self.focus)
///             .child(self.title().to_owned())
///     }
/// }
/// ```
///
/// - The impl stays as it is, the marks taken off, and the view gets
///   [`ViewCommands`]: its `CONTEXT` is `context`, and its `attach` puts
///   an element in that key context with a handler of each command that
///   calls its method with a [`Cx`]. A handwritten `Render` calls
///   `Self::attach(div(), cx)` on its root element, as above; the view
///   macros attach a view they host themselves.
/// - Each marked method makes a command as a fn does: `Rename`, the
///   method's name in PascalCase, is its type, with the method's
///   visibility; its ID is `<crate>.rename` unless `id = ".."`; its title
///   is the method's first doc line; and `key`, `menu` and `order` are as
///   a fn's. Its context is the impl's, so its keys, its menu item and
///   its palette entry hold only while focus is inside the view, unless
///   `context = ".."` on the method gives another.
///   `palette = false` on the method keeps it out of the command palette.
/// - Unmarked methods are left alone.
///
/// A marked method's signature is `fn(&mut self, &mut Cx<Self>)`, or the
/// error is on its name:
///
/// ```compile_fail,E0061
/// use kiln::Cx;
///
/// struct NoteView;
///
/// #[kiln::command(context = "note")]
/// impl NoteView {
///     /// Rename Note
///     #[command]
///     fn rename(&mut self) {}
/// }
/// ```
///
/// and a method makes a command only in such an impl, the error naming
/// the attribute it needs there:
///
/// ```compile_fail
/// use kiln::Cx;
///
/// struct NoteView;
///
/// impl NoteView {
///     /// Rename Note
///     #[kiln::command(key = "f2")]
///     fn rename(&mut self, _: &mut Cx<Self>) {}
/// }
/// ```
///
/// ```compile_fail
/// use kiln::{Cx, command};
///
/// struct NoteView;
///
/// impl NoteView {
///     /// Rename Note
///     #[command(key = "f2")]
///     fn rename(&mut self, _: &mut Cx<Self>) {}
/// }
/// ```
pub use kiln_macros::command;
/// Makes a struct that implements [`Item`] and [`View`] a pane item, a
/// tab as a function of its state, its kind restoring from the saved
/// layout with no register call; `kind` optional:
///
/// ```
/// use kiln::{
///     Cx, Item, View,
///     gpui::{App, AppContext as _, FocusHandle, IntoElement, SharedString, Window, div,
///         prelude::*},
///     prelude::*,
///     serde::{Deserialize, Serialize},
///     shell::Workspace,
/// };
///
/// #[derive(Clone, PartialEq, Serialize, Deserialize)]
/// #[serde(crate = "kiln::serde")]
/// struct NoteState {
///     name: String,
/// }
///
/// #[kiln::item]
/// struct Note {
///     name: String,
///     focus: FocusHandle,
/// }
///
/// impl Item for Note {
///     type State = NoteState;
///
///     fn new(state: NoteState, _: &mut Window, cx: &mut App) -> Option<Self> {
///         Some(Self { name: state.name, focus: cx.focus_handle() })
///     }
///
///     fn state(&self) -> NoteState {
///         NoteState { name: self.name.clone() }
///     }
///
///     fn title(&self, _: &App) -> SharedString {
///         self.name.clone().into()
///     }
///
///     fn focus(&self, _: &App) -> FocusHandle {
///         self.focus.clone()
///     }
/// }
///
/// impl View for Note {
///     fn render(&mut self, _: &mut Cx<Self>) -> impl IntoElement {
///         div().child(self.name.clone())
///     }
/// }
///
/// /// Open Inbox
/// #[kiln::command(key = "secondary-i")]
/// fn open_inbox(workspace: &mut Workspace, cx: &mut Cx<Workspace>) {
///     // The inbox's tab if it's open in the active pane, else a new one.
///     let inbox = NoteState { name: "Inbox".into() };
///     workspace.open::<Note>(inbox, cx.window, cx.cx);
/// }
/// ```
///
/// - The struct stays as it is. It gets GPUI's `Render`, through
///   [`View::render`] around its element tracking [`Item::focus`], at the
///   pane's full size, its [`ViewCommands`] attached if it has them
///   (`#[kiln::command(context = "..")]` on its impl); `Focusable`,
///   [`Item::focus`]; and [`shell::PaneItem`], its tab's title, icon, dirty
///   dot and status items from [`Item`]'s, its saved state [`Item::state`]
///   through [`shell::item::ItemState`].
/// - Its kind in saved layouts is `<crate>.<name>`, `<name>` the struct's
///   in snake case (`notes.note`), unless `kind = ".."`, which keeps an
///   existing one (`kind = "chat.conversation"`). A saved item of its kind
///   restores through [`Item::new`]; one it gives `None` for is left out.
/// - [`WorkspaceExt::open`] shows the item of a state in the active pane:
///   its tab, if one there has an equal [`Item::state`], else a new one;
///   [`WorkspaceExt::active_state`] gives the active item's state.
/// - An item whose restore reads a file first, or that splits, implements
///   [`shell::PaneItem`] by hand, as below, its restore a `Task`.
///
/// The macro records the kind with `inventory`, as `#[kiln::command]`
/// records a command: [`app()`] adds the app's own target's, a library
/// crate's [`module!`] its own ([`Contributions::add_item_kind`]).
///
/// It expands to the struct, then:
///
/// ```ignore
/// impl Render for Note {
///     fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
///         let focus = Item::focus(self, cx);
///         let element = div().track_focus(&focus).size_full()
///             .child(View::render(self, &mut Cx { window, cx }).into_any_element());
///         kiln::__private::attach!(Note, element, cx) // `Note::attach(element, cx)` if it has commands
///     }
/// }
/// impl Focusable for Note { /* Item::focus(self, cx) */ }
/// impl PaneItem for Note {
///     const KIND: &'static str = "<crate>.note";
///     /* tab_title, tab_icon, is_dirty and status_items: Item's */
///     /* serialize, restore and apply_state: item::state's, through ItemState */
/// }
/// impl ItemState for Note {
///     type State = NoteState;
///     /* state: Some(Item::state(self)); from_state: Item::new's, at once */
/// }
/// inventory::submit!(ItemRecord {
///     kind: "<crate>.note", register: layout::register_item::<Note>, file: file!(), line: line!(), ..
/// });
/// ```
///
/// which is, by hand, in a crate named `notes`:
///
/// ```
/// use kiln::{
///     Contributions,
///     command::Value,
///     gpui::{App, AppContext as _, Context, Entity, FocusHandle, Focusable, IntoElement,
///         Render, SharedString, Task, Window, div, prelude::*},
///     serde::{Deserialize, Serialize},
///     shell::{PaneItem, item::{ItemState, state}, layout},
/// };
///
/// #[derive(Clone, PartialEq, Serialize, Deserialize)]
/// #[serde(crate = "kiln::serde")]
/// struct NoteState {
///     name: String,
/// }
///
/// struct Note {
///     name: String,
///     focus: FocusHandle,
/// }
///
/// impl Render for Note {
///     fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
///         div()
///             .track_focus(&self.focus)
///             .size_full()
///             .child(self.name.clone())
///     }
/// }
///
/// impl Focusable for Note {
///     fn focus_handle(&self, _: &App) -> FocusHandle {
///         self.focus.clone()
///     }
/// }
///
/// impl ItemState for Note {
///     type State = NoteState;
///
///     fn state(&self, _: &App) -> Option<NoteState> {
///         Some(NoteState { name: self.name.clone() })
///     }
///
///     fn from_state(state: NoteState, _: &mut Window, cx: &mut App) -> Task<Option<Entity<Self>>> {
///         let note = cx.new(|cx| Note { name: state.name, focus: cx.focus_handle() });
///         Task::ready(Some(note))
///     }
/// }
///
/// impl PaneItem for Note {
///     const KIND: &'static str = "notes.note";
///
///     fn tab_title(&self, _: &App) -> SharedString {
///         self.name.clone().into()
///     }
///
///     fn serialize(&self, cx: &App) -> Option<Value> {
///         state::serialize(self, cx)
///     }
///
///     fn restore(state: Value, window: &mut Window, cx: &mut App) -> Task<Option<Entity<Self>>> {
///         state::restore::<Self>(state, window, cx)
///     }
///
///     fn apply_state(&mut self, state: &Value, window: &mut Window, cx: &mut Context<Self>) {
///         state::apply(self, state, window, cx)
///     }
/// }
///
/// fn module(cx: &mut App) {
///     Contributions::add_item_kind(cx, layout::register_item::<Note>);
/// }
/// # let _: kiln::Module = module;
/// ```
///
/// with the "open or activate" of [`WorkspaceExt::open`] written for it,
/// over the active pane's items.
///
/// A struct that isn't an [`Item`] doesn't compile, the error on its name:
///
/// ```compile_fail,E0277
/// use kiln::{Cx, View, gpui::{IntoElement, div}};
///
/// #[kiln::item]
/// struct Note;
///
/// impl View for Note {
///     fn render(&mut self, _: &mut Cx<Self>) -> impl IntoElement {
///         div()
///     }
/// }
/// ```
///
/// nor does a kind that isn't one, the error on it:
///
/// ```compile_fail
/// use kiln::{Cx, Item, View, gpui::{App, FocusHandle, IntoElement, SharedString, Window, div}};
///
/// #[kiln::item(kind = "Notes")]
/// struct Note(FocusHandle);
/// # impl Item for Note {
/// #     type State = ();
/// #     fn new(_: (), _: &mut Window, _: &mut App) -> Option<Self> { None }
/// #     fn state(&self) {}
/// #     fn title(&self, _: &App) -> SharedString { "Note".into() }
/// #     fn focus(&self, _: &App) -> FocusHandle { self.0.clone() }
/// # }
/// # impl View for Note {
/// #     fn render(&mut self, _: &mut Cx<Self>) -> impl IntoElement { div() }
/// # }
/// ```
pub use kiln_macros::item;
/// Makes a struct or enum an app model: made from its `Default` when
/// [`app()`] starts, so that `cx.model::<M>()`, `cx.watch::<M>()` and
/// `cx.update_model(..)` find it with no register call. With `persist` it's
/// kept in the state directory, in `<state>/<name>.json`, `name` the type's
/// name in snake case, or `file = ".."`, without the `.json`; the type must
/// then be `Serialize`, `Deserialize` and `Default`, and one that isn't is
/// reported at its name.
///
/// ```
/// use kiln::serde::{Deserialize, Serialize};
///
/// /// Kept in `<state>/recent.json`.
/// #[kiln::model(persist, file = "recent")]
/// #[derive(Default, Serialize, Deserialize)]
/// #[serde(crate = "kiln::serde")]
/// struct RecentFiles {
///     paths: Vec<String>,
/// }
///
/// /// Made at start, kept in memory.
/// #[kiln::model]
/// #[derive(Default)]
/// struct Selection(Option<usize>);
/// ```
///
/// It expands to the type, then an `inventory` record, which [`app()`], or
/// the crate's [`module!`], contributes ([`Contributions::add_model`]):
///
/// ```ignore
/// inventory::submit!(ModelRecord {
///     name: "RecentFiles",
///     add: { fn add(cx: &mut App) { persist::<RecentFiles>(cx, "recent") } add },
///     file: file!(), line: line!(), ..
/// });
/// inventory::submit!(ModelRecord {
///     name: "Selection", add: model::register::<Selection>, file: file!(), line: line!(), ..
/// });
/// ```
///
/// which is, by hand:
///
/// ```
/// use kiln::{
///     Contributions,
///     gpui::App,
///     model::{self, persist::persist},
///     serde::{Deserialize, Serialize},
/// };
///
/// #[derive(Default, Serialize, Deserialize)]
/// #[serde(crate = "kiln::serde")]
/// struct RecentFiles {
///     paths: Vec<String>,
/// }
///
/// #[derive(Default)]
/// struct Selection(Option<usize>);
///
/// fn module(cx: &mut App) {
///     Contributions::add_model(cx, |cx| persist::<RecentFiles>(cx, "recent"));
///     Contributions::add_model(cx, model::register::<Selection>);
/// }
/// # let _: kiln::Module = module;
/// ```
///
/// An argument it doesn't know doesn't compile, the error on it:
///
/// ```compile_fail
/// #[kiln::model(persistent)]
/// #[derive(Default)]
/// struct Recent(Vec<String>);
/// ```
///
/// nor does `persist` on a type that isn't `Serialize`:
///
/// ```compile_fail
/// #[kiln::model(persist)]
/// #[derive(Default)]
/// struct Recent(Vec<String>);
/// ```
pub use kiln_macros::model;
/// Makes a struct that implements [`View`] a dock panel, in each
/// workspace window [`app()`] opens, with its toggle command, key, View
/// menu item and rail button; `title`, `icon`, `dock`, `toggle_key`,
/// `order`, `id` and `new` optional:
///
/// ```
/// use kiln::{Cx, View, gpui::{IntoElement, ParentElement as _, div}};
///
/// #[kiln::panel(title = "Notes", icon = FileText, dock = Left, toggle_key = "secondary-b")]
/// #[derive(Default)]
/// struct Notes {
///     filter: String,
/// }
///
/// impl View for Notes {
///     fn render(&mut self, cx: &mut Cx<Self>) -> impl IntoElement {
///         div().child(format!("notes matching {}", self.filter))
///     }
/// }
/// ```
///
/// - The struct gets a hidden field, `__kiln:
///   kiln::__private::ViewState`, which holds its focus handle: a unit
///   struct becomes one with that field, a tuple struct's is the last.
///   It's [`Default`], and kiln fills it as it builds the panel. Put
///   `#[kiln::panel]` above the struct's derives, as above, so they see
///   the field: a derive above it sees the struct without it.
/// - It gets GPUI's `Render`, through [`View::render`] around its element
///   tracking its focus handle, at the dock's full size, its
///   [`ViewCommands`] attached if it has them (`#[kiln::command(context =
///   "..")]` on its impl); `Focusable`, that handle; and
///   [`shell::DockItem`], from the arguments.
/// - It's built for each window from [`Default`], or by `new = path`, a
///   `fn(&mut Cx<Self>) -> Self`, which fills `__kiln` with
///   `Default::default()`.
/// - Its ID is `<crate>.<name>`, `<name>` the struct's in snake case
///   (`macro_panel.notes`), unless `id = ".."`; its toggle command's is
///   `<id>.toggle`, titled `Toggle <title>`: on `toggle_key`, if given,
///   in the palette, and in the View menu. Its title is `title`'s, else
///   the struct's first doc line.
/// - `dock` is `Left` unless `Right` or `Bottom`; `icon` is a
///   [`ui::Icon`], `Files` unless given; `order` places it on its dock's
///   rail and its toggle in the View menu, lower first, `0` unless given,
///   then by file and line.
/// - Whether it shows, and the dock's size, are in the saved layout.
///
/// The macro records the panel with `inventory`, as `#[kiln::command]`
/// records a command: [`app()`] adds the app's own target's, a library
/// crate's [`module!`] its own ([`Contributions::add_panel`]).
///
/// It expands to the struct, its field added, then:
///
/// ```ignore
/// impl Render for Notes {
///     fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
///         let focus = self.__kiln.focus_handle(cx);
///         let element = div().track_focus(&focus).size_full()
///             .child(View::render(self, &mut Cx { window, cx }).into_any_element());
///         kiln::__private::attach!(Notes, element, cx) // `Notes::attach(element, cx)` if it has commands
///     }
/// }
/// impl Focusable for Notes { /* self.__kiln.focus_handle(cx) */ }
/// impl DockItem for Notes { /* id, title, icon, and the toggle as show_command */ }
/// inventory::submit!(PanelRecord {
///     id: "<crate>.notes", title: "Notes", icon: Icon::FileText, position: DockPosition::Left,
///     toggle_key: Some("secondary-b"), make, file: file!(), line: line!(), ..
/// });
/// ```
///
/// which is, by hand, in a crate named `notes`:
///
/// ```
/// use kiln::{
///     Contributions, Cx, PanelSpec,
///     gpui::{App, AppContext as _, Context, FocusHandle, Focusable, IntoElement, Render,
///         SharedString, Window, div, prelude::*},
///     shell::{DockItem, DockPosition, PanelId},
///     ui::Icon,
/// };
///
/// struct Notes {
///     focus: FocusHandle,
///     filter: String,
/// }
///
/// impl Render for Notes {
///     fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
///         div()
///             .track_focus(&self.focus)
///             .size_full()
///             .child(format!("notes matching {}", self.filter))
///     }
/// }
///
/// impl Focusable for Notes {
///     fn focus_handle(&self, _: &App) -> FocusHandle {
///         self.focus.clone()
///     }
/// }
///
/// impl DockItem for Notes {
///     fn id(&self) -> PanelId {
///         PanelId("notes.notes")
///     }
///     fn title(&self, _: &App) -> SharedString {
///         "Notes".into()
///     }
///     fn icon(&self, _: &App) -> Icon {
///         Icon::FileText
///     }
///     fn show_command(&self) -> Option<&'static str> {
///         Some("notes.notes.toggle")
///     }
/// }
///
/// fn module(cx: &mut App) {
///     Contributions::add_panel(cx, PanelSpec {
///         id: PanelId("notes.notes"),
///         title: "Notes".into(),
///         icon: Icon::FileText,
///         position: DockPosition::Left,
///         order: 0,
///         toggle_key: "secondary-b".into(),
///         make: |_, cx| {
///             cx.new(|cx| Notes { focus: cx.focus_handle(), filter: String::new() }).into()
///         },
///     });
/// }
/// # let _: kiln::Module = module;
/// ```
///
/// A struct that isn't a [`View`] doesn't compile, the error on its name:
///
/// ```compile_fail,E0277
/// #[kiln::panel(title = "Notes")]
/// #[derive(Default)]
/// struct Notes;
/// ```
///
/// nor does an argument it doesn't know, the error on its name:
///
/// ```compile_fail
/// use kiln::{Cx, View, gpui::{IntoElement, div}};
///
/// #[kiln::panel(title = "Notes", shortcut = "secondary-b")]
/// #[derive(Default)]
/// struct Notes;
///
/// impl View for Notes {
///     fn render(&mut self, _: &mut Cx<Self>) -> impl IntoElement {
///         div()
///     }
/// }
/// ```
pub use kiln_macros::panel;
/// Makes a struct of named fields a section of the app's settings: its
/// own table of the one `settings.toml`, `section` naming it, each field
/// a key, `#[setting(default = ..)]` its default.
///
/// ```
/// use kiln::Cx;
///
/// /// `[notes]` in settings.toml.
/// #[kiln::settings(section = "notes")]
/// #[derive(Clone, Debug)]
/// pub struct NotesSettings {
///     /// The editor's font size, in points.
///     #[setting(default = 14.0)]
///     pub font_size: f32,
///     /// The font, by family name.
///     #[setting(default = "Menlo")]
///     pub font: String,
///     /// Whether lines wrap.
///     pub wrap: bool,
/// }
///
/// assert_eq!(NotesSettings::default().font_size, 14.0);
///
/// // In a view, which redraws after a reload changes them:
/// fn font_size<V: 'static>(cx: &mut Cx<V>) -> f32 {
///     cx.setting::<NotesSettings>().font_size
/// }
/// # let _ = font_size::<()>;
/// ```
///
/// - The struct stays as written, without its `#[setting]`s, and gets
///   serde's `Serialize` and `Deserialize` with `#[serde(crate =
///   "kiln::serde", default)]`, so a file sets only the keys it wants;
///   `PartialEq`; schemars' `JsonSchema` ([`schemars`]), each field's doc
///   line its key's description ([`settings_schema`]); a `Default`, each
///   field `default = ..`'s value (a string literal `Into`'d, for a
///   `String`), or its type's `Default` without one; and
///   [`SettingsSection`]. Each field's type needs those too: a nested
///   table is a struct deriving them, with `#[serde(default)]`.
/// - [`app()`] reads the section from `[notes]` of the user's
///   `settings.toml` in the config directory ([`Config::settings_file`]),
///   watched: `cx.setting::<S>()` in a view ([`Cx::setting`]) reads it
///   and redraws the view after a reload changes it, and every window
///   redraws after one does (`cx.refresh_windows()`).
/// - Every section shares the file. A key is checked against the
///   sections' tables and each one's schema: a table of another section
///   isn't unknown, while a key in no section's table, or one the
///   section doesn't have, is a warning, and a value its type can't take
///   an error, which keeps the section's last good table.
///
/// The macro records the section with `inventory`, as `#[kiln::command]`
/// records a command: [`app()`] adds the app's own target's, a library
/// crate's [`module!`] its own ([`Contributions::add_settings`]).
///
/// It expands to the struct, its `#[setting]`s removed, then:
///
/// ```ignore
/// #[derive(Serialize, Deserialize, JsonSchema, PartialEq)]
/// #[serde(crate = "::kiln::serde", default)]
/// #[schemars(crate = "::kiln::schemars")]
/// pub struct NotesSettings { .. }
///
/// impl Default for NotesSettings {
///     fn default() -> Self {
///         Self { font_size: 14.0, font: Into::into("Menlo"), wrap: Default::default() }
///     }
/// }
/// impl SettingsSection for NotesSettings {
///     const SECTION: &'static str = "notes";
/// }
/// inventory::submit!(SettingsRecord {
///     section: "notes", add: add_section::<NotesSettings>, file: file!(), line: line!(), ..
/// });
/// ```
///
/// which is, by hand, [`Config::section`] through
/// [`Contributions::add_settings`]:
///
/// ```
/// use kiln::{
///     Config, Contributions, SettingsSection,
///     gpui::App,
///     schemars::JsonSchema,
///     serde::{Deserialize, Serialize},
/// };
///
/// /// `[notes]` in settings.toml.
/// #[derive(Clone, Debug, PartialEq, Deserialize, Serialize, JsonSchema)]
/// #[serde(crate = "kiln::serde", default)]
/// #[schemars(crate = "kiln::schemars")]
/// pub struct NotesSettings {
///     /// The editor's font size, in points.
///     pub font_size: f32,
/// }
///
/// impl Default for NotesSettings {
///     fn default() -> Self {
///         Self { font_size: 14.0 }
///     }
/// }
///
/// impl SettingsSection for NotesSettings {
///     const SECTION: &'static str = "notes";
/// }
///
/// fn module(cx: &mut App) {
///     Contributions::add_settings(cx, |config: Config| config.section::<NotesSettings>());
/// }
/// # let _: kiln::Module = module;
/// ```
///
/// A section that isn't a table's bare name doesn't compile, the error on
/// it:
///
/// ```compile_fail
/// #[kiln::settings(section = "notes.editor")]
/// struct NotesSettings {
///     font_size: f32,
/// }
/// ```
///
/// nor does an argument `#[setting]` doesn't know:
///
/// ```compile_fail
/// #[kiln::settings(section = "notes")]
/// struct NotesSettings {
///     #[setting(fallback = 14.0)]
///     font_size: f32,
/// }
/// ```
pub use kiln_macros::settings;
/// Makes a fn a status bar source, in each workspace window [`app()`]
/// opens: `fn(cx: &App) -> T`, `T` a `String`, an `Option<String>` (`None`
/// showing nothing) or a [`shell::StatusItem`]; `side`, `order` and `id`
/// optional:
///
/// ```
/// use kiln::gpui::App;
///
/// #[derive(Default)]
/// struct NoteCount(usize);
///
/// impl kiln::gpui::Global for NoteCount {}
///
/// /// The note count, on the left of the bar.
/// #[kiln::status(side = Left, order = 0)]
/// fn count(cx: &App) -> String {
///     let count = cx.try_global::<NoteCount>().map_or(0, |count| count.0);
///     format!("{count} notes")
/// }
/// ```
///
/// - The fn stays as it is, and is called each time the bar draws, its
///   text shown as the item `<crate>.<fn>` (`notes.count`), unless `id =
///   ".."`, on `side` (`Left` unless `Right`), by `order` (`0` unless
///   given), then by file and line. A [`shell::StatusItem`] is shown as
///   it is, its own ID, side and priority.
/// - The bar draws as the workspace changes: its active item, its focus,
///   its docks. To show a change at once, notify the workspace
///   ([`Cx::workspace`]).
/// - It runs on every draw, so keep it O(1): read a value, don't compute
///   one.
///
/// The macro records the source with `inventory`, as `#[kiln::command]`
/// records a command: [`app()`] adds the app's own target's, a library
/// crate's [`module!`] its own ([`Contributions::add_status`]).
///
/// It expands to the fn, then:
///
/// ```ignore
/// fn source(cx: &App) -> Option<StatusItem> {
///     IntoStatus::into_status(count(cx), "<crate>.count", StatusSide::Left, 0)
/// }
/// inventory::submit!(StatusRecord {
///     id: "<crate>.count", side: StatusSide::Left, order: 0, item: source,
///     file: file!(), line: line!(), ..
/// });
/// ```
///
/// which is, by hand, in a crate named `notes`, the
/// [`shell::Workspace::add_status_source`] of each window:
///
/// ```
/// use kiln::{
///     Contributions, StatusSpec,
///     gpui::App,
///     shell::{StatusItem, StatusSide},
/// };
///
/// fn count(_: &App) -> Option<StatusItem> {
///     Some(StatusItem::new("notes.count", StatusSide::Left, 0, "12 notes"))
/// }
///
/// fn module(cx: &mut App) {
///     Contributions::add_status(cx, StatusSpec {
///         side: StatusSide::Left,
///         order: 0,
///         item: count,
///     });
/// }
/// # let _: kiln::Module = module;
/// ```
///
/// A fn whose signature isn't a source's doesn't compile, the error on
/// its name:
///
/// ```compile_fail,E0061
/// #[kiln::status]
/// fn count() -> String {
///     "12 notes".to_owned()
/// }
/// ```
pub use kiln_macros::status;
pub use kiln_platform::WindowSpec;
pub use kiln_platform::config::{ConfigDiagnostics, GlobalSettings};
/// A window's root view, around the app's: the chrome the platform wants
/// and the per-window setup. See [`kiln_shell::Root`].
pub use kiln_shell::Root;
pub use model::provide;
/// schemars, for a settings section's schema without a dependency of its
/// own: `#[derive(JsonSchema)]` with `#[schemars(crate = "kiln::schemars")]`.
pub use schemars;
pub use settings::{SettingsSection, schema as settings_schema};
pub use setup::{AppInfo, Config, application, catalog, init, window_options};
pub use tables::{KeyBinding, KeyTable, MenuTable, TableItem, TableMenu};
pub use view::{View, ViewCommands};

/// The extension traits that put kiln's calls on GPUI's own types:
/// `use kiln::prelude::*;` then `cx.theme()`, `cx.settings::<S>()`,
/// `cx.set_theme_mode(..)`, `cx.binding_label(..)`,
/// `window.run_command(..)` or `workspace.open::<Note>(..)`; and
/// [`Command`], for a typed command's ID: `NewNote::id()`.
pub mod prelude {
    pub use crate::ext::{AppExt, WindowExt};
    pub use crate::item::WorkspaceExt;
    pub use kiln_platform::command_actions::Command;
    pub use kiln_ui::ActiveTheme;
}

// Commands: `kiln::commands!` declares typed commands (GPUI actions with a
// title and context), and these dispatch and record them.
pub use kiln_platform::command_actions::{
    Command, CommandInfo, current_invocation, dispatch_invocation, register_command,
};
pub use kiln_platform::commands;

/// Declares a library crate's [`Module`], `$name`, which adds the commands
/// its `#[kiln::command]`s declared: `kiln::module!(pub fn module);`, then
/// `kiln::app(..).module(notes::module)` in the app. An app's own binary
/// needs none: [`app()`] adds its commands itself.
///
/// ```
/// kiln::module!(pub fn module);
///
/// let module: kiln::Module = module;
/// # let _ = module;
/// ```
#[macro_export]
macro_rules! module {
    ($(#[$attr:meta])* $vis:vis fn $name:ident) => {
        $(#[$attr])*
        $vis fn $name(cx: &mut $crate::gpui::App) {
            $crate::__private::add_crate_commands(cx, ::std::env!("CARGO_CRATE_NAME"));
        }
    };
}

/// `#[kiln::command]`'s command type: a unit-struct GPUI action named
/// `$action`, documented by `$doc`, written as [`commands!`] writes one.
#[doc(hidden)]
#[macro_export]
macro_rules! __command {
    ($vis:vis $name:ident, $action:expr, [$($doc:literal),*]) => {
        $(#[doc = $doc])*
        #[derive(::std::clone::Clone, ::std::cmp::PartialEq, ::std::default::Default, ::std::fmt::Debug)]
        $vis struct $name;

        impl $crate::gpui::Action for $name {
            fn name(&self) -> &'static str {
                <Self as $crate::gpui::Action>::name_for_type()
            }

            fn name_for_type() -> &'static str
            where
                Self: Sized,
            {
                $action
            }

            fn partial_eq(&self, action: &dyn $crate::gpui::Action) -> bool {
                action.as_any().downcast_ref::<Self>().is_some_and(|a| self == a)
            }

            fn boxed_clone(&self) -> ::std::boxed::Box<dyn $crate::gpui::Action> {
                ::std::boxed::Box::new(self.clone())
            }

            fn build(
                _: $crate::gpui::private::serde_json::Value,
            ) -> $crate::gpui::Result<::std::boxed::Box<dyn $crate::gpui::Action>> {
                ::std::result::Result::Ok(::std::boxed::Box::new(Self))
            }

            fn documentation() -> ::std::option::Option<&'static str>
            where
                Self: Sized,
            {
                static DOC: ::std::sync::OnceLock<::std::option::Option<::std::string::String>> =
                    ::std::sync::OnceLock::new();
                DOC.get_or_init(|| {
                    $crate::platform::command_actions::documentation_from_doc(&[$($doc),*])
                })
                .as_deref()
            }
        }

        impl $crate::Command for $name {}

        // GPUI's action registry finds its actions through `inventory`,
        // as its derive arranges.
        const _: () = {
            fn builder() -> $crate::gpui::MacroActionData {
                $crate::gpui::MacroActionData {
                    name: <$name as $crate::gpui::Action>::name_for_type(),
                    type_id: ::std::any::TypeId::of::<$name>(),
                    build: <$name as $crate::gpui::Action>::build,
                    json_schema: <$name as $crate::gpui::Action>::action_json_schema,
                    deprecated_aliases: <$name as $crate::gpui::Action>::deprecated_aliases(),
                    deprecation_message: <$name as $crate::gpui::Action>::deprecation_message(),
                    documentation: <$name as $crate::gpui::Action>::documentation(),
                }
            }
            $crate::gpui::private::inventory::submit! {
                $crate::gpui::MacroActionBuilder(builder)
            }
        };
    };
}

/// What kiln's macros expand to names: not for use by hand.
#[doc(hidden)]
pub mod __private {
    pub use crate::__attach as attach;
    pub use crate::contributions::{
        CommandRecord, RunRecorded, add_crate_commands, register_recorded as register_command,
    };
    pub use crate::contributions::{
        ItemRecord, ModelRecord, PanelRecord, SettingsRecord, StatusRecord, add_section,
    };
    pub use crate::item::restore_item;
    pub use crate::view::{
        Attach, AttachNothing, AttachViewCommands, IntoStatus, ViewState, make_panel, run_in_view,
        view_handler, view_root,
    };
    pub use inventory;
}

/// The app commands, as typed actions: `app.quit` is `app::Quit`. `init`
/// registers them and runs them app-wide; bind them as `app.quit`.
pub mod app {
    kiln_platform::commands!(
        app,
        [
            /// Quit
            Quit,
            /// Hide
            Hide,
            /// Hide Others
            HideOthers,
            /// Show All
            ShowAll,
        ]
    );
}

/// The window commands, as typed actions on the active window:
/// `window.close` is `window::Close`.
pub mod window {
    kiln_platform::commands!(
        window,
        [
            /// Minimize
            Minimize,
            /// Zoom
            Zoom,
            /// Toggle Full Screen
            ToggleFullscreen,
            /// Close Window
            Close,
        ]
    );
}
