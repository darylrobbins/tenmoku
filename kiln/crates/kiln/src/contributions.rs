// SPDX-License-Identifier: LicenseRef-Proprietary

//! What an app, its modules and later its plugins add to it, as plain data:
//! the [`Contributions`] registry. Commands come with their default keys
//! and the menu they show in, and may be added before [`crate::init`] or
//! any time after.
//!
//! ```ignore
//! Contributions::add_command(
//!     cx,
//!     CommandSpec::new::<notes::NewNote>("New Note")
//!         .keys("secondary-n")
//!         .menu("File", 10),
//! );
//! ```
//!
//! Each addition after `init` re-resolves the keymap and menus once the
//! current update is over, all of that update's additions together:
//!
//! 1. the keymap's layers, in order: kiln's bundled keys, the
//!    contributed commands' default keys, the app's keymap, then the
//!    user's file; the app's and the user's are checked again against
//!    every command registered now, so a binding to a command registered
//!    since it was first read now binds;
//! 2. the menus: the app's table, checked again the same way, with each
//!    contributed command's item placed in its menu, a menu missing from
//!    the table added after the others.
//!
//! The new keymap is swapped in ([`kiln_platform::keys::set_keymap`]) and
//! the menus reinstalled ([`kiln_platform::menus::install`]). A reload of
//! the user's keymap keeps the contributed layer under it.
//!
//! The rest is added before `init`, as a [`Module`] does when
//! [`crate::app()`] starts: panels ([`Contributions::add_panel`]), each with
//! a toggle command, which [`crate::app()`] puts in each workspace window;
//! status sources ([`Contributions::add_status`]); settings stores
//! ([`Contributions::add_settings`]); and item kinds and models
//! ([`Contributions::add_item_kind`], [`Contributions::add_model`]), which
//! `init` sets up once it has run, and an addition after it at once.
//!
//! A library crate contributes through a [`Module`], one function the app
//! passes to [`crate::AppBuilder::module`]: kiln's macros register only
//! the app crate's own items by themselves, and a library crate's would go
//! missing when nothing referenced the crate.

use std::{borrow::Cow, path::PathBuf, rc::Rc};

use gpui::{App, Context, Entity, Global, SharedString, Window};
use kiln_command::{
    CommandId, ContextFrame, ContextPredicate, ContextStack, Dispatched, Effect, RegisterError,
    Value,
};
use kiln_core::{
    CatalogError, CommandCatalog, CommandItem, Diagnostic, Entry, ItemKind, KeymapLayer, Location,
    Menu, MenuBar, MenuItem, Os, Severity,
};
use kiln_platform::{
    command_actions::{Command, CommandInfo, dispatch_window, register_command},
    commands::register_app_commands,
    config::SharedKeymap,
    menus,
};
use kiln_shell::{DockItem, DockPosition, PanelId, StatusItem, StatusSide, Workspace};
use kiln_ui::Icon;

use crate::setup::{Catalogs, Config, toml_escape};

/// The path the contributed keys' diagnostics and bindings name.
pub const CONTRIBUTED_KEYS_PATH: &str = "<contributed keys>";

/// The path the contributed menu items' diagnostics name.
pub const CONTRIBUTED_MENUS_PATH: &str = "<contributed menus>";

/// Records a command when it's contributed: the typed command, with the
/// title and context its [`CommandSpec`] gives it, and any handler of the
/// app's. [`register_command::<A>`] is one.
pub type RegisterCommand = fn(&mut App, CommandInfo);

/// A command's default keys, as a keymap file's `keys` may give them.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum Keys {
    /// None.
    #[default]
    None,
    /// The same keys on every platform, `secondary` being cmd on macOS
    /// and ctrl elsewhere: `"secondary-n"`.
    All(Cow<'static, str>),
    /// Keys by platform name (`macos`, `linux` or `windows`), with
    /// `default` for any not named: on a platform with neither, none.
    ByPlatform(Vec<(Cow<'static, str>, Cow<'static, str>)>),
}

impl From<&'static str> for Keys {
    fn from(keys: &'static str) -> Self {
        Self::All(Cow::Borrowed(keys))
    }
}

impl From<String> for Keys {
    fn from(keys: String) -> Self {
        Self::All(Cow::Owned(keys))
    }
}

impl Keys {
    /// The keys as a keymap file's `keys` value, or `None` for none.
    fn toml(&self) -> Option<String> {
        match self {
            Self::None => None,
            Self::All(keys) => Some(format!("\"{}\"", toml_escape(keys))),
            Self::ByPlatform(by) => {
                let by: Vec<String> = by
                    .iter()
                    .map(|(os, keys)| {
                        format!("\"{}\" = \"{}\"", toml_escape(os), toml_escape(keys))
                    })
                    .collect();
                Some(format!("{{ {} }}", by.join(", ")))
            }
        }
    }
}

