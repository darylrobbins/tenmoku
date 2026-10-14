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

use std::{borrow::Cow, path::PathBuf, rc::Rc};

use gpui::{App, Global};
use kiln_command::{CommandId, ContextFrame, ContextPredicate, ContextStack, Value};
use kiln_core::{
    CatalogError, CommandCatalog, CommandItem, Diagnostic, Entry, ItemKind, KeymapLayer, Location,
    Menu, MenuBar, MenuItem, Os, Severity,
};
use kiln_platform::{
    command_actions::{Command, CommandInfo, register_command},
    config::SharedKeymap,
    menus,
};

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

/// Everything contributed to the app, as a GPUI global: see the module
/// docs.
#[derive(Default)]
pub struct Contributions {
    commands: Vec<CommandSpec>,
    /// Those added before `init`, to register when it runs.
    unregistered: Vec<CommandSpec>,
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
    pub fn add_command(cx: &mut App, spec: CommandSpec) {
        Self::add_commands(cx, [spec]);
    }

    /// [`Contributions::add_command`] for each of `specs`, in order.
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
        }
        if started {
            schedule(cx);
        }
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
            return;
        }
        let spec = unregistered.remove(0);
        (spec.register)(cx, spec.info());
    }
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
