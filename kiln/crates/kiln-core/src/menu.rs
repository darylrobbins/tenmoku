// SPDX-License-Identifier: Apache-2.0

//! The menu bar as data: a table of menus and items, read from TOML, whose
//! shortcuts come from the active [`Keymap`]. GPUI-free, so an app's menus,
//! and a host's changes to them, are data; `kiln-platform` installs them.
//!
//! ```toml
//! [[menus]]
//! name = "Edit"
//!
//! [[menus.items]]
//! title = "Paste"
//! command = "editor.paste"
//! os_action = "paste"                                # optional
//! alternatives = [{ command = "text_input.paste", when = "text_input" }]
//!
//! [[menus.items]]
//! separator = true
//!
//! [[menus.items]]
//! title = "Services"
//! system = "services"
//! ```
//!
//! An item also takes `args` (a table, as in keymaps), `id` (the command ID
//! by default, unique within its menu) and `shortcut_context` (the frame
//! names its shortcut is resolved in, the app's by default: see
//! [`MenuBar::parse_in`]).
//!
//! A table that ships with an app names a catalogue key (kiln-l10n) instead
//! of literal text: `title_key` in place of an item's `title` or a menu's
//! `name`, as in `title_key = "notes-menu-archive"`. Its `title` (or `name`)
//! is then the key's en-US text, and [`CommandItem::label`],
//! [`SystemItem::label`] and [`Menu::label`] give the key, to show in the
//! active locale. Each title takes the arguments [`set_label_arg`] set,
//! such as `$app`. A menu's [`Menu::id`], which lookups, layers and
//! extensions find it by, is its en-US name, so it is the same in every
//! locale. A system item with no title is `kiln-core-menu-services`.
//! A host's extension layer (see [`MenuBar::extend`]) may set `before` or
//! `after` to an item's `id`, or `remove = true`. Unknown fields are errors.

use std::{
    collections::HashSet,
    ops::Range,
    path::Path,
    sync::{Arc, LazyLock, PoisonError, RwLock},
};

use crate::{Diagnostic, Key, Layer, Os, Severity, platform::PlatformFilter};
use kiln_command::{
    CommandId, ContextFrame, ContextPredicate, ContextStack, Invocation, Source, Value,
};
use kiln_l10n::{Args, Catalogue, Resources, SharedString, Text, tr};
use toml::de::{DeTable, DeValue};

use crate::{
    keymap::{CatalogError, CommandCatalog, Keymap},
    keystroke::KeySequence,
    matcher::{KeyOutcome, Matcher},
};

/// The path diagnostics name for the built-in menus.
pub const DEFAULT_MENUS_PATH: &str = "<default menus>";

/// The menus, left to right. The first is the app menu.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MenuBar {
    pub menus: Vec<Menu>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Menu {
    /// What [`MenuBar::menu`], a layer, an extension and a chosen item
    /// find the menu by: its en-US `name`, the same in every locale. kiln
    /// gives the native macOS Window menu its role by it (`Window`).
    pub id: String,
    /// The menu's title in en-US: the table's `name`, or its `title_key`'s
    /// en-US text.
    pub name: String,
    /// The catalogue key of a bundled menu's title.
    pub title_key: Option<String>,
    pub items: Vec<MenuItem>,
}

/// An item, and where a layer puts it.
#[derive(Clone, Debug, PartialEq)]
pub struct MenuItem {
    pub kind: ItemKind,
    /// Only for [`MenuBar::extend`]; ignored elsewhere.
    pub place: Option<Place>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum ItemKind {
    Command(CommandItem),
    Separator,
    System(SystemItem),
    /// `remove = true` in a layer: removes the item with this ID. Ignored
    /// outside [`MenuBar::extend`].
    Remove(String),
}

/// Where a layer's item goes, by the ID of an item in the menu it extends.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Place {
    Before(String),
    After(String),
}