/// Where a command's item goes: the menu named `menu`, after the items the
/// app's table gives it, in `order` among the contributed ones (then in
/// the order added).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MenuSpec {
    pub menu: Cow<'static, str>,
    pub order: i32,
}

/// A command, its default keys and its menu item.
#[derive(Clone, Debug)]
pub struct CommandSpec {
    pub id: CommandId,
    /// What menus and the palette call it.
    pub title: String,
    /// Where it applies, as a key context predicate; its default keys
    /// hold only there. `None` is everywhere.
    pub context: Option<&'static str>,
    pub keys: Keys,
    pub menu: Option<MenuSpec>,
    pub register: RegisterCommand,
}

impl CommandSpec {
    /// The typed command `A`, titled `title`, with no context, keys or
    /// menu until given.
    pub fn new<A: Command>(title: impl Into<String>) -> Self {
        Self {
            id: A::id(),
            title: title.into(),
            context: None,
            keys: Keys::None,
            menu: None,
            register: register_command::<A>,
        }
    }

    pub fn context(mut self, context: &'static str) -> Self {
        self.context = Some(context);
        self
    }

    pub fn keys(mut self, keys: impl Into<Keys>) -> Self {
        self.keys = keys.into();
        self
    }

    pub fn menu(mut self, menu: impl Into<Cow<'static, str>>, order: i32) -> Self {
        self.menu = Some(MenuSpec {
            menu: menu.into(),
            order,
        });
        self
    }

    fn info(&self) -> CommandInfo {
        CommandInfo {
            title: self.title.clone(),
            context: self.context,
        }
    }
}

/// What a library crate contributes to an app, as one function that adds
/// it all to [`Contributions`]: the app passes it to
/// [`crate::AppBuilder::module`], which calls it before `init`. In a debug
/// build, one that contributes nothing logs a warning naming it.
///
/// ```
/// use kiln::{CommandSpec, Contributions, gpui::App};
///
/// mod outline {
///     kiln::commands!(outline, [
///         /// Collapse All
///         CollapseAll,
///     ]);
/// }
///
/// /// The outline crate's module: `kiln::app(..).module(outline_module)`.
/// pub fn outline_module(cx: &mut App) {
///     Contributions::add_command(cx, CommandSpec::new::<outline::CollapseAll>("Collapse All"));
/// }
///
/// let module: kiln::Module = outline_module;
/// # let _ = module;
/// ```
pub type Module = fn(&mut App);

/// A panel, made anew for each workspace window, entity and type both
/// hidden: `Entity<T>` for a [`DockItem`] `T` converts into one.
pub struct AnyDockPanel(AddPanel);

/// Adds a panel to a workspace's dock at a position, the panel's ID
/// expected.
type AddPanel =
    Box<dyn FnOnce(DockPosition, PanelId, &mut Workspace, &mut Window, &mut Context<Workspace>)>;

impl AnyDockPanel {
    pub fn new<T: DockItem>(panel: Entity<T>) -> Self {
        Self(Box::new(
            move |position, expected, workspace, window, cx| {
                let id = panel.read(cx).id();
                if id != expected {
                    log::warn!(
                        "the panel contributed as {} is {}: `{}.toggle` won't find it",
                        expected.0,
                        id.0,
                        expected.0
                    );
                }
                workspace.add_panel(position, panel, window, cx);
            },
        ))
    }

    /// Adds the panel to `workspace`'s dock at `position`.
    pub(crate) fn add(
        self,
        position: DockPosition,
        expected: PanelId,
        workspace: &mut Workspace,
        window: &mut Window,
        cx: &mut Context<Workspace>,
    ) {
        (self.0)(position, expected, workspace, window, cx);
    }
}

impl<T: DockItem> From<Entity<T>> for AnyDockPanel {
    fn from(panel: Entity<T>) -> Self {
        Self::new(panel)
    }
}

/// Makes a contributed panel for a workspace window.
pub type MakePanel = fn(&mut Window, &mut App) -> AnyDockPanel;

/// A panel for each workspace window's dock at `position`, and the command
/// that toggles it, `<id>.toggle` titled `Toggle <title>`, in the View
/// menu.
#[derive(Clone)]
pub struct PanelSpec {
    /// The panel's [`DockItem::id`].
    pub id: PanelId,
    /// What the toggle command is titled after: the panel's
    /// [`DockItem::title`].
    pub title: SharedString,
    /// The panel's [`DockItem::icon`], for those listing panels without
    /// making one.
    pub icon: Icon,
    pub position: DockPosition,
    /// Lower comes first on the dock's rail, and in the View menu; equal
    /// ones go in the order added.
    pub order: i32,
    /// The toggle command's default keys: [`Keys::None`] for none.
    pub toggle_key: Keys,
    pub make: MakePanel,
}

