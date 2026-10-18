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
//!
//! `#[kiln::command]` records its command as a [`CommandRecord`],
//! `#[kiln::panel]` its panel as a [`PanelRecord`], `#[kiln::status]`
//! its source as a [`StatusRecord`], `#[kiln::item]` its kind as an
//! [`ItemRecord`] and `#[kiln::settings]` its section as a
//! [`SettingsRecord`], which [`Contributions::add_recorded`]
//! adds when the record was compiled into the app's own target (a binary,
//! an example or an integration test), and the crate's `kiln::module!`
//! when it was compiled into a library.

use std::{any::TypeId, borrow::Cow, collections::HashSet, path::PathBuf, rc::Rc};

use gpui::{App, Context, Entity, Global, SharedString, Window};
use kiln_command::{
    CommandId, ContextFrame, ContextPredicate, ContextStack, Dispatched, Effect, Invocation,
    RegisterError, Value,
};
use kiln_core::{
    CatalogError, CommandCatalog, CommandItem, Diagnostic, Entry, ItemKind, KeymapLayer, Location,
    Menu, MenuBar, MenuItem, Os, Severity,
};
use kiln_platform::{
    command_actions::{
        Command, CommandInfo, current_invocation, dispatch_notifies, dispatch_window,
        register_command, set_dispatch, typed_command,
    },
    commands::{notify_observers, register_app_commands},
    config::SharedKeymap,
    menus,
};
use kiln_shell::{
    DockItem, DockPosition, DockView as _, PanelId, StatusItem, StatusSide, Workspace,
};
use kiln_ui::Icon;