/// An item that runs a command.
#[derive(Clone, Debug, PartialEq)]
pub struct CommandItem {
    /// Unique within its menu.
    pub id: String,
    /// The title in en-US: the table's `title`, or its `title_key`'s en-US
    /// text.
    pub title: String,
    /// The catalogue key of a bundled item's title.
    pub title_key: Option<String>,
    pub command: CommandId,
    /// `Null` when it has none.
    pub args: Value,
    /// The standard edit action macOS knows the item as.
    pub os_action: Option<OsAction>,
    /// Commands run instead where their `when` holds; the first that holds
    /// wins.
    pub alternatives: Vec<Alternative>,
    /// The stack [`CommandItem::shortcut`] resolves keys in.
    pub shortcut_context: ContextStack,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Alternative {
    pub command: CommandId,
    pub args: Value,
    pub when: ContextPredicate,
}

/// A submenu the OS fills, such as Services.
#[derive(Clone, Debug, PartialEq)]
pub struct SystemItem {
    /// The system menu's name (`services`) unless set.
    pub id: String,
    /// The title in en-US, as [`CommandItem::title`].
    pub title: String,
    /// The catalogue key of its title: `kiln-core-menu-services` for a
    /// Services item the table gives no title.
    pub title_key: Option<String>,
    pub menu: SystemMenu,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum SystemMenu {
    Services,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum OsAction {
    Cut,
    Copy,
    Paste,
    SelectAll,
    Undo,
    Redo,
}

const OS_ACTIONS: [(&str, OsAction); 6] = [
    ("cut", OsAction::Cut),
    ("copy", OsAction::Copy),
    ("paste", OsAction::Paste),
    ("select_all", OsAction::SelectAll),
    ("undo", OsAction::Undo),
    ("redo", OsAction::Redo),
];

impl MenuBar {
    /// Parses a menu table's `source`, read from `path`, as
    /// [`crate::KeymapLayer::parse`] does a keymap: the bar if it has no
    /// errors, and the diagnostics either way.
    ///
    /// A syntax error, an unknown field, a value of the wrong type, a
    /// malformed command ID or predicate, an item that isn't exactly one
    /// kind, and a repeated ID are errors. With a `catalog`, an item or
    /// alternative whose command it doesn't know or whose args it rejects
    /// is dropped with a warning, and the rest loads.
    ///
    /// The table is this platform's: see [`MenuBar::parse_for`]. Items
    /// resolve their shortcuts in no context unless they name one: see
    /// [`MenuBar::parse_in`].
    pub fn parse(
        path: &Path,
        source: &str,
        catalog: Option<&dyn CommandCatalog>,
    ) -> (Option<Self>, Vec<Diagnostic>) {
        Self::parse_for(path, source, catalog, Os::current())
    }

    /// [`MenuBar::parse`] as on `os`, so one table serves every platform: a
    /// menu or item with `platform = "macos"` (or `"!macos"`, or a list such
    /// as `"linux, windows"`) is left out elsewhere, along with separators
    /// left stray and menus left empty. Every platform's filters are
    /// checked, so a mistake in one shows wherever the table is loaded.
    pub fn parse_for(
        path: &Path,
        source: &str,
        catalog: Option<&dyn CommandCatalog>,
        os: Os,
    ) -> (Option<Self>, Vec<Diagnostic>) {
        Self::parse_in(path, source, catalog, os, &[])
    }

    /// [`MenuBar::parse_for`], with the frames an item's shortcut is
    /// resolved in when it doesn't name its own `shortcut_context`: the
    /// app's, those its main view usually has, such as `["main", "editor"]`,
    /// so a binding limited to them shows in the menu. With none, only
    /// bindings that hold in no context show. It's the app's, not the
    /// table's, so a host's layer parsed alone resolves the same way.
    pub fn parse_in(
        path: &Path,
        source: &str,
        catalog: Option<&dyn CommandCatalog>,
        os: Os,
        shortcut_context: &[&str],
    ) -> (Option<Self>, Vec<Diagnostic>) {
        let mut layer = Layer::default();
        layer.path = path.to_owned();
        layer.source = source.to_owned();
        let mut parser = Parser {
            layer,
            catalog,
            os,
            shortcut_context: shortcut_context
                .iter()
                .map(|&name| name.to_owned())
                .collect(),
            diagnostics: Vec::new(),
        };
        let bar = parser.parse();
        let ok = !parser.diagnostics.iter().any(Diagnostic::is_error);
        (ok.then_some(bar), parser.diagnostics)
    }

    /// The menu with `id`, its en-US name.
    pub fn menu(&self, id: &str) -> Option<&Menu> {
        self.menus.iter().find(|menu| menu.id == id)
    }

    /// Merges a host's `layer` in. A layer menu with the ID of one of these
    /// merges into it: an item placed `before` or `after` an ID goes there,
    /// a `remove` item removes the item with its ID, an item whose ID is
    /// taken replaces that item (in its place, unless placed), and the rest
    /// are appended. Any other layer
    /// menu goes in before `Window`, or last without one. Then leading,
    /// trailing and doubled separators are dropped, and menus left empty.
    pub fn extend(&mut self, layer: MenuBar) {
        for menu in layer.menus {
            match self.menus.iter_mut().find(|own| own.id == menu.id) {
                Some(own) => own.merge(menu.items),
                None => {
                    let at = self
                        .menus
                        .iter()
                        .position(|own| own.id == "Window")
                        .unwrap_or(self.menus.len());
                    let mut new = Menu {
                        id: menu.id,
                        name: menu.name,
                        title_key: menu.title_key,
                        items: Vec::new(),
                    };
                    new.merge(menu.items);
                    self.menus.insert(at, new);
                }
            }
        }
        self.tidy();
    }

    fn tidy(&mut self) {
        for menu in &mut self.menus {
            menu.tidy();
        }
        self.menus.retain(|menu| !menu.items.is_empty());
    }
}

impl Menu {
    /// The menu's title, to resolve in the active locale: its key for a
    /// bundled menu, else its name as written.
    pub fn label(&self) -> Text {
        label(self.title_key.as_deref(), &self.name)
    }

    /// The item with `id`.
    pub fn item(&self, id: &str) -> Option<&MenuItem> {
        self.items.iter().find(|item| item.defines(id))
    }

    fn position(&self, id: &str) -> Option<usize> {
        self.items.iter().position(|item| item.defines(id))
    }

    fn merge(&mut self, items: Vec<MenuItem>) {
        for mut item in items {
            if let ItemKind::Remove(id) = &item.kind {
                if let Some(ix) = self.position(id) {
                    self.items.remove(ix);
                }
                continue;
            }
            let taken = item.id().and_then(|id| self.position(id));
            let Some(place) = item.place.take() else {
                // Replaced where it was, or appended.
                match taken {
                    Some(ix) => self.items[ix] = item,
                    None => self.items.push(item),
                }
                continue;
            };
            if let Some(ix) = taken {
                self.items.remove(ix);
            }
            let at = match place {
                Place::Before(id) => self.position(&id),
                Place::After(id) => self.position(&id).map(|ix| ix + 1),
            };
            self.items.insert(at.unwrap_or(self.items.len()), item);
        }
    }

    /// Drops leading, trailing and doubled separators.
    fn tidy(&mut self) {
        let mut items = Vec::with_capacity(self.items.len());
        for item in self.items.drain(..) {
            let separator = item.kind == ItemKind::Separator;
            let after_separator = items
                .last()
                .is_none_or(|last: &MenuItem| last.kind == ItemKind::Separator);
            if !(separator && after_separator) {
                items.push(item);
            }
        }
        if items
            .last()
            .is_some_and(|last| last.kind == ItemKind::Separator)
        {
            items.pop();
        }
        self.items = items;
    }
}

impl MenuItem {
    /// The item's ID: a command or system item's, or the one a `remove`
    /// names. Separators have none.
    pub fn id(&self) -> Option<&str> {
        match &self.kind {
            ItemKind::Command(item) => Some(&item.id),
            ItemKind::System(item) => Some(&item.id),
            ItemKind::Remove(id) => Some(id),
            ItemKind::Separator => None,
        }
    }

    pub fn command(&self) -> Option<&CommandItem> {
        match &self.kind {
            ItemKind::Command(item) => Some(item),
            _ => None,
        }
    }

    /// Whether this is the item with `id`, not a `remove` naming it.
    fn defines(&self, id: &str) -> bool {
        !matches!(self.kind, ItemKind::Remove(_)) && self.id() == Some(id)
    }
}

impl CommandItem {
    /// The item's title, to resolve in the active locale: its key, with the
    /// arguments [`set_label_arg`] set, for a bundled item; else its title
    /// as written, verbatim.
    pub fn label(&self) -> Text {
        label(self.title_key.as_deref(), &self.title)
    }

    /// The first single-keystroke entry of [`shortcuts`] in the item's
    /// `shortcut_context` that isn't a mouse click: the key a menu shows,
    /// which can't be a chord or a click.
    pub fn shortcut(&self, keymap: &Keymap) -> Option<KeySequence> {
        shortcuts(keymap, &self.command, &self.args, &self.shortcut_context)
            .into_iter()
            .find(|keys| matches!(keys.keystrokes(), [key] if key.key != Key::Click))
    }

    /// What choosing the item asks for in `stack`: the first alternative
    /// whose `when` holds there, or else the item's own command, with its
    /// args, from the menu, in `stack`.
    pub fn invocation(&self, stack: &ContextStack) -> Invocation {
        let (command, args) = self
            .alternatives
            .iter()
            .find(|alternative| alternative.when.eval(stack))
            .map_or((&self.command, &self.args), |alternative| {
                (&alternative.command, &alternative.args)
            });
        Invocation::new(command.clone())
            .args(args.clone())
            .source(Source::Menu)
            .context(stack.clone())
    }
}

impl SystemItem {
    /// The item's title, to resolve in the active locale, as
    /// [`CommandItem::label`].
    pub fn label(&self) -> Text {
        label(self.title_key.as_deref(), &self.title)
    }
}

/// The key of a Services item's title when its table gives none.
pub const SERVICES_KEY: &str = "kiln-core-menu-services";

/// Process-wide, as the catalogue is: what [`set_label_arg`] set.
static LABEL_ARGS: LazyLock<RwLock<Args>> = LazyLock::new(RwLock::default);

/// Gives every keyed menu title the argument `name`: kiln sets `app` to the
/// app's name before it parses its menus, for titles such as
/// `Hide { $app }`. A second call for the same `name` replaces the first.
pub fn set_label_arg(name: &'static str, value: impl Into<SharedString>) {
    LABEL_ARGS
        .write()
        .unwrap_or_else(PoisonError::into_inner)
        .set(name, value.into());
}

fn label_args() -> Args {
    LABEL_ARGS
        .read()
        .unwrap_or_else(PoisonError::into_inner)
        .clone()
}

fn label(key: Option<&str>, title: &str) -> Text {
    match key {
        Some(key) => Text::keyed(key.to_owned(), label_args()),
        None => Text::verbatim(title.to_owned()),
    }
}

/// `key`'s en-US text with the arguments [`set_label_arg`] set, whatever
/// the active locale, from every crate's catalogue linked in; `None` if
/// en-US has no such message.
fn en_us(key: &str) -> Option<String> {
    static EN_US: LazyLock<Catalogue> =
        LazyLock::new(|| Catalogue::new(&Resources::registered(), &[]));
    let found = EN_US.try_format(key, &label_args())?;
    Some(found.text.to_string())
}

/// Every key sequence that, pressed in `stack`, runs `command` with `args`,
/// the one taking precedence first: the later layer, then the later entry.
///
/// Each binding of `command` with those args is kept only if a fresh
/// [`Matcher`], fed its keys in `stack` (and flushed if they leave it
/// pending), dispatches exactly that. So a binding a user unbinds, or one
/// a deeper context's binding for the same keys hides, isn't listed.
pub fn shortcuts(
    keymap: &Keymap,
    command: &CommandId,
    args: &Value,
    stack: &ContextStack,
) -> Vec<KeySequence> {
    let shared = Arc::new(keymap.clone());
    let mut found: Vec<KeySequence> = Vec::new();
    // Bindings come in the order read, so later layers and entries last.
    for binding in keymap.bindings_for(command).into_iter().rev() {
        // A binding whose context doesn't hold can't run, though a shorter
        // one for a prefix of its keys might.
        if &binding.args != args || found.contains(&binding.keys) || binding.depth(stack).is_none()
        {
            continue;
        }
        let mut matcher = Matcher::new(shared.clone());
        let keystrokes = binding.keys.keystrokes();
        let mut outcome = KeyOutcome::Pending;
        for (ix, &keystroke) in keystrokes.iter().enumerate() {
            outcome = matcher.push(keystroke, stack);
            // Run or failed before its last key: something else took them.
            if ix + 1 < keystrokes.len()
                && !matches!(
                    outcome,
                    KeyOutcome::Pending | KeyOutcome::DispatchPending(_)
                )
            {
                outcome = KeyOutcome::Unmatched(Vec::new());
                break;
            }
        }
        if outcome == KeyOutcome::Pending {
            outcome = matcher.flush();
        }
        // A key that runs its binding and keeps listening still runs it.
        let runs = matches!(&outcome, KeyOutcome::Dispatch(invocation)
            | KeyOutcome::DispatchPending(invocation)
            if &invocation.command == command && &invocation.args == args);
        if runs {
            found.push(binding.keys.clone());
        }
    }
    found
}

struct Parser<'a> {
    /// The file's path and source, for diagnostics.
    layer: Layer,
    catalog: Option<&'a dyn CommandCatalog>,
    /// The platform the table is for.
    os: Os,
    /// The frames shortcuts are resolved in when an item names none.
    shortcut_context: Vec<String>,
    diagnostics: Vec<Diagnostic>,
}

/// A table's fields, their spans and its key path, for diagnostics.
struct Fields<'a, 'i> {
    table: &'a DeTable<'i>,
    values: &'a toml::Table,
    span: Range<usize>,
    key: String,
}

impl Fields<'_, '_> {
    fn key(&self, name: &str) -> String {
        format!("{}.{name}", self.key)
    }