impl PanelSpec {
    /// The toggle command's ID, `<id>.toggle`, or why it isn't one.
    fn toggle_id(&self) -> Result<CommandId, kiln_command::InvalidCommandId> {
        CommandId::parse(&format!("{}.toggle", self.id.0))
    }
}

/// A status source for each workspace window's bar, called each time it
/// draws ([`Workspace::add_status_source`]): the item it gives sits on
/// the item's own side, by its priority. The sources are added by `side`,
/// then `order`, then the order added, which places items whose side and
/// priority tie.
#[derive(Clone, Copy, Debug)]
pub struct StatusSpec {
    pub side: StatusSide,
    pub order: i32,
    pub item: fn(&App) -> Option<StatusItem>,
}

/// Everything contributed to the app, as a GPUI global: see the module
/// docs.
#[derive(Default)]
pub struct Contributions {
    commands: Vec<CommandSpec>,
    /// Those added before `init`, to register when it runs.
    unregistered: Vec<CommandSpec>,
    panels: Vec<PanelSpec>,
    statuses: Vec<StatusSpec>,
    settings: Vec<fn(Config) -> Config>,
    /// The item kinds and models added before `init`, to set up once it
    /// has run, in the order added.
    unrun: Vec<fn(&mut App)>,
    /// How many additions of any kind there have been.
    added: usize,
    item_kinds: usize,
    /// Set by `init`, from when additions re-resolve.
    resolver: Option<Resolver>,
    scheduled: bool,
    resolutions: usize,
}

impl Global for Contributions {}

/// What re-resolving needs from `init`.
struct Resolver {
    config: Config,
    os: Os,
    keymap: SharedKeymap,
    /// The menus' diagnostics last logged.
    menu_diagnostics: Vec<Diagnostic>,
}

impl Contributions {
    /// Adds `spec`'s command, replacing one contributed before with its
    /// ID. Before [`crate::init`], it's registered when `init` registers
    /// the app's commands, and its keys and menu item are resolved with
    /// the app's. After, it's registered now, and the keymap and menus are
    /// resolved again once the current update is over.
    ///
    /// What `#[kiln::command]` writes, by hand:
    ///
    /// ```
    /// use kiln::{CommandSpec, Contributions, gpui::App};
    ///
    /// mod notes {
    ///     kiln::commands!(notes, [
    ///         /// New Note
    ///         NewNote,
    ///     ]);
    /// }
    ///
    /// fn module(cx: &mut App) {
    ///     Contributions::add_command(
    ///         cx,
    ///         CommandSpec::new::<notes::NewNote>("New Note")
    ///             .keys("secondary-n")
    ///             .menu("File", 10),
    ///     );
    /// }
    /// # let _: kiln::Module = module;
    /// ```
    pub fn add_command(cx: &mut App, spec: CommandSpec) {
        Self::add_commands(cx, [spec]);
    }

    /// [`Contributions::add_command`] for each of `specs`, in order.
    ///
    /// ```
    /// use kiln::{CommandSpec, Contributions, gpui::App};
    ///
    /// mod notes {
    ///     kiln::commands!(notes, [
    ///         /// Archive
    ///         Archive,
    ///         /// Pin
    ///         #[command(context = "notes_list")]
    ///         Pin,
    ///     ]);
    /// }
    ///
    /// fn module(cx: &mut App) {
    ///     Contributions::add_commands(cx, [
    ///         CommandSpec::new::<notes::Archive>("Archive Note").menu("Note", 0),
    ///         CommandSpec::new::<notes::Pin>("Pin Note")
    ///             .context("notes_list")
    ///             .keys("secondary-p"),
    ///     ]);
    /// }
    /// # let _: kiln::Module = module;
    /// ```
    pub fn add_commands(cx: &mut App, specs: impl IntoIterator<Item = CommandSpec>) {
        let started = cx
            .try_global::<Self>()
            .is_some_and(|this| this.resolver.is_some());
        for spec in specs {
            if started {
                (spec.register)(cx, spec.info());
            }
            let this = cx.default_global::<Self>();
            this.commands.retain(|added| added.id != spec.id);
            if !started {
                this.unregistered.retain(|added| added.id != spec.id);
                this.unregistered.push(spec.clone());
            }
            this.commands.push(spec);
            this.added += 1;
        }
        if started {
            schedule(cx);
        }
    }