use crate::{
    Cx, SettingsSection, extensions,
    setup::{Catalogs, Config, toml_escape},
    tables::{KeyBinding, KeyTable, MenuTable, TableItem},
};

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
/// app's table gives it, in `order` among the contributed ones, then by
/// where it was `declared`, those declared nowhere first, then in the order
/// added.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MenuSpec {
    pub menu: Cow<'static, str>,
    pub order: i32,
    /// The file and line `#[kiln::command]` declared the command at, so
    /// its items go in source order whichever crate or module added them;
    /// `None` by hand.
    pub declared: Option<(&'static str, u32)>,
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
    /// Whether the command palette leaves it out: `palette = false`.
    pub unlisted: bool,
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
            unlisted: false,
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
            declared: None,
        });
        self
    }

    fn info(&self) -> CommandInfo {
        CommandInfo {
            title: self.title.clone(),
            context: self.context,
            unlisted: self.unlisted,
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

/// A command `#[kiln::command]` declared, as it records it with
/// `inventory`: what [`CommandSpec`] has, where it was declared, and which
/// crate and target it was compiled into.
#[doc(hidden)]
#[derive(Clone, Copy, Debug)]
pub struct CommandRecord {
    pub id: &'static str,
    pub title: &'static str,
    pub context: Option<&'static str>,
    pub keys: Option<&'static str>,
    pub menu: Option<&'static str>,
    pub order: i32,
    /// `false` for `palette = false`: the palette leaves it out.
    pub palette: bool,
    pub file: &'static str,
    pub line: u32,
    /// `CARGO_CRATE_NAME`.
    pub krate: &'static str,
    /// `CARGO_BIN_NAME`: set for a binary or an example.
    pub bin: Option<&'static str>,
    /// `CARGO_TARGET_TMPDIR`: set for an integration test.
    pub test_dir: Option<&'static str>,
    /// Registers the command, and its handler (see [`register_recorded`]).
    pub register: RegisterCommand,
    pub run: RunRecorded,
}

inventory::collect!(CommandRecord);

/// A `#[kiln::command]` fn: the handler, run on the workspace of the
/// window the command was dispatched in.
#[doc(hidden)]
pub type RunRecorded = fn(&mut Workspace, &mut Cx<'_, '_, Workspace>);

impl CommandRecord {
    fn spec(&self) -> CommandSpec {
        let id = CommandId::parse(self.id).unwrap_or_else(|error| {
            panic!(
                "#[kiln::command] at {}:{}: {error}; give it an ID with `id = \"..\"`",
                self.file, self.line
            )
        });
        CommandSpec {
            id,
            title: self.title.to_owned(),
            context: self.context,
            keys: self.keys.map_or(Keys::None, Keys::from),
            menu: self.menu.map(|menu| MenuSpec {
                menu: Cow::Borrowed(menu),
                order: self.order,
                declared: Some((self.file, self.line)),
            }),
            unlisted: !self.palette,
            register: self.register,
        }
    }
}

/// A panel `#[kiln::panel]` declared, as it records it with `inventory`:
/// what [`PanelSpec`] has, and where it was declared, as a
/// [`CommandRecord`] has it.
#[doc(hidden)]
#[derive(Clone, Copy, Debug)]
pub struct PanelRecord {
    pub id: &'static str,
    pub title: &'static str,
    pub icon: Icon,
    pub position: DockPosition,
    pub order: i32,
    pub toggle_key: Option<&'static str>,
    pub make: MakePanel,
    pub file: &'static str,
    pub line: u32,
    pub krate: &'static str,
    pub bin: Option<&'static str>,
    pub test_dir: Option<&'static str>,
}

inventory::collect!(PanelRecord);

impl PanelRecord {
    fn spec(&self) -> PanelSpec {
        PanelSpec {
            id: PanelId(self.id),
            title: self.title.into(),
            icon: self.icon,
            position: self.position,
            order: self.order,
            toggle_key: self.toggle_key.map_or(Keys::None, Keys::from),
            make: self.make,
        }
    }
}

/// A status source `#[kiln::status]` declared, as it records it with
/// `inventory`: what [`StatusSpec`] has, its item's ID, and where it was
/// declared.
#[doc(hidden)]
#[derive(Clone, Copy, Debug)]
pub struct StatusRecord {
    pub id: &'static str,
    pub side: StatusSide,
    pub order: i32,
    pub item: fn(&App) -> Option<StatusItem>,
    pub file: &'static str,
    pub line: u32,
    pub krate: &'static str,
    pub bin: Option<&'static str>,
    pub test_dir: Option<&'static str>,
}

inventory::collect!(StatusRecord);

impl StatusRecord {
    fn spec(&self) -> StatusSpec {
        StatusSpec {
            side: self.side,
            order: self.order,
            item: self.item,
        }
    }
}

/// An item kind `#[kiln::item]` declared, as it records it with
/// `inventory`: its kind, what [`Contributions::add_item_kind`] takes, and
/// where it was declared.
#[doc(hidden)]
#[derive(Clone, Copy, Debug)]
pub struct ItemRecord {
    pub kind: &'static str,
    pub register: fn(&mut App),
    pub file: &'static str,
    pub line: u32,
    pub krate: &'static str,
    pub bin: Option<&'static str>,
    pub test_dir: Option<&'static str>,
}

inventory::collect!(ItemRecord);

/// A settings section `#[kiln::settings]` declared, as it records it with
/// `inventory`: its table's name, what [`Contributions::add_settings`]
/// takes, and where it was declared.
#[doc(hidden)]
#[derive(Clone, Copy, Debug)]
pub struct SettingsRecord {
    pub section: &'static str,
    pub add: fn(Config) -> Config,
    pub file: &'static str,
    pub line: u32,
    pub krate: &'static str,
    pub bin: Option<&'static str>,
    pub test_dir: Option<&'static str>,
}

inventory::collect!(SettingsRecord);

/// `config` with the section `S`: what a [`SettingsRecord`] adds.
#[doc(hidden)]
pub fn add_section<S: SettingsSection>(config: Config) -> Config {
    config.section::<S>()
}

/// A model `#[kiln::model]` declared, as it records it with `inventory`:
/// its type's name, what [`Contributions::add_model`] takes (the model
/// registered, or persisted), and where it was declared.
#[doc(hidden)]
#[derive(Clone, Copy, Debug)]
pub struct ModelRecord {
    pub name: &'static str,
    pub add: fn(&mut App),
    pub file: &'static str,
    pub line: u32,
    pub krate: &'static str,
    pub bin: Option<&'static str>,
    pub test_dir: Option<&'static str>,
}

inventory::collect!(ModelRecord);

/// A record one of kiln's macros submitted: what it's called, where it
/// was declared, and which crate and target it was compiled into.
trait Recorded {
    /// The attribute that recorded it, as a warning names it in the
    /// plural: `#[kiln::command]s`.
    const ATTRIBUTE: &'static str;

    fn id(&self) -> &'static str;
    fn site(&self) -> (&'static str, u32);
    /// `CARGO_CRATE_NAME`.
    fn krate(&self) -> &'static str;
    /// Whether it was compiled into the app's own target, a binary, an
    /// example or an integration test (`CARGO_BIN_NAME` or
    /// `CARGO_TARGET_TMPDIR` set), rather than a library.
    fn in_app(&self) -> bool;
}

macro_rules! recorded {
    ($record:ty, $attribute:literal) => {
        recorded!($record, $attribute, id);
    };
    ($record:ty, $attribute:literal, $id:ident) => {
        impl Recorded for $record {
            const ATTRIBUTE: &'static str = $attribute;

            fn id(&self) -> &'static str {
                self.$id
            }

            fn site(&self) -> (&'static str, u32) {
                (self.file, self.line)
            }

            fn krate(&self) -> &'static str {
                self.krate
            }

            fn in_app(&self) -> bool {
                self.bin.is_some() || self.test_dir.is_some()
            }
        }
    };
}

recorded!(CommandRecord, "#[kiln::command]s");
recorded!(PanelRecord, "#[kiln::panel]s");
recorded!(StatusRecord, "#[kiln::status]es");
recorded!(ItemRecord, "#[kiln::item]s", kind);
recorded!(SettingsRecord, "#[kiln::settings]", section);
recorded!(ModelRecord, "#[kiln::model]s", name);

/// Every record of a kind linked in.
fn linked<R: inventory::Collect>() -> Vec<&'static R> {
    inventory::iter::<R>.into_iter().collect()
}

/// The records compiled into the app's own targets, by file, then line:
/// the order their menu items take within an `order`, and their panels
/// and status sources within theirs.
fn app_records<'a, R: Recorded>(records: impl IntoIterator<Item = &'a R>) -> Vec<&'a R> {
    sorted(records.into_iter().filter(|record| record.in_app()))
}

/// The records compiled into the library crate `krate`, by file, then
/// line.
fn crate_records<'a, R: Recorded>(
    records: impl IntoIterator<Item = &'a R>,
    krate: &str,
) -> Vec<&'a R> {
    sorted((records.into_iter()).filter(|record| !record.in_app() && record.krate() == krate))
}

fn sorted<'a, R: Recorded>(records: impl Iterator<Item = &'a R>) -> Vec<&'a R> {
    let mut records: Vec<_> = records.collect();
    records.sort_by_key(|record| record.site());
    records
}

/// What a library crate left out records: the attributes, and the IDs.
#[derive(Default)]
struct LeftOut {
    attributes: Vec<&'static str>,
    ids: Vec<&'static str>,
}