    fn span(&self, name: &str) -> Range<usize> {
        self.table
            .get(name)
            .map_or(self.span.clone(), |value| value.span())
    }
}

impl Parser<'_> {
    fn parse(&mut self) -> MenuBar {
        let source = self.layer.source.clone();
        let (document, values) = match (DeTable::parse(&source), source.parse::<toml::Table>()) {
            (Ok(document), Ok(values)) => (document, values),
            (Err(error), _) => {
                let offset = error.span().map_or(0, |span| span.start);
                self.error(None, offset, error.message().trim_end().to_owned());
                return MenuBar::default();
            }
            // Such as an integer too big for an i64.
            (_, Err(error)) => {
                let offset = error.span().map_or(0, |span| span.start);
                self.error(None, offset, error.message().trim_end().to_owned());
                return MenuBar::default();
            }
        };
        let root = Fields {
            table: document.get_ref(),
            values: &values,
            span: 0..0,
            key: String::new(),
        };
        self.unknown_fields(&root, &["menus"]);
        let menus = self.tables(&root, "menus", "menus");
        let mut names = HashSet::new();
        let mut bar = MenuBar::default();
        for fields in menus {
            if let Some(menu) = self.menu(&fields) {
                if !names.insert(menu.id.clone()) {
                    let id = format!("{:?}", menu.id);
                    let message = tr!("kiln-core-menu-repeated", menu = id).into();
                    self.error(Some(&fields.key), fields.span.start, message);
                }
                bar.menus.push(menu);
            }
        }
        bar
    }