    /// Adds `spec`'s panel, replacing one contributed before with its ID,
    /// and its toggle command, `<id>.toggle` titled `Toggle <title>`, on
    /// `toggle_key` and in the View menu, by `order`. [`crate::app()`] adds
    /// the panels to each workspace window, by `order`, then in the order
    /// added, after its own [`crate::AppBuilder::panel`]s. The command
    /// shows the dock with the panel, or toggles the dock if the panel is
    /// the one it shows, through [`Workspace::toggle_dock`]: focus doesn't
    /// stay in a panel it hides. Before `init` only, since the command is
    /// registered with the app's: after, the panel is left out, with a
    /// warning.
    ///
    /// What `#[kiln::panel]` writes, by hand:
    ///
    /// ```
    /// use kiln::{
    ///     Contributions, PanelSpec,
    ///     gpui::{App, AppContext as _, Context, FocusHandle, Focusable, IntoElement, Render,
    ///         SharedString, Window, div, prelude::*},
    ///     shell::{DockItem, DockPosition, PanelId},
    ///     ui::Icon,
    /// };
    ///
    /// struct Outline(FocusHandle);
    ///
    /// impl Render for Outline {
    ///     fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
    ///         div().track_focus(&self.0)
    ///     }
    /// }
    ///
    /// impl Focusable for Outline {
    ///     fn focus_handle(&self, _: &App) -> FocusHandle {
    ///         self.0.clone()
    ///     }
    /// }
    ///
    /// impl DockItem for Outline {
    ///     fn id(&self) -> PanelId {
    ///         PanelId("outline")
    ///     }
    ///     fn title(&self, _: &App) -> SharedString {
    ///         "Outline".into()
    ///     }
    ///     fn icon(&self, _: &App) -> Icon {
    ///         Icon::Files
    ///     }
    ///     fn show_command(&self) -> Option<&'static str> {
    ///         Some("outline.toggle")
    ///     }
    /// }
    ///
    /// fn module(cx: &mut App) {
    ///     Contributions::add_panel(cx, PanelSpec {
    ///         id: PanelId("outline"),
    ///         title: "Outline".into(),
    ///         icon: Icon::Files,
    ///         position: DockPosition::Left,
    ///         order: 10,
    ///         toggle_key: "secondary-shift-o".into(),
    ///         make: |_, cx| cx.new(|cx| Outline(cx.focus_handle())).into(),
    ///     });
    /// }
    /// # let _: kiln::Module = module;
    /// ```
    pub fn add_panel(cx: &mut App, spec: PanelSpec) {
        if Self::started(cx) {
            log::warn!(
                "the panel {} was contributed after init, and is left out: contribute it before, \
                 from a kiln::Module",
                spec.id.0
            );
            return;
        }
        let toggle = match spec.toggle_id() {
            Ok(id) => Some(CommandSpec {
                id,
                title: format!("Toggle {}", spec.title),
                context: None,
                keys: spec.toggle_key.clone(),
                menu: Some(MenuSpec {
                    menu: Cow::Borrowed("View"),
                    order: spec.order,
                }),
                // Registered in the app's registry by `register_early`.
                register: |_, _| {},
            }),
            Err(error) => {
                log::warn!("the panel {} has no toggle command: {error}", spec.id.0);
                None
            }
        };
        let this = cx.default_global::<Self>();
        this.panels.retain(|added| added.id != spec.id);
        this.panels.push(spec);
        this.added += 1;
        Self::add_commands(cx, toggle);
    }

    /// Has saved items of a kind come back with the saved layout: `register`
    /// is [`kiln_shell::layout::register_item`] for the item's type. It
    /// runs once `init` has, or now after it.
    ///
    /// What `#[kiln::item]` writes, by hand:
    ///
    /// ```
    /// use kiln::{Contributions, gpui::App, shell::layout};
    /// # use kiln::{command::Value, gpui::{AppContext as _, Context, Entity, FocusHandle,
    /// #     Focusable, IntoElement, Render, SharedString, Task, Window, div}, shell::PaneItem};
    /// # struct Note(FocusHandle);
    /// # impl Render for Note {
    /// #     fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement { div() }
    /// # }
    /// # impl Focusable for Note {
    /// #     fn focus_handle(&self, _: &App) -> FocusHandle { self.0.clone() }
    /// # }
    /// # impl PaneItem for Note {
    /// #     const KIND: &'static str = "notes.note";
    /// #     fn tab_title(&self, _: &App) -> SharedString { "Note".into() }
    /// #     fn serialize(&self, _: &App) -> Option<Value> { Some(Value::Null) }
    /// #     fn restore(_: Value, _: &mut Window, cx: &mut App) -> Task<Option<Entity<Self>>> {
    /// #         Task::ready(Some(cx.new(|cx| Note(cx.focus_handle()))))
    /// #     }
    /// # }
    ///
    /// fn module(cx: &mut App) {
    ///     Contributions::add_item_kind(cx, layout::register_item::<Note>);
    /// }
    /// # let _: kiln::Module = module;
    /// ```
    pub fn add_item_kind(cx: &mut App, register: fn(&mut App)) {
        cx.default_global::<Self>().item_kinds += 1;
        Self::add_run(cx, register);
    }