impl LeftOut {
    /// Adds `records`' library crates not in `claimed` to `crates`.
    fn add<'a, R: Recorded + 'a>(
        crates: &mut Vec<(&'static str, LeftOut)>,
        records: impl IntoIterator<Item = &'a R>,
        claimed: &[&str],
    ) {
        for record in sorted(records.into_iter()) {
            let krate = record.krate();
            if record.in_app() || claimed.contains(&krate) {
                continue;
            }
            let ix = match crates.iter().position(|(name, _)| *name == krate) {
                Some(ix) => ix,
                None => {
                    crates.push((krate, LeftOut::default()));
                    crates.len() - 1
                }
            };
            let left = &mut crates[ix].1;
            if !left.attributes.contains(&R::ATTRIBUTE) {
                left.attributes.push(R::ATTRIBUTE);
            }
            left.ids.push(record.id());
        }
    }
}

/// A warning for each library crate in `crates` whose records are linked
/// in but no `kiln::module!` claimed, naming the crate and its records.
fn warnings(mut crates: Vec<(&'static str, LeftOut)>) -> Vec<String> {
    crates.sort_by_key(|(krate, _)| *krate);
    (crates.into_iter())
        .map(|(krate, left)| {
            format!(
                "the crate {krate}'s {} ({}) are left out: no kiln::module! \
                 claimed them. Add `kiln::module!(pub fn module);` to {krate} and pass \
                 `.module({krate}::module)` to kiln::app",
                left.attributes.join(" and "),
                left.ids.join(", ")
            )
        })
        .collect()
}

/// [`warnings`] of commands alone.
#[cfg(test)]
fn unclaimed<'a>(
    records: impl IntoIterator<Item = &'a CommandRecord>,
    claimed: &[&str],
) -> Vec<String> {
    let mut crates = Vec::new();
    LeftOut::add(&mut crates, records, claimed);
    warnings(crates)
}

/// Registers `A`, the type `#[kiln::command]` made, with `info`, and its
/// handler `run` ([`workspace_handler`]), once however often it's
/// registered: GPUI keeps every listener, so a second would run it twice.
#[doc(hidden)]
pub fn register_recorded<A: Command>(cx: &mut App, info: CommandInfo, run: RunRecorded) {
    register_command::<A>(cx, info);
    let handled = &mut cx.default_global::<Contributions>().handled;
    if handled.insert(TypeId::of::<A>()) {
        cx.on_action(workspace_handler::<A>(run));
    }
}

/// An app-wide handler of the command `A` (`cx.on_action(..)`) that runs
/// `run` on the workspace of the window `A` was dispatched in, else the
/// active one, once the dispatch is over (GPUI has the window out for it),
/// as `#[kiln::command]` does. Within `run`, [`current_invocation`] and
/// [`dispatch_window`] give the invocation and window as in a handler run
/// during the dispatch; and the invocation observers
/// ([`kiln_platform::commands::observe_invocations`]) hear of it once,
/// after `run`, unless its dispatch tells them
/// ([`dispatch_notifies`]): [`dispatch_invocation`](crate::dispatch_invocation)
/// does when a view on the focus path handles `A` too, as it found before
/// dispatching, whatever focus does before `run`.
pub fn workspace_handler<A: Command>(
    run: fn(&mut Workspace, &mut Cx<'_, '_, Workspace>),
) -> impl Fn(&A, &mut App) + 'static {
    move |_: &A, cx: &mut App| {
        let id = A::id();
        let invocation = current_invocation(cx).filter(|invocation| invocation.command == id);
        // Read now, during the dispatch, from what the dispatch decided.
        let told = invocation.is_some() && dispatch_notifies(cx);
        on_workspace(cx, move |workspace, window, cx| {
            let invocation = invocation.unwrap_or_else(|| Invocation::new(id));
            let previous = set_dispatch(Some(invocation.clone()), Some(window.window_handle()), cx);
            run(workspace, &mut Cx { window, cx });
            set_dispatch(previous.0, previous.1, cx);
            if !told {
                notify_observers(&invocation, cx);
            }
        });
    }
}

/// A catalog of the `#[kiln::command]`s the crate `krate`
/// (`CARGO_CRATE_NAME`) declared, as `inventory` recorded them, needing no
/// app: for checking a library's keymaps and menus against its commands
/// without starting one. It knows their IDs, and accepts them with no args
/// (`Value::Null`), as their unit actions take none.
pub fn recorded_catalog(krate: &'static str) -> Box<dyn CommandCatalog> {
    let ids = (linked::<CommandRecord>().into_iter())
        .filter(|record| record.krate == krate)
        .filter_map(|record| CommandId::parse(record.id).ok())
        .collect();
    Box::new(RecordedCatalog(ids))
}

/// [`recorded_catalog`]'s commands.
struct RecordedCatalog(HashSet<CommandId>);

impl CommandCatalog for RecordedCatalog {
    fn check(&self, command: &CommandId, args: &Value) -> Result<(), CatalogError> {
        if !self.0.contains(command) {
            return Err(CatalogError::UnknownCommand);
        }
        match args {
            Value::Null => Ok(()),
            _ => Err(CatalogError::BadArgs(format!("{command} takes no args"))),
        }
    }
}