    fn menu(&mut self, fields: &Fields<'_, '_>) -> Option<Menu> {
        self.unknown_fields(fields, &["name", "title_key", "items", "platform"]);
        let included = self.platform(fields);
        let (name, title_key) = self.title(fields, "name", true)?;
        let id = name.clone();
        let key = fields.key("items");
        let menu_id = &id;
        let mut items = Vec::new();
        let mut ids = HashSet::new();
        let mut dropped = false;
        for item in self.tables(fields, "items", &key) {
            match self.item(&item) {
                Ok(Some(parsed)) => {
                    if let Some(id) = parsed.id()
                        && !matches!(parsed.kind, ItemKind::Remove(_))
                        && !ids.insert(id.to_owned())
                    {
                        let message = tr!(
                            "kiln-core-menu-id-repeated",
                            id = format!("{id:?}"),
                            menu = format!("{menu_id:?}")
                        )
                        .into();
                        self.error(Some(&item.key), item.span.start, message);
                    }
                    items.push(parsed);
                }
                Ok(None) => dropped = true,
                Err(()) => {}
            }
        }
        if !included {
            return None;
        }
        let mut menu = Menu {
            id,
            name,
            title_key,
            items,
        };
        // Dropping items leaves no stray separators, nor an empty menu.
        if dropped {
            menu.tidy();
            if menu.items.is_empty() {
                return None;
            }
        }
        Some(menu)
    }