    /// Shows the item `spec.item` gives on each workspace window's status
    /// bar, as [`crate::AppBuilder::status`] does, after the app's own
    /// sources, by side, then order. For windows opened from now on.
    ///
    /// What `#[kiln::status]` writes, by hand:
    ///
    /// ```
    /// use kiln::{
    ///     Contributions, StatusSpec,
    ///     gpui::App,
    ///     shell::{StatusItem, StatusSide},
    /// };
    ///
    /// fn word_count(_: &App) -> Option<StatusItem> {
    ///     Some(StatusItem::new("notes.words", StatusSide::Right, 10, "12 words"))
    /// }
    ///
    /// fn module(cx: &mut App) {
    ///     Contributions::add_status(cx, StatusSpec {
    ///         side: StatusSide::Right,
    ///         order: 10,
    ///         item: word_count,
    ///     });
    /// }
    /// # let _: kiln::Module = module;
    /// ```
    pub fn add_status(cx: &mut App, spec: StatusSpec) {
        let this = cx.default_global::<Self>();
        this.statuses.push(spec);
        this.added += 1;
    }

    /// Adds a settings store to the app's [`Config`] before `init`:
    /// `add` is a [`Config::settings`] call, say. [`crate::app()`] applies
    /// them after its own; by hand, [`Contributions::configure`] does.
    /// After `init` it's too late: it's left out, with a warning.
    ///
    /// What `#[kiln::settings]` writes, by hand:
    ///
    /// ```
    /// use kiln::{Config, Contributions, gpui::App, serde::{Deserialize, Serialize}};
    ///
    /// #[derive(Clone, Debug, Default, PartialEq, Deserialize, Serialize)]
    /// #[serde(crate = "kiln::serde", default)]
    /// struct OutlineSettings {
    ///     depth: u32,
    /// }
    ///
    /// fn module(cx: &mut App) {
    ///     Contributions::add_settings(cx, |config: Config| {
    ///         config.settings::<OutlineSettings>(None, None)
    ///     });
    /// }
    /// # let _: kiln::Module = module;
    /// ```
    pub fn add_settings(cx: &mut App, add: fn(Config) -> Config) {
        if Self::started(cx) {
            log::warn!("settings contributed after init are left out: contribute them before");
            return;
        }
        let this = cx.default_global::<Self>();
        this.settings.push(add);
        this.added += 1;
    }

    /// Sets up a model, such as a global the module's views read: `make`
    /// runs once `init` has, or now after it. `kiln::model::register::<M>`
    /// makes one of kiln's models, which a view reads with
    /// [`crate::Cx::watch`].
    ///
    /// What `#[kiln::model]` writes, by hand:
    ///
    /// ```
    /// use kiln::{Contributions, gpui::{App, Global}};
    ///
    /// #[derive(Default)]
    /// struct Recent(Vec<String>);
    ///
    /// impl Global for Recent {}
    ///
    /// fn module(cx: &mut App) {
    ///     Contributions::add_model(cx, |cx| cx.set_global(Recent::default()));
    /// }
    /// # let _: kiln::Module = module;
    /// ```
    pub fn add_model(cx: &mut App, make: fn(&mut App)) {
        Self::add_run(cx, make);
    }

    /// Runs `run` now after `init`, else once it has.
    fn add_run(cx: &mut App, run: fn(&mut App)) {
        let started = Self::started(cx);
        let this = cx.default_global::<Self>();
        this.added += 1;
        if !started {
            this.unrun.push(run);
            return;
        }
        run(cx);
    }

    /// `config` with the contributed settings stores added, for an app
    /// that calls [`crate::init`] itself: `kiln::init(cx,
    /// Contributions::configure(cx, config))`.
    pub fn configure(cx: &App, mut config: Config) -> Config {
        let adds = cx
            .try_global::<Self>()
            .map(|this| this.settings.clone())
            .unwrap_or_default();
        for add in adds {
            config = add(config);
        }
        config
    }