/// Adds the records the library crate `krate` linked in, as its
/// `kiln::module!` does, and has it claimed: see [`unclaimed`].
#[doc(hidden)]
pub fn add_crate_commands(cx: &mut App, krate: &'static str) {
    let claimed = &mut cx.default_global::<Contributions>().claimed;
    if !claimed.contains(&krate) {
        claimed.push(krate);
    }
    let specs: Vec<CommandSpec> = (crate_records(linked(), krate).into_iter())
        .map(CommandRecord::spec)
        .collect();
    Contributions::add_commands(cx, specs);
    add_panels_and_statuses(
        cx,
        crate_records(linked(), krate),
        crate_records(linked(), krate),
    );
    add_items(cx, crate_records(linked(), krate));
    add_sections(cx, crate_records(linked(), krate));
    add_models(cx, crate_records(linked(), krate));
}

/// Adds the models of `models`.
fn add_models(cx: &mut App, models: Vec<&ModelRecord>) {
    for model in models {
        Contributions::add_model(cx, model.add);
    }
}

/// Adds the settings sections of `sections`.
fn add_sections(cx: &mut App, sections: Vec<&SettingsRecord>) {
    for section in sections {
        Contributions::add_settings(cx, section.add);
    }
}

/// Adds the item kinds of `items`, so their saved items restore.
fn add_items(cx: &mut App, items: Vec<&ItemRecord>) {
    for item in items {
        Contributions::add_item_kind(cx, item.register);
    }
}

/// Adds `panels` and `statuses`, each placed by where it was declared
/// among those of its `order`, whatever adds them.
fn add_panels_and_statuses(cx: &mut App, panels: Vec<&PanelRecord>, statuses: Vec<&StatusRecord>) {
    for panel in panels {
        Contributions::add_panel_declared(cx, panel.spec(), Some(panel.site()));
    }
    for status in statuses {
        Contributions::add_status_declared(cx, status.spec(), Some(status.site()));
    }
}