    /// The item; `Ok(None)` if the catalog drops it, `Err` if it has errors.
    fn item(&mut self, fields: &Fields<'_, '_>) -> Result<Option<MenuItem>, ()> {
        self.unknown_fields(
            fields,
            &[
                "title",
                "title_key",
                "command",
                "args",
                "id",
                "os_action",
                "alternatives",
                "shortcut_context",
                "separator",
                "system",
                "before",
                "after",
                "remove",
                "platform",
            ],
        );
        let errors = self.error_count();
        let included = self.platform(fields);
        let place = match (
            self.string(fields, "before", false),
            self.string(fields, "after", false),
        ) {
            (Some(_), Some(_)) => {
                self.error(
                    Some(&fields.key),
                    fields.span.start,
                    tr!("kiln-core-menu-before-and-after").into(),
                );
                None
            }
            (before, after) => before.map(Place::Before).or(after.map(Place::After)),
        };
        let id = self.string(fields, "id", false);
        let separator = self.bool(fields, "separator");
        let remove = self.bool(fields, "remove");
        let system = self.string(fields, "system", false);
        let command = self.command(fields, "command", false);
        let kinds = [
            separator,
            remove,
            system.is_some(),
            command.is_some() && !remove,
        ];
        if kinds.iter().filter(|&&kind| kind).count() != 1 {
            let message = tr!("kiln-core-menu-item-kind").into();
            self.error(Some(&fields.key), fields.span.start, message);
            return Err(());
        }
        let kind = if separator {
            ItemKind::Separator
        } else if remove {
            let id = id.or_else(|| command.map(|command| command.as_str().to_owned()));
            let Some(id) = id else {
                let message = tr!("kiln-core-menu-remove-needs-id").into();
                self.error(Some(&fields.key), fields.span.start, message);
                return Err(());
            };
            ItemKind::Remove(id)
        } else if let Some(system) = system {
            if system != "services" {
                let quoted = format!("{system:?}");
                let message = tr!("kiln-core-menu-system-unknown", name = quoted).into();
                self.error(
                    Some(&fields.key("system")),
                    fields.span("system").start,
                    message,
                );
            }
            let (title, title_key) = match self.title(fields, "title", false) {
                Some(title) => title,
                None => (
                    en_us(SERVICES_KEY).unwrap_or_else(|| SERVICES_KEY.to_owned()),
                    Some(SERVICES_KEY.to_owned()),
                ),
            };
            ItemKind::System(SystemItem {
                title,
                title_key,
                id: id.unwrap_or(system),
                menu: SystemMenu::Services,
            })
        } else {
            let command = command.expect("one kind is set");
            let title = self.title(fields, "title", true);
            let args = self.args(fields);
            let os_action = self.string(fields, "os_action", false).and_then(|name| {
                let found = OS_ACTIONS.iter().find(|(known, _)| *known == name);
                if found.is_none() {
                    let quoted = format!("{name:?}");
                    let message = tr!("kiln-core-menu-os-action-unknown", name = quoted).into();
                    self.error(
                        Some(&fields.key("os_action")),
                        fields.span("os_action").start,
                        message,
                    );
                }
                found.map(|(_, action)| *action)
            });
            let alternatives = self.alternatives(fields);
            let shortcut_context = self.shortcut_context(fields);
            if self.error_count() > errors {
                return Err(());
            }
            let args = args.unwrap_or(Value::Null);
            // Left out here, before the catalog can warn about another
            // platform's command.
            if !included || !self.known(fields, "command", &command, &args) {
                return Ok(None);
            }
            let (title, title_key) = title.expect("checked for errors");
            ItemKind::Command(CommandItem {
                id: id.unwrap_or_else(|| command.as_str().to_owned()),
                title,
                title_key,
                command,
                args,
                os_action,
                alternatives,
                shortcut_context,
            })
        };
        if self.error_count() > errors {
            return Err(());
        }
        if !included {
            return Ok(None);
        }
        Ok(Some(MenuItem { kind, place }))
    }