    /// Every panel contributed, by order, then in the order added.
    pub fn panels(cx: &App) -> Vec<PanelSpec> {
        let mut panels = cx
            .try_global::<Self>()
            .map(|this| this.panels.clone())
            .unwrap_or_default();
        panels.sort_by_key(|panel| panel.order);
        panels
    }

    /// Every status source contributed, by side, then order, then in the
    /// order added.
    pub fn statuses(cx: &App) -> Vec<StatusSpec> {
        let mut statuses = cx
            .try_global::<Self>()
            .map(|this| this.statuses.clone())
            .unwrap_or_default();
        statuses.sort_by_key(|status| (status.side, status.order));
        statuses
    }

    /// How many additions of any kind there have been: a module that adds
    /// nothing leaves it as it was.
    pub fn count(cx: &App) -> usize {
        cx.try_global::<Self>().map_or(0, |this| this.added)
    }

    /// Whether an item kind was contributed.
    pub(crate) fn has_item_kinds(cx: &App) -> bool {
        cx.try_global::<Self>()
            .is_some_and(|this| this.item_kinds > 0)
    }

    /// Whether `init` has run.
    fn started(cx: &App) -> bool {
        cx.try_global::<Self>()
            .is_some_and(|this| this.resolver.is_some())
    }

    /// Every command contributed, in the order added.
    pub fn commands(cx: &App) -> &[CommandSpec] {
        cx.try_global::<Self>()
            .map_or(&[], |this| this.commands.as_slice())
    }

    /// How many times additions after `init` have re-resolved the keymap
    /// and menus.
    pub fn resolutions(cx: &App) -> usize {
        cx.try_global::<Self>().map_or(0, |this| this.resolutions)
    }
}

/// Re-resolves once the current update is over, unless that's arranged.
fn schedule(cx: &mut App) {
    let this = cx.global_mut::<Contributions>();
    if std::mem::replace(&mut this.scheduled, true) {
        return;
    }
    cx.defer(resolve);
}

/// Registers the commands contributed before `init`: called by `init`
/// with the app's own.
pub(crate) fn register_early(cx: &mut App) {
    if !cx.has_global::<Contributions>() {
        return;
    }
    // One at a time, so one a registration contributes, or replaces, is
    // registered too, as it now is.
    loop {
        let unregistered = &mut cx.global_mut::<Contributions>().unregistered;
        if unregistered.is_empty() {
            break;
        }
        let spec = unregistered.remove(0);
        (spec.register)(cx, spec.info());
    }
    // After, so a panel a registration contributed has its toggle too.
    register_toggles(cx);
}

/// The keymap layers and menus the app ships, its contributions with
/// them, and their diagnostics.
pub(crate) struct Shipped {
    pub layers: Vec<KeymapLayer>,
    pub keymap_diagnostics: Vec<Diagnostic>,
    pub menu_bar: MenuBar,
    pub menu_diagnostics: Vec<Diagnostic>,
}

impl Shipped {
    pub(crate) fn diagnostics(&self) -> Vec<Diagnostic> {
        [self.keymap_diagnostics.as_slice(), &self.menu_diagnostics].concat()
    }
}

/// The catalog the app ships its keymap and menus checked against:
/// [`crate::catalog`], then the app's own.
pub(crate) fn shipped_catalog(config: &Config, cx: &App) -> Rc<dyn CommandCatalog> {
    let mut catalogs = vec![crate::catalog(cx)];
    catalogs.extend(config.catalogs.iter().cloned());
    Rc::new(Catalogs(catalogs))
}

/// What `config` ships on `os`, with what's contributed, checked against
/// `catalog`.
pub(crate) fn shipped(config: &Config, os: Os, catalog: &dyn CommandCatalog, cx: &App) -> Shipped {
    let commands = Contributions::commands(cx);
    let (contributed, mut keymap_diagnostics) = keymap_layer(commands, os, catalog);
    let (layers, found) = config.own_keymap(os, Some(catalog), Some(contributed));
    keymap_diagnostics.extend(found);
    let (mut menu_bar, mut menu_diagnostics) = config.menu_bar_for(os, Some(catalog));
    let found = place_items(
        &mut menu_bar,
        commands,
        catalog,
        &config.menu_shortcut_context,
    );
    menu_diagnostics.extend(found);
    Shipped {
        layers,
        keymap_diagnostics,
        menu_bar,
        menu_diagnostics,
    }
}