/// Where a record was declared, file then line, for those placed by it;
/// `None` for one added by hand, which goes before them in its `order`, as
/// [`MenuSpec::declared`] does.
type Declared = Option<(&'static str, u32)>;

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
    /// ones go in the order added, then `#[kiln::panel]`s by file and line.
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
/// then `order`, then the order added (`#[kiln::status]`es after, by file
/// and line), which places items whose side and priority tie.
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
    panels: Vec<(PanelSpec, Declared)>,
    statuses: Vec<(StatusSpec, Declared)>,
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
    /// The `#[kiln::command]` types whose handler is set: GPUI keeps every
    /// listener, so a second would run it twice.
    handled: HashSet<TypeId>,
    /// The library crates whose records a `kiln::module!` added.
    claimed: Vec<&'static str>,
    /// The app's typed keys, in place of the commands' own.
    key_table: Option<KeyTable>,
    /// The app's typed menu bar, in place of the table and contributed
    /// items.
    menu_table: Option<MenuTable>,
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
    /// the app's; but if kiln-platform's commands are set up already
    /// ([`kiln_platform::commands::init`]), as in a test app that never
    /// runs `init`, it's registered now, and `init` doesn't register it
    /// again. After `init`, it's registered now, and the keymap and menus
    /// are resolved again once the current update is over.
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

    /// Adds every `#[kiln::command]` compiled into the app's own target (a
    /// binary, an example or an integration test), by file, then line, as
    /// [`Contributions::add_commands`] does, then its `#[kiln::panel]`s and
    /// `#[kiln::status]`es the same way ([`Contributions::add_panel`],
    /// [`Contributions::add_status`]), so that those tying on `order` go by
    /// file and line, its `#[kiln::item]`s' kinds
    /// ([`Contributions::add_item_kind`]) and its `#[kiln::settings]`
    /// sections ([`Contributions::add_settings`]) and its `#[kiln::model]`s
    /// ([`Contributions::add_model`]): [`crate::app()`] calls it first
    /// when it starts. A library crate's are left to its `kiln::module!`.
    pub fn add_recorded(cx: &mut App) {
        let specs: Vec<CommandSpec> = (app_records(linked()).into_iter())
            .map(CommandRecord::spec)
            .collect();
        Self::add_commands(cx, specs);
        add_panels_and_statuses(cx, app_records(linked()), app_records(linked()));
        add_items(cx, app_records(linked()));
        add_sections(cx, app_records(linked()));
        add_models(cx, app_records(linked()));
    }

    /// Warns of each library crate whose `#[kiln::command]`s,
    /// `#[kiln::panel]`s, `#[kiln::status]`es, `#[kiln::item]`s or
    /// `#[kiln::settings]` are
    /// linked in but no `kiln::module!` added, naming it: in a debug
    /// build, [`crate::app()`] calls it once its modules have run.
    pub fn warn_unclaimed(cx: &App) {
        let claimed = cx
            .try_global::<Self>()
            .map(|this| this.claimed.clone())
            .unwrap_or_default();
        let mut crates = Vec::new();
        LeftOut::add(&mut crates, linked::<CommandRecord>(), &claimed);
        LeftOut::add(&mut crates, linked::<PanelRecord>(), &claimed);
        LeftOut::add(&mut crates, linked::<StatusRecord>(), &claimed);
        LeftOut::add(&mut crates, linked::<ItemRecord>(), &claimed);
        LeftOut::add(&mut crates, linked::<SettingsRecord>(), &claimed);
        LeftOut::add(&mut crates, linked::<ModelRecord>(), &claimed);
        for warning in warnings(crates) {
            log::warn!("{warning}");
        }
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
        // An app whose commands can run without `init`, such as a test
        // app that sets up kiln-platform's commands alone, has them
        // registered now, and `init` doesn't register them again.
        let register_now = started || kiln_platform::commands::initialized(cx);
        for spec in specs {
            if register_now {
                (spec.register)(cx, spec.info());
            }
            let this = cx.default_global::<Self>();
            this.commands.retain(|added| added.id != spec.id);
            if register_now {
                this.unregistered.retain(|added| added.id != spec.id);
            } else {
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

    /// Puts `table` in place of the contributed commands' default keys:
    /// [`crate::AppBuilder::keys`] calls it. The keymap is resolved again
    /// once the current update is over if `init` has run.
    pub fn set_key_table(cx: &mut App, table: KeyTable) {
        cx.default_global::<Self>().key_table = Some(table);
        Self::table_changed(cx);
    }

    /// Puts `table` in place of the menu table and the contributed items:
    /// [`crate::AppBuilder::menu_bar`] calls it.
    pub fn set_menu_table(cx: &mut App, table: MenuTable) {
        cx.default_global::<Self>().menu_table = Some(table);
        Self::table_changed(cx);
    }

    fn table_changed(cx: &mut App) {
        if cx.global::<Self>().resolver.is_some() {
            schedule(cx);
        }
    }

    /// Adds `spec`'s panel, replacing one contributed before with its ID,
    /// and its toggle command, `<id>.toggle` titled `Toggle <title>`, on
    /// `toggle_key` and in the View menu, by `order`. [`crate::app()`] adds
    /// the panels to each workspace window, by `order`, then in the order
    /// added, `#[kiln::panel]`s after by file and line (their View menu
    /// items too, among the commands' declared there), after its own
    /// [`crate::AppBuilder::panel`]s. The command
    /// hides the dock if it shows the panel, through
    /// [`Workspace::toggle_dock`], else shows the dock with it, as a click
    /// on its rail button does (focusing it if it's
    /// [`DockItem::focus_on_show`], and joining it to the rail): either
    /// way, focus doesn't stay in a panel it hides. Before `init` only,
    /// since the command is registered with the app's: after, the panel is
    /// left out, with a warning.
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
        Self::add_panel_declared(cx, spec, None);
    }

    /// [`Contributions::add_panel`], the panel and its View menu item
    /// placed by `declared` among those of its order.
    fn add_panel_declared(cx: &mut App, spec: PanelSpec, declared: Declared) {
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
                    declared,
                }),
                unlisted: false,
                // Registered in the app's registry by `register_early`.
                register: |_, _| {},
            }),
            Err(error) => {
                log::warn!("the panel {} has no toggle command: {error}", spec.id.0);
                None
            }
        };
        let this = cx.default_global::<Self>();
        this.panels.retain(|(added, _)| added.id != spec.id);
        this.panels.push((spec, declared));
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
        Self::add_status_declared(cx, spec, None);
    }

    /// [`Contributions::add_status`], placed by `declared` among those of
    /// its side and order.
    fn add_status_declared(cx: &mut App, spec: StatusSpec, declared: Declared) {
        let this = cx.default_global::<Self>();
        this.statuses.push((spec, declared));
        this.added += 1;
    }

    /// Adds a settings store to the app's [`Config`] before `init`:
    /// `add` is a [`Config::settings`] call, say. [`crate::app()`] applies
    /// them after its own; by hand, [`Contributions::configure`] does.
    /// After `init` it's too late: it's left out, with a warning.
    ///
    /// What `#[kiln::settings]` writes, by hand (see [`crate::settings`]):
    ///
    /// ```
    /// use kiln::{
    ///     Config, Contributions, SettingsSection,
    ///     gpui::App,
    ///     schemars::JsonSchema,
    ///     serde::{Deserialize, Serialize},
    /// };
    ///
    /// #[derive(Clone, Debug, Default, PartialEq, Deserialize, Serialize, JsonSchema)]
    /// #[serde(crate = "kiln::serde", default)]
    /// #[schemars(crate = "kiln::schemars")]
    /// struct OutlineSettings {
    ///     depth: u32,
    /// }
    ///
    /// impl SettingsSection for OutlineSettings {
    ///     const SECTION: &'static str = "outline";
    /// }
    ///
    /// fn module(cx: &mut App) {
    ///     Contributions::add_settings(cx, |config: Config| {
    ///         config.section::<OutlineSettings>()
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

    /// Every panel contributed, by order, then those added by hand in the
    /// order added, then `#[kiln::panel]`s by file and line, whatever
    /// module added them.
    pub fn panels(cx: &App) -> Vec<PanelSpec> {
        let mut panels = cx
            .try_global::<Self>()
            .map(|this| this.panels.clone())
            .unwrap_or_default();
        panels.sort_by_key(|(panel, declared)| (panel.order, *declared));
        panels.into_iter().map(|(panel, _)| panel).collect()
    }

    /// Every status source contributed, by side, then order, then those
    /// added by hand in the order added, then `#[kiln::status]`es by file
    /// and line, whatever module added them.
    pub fn statuses(cx: &App) -> Vec<StatusSpec> {
        let mut statuses = cx
            .try_global::<Self>()
            .map(|this| this.statuses.clone())
            .unwrap_or_default();
        statuses.sort_by_key(|(status, declared)| (status.side, status.order, *declared));
        statuses.into_iter().map(|(status, _)| status).collect()
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
/// them, and their diagnostics; then the extensions' layers laid over
/// them, whose diagnostics are kept apart.
pub(crate) struct Shipped {
    pub layers: Vec<KeymapLayer>,
    pub keymap_diagnostics: Vec<Diagnostic>,
    pub menu_bar: MenuBar,
    pub menu_diagnostics: Vec<Diagnostic>,
    /// The extensions' keymap and menu files' problems: theirs, not the
    /// app's, so [`Config::fail_fast`] leaves them be.
    pub extension_diagnostics: Vec<Diagnostic>,
}

impl Shipped {
    /// The diagnostics of what the app ships, not the extensions'.
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
/// `catalog`, then the extensions' keymap and menu layers over it
/// ([`crate::extensions`]).
pub(crate) fn shipped(config: &Config, os: Os, catalog: &dyn CommandCatalog, cx: &App) -> Shipped {
    let commands = Contributions::commands(cx);
    let this = cx.try_global::<Contributions>();
    let bindings: Vec<KeyBinding> = match this.and_then(|this| this.key_table.as_ref()) {
        Some(table) => table.bindings.clone(),
        None => (commands.iter())
            .map(|command| KeyBinding {
                command: command.id.clone(),
                keys: command.keys.clone(),
                context: command.context,
            })
            .collect(),
    };
    let (contributed, mut keymap_diagnostics) = keymap_layer(&bindings, os, catalog);
    let (mut layers, found) = config.own_keymap(os, Some(catalog), Some(contributed));
    keymap_diagnostics.extend(found);
    let (extension_layers, mut extension_diagnostics) = extensions::keymap_layers(cx, os, catalog);
    layers.extend(extension_layers);
    let (mut menu_bar, menu_diagnostics) = match this.and_then(|this| this.menu_table.as_ref()) {
        Some(table) => table_menu_bar(table, catalog, &config.menu_shortcut_context, cx),
        None => {
            let (mut menu_bar, mut menu_diagnostics) = config.menu_bar_for(os, Some(catalog));
            let found = place_items(
                &mut menu_bar,
                commands,
                catalog,
                &config.menu_shortcut_context,
            );
            menu_diagnostics.extend(found);
            (menu_bar, menu_diagnostics)
        }
    };
    extension_diagnostics.extend(extensions::extend_menus(
        cx,
        &mut menu_bar,
        os,
        catalog,
        &config.menu_shortcut_context,
    ));
    Shipped {
        layers,
        keymap_diagnostics,
        menu_bar,
        menu_diagnostics,
        extension_diagnostics,
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
    let panels = Contributions::panels(cx);
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
/// once the dispatch is over (the window is out for it): hides the dock at
/// `position` if it shows the panel `id`, else shows the dock with it.
fn toggle_panel(cx: &mut App, position: DockPosition, id: PanelId) {
    on_workspace(cx, move |workspace, window, cx| {
        let Some(dock) = workspace.dock(position).cloned() else {
            return;
        };
        let showing = dock.read(cx).is_visible() && dock.read(cx).active() == Some(id);
        if showing {
            workspace.toggle_dock(position, window, cx);
            return;
        }
        // Shown with its focus and on the rail, as a rail click shows it;
        // focus doesn't stay in the panel it replaces.
        dock.update(cx, |dock, cx| dock.show(id, window, cx));
        if dock.read(cx).dock().hidden_panel_focused(window, cx) {
            workspace.focus_active(window, cx);
        }
    });
}

/// Runs `f` on the workspace of the window the command being dispatched
/// was dispatched in, else the active one, once the dispatch is over (the
/// window is out for it); nothing if that window's root isn't a workspace.
fn on_workspace(
    cx: &mut App,
    f: impl FnOnce(&mut Workspace, &mut Window, &mut Context<Workspace>) + 'static,
) {
    let Some(handle) = dispatch_window(cx).or_else(|| cx.active_window()) else {
        return;
    };
    cx.defer(move |cx| {
        handle
            .update(cx, |root, window, cx| {
                let Ok(workspace) = root.downcast::<Workspace>() else {
                    return;
                };
                workspace.update(cx, |workspace, cx| f(workspace, window, cx));
            })
            .ok();
    });
}

/// Resolves the keymap and menus again once the current update is over,
/// as the extensions' keymap or menu files changed: once [`start`] has
/// them resolve, as `init` resolves them itself until then.
pub(crate) fn resolve_again(cx: &mut App) {
    if cx
        .try_global::<Contributions>()
        .is_some_and(|this| this.resolver.is_some())
    {
        schedule(cx);
    }
}

/// Resolves the keymap and menus again, with every command registered and
/// contributed now, and the extensions' files as last read, and installs
/// them.
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
        extension_diagnostics,
    } = shipped;
    extensions::set_laid(cx, extension_diagnostics, true);
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
pub(crate) fn keymap_layer(
    commands: &[KeyBinding],
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
            toml_escape(command.command.as_str())
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
                Some(key) => key.replacen("bindings[0]", command.command.as_str(), 1),
                None => command.command.to_string(),
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
                    key: Some(format!("{}.keys", command.command)),
                    line,
                    column: 1,
                    message: format!(
                        "{} is bound to both {other} and {}",
                        entry.keys, command.command
                    ),
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

/// The menu bar `table` is, its items checked against `catalog` and their
/// shortcuts resolved in `frames`, each titled as the command is registered
/// else as the table has it. An item whose command `catalog` doesn't know,
/// or won't run without args, is left out, with a warning, as a menu
/// table's is. A menu named twice, or a command twice in one menu, is an
/// error, as in a menu table, and the table is left out: no menu shows.
fn table_menu_bar(
    table: &MenuTable,
    catalog: &dyn CommandCatalog,
    frames: &[String],
    cx: &App,
) -> (MenuBar, Vec<Diagnostic>) {
    let repeated = repeats(table);
    if !repeated.is_empty() {
        return (MenuBar::default(), repeated);
    }
    let mut diagnostics = Vec::new();
    let mut bar = MenuBar::default();
    for menu in table.menus() {
        let mut items = Vec::new();
        for (ix, item) in menu.items.iter().enumerate() {
            let (command, title) = match item {
                TableItem::Separator => {
                    items.push(MenuItem {
                        kind: ItemKind::Separator,
                        place: None,
                    });
                    continue;
                }
                TableItem::Command { command, title } => (command, title),
            };
            let message = match catalog.check(command, &Value::Null) {
                Ok(()) => None,
                Err(CatalogError::UnknownCommand) => {
                    Some(format!("unknown command {command}; the item is left out"))
                }
                Err(CatalogError::BadArgs(message)) => Some(format!(
                    "bad args for {command}: {message}; the item is left out"
                )),
            };
            if let Some(message) = message {
                diagnostics.push(Diagnostic {
                    severity: Severity::Warning,
                    file: PathBuf::from(CONTRIBUTED_MENUS_PATH),
                    key: Some(format!("{}.items[{ix}]", menu.name)),
                    line: ix + 1,
                    column: 1,
                    message,
                });
                continue;
            }
            let title =
                typed_command(command, cx).map_or_else(|| title.clone(), |typed| typed.title);
            items.push(MenuItem {
                kind: ItemKind::Command(CommandItem {
                    id: command.to_string(),
                    title,
                    command: command.clone(),
                    args: Value::Null,
                    os_action: None,
                    alternatives: Vec::new(),
                    shortcut_context: frames
                        .iter()
                        .map(|frame| ContextFrame::new(frame.as_str()))
                        .collect(),
                }),
                place: None,
            });
        }
        bar.menus.push(Menu {
            name: menu.name.to_owned(),
            items,
        });
    }
    (bar, diagnostics)
}

/// An error for each menu `table` names again, and each command a menu
/// has again, worded as a menu table's: menu dispatch finds a menu by its
/// name and an item by its ID, so the second would never run.
fn repeats(table: &MenuTable) -> Vec<Diagnostic> {
    let error = |key: String, line: usize, message: String| Diagnostic {
        severity: Severity::Error,
        file: PathBuf::from(CONTRIBUTED_MENUS_PATH),
        key: Some(key),
        line,
        column: 1,
        message,
    };
    let mut diagnostics = Vec::new();
    let mut names = HashSet::new();
    for (menu_ix, menu) in table.menus().iter().enumerate() {
        if !names.insert(menu.name) {
            let message = format!("menu {:?} is repeated", menu.name);
            diagnostics.push(error(menu.name.to_owned(), menu_ix + 1, message));
        }
        let mut ids = HashSet::new();
        for (ix, item) in menu.items.iter().enumerate() {
            if let TableItem::Command { command, .. } = item
                && !ids.insert(command)
            {
                let message = format!(
                    "id {:?} is repeated in menu {:?}",
                    command.as_str(),
                    menu.name
                );
                diagnostics.push(error(format!("{}.items[{ix}]", menu.name), ix + 1, message));
            }
        }
    }
    diagnostics
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
    // By order within each menu, then where declared, menus in the order
    // first named.
    let mut names: Vec<&str> = Vec::new();
    for (_, spec) in &placed {
        if !names.contains(&&*spec.menu) {
            names.push(&spec.menu);
        }
    }
    let rank = |spec: &MenuSpec| names.iter().position(|name| *name == spec.menu);
    placed.sort_by_key(|(_, spec)| (rank(spec), spec.order, spec.declared));
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

#[cfg(test)]
mod tests {
    use super::*;

    fn register(_: &mut App, _: CommandInfo) {}

    fn run(_: &mut Workspace, _: &mut Cx<'_, '_, Workspace>) {}

    /// A record of `id` in `krate`, at `file`'s `line`, compiled into a
    /// library.
    const fn record(
        id: &'static str,
        krate: &'static str,
        file: &'static str,
        line: u32,
    ) -> CommandRecord {
        CommandRecord {
            id,
            title: "Title",
            context: None,
            keys: None,
            menu: None,
            order: 0,
            palette: true,
            file,
            line,
            krate,
            bin: None,
            test_dir: None,
            register,
            run,
        }
    }

    const fn in_bin(record: CommandRecord) -> CommandRecord {
        CommandRecord {
            bin: Some("notes"),
            ..record
        }
    }

    const fn in_test(record: CommandRecord) -> CommandRecord {
        CommandRecord {
            test_dir: Some("/target/tmp"),
            ..record
        }
    }

    /// Records as `inventory` might list them: in no order, from the app's
    /// binary, its integration test, and two library crates.
    const RECORDS: [CommandRecord; 6] = [
        record("outline.collapse", "outline", "src/outline.rs", 9),
        in_bin(record("notes.save", "notes", "src/main.rs", 40)),
        record("tags.add", "tags", "src/lib.rs", 3),
        in_test(record("app.check", "app", "tests/app.rs", 2)),
        in_bin(record("notes.new_note", "notes", "src/main.rs", 12)),
        record("outline.expand", "outline", "src/outline.rs", 4),
    ];

    fn ids(records: Vec<&CommandRecord>) -> Vec<&str> {
        records.into_iter().map(|record| record.id).collect()
    }

    #[test]
    fn the_app_collects_its_own_targets_records_by_file_then_line() {
        assert_eq!(
            ids(app_records(&RECORDS)),
            ["notes.new_note", "notes.save", "app.check"]
        );
    }

    #[test]
    fn a_library_crates_records_apply_only_through_its_module() {
        assert_eq!(
            ids(crate_records(&RECORDS, "outline")),
            ["outline.expand", "outline.collapse"]
        );
        assert_eq!(ids(crate_records(&RECORDS, "tags")), ["tags.add"]);
        // The binary's are the app's, though its crate is `notes` too.
        assert_eq!(ids(crate_records(&RECORDS, "notes")), Vec::<&str>::new());
        assert!(
            !app_records(&RECORDS)
                .iter()
                .any(|record| record.krate == "outline")
        );
    }

    #[test]
    fn unclaimed_library_records_warn_naming_the_crate() {
        let warnings = unclaimed(&RECORDS, &["outline"]);
        assert_eq!(warnings.len(), 1, "{warnings:?}");
        assert!(
            warnings[0].starts_with("the crate tags's #[kiln::command]s (tags.add) are left out"),
            "{warnings:?}"
        );
        assert!(
            warnings[0].contains(".module(tags::module)"),
            "{warnings:?}"
        );

        let warnings = unclaimed(&RECORDS, &[]);
        let named: Vec<&str> = (warnings.iter())
            .map(|warning| warning.split(['\'', ' ']).nth(2).unwrap())
            .collect();
        assert_eq!(named, ["outline", "tags"], "{warnings:?}");
        assert!(warnings[0].contains("(outline.expand, outline.collapse)"));
        assert_eq!(
            unclaimed(&RECORDS, &["outline", "tags"]),
            Vec::<String>::new()
        );
    }

    #[test]
    fn a_record_makes_its_spec() {
        let record = CommandRecord {
            context: Some("editor"),
            keys: Some("secondary-n"),
            menu: Some("File"),
            order: 3,
            ..record("notes.new_note", "notes", "src/main.rs", 1)
        };
        let spec = record.spec();
        assert_eq!(spec.id.as_str(), "notes.new_note");
        assert_eq!(spec.title, "Title");
        assert_eq!(spec.context, Some("editor"));
        assert_eq!(spec.keys, Keys::from("secondary-n"));
        assert_eq!(
            spec.menu,
            Some(MenuSpec {
                menu: "File".into(),
                order: 3,
                declared: Some(("src/main.rs", 1)),
            })
        );
    }

    fn no_panel(_: &mut Window, _: &mut App) -> AnyDockPanel {
        unreachable!("not made")
    }

    /// A `#[kiln::panel]`'s record of `id` at `file`'s `line`, in a library.
    const fn panel(id: &'static str, order: i32, file: &'static str, line: u32) -> PanelRecord {
        PanelRecord {
            id,
            title: "Title",
            icon: Icon::Files,
            position: DockPosition::Left,
            order,
            toggle_key: None,
            make: no_panel,
            file,
            line,
            krate: "lib",
            bin: None,
            test_dir: None,
        }
    }

    fn a_item(_: &App) -> Option<StatusItem> {
        Some(StatusItem::new("a", StatusSide::Left, 0, "a"))
    }

    fn z_item(_: &App) -> Option<StatusItem> {
        Some(StatusItem::new("z", StatusSide::Left, 0, "z"))
    }

    fn hand_item(_: &App) -> Option<StatusItem> {
        Some(StatusItem::new("hand", StatusSide::Left, 0, "hand"))
    }

    const fn status(
        item: fn(&App) -> Option<StatusItem>,
        file: &'static str,
        line: u32,
    ) -> StatusRecord {
        StatusRecord {
            id: "lib.status",
            side: StatusSide::Left,
            order: 0,
            item,
            file,
            line,
            krate: "lib",
            bin: None,
            test_dir: None,
        }
    }

    const Z_PANEL: PanelRecord = panel("z.panel", 0, "src/z.rs", 1);
    const A_PANEL: PanelRecord = panel("a.panel", 0, "src/a.rs", 9);
    const A_LATE: PanelRecord = panel("a.late", 1, "src/a.rs", 2);
    const Z_STATUS: StatusRecord = status(z_item, "src/z.rs", 1);
    const A_STATUS: StatusRecord = status(a_item, "src/a.rs", 9);

    #[gpui::test]
    fn recorded_panels_and_statuses_go_by_file_and_line_whatever_module_adds_them(
        cx: &mut gpui::TestAppContext,
    ) {
        cx.update(|cx| {
            // The module of z.rs adds its records before that of a.rs.
            add_panels_and_statuses(cx, vec![&Z_PANEL], vec![&Z_STATUS]);
            add_panels_and_statuses(cx, vec![&A_LATE, &A_PANEL], vec![&A_STATUS]);
            // By hand: first among its order, as added.
            Contributions::add_panel(
                cx,
                PanelSpec {
                    id: PanelId("hand"),
                    ..A_PANEL.spec()
                },
            );
            Contributions::add_status(
                cx,
                StatusSpec {
                    side: StatusSide::Left,
                    order: 0,
                    item: hand_item,
                },
            );

            let panels: Vec<&str> = (Contributions::panels(cx).iter())
                .map(|panel| panel.id.0)
                .collect();
            assert_eq!(panels, ["hand", "a.panel", "z.panel", "a.late"]);
            let statuses: Vec<String> = (Contributions::statuses(cx).iter())
                .map(|status| (status.item)(cx).unwrap().id.to_string())
                .collect();
            assert_eq!(statuses, ["hand", "a", "z"]);

            // The toggles' View menu items are placed where the panels are
            // declared, among the commands' declared there.
            let declared = |id: &str| {
                (Contributions::commands(cx).iter())
                    .find(|command| command.id.as_str() == id)
                    .and_then(|command| command.menu.clone())
                    .map(|menu| menu.declared)
            };
            assert_eq!(declared("a.panel.toggle"), Some(Some(("src/a.rs", 9))));
            assert_eq!(declared("hand.toggle"), Some(None));
        });
    }
}