    /// Whether the table's `platform` filter includes this table's platform;
    /// a malformed one is an error, and includes it.
    fn platform(&mut self, fields: &Fields<'_, '_>) -> bool {
        let Some(text) = self.string(fields, "platform", false) else {
            return true;
        };
        match PlatformFilter::parse(&text) {
            Ok(filter) => filter.matches(self.os),
            Err(message) => {
                let span = fields.span("platform");
                self.error(Some(&fields.key("platform")), span.start, message);
                true
            }
        }
    }

    fn alternatives(&mut self, fields: &Fields<'_, '_>) -> Vec<Alternative> {
        let key = fields.key("alternatives");
        let mut alternatives = Vec::new();
        for alternative in self.tables(fields, "alternatives", &key) {
            self.unknown_fields(&alternative, &["command", "args", "when"]);
            let errors = self.error_count();
            let command = self.command(&alternative, "command", true);
            let args = self.args(&alternative);
            let when = self.string(&alternative, "when", true).and_then(|text| {
                ContextPredicate::parse(&text)
                    .map_err(|error| {
                        self.error(
                            Some(&alternative.key("when")),
                            alternative.span("when").start,
                            error.to_string(),
                        )
                    })
                    .ok()
            });
            if self.error_count() > errors {
                continue;
            }
            let (Some(command), Some(when)) = (command, when) else {
                continue;
            };
            let args = args.unwrap_or(Value::Null);
            if self.known(&alternative, "command", &command, &args) {
                alternatives.push(Alternative {
                    command,
                    args,
                    when,
                });
            }
        }
        alternatives
    }