/// Has additions re-resolve from now on: called by `init` once it has
/// installed what it shipped.
pub(crate) fn start(
    cx: &mut App,
    config: Config,
    os: Os,
    keymap: SharedKeymap,
    menu_diagnostics: Vec<Diagnostic>,
) {
    let this = cx.default_global::<Contributions>();
    this.resolver = Some(Resolver {
        config,
        os,
        keymap,
        menu_diagnostics,
    });
    this.scheduled = false;
    // One at a time, so one that contributes another runs it now.
    loop {
        let unrun = &mut cx.global_mut::<Contributions>().unrun;
        if unrun.is_empty() {
            return;
        }
        let run = unrun.remove(0);
        run(cx);
    }
}

/// Registers each contributed panel's toggle command in the app's
/// registry, where the palette lists it: before the keymap and menus are
/// checked against it, which share the registry from then on.
fn register_toggles(cx: &mut App) {
    let panels = cx.global::<Contributions>().panels.clone();
    register_app_commands(cx, |registry| {
        for panel in panels {
            let Ok(id) = panel.toggle_id() else {
                continue;
            };
            let (position, panel_id) = (panel.position, panel.id);
            let title = format!("Toggle {}", panel.title);
            let registered = registry.register(id.as_str(), &title, move |cx: &mut &mut App, _| {
                toggle_panel(cx, position, panel_id);
                Ok(Dispatched::new(Effect::Nothing))
            });
            match registered {
                Ok(_) | Err(RegisterError::Duplicate(_)) => {}
                Err(error) => log::warn!("the panel {} has no toggle command: {error}", panel_id.0),
            }
        }
    });
}

/// `<id>.toggle`, in the window it was dispatched in, else the active one,
/// once the dispatch is over (the window is out for it): shows the dock at
/// `position` with the panel `id`, or toggles the dock if that's the panel
/// it shows.
fn toggle_panel(cx: &mut App, position: DockPosition, id: PanelId) {
    let Some(handle) = dispatch_window(cx).or_else(|| cx.active_window()) else {
        return;
    };
    cx.defer(move |cx| {
        handle
            .update(cx, |root, window, cx| {
                let Ok(workspace) = root.downcast::<Workspace>() else {
                    return;
                };
                workspace.update(cx, |workspace, cx| {
                    let Some(dock) = workspace.dock(position).cloned() else {
                        return;
                    };
                    if dock.read(cx).active() == Some(id) {
                        workspace.toggle_dock(position, window, cx);
                    } else {
                        dock.update(cx, |dock, cx| dock.show(id, window, cx));
                    }
                });
            })
            .ok();
    });
}

/// Resolves the keymap and menus again, with every command registered and
/// contributed now, and installs them.
fn resolve(cx: &mut App) {
    let this = cx.global_mut::<Contributions>();
    this.scheduled = false;
    let Some(resolver) = this.resolver.take() else {
        return;
    };
    let catalog = shipped_catalog(&resolver.config, cx);
    let shipped = shipped(&resolver.config, resolver.os, &*catalog, cx);
    resolver.config.check_fail_fast(&shipped.diagnostics());
    let Shipped {
        layers,
        keymap_diagnostics,
        menu_bar,
        menu_diagnostics,
    } = shipped;
    resolver.keymap.update(cx, |store| {
        store.set_defaults(layers, keymap_diagnostics, Some(Box::new(catalog)))
    });
    menus::install(cx, menu_bar);
    if menu_diagnostics != resolver.menu_diagnostics {
        let target = kiln_platform::config::log_target(cx);
        kiln_platform::config::log_diagnostics_as(target, &menu_diagnostics);
    }
    let this = cx.global_mut::<Contributions>();
    this.resolver = Some(Resolver {
        menu_diagnostics,
        ..resolver
    });
    this.resolutions += 1;
}

/// The contributed commands' default keys on `os`, as a keymap layer, and
/// its diagnostics. Each command's keys are read as a keymap file's
/// binding is, with the same checks (every platform's keys, clicks, the
/// command and its args against `catalog`), and its diagnostics name the
/// command. So are the same keys in the same context for two commands,
/// the later one winning.
fn keymap_layer(
    commands: &[CommandSpec],
    os: Os,
    catalog: &dyn CommandCatalog,
) -> (KeymapLayer, Vec<Diagnostic>) {
    let path = PathBuf::from(CONTRIBUTED_KEYS_PATH);
    let mut entries: Vec<Entry> = Vec::new();
    let mut diagnostics = Vec::new();
    for (ix, command) in commands.iter().enumerate() {
        let Some(keys) = command.keys.toml() else {
            continue;
        };
        let mut source = format!(
            "[[bindings]]\ncommand = \"{}\"\nkeys = {keys}\n",
            toml_escape(command.id.as_str())
        );
        if let Some(context) = command.context {
            source.push_str(&format!("context = \"{}\"\n", toml_escape(context)));
        }
        let (layer, found) = KeymapLayer::parse_for(&path, &source, Some(catalog), os);
        // Named for the command, the `ix`th contributed, not the binding
        // written for it.
        let line = ix + 1;
        diagnostics.extend(found.into_iter().map(|diagnostic| Diagnostic {
            key: Some(match diagnostic.key {
                Some(key) => key.replacen("bindings[0]", command.id.as_str(), 1),
                None => command.id.to_string(),
            }),
            line,
            column: 1,
            ..diagnostic
        }));
        for mut entry in layer.into_iter().flat_map(|layer| layer.entries) {
            let same = entries
                .iter()
                .find(|other| other.keys == entry.keys && other.context == entry.context);
            if let Some(other) = same.and_then(|other| other.command.as_ref()) {
                diagnostics.push(Diagnostic {
                    severity: Severity::Warning,
                    file: path.clone(),
                    key: Some(format!("{}.keys", command.id)),
                    line,
                    column: 1,
                    message: format!("{} is bound to both {other} and {}", entry.keys, command.id),
                });
            }
            entry.location = Location {
                file: path.clone(),
                line,
                column: 1,
            };
            entries.push(entry);
        }
    }
    (KeymapLayer { path, entries }, diagnostics)
}

/// Puts each contributed command's item in its menu: in place of an item
/// of the same ID, or after the menu's others, by order. A menu the bar
/// lacks is added at its end. Each item's shortcut resolves in `frames`,
/// then the command's context if it names one frame. An item whose command
/// `catalog` doesn't know, or won't run without args, is left out, with a
/// warning, as a menu table's is.
fn place_items(
    bar: &mut MenuBar,
    commands: &[CommandSpec],
    catalog: &dyn CommandCatalog,
    frames: &[String],
) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    let mut placed: Vec<(&CommandSpec, &MenuSpec)> = Vec::new();
    for (ix, command) in commands.iter().enumerate() {
        let Some(spec) = &command.menu else {
            continue;
        };
        let message = match catalog.check(&command.id, &Value::Null) {
            Ok(()) => {
                placed.push((command, spec));
                continue;
            }
            Err(CatalogError::UnknownCommand) => {
                format!("unknown command {}; the item is left out", command.id)
            }
            Err(CatalogError::BadArgs(message)) => {
                format!(
                    "bad args for {}: {message}; the item is left out",
                    command.id
                )
            }
        };
        diagnostics.push(Diagnostic {
            severity: Severity::Warning,
            file: PathBuf::from(CONTRIBUTED_MENUS_PATH),
            key: Some(format!("{}.menu", command.id)),
            line: ix + 1,
            column: 1,
            message,
        });
    }
    // By order within each menu, menus in the order first named.
    let mut names: Vec<&str> = Vec::new();
    for (_, spec) in &placed {
        if !names.contains(&&*spec.menu) {
            names.push(&spec.menu);
        }
    }
    let rank = |spec: &MenuSpec| names.iter().position(|name| *name == spec.menu);
    placed.sort_by_key(|(_, spec)| (rank(spec), spec.order));
    for (command, spec) in placed {
        let menu = match bar.menus.iter().position(|menu| menu.name == spec.menu) {
            Some(ix) => &mut bar.menus[ix],
            None => {
                bar.menus.push(Menu {
                    name: spec.menu.to_string(),
                    items: Vec::new(),
                });
                bar.menus.last_mut().expect("just pushed")
            }
        };
        let mut shortcut_context: ContextStack = frames
            .iter()
            .map(|frame| ContextFrame::new(frame.as_str()))
            .collect();
        if let Some(frame) = command.context.and_then(frame_name)
            && !frames.contains(&frame)
        {
            shortcut_context = shortcut_context
                .frames()
                .iter()
                .cloned()
                .chain([ContextFrame::new(frame)])
                .collect();
        }
        let id = command.id.to_string();
        let item = MenuItem {
            kind: ItemKind::Command(CommandItem {
                id: id.clone(),
                title: command.title.clone(),
                command: command.id.clone(),
                args: Value::Null,
                os_action: None,
                alternatives: Vec::new(),
                shortcut_context,
            }),
            place: None,
        };
        match menu.items.iter().position(|item| item.id() == Some(&id)) {
            Some(ix) => menu.items[ix] = item,
            None => menu.items.push(item),
        }
    }
    diagnostics
}

/// The frame name a context predicate is, if it's one identifier
/// (`notes_list`, `notes.list`, `(notes)`).
fn frame_name(context: &str) -> Option<String> {
    match ContextPredicate::parse(context).ok()? {
        ContextPredicate::Identifier(name) => Some(name),
        _ => None,
    }
}