    fn shortcut_context(&mut self, fields: &Fields<'_, '_>) -> ContextStack {
        let names: Vec<String> = match fields.values.get("shortcut_context") {
            None => self.shortcut_context.clone(),
            Some(toml::Value::Array(names)) if names.iter().all(toml::Value::is_str) => names
                .iter()
                .filter_map(|name| name.as_str().map(str::to_owned))
                .collect(),
            Some(_) => {
                self.error(
                    Some(&fields.key("shortcut_context")),
                    fields.span("shortcut_context").start,
                    tr!("kiln-core-menu-expected-frames").into(),
                );
                Vec::new()
            }
        };
        names.into_iter().map(ContextFrame::new).collect()
    }

    /// Whether the catalog, if any, takes `command` with `args`. If not, the
    /// item or alternative is dropped with a warning.
    fn known(
        &mut self,
        fields: &Fields<'_, '_>,
        name: &str,
        command: &CommandId,
        args: &Value,
    ) -> bool {
        let Some(catalog) = self.catalog else {
            return true;
        };
        let message = match catalog.check(command, args) {
            Ok(()) => return true,
            Err(CatalogError::UnknownCommand) => tr!(
                "kiln-core-menu-unknown-command",
                command = command.to_string()
            )
            .into(),
            Err(CatalogError::BadArgs(message)) => tr!(
                "kiln-core-menu-bad-args",
                command = command.to_string(),
                reason = message
            )
            .into(),
        };
        let key = fields.key(name);
        self.push(
            Severity::Warning,
            Some(&key),
            fields.span(name).start,
            message,
        );
        false
    }

    /// The array of tables `name` of `fields`, each with its key path:
    /// `{key}[{ix}]`. Missing, it's empty.
    fn tables<'a, 'i>(
        &mut self,
        fields: &Fields<'a, 'i>,
        name: &str,
        key: &str,
    ) -> Vec<Fields<'a, 'i>> {
        let (Some(spanned), Some(value)) = (fields.table.get(name), fields.values.get(name)) else {
            return Vec::new();
        };
        let (DeValue::Array(items), toml::Value::Array(values)) = (spanned.get_ref(), value) else {
            let message = tr!(
                "kiln-core-config-expected-tables",
                found = spanned.get_ref().type_str()
            )
            .into();
            self.error(Some(key), spanned.span().start, message);
            return Vec::new();
        };
        let mut tables = Vec::new();
        for (ix, (item, values)) in items.iter().zip(values).enumerate() {
            let key = format!("{key}[{ix}]");
            match (item.get_ref(), values) {
                (DeValue::Table(table), toml::Value::Table(values)) => tables.push(Fields {
                    table,
                    values,
                    span: item.span(),
                    key,
                }),
                _ => {
                    let message = tr!(
                        "kiln-core-config-expected-table",
                        found = item.get_ref().type_str()
                    )
                    .into();
                    self.error(Some(&key), item.span().start, message);
                }
            }
        }
        tables
    }

    /// The string `name`. Missing, it's an error if `required`; of another
    /// type, it's an error.
    fn string(&mut self, fields: &Fields<'_, '_>, name: &str, required: bool) -> Option<String> {
        match fields.values.get(name) {
            Some(toml::Value::String(text)) => Some(text.clone()),
            None => {
                if required {
                    let message = tr!("kiln-core-config-missing", name = name).into();
                    self.error(Some(&fields.key), fields.span.start, message);
                }
                None
            }
            Some(value) => {
                let message =
                    tr!("kiln-core-config-expected-string", found = value.type_str()).into();
                self.error(Some(&fields.key(name)), fields.span(name).start, message);
                None
            }
        }
    }

    /// A title: the literal string `name` (an item's `title`, a menu's
    /// `name`), or the en-US text of the catalogue key `title_key`, with the
    /// key. Both set is an error, and neither is if `required`; a key en-US
    /// has no message for is a warning, and titles it as the key.
    fn title(
        &mut self,
        fields: &Fields<'_, '_>,
        name: &str,
        required: bool,
    ) -> Option<(String, Option<String>)> {
        let key = self.string(fields, "title_key", false);
        let Some(key) = key else {
            return self
                .string(fields, name, required)
                .map(|title| (title, None));
        };
        if fields.values.contains_key(name) {
            let message = tr!("kiln-core-menu-title-and-key", name = name).into();
            self.error(Some(&fields.key), fields.span.start, message);
            return None;
        }
        let title = en_us(&key).unwrap_or_else(|| {
            let message = tr!("kiln-core-menu-title-key-unknown", key = format!("{key:?}")).into();
            let span = fields.span("title_key");
            self.push(
                Severity::Warning,
                Some(&fields.key("title_key")),
                span.start,
                message,
            );
            key.clone()
        });
        Some((title, Some(key)))
    }

    /// The boolean `name`, false if missing.
    fn bool(&mut self, fields: &Fields<'_, '_>, name: &str) -> bool {
        match fields.values.get(name) {
            None => false,
            Some(toml::Value::Boolean(value)) => *value,
            Some(value) => {
                let message = tr!(
                    "kiln-core-config-expected-boolean",
                    found = value.type_str()
                )
                .into();
                self.error(Some(&fields.key(name)), fields.span(name).start, message);
                false
            }
        }
    }

    fn command(
        &mut self,
        fields: &Fields<'_, '_>,
        name: &str,
        required: bool,
    ) -> Option<CommandId> {
        let text = self.string(fields, name, required)?;
        CommandId::parse(&text)
            .map_err(|error| {
                let message = format!("{}: {}", error.reason, error.id);
                self.error(Some(&fields.key(name)), fields.span(name).start, message);
            })
            .ok()
    }

    /// The `args` table as JSON, if set.
    fn args(&mut self, fields: &Fields<'_, '_>) -> Option<Value> {
        let value = fields.values.get("args")?;
        let error = match value {
            toml::Value::Table(_) => match serde_json::to_value(value) {
                Ok(args) => return Some(args),
                Err(error) => error.to_string(),
            },
            value => tr!("kiln-core-config-expected-table", found = value.type_str()).into(),
        };
        self.error(Some(&fields.key("args")), fields.span("args").start, error);
        None
    }

    /// Each field of `fields` not in `known` is an error, at the field.
    fn unknown_fields(&mut self, fields: &Fields<'_, '_>, known: &[&str]) {
        let mut unknown: Vec<_> = fields
            .table
            .iter()
            .filter(|(name, _)| !known.contains(&name.get_ref().as_ref()))
            .map(|(name, _)| (name.span().start, name.get_ref().to_string()))
            .collect();
        unknown.sort();
        for (offset, name) in unknown {
            let key = match fields.key.as_str() {
                "" => name,
                prefix => format!("{prefix}.{name}"),
            };
            self.error(
                Some(&key),
                offset,
                tr!("kiln-core-config-unknown-field").into(),
            );
        }
    }

    fn error_count(&self) -> usize {
        self.diagnostics.iter().filter(|d| d.is_error()).count()
    }

    fn error(&mut self, key: Option<&str>, offset: usize, message: String) {
        self.push(Severity::Error, key, offset, message);
    }

    fn push(&mut self, severity: Severity, key: Option<&str>, offset: usize, message: String) {
        let diagnostic =
            self.layer
                .diagnostic_at(severity, key.map(str::to_owned), offset, message);
        self.diagnostics.push(diagnostic);
    }
}
