//! The Settings editor (`docs/design/design-system.md`, "Settings"): a tab,
//! opened by `workbench.open_settings_ui`, that lists every setting
//! [`SettingsModel`] knows, grouped as the schema nests them, with one line
//! of help and its value, which it edits in place: a switch, a dropdown, an
//! input or a list editor by the setting's kind, or a button to open
//! `settings.toml` for a table ([`controls`]).
//!
//! - On top, the page title, Settings, in the serif (the page title role,
//!   Instrument Serif 30/36), the You / This project switch beside it, and
//!   under them a search field, focused when the tab opens, that filters
//!   the list as it's typed ([`SettingsModel::search`]), by a setting's key
//!   (`editor.font`) as well as its words; a group with nothing found loses
//!   its heading, and nothing found at all says so.
//! - With nothing searched for, the Common section comes first: the theme
//!   cards, then the settings in [`COMMON`], which then show there and not
//!   in their own groups ([`layout`]).
//! - The switch picks the scope, You (the user's file) or This project (the
//!   root's): a row shows the selected scope's file's value, else what it
//!   inherits, and an accent dot and Reset if that file sets it. This
//!   project shows only with a folder open, and dims the rows a project's
//!   file can't set ([`SettingsModel::workspace_allowed`]), whose controls
//!   are off.
//! - A change is written to the selected scope's file
//!   ([`fe_config::set_setting`], [`fe_config::reset_setting`]), keeping
//!   its comments and key order, off the main thread and after the writes
//!   before it, even once the tab is closed. Its row shows it at once, and
//!   once it's written the files are read again and shown, not waiting
//!   for the watcher; a read that started earlier and finishes later is
//!   dropped. A write that fails, or a file that's there but can't be
//!   written to, says why in a toast, and the row shows the file's value
//!   again.
//! - The files are read off the main thread when the tab opens, when the
//!   scope changes, when the root changes, when the config service's
//!   watcher sees either file change ([`ChangedFiles`]), even in a way
//!   that changes no effective setting, when the workbench reloads its
//!   settings ([`Workbench::reload_settings`]), and when the tab is
//!   activated or focus comes back into it from elsewhere. The watcher
//!   watches every workbench's root's project file, not only the launch
//!   root's.
//! - Up and Down move the selected row, scrolling it into view; Down in
//!   the search field moves into the list, and in a list's item they move
//!   to the item before or after it. `/` or secondary-f goes back to
//!   the search, and Escape clears it. The search and the rows' inputs
//!   take the text inputs' editing and caret keys (`text_input.backspace`,
//!   `text_input.move_left` and the like). On the selected row, Space toggles a switch and Enter works
//!   its control: it opens a dropdown, goes into an input, or opens
//!   `settings.toml`. The palette's `settings.add_list_item` and
//!   `settings.remove_list_item` act on the list item with focus (adding
//!   to the selected row's list with none), `settings.reset_to_default` on
//!   the selected row, and `settings.switch_scope` switches the scope.
//! - Only the rows in view are built: the list is a gpui `list`, its rows
//!   as tall as they draw ([`layout`] has how they're laid out).
//!
//! It has no path and serializes to nothing, so it isn't kept in saved
//! layouts, and closes as any tab does.

use std::{
    cell::RefCell,
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
    rc::Rc,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};

use fe_command::Value as Json;
use fe_config::{SettingEditError, SettingEntry, SettingsModel};
use fe_gpui::commands::RunCommand;
use fe_ui::{
    Button, FamilySlot, FontFamilies, Icon, PAGE_TITLE, SegmentedControl, TextInput,
    TextInputEvent, TextRole, height, ui_theme,
};
use gpui::{
    App, AppContext as _, Context, Entity, FocusHandle, Focusable, Global, InteractiveElement as _,
    IntoElement, KeyDownEvent, ListAlignment, ListOffset, ListState, MouseButton,
    ParentElement as _, Pixels, Render, SharedString, StatefulInteractiveElement as _, Styled as _,
    Subscription, Task, WeakEntity, Window, div, list, px,
};
use kiln::platform::{
    config::{self, ChangedFiles},
    locale::ActiveLocale,
};
use kiln_l10n::{text, tr};
use kiln_shell::{PaneItem, PaneView as _};

use crate::{
    RootChanged, Workbench, open,
    picker::{self, PaletteOp},
    setup_wizard_preview::Swatches,
    workbench::toast::{self, Toast},
};

#[path = "settings_editor_controls.rs"]
mod controls;

#[path = "settings_editor_choices.rs"]
mod choices;

#[path = "settings_editor_layout.rs"]
mod layout;

#[cfg(test)]
#[path = "settings_editor_layout_tests.rs"]
mod layout_tests;

#[cfg(test)]
#[path = "settings_editor_choices_tests.rs"]
mod choices_tests;

#[cfg(test)]
#[path = "settings_editor_choices_font_tests.rs"]
mod choices_font_tests;

#[cfg(test)]
#[path = "settings_editor_open_file_tests.rs"]
mod open_file_tests;

#[cfg(test)]
#[path = "settings_editor_caret_tests.rs"]
mod caret_tests;

#[cfg(test)]
#[path = "settings_editor_locale_tests.rs"]
mod locale_tests;

use controls::{Change, Editing, Field, ListField, Select};
use layout::Section;
#[cfg(test)]
pub(crate) use layout::ThemeCard;

/// The key context the editor's element sets.
pub(crate) const KEY_CONTEXT: &str = "settings_editor";

/// The height of a setting's title line: its label, its key and Reset.
const LINE_HEIGHT: Pixels = px(22.);

/// The height of every row's control: the design's control, 26 px.
pub(crate) const CONTROL_HEIGHT: Pixels = height::CONTROL;

/// The page title's style: the design's page title, Instrument Serif 30/36.
pub(crate) const TITLE_ROLE: TextRole = PAGE_TITLE;

/// How far past the list's ends it lays rows out, so the rows next to the
/// view are measured before Up or Down reaches them.
const OVERDRAW: Pixels = px(240.);

/// The settings the Common section shows under the theme cards, in order,
/// by dotted key. With nothing searched for, they show there and not in
/// their own groups.
pub(crate) const COMMON: [&str; 4] = [
    "workbench.rail",
    "editor.font_family",
    "editor.font_size",
    "vim.enabled",
];

/// The top-level tables in the order the page lists them after the Common
/// section, the most often changed first (the UI audit's SE1); any other
/// after them, in the schema's order.
const SECTION_ORDER: [&str; 11] = [
    "editor",
    "theme",
    "workbench",
    "vim",
    "terminal",
    "lsp",
    "tree",
    "status_bar",
    "chat",
    "extensions",
    "update",
];

/// The setting the Common section's theme cards set.
pub(crate) const THEME_MODE: &str = "theme.mode";

/// The theme settings a theme card removes where they'd show another theme
/// than the card's ([`SettingsEditor::pick_theme_card`]).
const THEME_DARK: &str = "theme.dark";
const THEME_LIGHT: &str = "theme.light";
const THEME_NAME: &str = "theme.name";

/// Which settings file the editor shows values from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Scope {
    /// The user's `settings.toml`.
    User,
    /// The root's `.tenmoku/settings.toml`.
    Workspace,
}

impl Scope {
    fn setting_scope(self) -> fe_config::SettingScope {
        match self {
            Scope::User => fe_config::SettingScope::User,
            Scope::Workspace => fe_config::SettingScope::Workspace,
        }
    }
}

/// The user's settings file and the root's, as [`SettingsEditor::files`]
/// gives them.
type Files = (Option<PathBuf>, Option<PathBuf>);

/// The Settings tabs' writes, each after the one before, the last one
/// queued here: across tabs, so that a tab reopened writes after what the
/// old one left, and past them, so that what a closed tab took is written.
/// Once one lands, every other open tab reads the files again, not
/// waiting for the watcher.
#[derive(Default)]
struct SettingsWrites {
    last: Option<Task<()>>,
    tabs: Vec<WeakEntity<SettingsEditor>>,
}

impl Global for SettingsWrites {}

/// What a pending change is to: a dotted key, in a scope, in the file that
/// scope wrote to then (`None` with no file to write, no folder open). Two
/// changes to one key in different files, or different roots', are two.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct PendingKey {
    key: String,
    scope: Scope,
    file: Option<PathBuf>,
}

/// A change written or waiting to be, not read back yet, to a key in a
/// scope's file.
struct Pending {
    /// Which change to the key it is: a later one supersedes it.
    generation: u64,
    change: Change,
}

/// A row of the list.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Row {
    /// A group's heading, as in `Editor > Minimap`.
    Group(String),
    /// A setting: its index in [`SettingsModel::entries`].
    Setting(usize),
}

/// The Settings tab: see the module docs.
pub(crate) struct SettingsEditor {
    /// The whole tab's, around the search's and the list's: focus coming
    /// into it from elsewhere reads the files again.
    root_focus: FocusHandle,
    /// The list's: where the keys go once Down leaves the search.
    focus: FocusHandle,
    search: Entity<TextInput>,
    workbench: WeakEntity<Workbench>,
    scope: Scope,
    /// The workbench's root, whose settings file the Workspace scope shows.
    root: Option<PathBuf>,
    /// `None` until the files are first read.
    model: Option<SettingsModel>,
    /// What the search finds, with its groups' headings.
    rows: Vec<Row>,
    /// How many of the rows, from the first, are the Common section's: its
    /// heading, with the theme cards, and its settings. None while
    /// searching.
    common: usize,
    /// The theme cards' colours, each theme's own, read once.
    swatches: Swatches,
    /// Where each theme card was drawn in the last frame, `None` for one
    /// that wasn't: what the keys scroll into view.
    card_bounds: Rc<RefCell<[Option<gpui::Bounds<Pixels>>; 3]>>,
    /// The top-level groups the rows show settings of, as the category
    /// navigation lists them.
    sections: Vec<Section>,
    /// The heading row of the section last clicked in the navigation,
    /// until the rows change.
    anchor: Option<usize>,
    /// Whether the list keeps the anchor's heading at the top: from the
    /// click until the wheel or the keys scroll it.
    pinned: bool,
    /// How many times in a row the list has scrolled back to the anchor's
    /// heading for the same spacer: it gives up after a few.
    repins: u8,
    /// The height of the blank row after the last, so that a short last
    /// section's heading can come to the top: what the rows from the
    /// anchor's heading on lack of the view's height, as last laid out.
    spacer: Pixels,
    /// The selected row, a setting's.
    selected: Option<usize>,
    /// The row under the mouse, a setting's.
    hovered: Option<usize>,
    list: ListState,
    /// The read of the files under way, dropped for a newer one.
    load: Task<()>,
    /// The number and text inputs, by dotted key, kept across renders so
    /// focus and a half-typed value survive them.
    fields: HashMap<String, Field>,
    /// The list editors, by dotted key.
    lists: HashMap<String, ListField>,
    /// The open dropdown.
    select: Option<Select>,
    /// The dropdowns whose rows show an input instead, after Other…, by
    /// dotted key.
    typing: HashSet<String>,
    /// The changes written or waiting to be, not read back yet, by dotted
    /// key, scope and the file they're to: what their rows show meanwhile.
    pending: HashMap<PendingKey, Pending>,
    /// The generation the next change gets.
    next_generation: u64,
    /// The number the next read of the files gets, in the order the reads
    /// start, shared with the reads off the main thread.
    reads: Arc<AtomicU64>,
    /// The number of the read the model is from: an older read, finishing
    /// later, is out of date.
    shown_read: u64,
    /// Holds the writes until a message comes, or the sender is dropped.
    #[cfg(test)]
    gate: Option<async_channel::Receiver<()>>,
    /// Each range of rows the list built, for the tests.
    #[cfg(test)]
    built: Vec<std::ops::Range<usize>>,
    _subscriptions: Vec<Subscription>,
}

impl SettingsEditor {
    /// An editor of the settings of `workbench`, rooted at `root`, reading
    /// them at once.
    fn new(
        workbench: &Entity<Workbench>,
        root: Option<PathBuf>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let ui = ui_theme(cx);
        let search =
            cx.new(|cx| TextInput::new(text!("workbench-settings-search"), &ui, cx).bordered());
        let root_focus = cx.focus_handle();
        let subscriptions = vec![
            // Back from another tab or app: in case the watcher missed a
            // change, or the files changed while it wasn't running.
            cx.on_focus_in(&root_focus, window, |this, _, cx| this.reload(cx)),
            // Either file changed, though maybe no setting did.
            cx.observe_global::<ChangedFiles>(|this, cx| {
                let changed = config::changed_files(cx);
                let (user, project) = this.files(cx);
                if [user, project]
                    .iter()
                    .flatten()
                    .any(|file| changed.contains(file))
                {
                    this.reload(cx);
                }
            }),
            cx.subscribe(&search, |this, _, event, cx| {
                if *event == TextInputEvent::Changed {
                    this.refilter(cx);
                }
            }),
            // The model's help, headings and variants are resolved as it's
            // read: read again in the language just set, keeping what's
            // typed; an open dropdown takes the new text as it lands
            // ([`SettingsEditor::show_read`]), and a row's message is
            // resolved as it's drawn.
            cx.observe_global::<ActiveLocale>(|this, cx| {
                this.reload(cx);
                cx.notify();
            }),
            cx.subscribe(workbench, |this, _, RootChanged(root), cx| {
                this.root = Some(root.clone());
                // Uncommitted inputs were typed against the old root's
                // file: drop them, and their blur subscriptions, so a late
                // blur writes nothing to the new root.
                this.select = None;
                this.typing.clear();
                this.fields.clear();
                this.lists.clear();
                this.reload(cx);
                cx.notify();
            }),
        ];
        let mut this = Self {
            root_focus,
            focus: cx.focus_handle(),
            search,
            workbench: workbench.downgrade(),
            scope: Scope::User,
            root,
            model: None,
            rows: Vec::new(),
            common: 0,
            swatches: Swatches::load(),
            card_bounds: Rc::default(),
            sections: Vec::new(),
            anchor: None,
            pinned: false,
            repins: 0,
            spacer: Pixels::ZERO,
            selected: None,
            hovered: None,
            list: ListState::new(0, ListAlignment::Top, OVERDRAW),
            load: Task::ready(()),
            fields: HashMap::new(),
            lists: HashMap::new(),
            select: None,
            typing: HashSet::new(),
            pending: HashMap::new(),
            next_generation: 0,
            reads: Arc::new(AtomicU64::new(1)),
            shown_read: 0,
            #[cfg(test)]
            gate: None,
            #[cfg(test)]
            built: Vec::new(),
            _subscriptions: subscriptions,
        };
        let tab = cx.weak_entity();
        // The wheel takes over from a section clicked.
        this.list.set_scroll_handler({
            let tab = tab.clone();
            move |_, _, cx| {
                tab.update(cx, |this, _| this.pinned = false).ok();
            }
        });
        let tabs = &mut cx.default_global::<SettingsWrites>().tabs;
        tabs.retain(|tab| tab.upgrade().is_some());
        tabs.push(tab);
        this.reload(cx);
        this
    }

    /// Reads the user's and the root's settings files again, off the main
    /// thread, and shows what they hold once read.
    pub(crate) fn reload(&mut self, cx: &mut Context<Self>) {
        let (user, project) = self.files(cx);
        let reads = self.reads.clone();
        let read = cx.background_spawn(async move { read_model(&reads, user, project) });
        self.load = cx.spawn(async move |this, cx| {
            let (read, model) = read.await;
            this.update(cx, |this, cx| this.show_read(read, model, cx))
                .ok();
        });
    }

    /// Shows `model`, from read number `read`, unless what's shown is from
    /// a read that started later.
    pub(crate) fn show_read(&mut self, read: u64, model: SettingsModel, cx: &mut Context<Self>) {
        if read < self.shown_read {
            return;
        }
        self.shown_read = read;
        self.model = Some(model);
        self.refresh_select(cx);
        self.refresh_messages();
        self.refilter(cx);
    }

    /// The number the next read of the files gets: see
    /// [`SettingsEditor::show_read`].
    #[cfg(test)]
    pub(crate) fn next_read(&self) -> u64 {
        self.reads.fetch_add(1, Ordering::SeqCst)
    }

    /// Holds every write from now until the sender sends, once a write, or
    /// is dropped.
    #[cfg(test)]
    pub(crate) fn hold_writes(&mut self) -> async_channel::Sender<()> {
        let (sender, gate) = async_channel::unbounded();
        self.gate = Some(gate);
        sender
    }

    /// The user's settings file and the root's.
    fn files(&self, cx: &App) -> Files {
        let project = self.root.as_deref().map(fe_config::project_settings_path);
        (open::settings_path(cx), project)
    }

    /// The file `scope` writes to, of `files`.
    fn target(scope: Scope, files: &Files) -> Option<PathBuf> {
        match scope {
            Scope::User => files.0.clone(),
            Scope::Workspace => files.1.clone(),
        }
    }

    /// Makes `change` at `key` in the selected scope's file, after the
    /// writes before it, any Settings tab's ([`SettingsWrites`]), then
    /// reads the files again and shows them, unless the files shown have
    /// changed since (a new root, say). Until then the row shows the
    /// change. If the write fails, a toast says why, tab or no tab, and the
    /// row shows the file's value again, unless a later change is waiting.
    fn write(&mut self, key: &[String], change: Change, cx: &mut Context<Self>) {
        let scope = self.scope();
        let root = self.root.clone().unwrap_or_default();
        let files = self.files(cx);
        // The write goes where `set_setting` puts the user's file: a test
        // must have it be the one the tab reads, not the real one.
        #[cfg(test)]
        if scope == Scope::User {
            assert_eq!(
                files.0,
                fe_config::user_settings_path(),
                "a test writing the user's settings sets TENMOKU_CONFIG_DIR to its config directory",
            );
        }
        self.next_generation += 1;
        let generation = self.next_generation;
        let dotted = key.join(".");
        let pending = Pending {
            generation,
            change: change.clone(),
        };
        let pending_key = PendingKey {
            key: dotted.clone(),
            scope,
            file: Self::target(scope, &files),
        };
        self.pending.insert(pending_key.clone(), pending);
        let (key, reads) = (key.to_vec(), self.reads.clone());
        #[cfg(test)]
        let gate = self.gate.clone();
        let previous = cx.default_global::<SettingsWrites>().last.take();
        let write = cx.spawn(async move |this, cx| {
            if let Some(previous) = previous {
                previous.await;
            }
            #[cfg(test)]
            if let Some(gate) = gate {
                gate.recv().await.ok();
            }
            let read = files.clone();
            let written = cx.background_spawn(async move {
                let key: Vec<&str> = key.iter().map(String::as_str).collect();
                change.write(scope.setting_scope(), &root, &key)?;
                Ok::<_, SettingEditError>(read_model(&reads, read.0, read.1))
            });
            let written = written.await;
            // Said here, not by the tab, which may have closed.
            if let Err(error) = &written {
                log::warn!("can't set {dotted}: {error}");
                cx.update(|cx| {
                    toast::show(
                        cx,
                        Toast::error(tr!("workbench-settings-cant-change", key = dotted.as_str()))
                            .detail(error.to_string()),
                    )
                });
            }
            if written.is_ok() {
                let tabs = cx.update(|cx| cx.default_global::<SettingsWrites>().tabs.clone());
                let others = tabs
                    .iter()
                    .filter(|tab| tab.entity_id() != this.entity_id());
                for tab in others {
                    tab.update(cx, |tab, cx| tab.reload(cx)).ok();
                }
            }
            this.update(cx, |this, cx| {
                this.written(pending_key, generation, files, written.ok(), cx)
            })
            .ok();
        });
        cx.default_global::<SettingsWrites>().last = Some(write);
        cx.notify();
    }

    /// Shows the files as change `generation`'s write to `key` in
    /// `scope`'s file read them after, if they're still the files shown;
    /// `None` if the write failed. Either way, the row stops showing the
    /// change, unless a later one is waiting.
    fn written(
        &mut self,
        pending: PendingKey,
        generation: u64,
        files: Files,
        written: Option<(u64, SettingsModel)>,
        cx: &mut Context<Self>,
    ) {
        if (self.pending.get(&pending)).is_some_and(|p| p.generation == generation) {
            self.pending.remove(&pending);
        }
        match written {
            Some((read, model)) if self.files(cx) == files => self.show_read(read, model, cx),
            // A new root's read may have missed the write.
            Some(_) => self.reload(cx),
            None => {}
        }
        cx.notify();
    }

    /// The rows for the search as it is now, keeping the selected setting
    /// selected if it's still found. New rows start at the top; the same
    /// rows, read again, are measured again where they are.
    fn refilter(&mut self, cx: &mut Context<Self>) {
        let selected = self.selected_setting();
        let cards = self.cards_selected();
        let (rows, common) = match &self.model {
            Some(model) => rows(model, self.search.read(cx).text()),
            None => (Vec::new(), 0),
        };
        if rows == self.rows {
            self.list.remeasure();
        } else {
            // And the spacer after them.
            self.list.reset(rows.len() + 1);
            self.anchor = None;
            self.pinned = false;
            self.spacer = Pixels::ZERO;
            self.hovered = None;
        }
        self.rows = rows;
        self.common = common;
        self.sections = match &self.model {
            Some(model) => layout::sections(model, &self.rows, common),
            None => Vec::new(),
        };
        self.selected = selected
            .and_then(|entry| (self.rows.iter()).position(|row| *row == Row::Setting(entry)))
            .or((cards && common > 0).then_some(0));
        cx.notify();
    }

    /// The selected setting's index in the model's entries.
    fn selected_setting(&self) -> Option<usize> {
        match self.rows.get(self.selected?)? {
            Row::Setting(entry) => Some(*entry),
            Row::Group(_) => None,
        }
    }

    /// The selected setting.
    fn selected_entry(&self) -> Option<&SettingEntry> {
        let entry = self.selected_setting()?;
        self.model.as_ref()?.entries().get(entry)
    }

    /// The setting at dotted `key`.
    fn entry(&self, key: &str) -> Option<&SettingEntry> {
        let model = self.model.as_ref()?;
        model.entries().iter().find(|entry| entry.key_path() == key)
    }

    /// `entry`'s value as the selected scope shows it, and whether that
    /// scope's file sets it: a change waiting to be written or read back
    /// shows as if it were.
    fn value(&self, entry: &SettingEntry, cx: &App) -> Option<(Json, bool)> {
        let model = self.model.as_ref()?;
        let scope = self.scope();
        let pending = self.pending.get(&PendingKey {
            key: entry.key_path(),
            scope,
            file: Self::target(scope, &self.files(cx)),
        });
        Some(match pending.map(|pending| &pending.change) {
            Some(Change::Set(value)) => (value.json(), true),
            Some(Change::Reset) => {
                let (value, _) = scope_value(model, entry, scope, true);
                (value, false)
            }
            None => scope_value(model, entry, scope, false),
        })
    }

    /// Whether the selected scope's file can't set `entry`.
    fn restricted(&self, entry: &SettingEntry) -> bool {
        let model = self.model.as_ref();
        self.scope() == Scope::Workspace && model.is_some_and(|m| !m.workspace_allowed(entry))
    }

    /// The scopes the editor offers: Workspace only with a folder open.
    fn scopes(&self) -> &'static [Scope] {
        match self.root {
            Some(_) => &[Scope::User, Scope::Workspace],
            None => &[Scope::User],
        }
    }

    /// The scope whose values show: Workspace falls back to User with no
    /// folder open.
    pub(crate) fn scope(&self) -> Scope {
        match self.root {
            Some(_) => self.scope,
            None => Scope::User,
        }
    }

    /// Shows `scope`'s values, reading the files again.
    pub(crate) fn set_scope(&mut self, scope: Scope, cx: &mut Context<Self>) {
        self.scope = scope;
        self.select = None;
        // The inputs are made again with the new scope's values, dropping
        // any message, and a late blur of the old ones commits nothing.
        self.typing.clear();
        self.fields.clear();
        self.lists.clear();
        self.reload(cx);
        cx.notify();
    }

    /// Shows `scope`'s values, as [`SettingsEditor::set_scope`] does; a
    /// row's input that had focus, made again for the new scope, gives it
    /// to the list of rows, where the keys go on from the selected row.
    fn switch_to(&mut self, scope: Scope, window: &mut Window, cx: &mut Context<Self>) {
        let editing = self.editing(window, cx).is_some();
        self.set_scope(scope, cx);
        if editing {
            window.focus(&self.focus, cx);
        }
    }

    /// The model the rows show, once the files are read.
    #[cfg(test)]
    pub(crate) fn model(&self) -> Option<&SettingsModel> {
        self.model.as_ref()
    }

    #[cfg(test)]
    pub(crate) fn rows(&self) -> &[Row] {
        &self.rows
    }

    #[cfg(test)]
    pub(crate) fn selected(&self) -> Option<usize> {
        self.selected
    }

    #[cfg(test)]
    pub(crate) fn search_input(&self) -> &Entity<TextInput> {
        &self.search
    }

    #[cfg(test)]
    pub(crate) fn list_focus(&self) -> &FocusHandle {
        &self.focus
    }

    /// Each range of rows the list built, dropping them.
    #[cfg(test)]
    pub(crate) fn take_built(&mut self) -> Vec<std::ops::Range<usize>> {
        std::mem::take(&mut self.built)
    }

    /// `entry`'s value as the selected scope's file and the one it
    /// inherits from hold it, as text, and whether that scope's file sets
    /// it; `None` until the files are read.
    #[cfg(test)]
    pub(crate) fn shown_value(&self, entry: &SettingEntry) -> Option<(String, bool)> {
        let model = self.model.as_ref()?;
        Some(as_text(scope_value(model, entry, self.scope(), false)))
    }

    /// [`SettingsEditor::shown_value`] as the row shows it, a change still
    /// being written included.
    #[cfg(test)]
    pub(crate) fn row_value(&self, entry: &SettingEntry, cx: &App) -> Option<(String, bool)> {
        Some(as_text(self.value(entry, cx)?))
    }

    /// Selects the setting below the selected one, or with `down` false the
    /// one above, skipping headings, or the first with none selected, and
    /// scrolls it into view.
    fn move_selection(&mut self, down: bool, window: &mut Window, cx: &mut Context<Self>) {
        // The settings, and the Common section's heading, its theme cards.
        let settings: Vec<usize> = (self.rows.iter().enumerate())
            .filter(|(ix, row)| matches!(row, Row::Setting(_)) || (*ix == 0 && self.common > 0))
            .map(|(ix, _)| ix)
            .collect();
        let at = self
            .selected
            .and_then(|selected| settings.iter().position(|ix| *ix == selected));
        let next = match (at, down) {
            (None, _) => settings.first(),
            (Some(at), true) => settings.get(at + 1).or(settings.get(at)),
            (Some(at), false) => settings.get(at.saturating_sub(1)),
        };
        let Some(&next) = next else {
            return;
        };
        self.selected = Some(next);
        self.select = None;
        self.pinned = false;
        // The theme cards' row: its ringed card, which a short or narrow
        // list may not show with the heading at its top.
        match next == 0 && self.common > 0 {
            true => self.reveal_cards(window, cx),
            false => self.reveal(next, down),
        }
        cx.notify();
    }

    /// Scrolls row `ix`, just selected going `down` or up, into view, as
    /// little as it takes. Going up to a group's first setting, its heading
    /// comes into view with it, the heading at the top, if both fit, at the
    /// heights they were drawn, and either is out of view. A row below the view that the list hasn't
    /// measured yet (keys pressed faster than it draws) comes to the top.
    fn reveal(&self, ix: usize, down: bool) {
        let list = &self.list;
        let view = list.viewport_bounds();
        let in_view = |ix: usize| {
            (list.bounds_for_item(ix))
                .is_some_and(|row| row.top() >= view.top() && row.bottom() <= view.bottom())
        };
        // Unmeasured, a setting is taken to be a tall one's height.
        let height = (list.bounds_for_item(ix)).map_or(px(120.), |row| row.size.height);
        // A heading as tall as it was drawn; unmeasured, as tall as it can
        // be: the Common section's, with its cards, is taller than others.
        let heading_height = |heading: usize| match list.bounds_for_item(heading) {
            Some(bounds) => bounds.size.height,
            None if heading == 0 && self.common > 0 => layout::COMMON_HEADING_HEIGHT,
            None => layout::HEADING_HEIGHT,
        };
        let heading = (ix.checked_sub(1)).filter(|heading| {
            !down
                && view.size.height >= heading_height(*heading) + height
                && matches!(self.rows[*heading], Row::Group(_))
        });
        let top = |item_ix| ListOffset {
            item_ix,
            offset_in_item: Pixels::ZERO,
        };
        match heading {
            Some(heading) if !(in_view(heading) && in_view(ix)) => list.scroll_to(top(heading)),
            Some(_) => {}
            None if in_view(ix) => {}
            None if ix <= list.logical_scroll_top().item_ix
                || list.bounds_for_item(ix).is_some() =>
            {
                list.scroll_to_reveal_item(ix)
            }
            None => list.scroll_to(top(ix)),
        }
    }

    /// The text inputs' editing and caret commands (`text_input.backspace`,
    /// `text_input.move_left` and the like, which an input leaves to its
    /// host), for the search or the
    /// row's input that has focus, unless that's off; others go on.
    fn on_input_command(&mut self, run: &RunCommand, window: &mut Window, cx: &mut Context<Self>) {
        let Some(edit) = picker::op(run, cx).and_then(PaletteOp::edit) else {
            return cx.propagate();
        };
        let input = match self.editing(window, cx) {
            Some(editing) => self.input_of(&editing),
            None if self.search.focus_handle(cx).is_focused(window) => Some(self.search.clone()),
            None => None,
        };
        match input {
            // A row's input that's off takes no edit: its row's file
            // can't set it.
            Some(input) if !input.read(cx).typing() => {}
            Some(input) => input.update(cx, edit),
            None => cx.propagate(),
        }
    }

    /// Its keys: see the module docs.
    fn on_key_down(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let keystroke = &event.keystroke;
        let m = keystroke.modifiers;
        let searching = self.search.focus_handle(cx).is_focused(window);
        let editing = self.editing(window, cx);
        let listing = self.focus.is_focused(window);
        match keystroke.key.as_str() {
            "left" | "right" | "home" | "end" | "enter" | "space"
                if !m.modified() && listing && self.cards_selected() =>
            {
                self.step_theme_card(keystroke.key.as_str(), window, cx);
            }
            "up" | "down" if !m.modified() && listing && self.select.is_some() => {
                self.move_highlight(keystroke.key == "down", cx);
            }
            // In a list's item: to the item before or after it.
            "up" | "down" if !m.modified() && matches!(editing, Some(Editing::Item(..))) => {
                if let Some(Editing::Item(key, item)) = &editing {
                    self.focus_item(key, *item, keystroke.key == "down", window, cx);
                }
            }
            "up" | "down" if !m.modified() && editing.is_none() => {
                let down = keystroke.key == "down";
                if searching && down {
                    window.focus(&self.focus, cx);
                }
                self.move_selection(down, window, cx);
            }
            "enter" if !m.modified() && editing.is_some() => {
                if let Some(editing) = editing {
                    self.commit(&editing, true, cx);
                }
            }
            "enter" if !m.modified() && listing => self.activate(window, cx),
            "space" if !m.modified() && listing => {
                if !self.toggle_selected(cx) {
                    return;
                }
            }
            "escape" if !m.modified() && self.select.is_some() => {
                self.select = None;
                cx.notify();
            }
            "escape" if !m.modified() && editing.is_some() => {
                if let Some(editing) = editing {
                    self.revert(&editing, cx);
                }
                window.focus(&self.focus, cx);
            }
            "/" if !m.modified() && !searching && editing.is_none() => {
                window.focus(&self.search.focus_handle(cx), cx);
            }
            "f" if m.secondary() && !m.alt && !m.shift => {
                window.focus(&self.search.focus_handle(cx), cx);
            }
            "escape" if !m.modified() && !self.search.read(cx).text().is_empty() => {
                self.search.update(cx, |search, cx| search.clear(cx));
            }
            _ => return,
        }
        cx.stop_propagation();
    }

    /// The Reset button of row `ix`, `entry`'s, which puts back its default.
    fn reset_button(
        &self,
        ix: usize,
        entry: &SettingEntry,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let key = entry.key.clone();
        let path = entry.key_path();
        div().flex_none().child(
            Button::new(("setting-reset", ix), text!("workbench-settings-reset"))
                .name(tr!("workbench-settings-reset-tooltip"))
                .ghost()
                .small()
                .debug_selector(move || selector("setting-reset", &path))
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.reset(&key, window, cx);
                    cx.stop_propagation();
                })),
        )
    }

    /// The selected scope's settings file, as the Open button shows it.
    pub(crate) fn open_file_target(&self, cx: &App) -> Option<PathBuf> {
        Self::target(self.scope(), &self.files(cx))
    }

    /// The page's top: the serif title, the You / This project switch and
    /// the Open button on one line, and the search under them.
    fn header(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let ui = ui_theme(cx);
        let tip = self.open_file_target(cx).map_or_else(
            || tr!("workbench-settings-no-config-dir").to_string(),
            |path| path.display().to_string(),
        );
        let shown = self.scope();
        let scopes = self.scopes();
        let labels = scopes.iter().map(|scope| match scope {
            Scope::User => tr!("workbench-settings-scope-user"),
            Scope::Workspace => tr!("workbench-settings-scope-project"),
        });
        let switch = div()
            .debug_selector(|| "settings-scope".into())
            .flex_none()
            .child(
                SegmentedControl::new("settings-scope", tr!("workbench-settings-scope"))
                    .segments(labels)
                    .selected(scopes.iter().position(|scope| *scope == shown).unwrap_or(0))
                    .on_change(cx.listener(move |this, ix: &usize, window, cx| {
                        if let Some(scope) = scopes.get(*ix) {
                            this.switch_to(*scope, window, cx);
                        }
                    })),
            );
        let open = div()
            .id("setting-open-file")
            .debug_selector(|| "setting-open-file".into())
            .flex_none()
            .text_color(ui.accent)
            .cursor_pointer()
            .hover(|style| style.underline())
            .child(tr!("workbench-settings-open-file"))
            .tooltip(move |_, cx| {
                cx.new(|_| fe_ui::Tooltip::new(tip.clone(), None::<&str>, &ui))
                    .into()
            })
            .on_click(cx.listener(|this, _, window, cx| this.edit_in_file(&[], window, cx)));
        let role = TITLE_ROLE;
        let family = (cx.try_global::<FontFamilies>().cloned().unwrap_or_default())
            .family(FamilySlot::Title, cx);
        let title = div()
            .debug_selector(|| "settings-title".into())
            .flex_none()
            .font_family(family)
            .text_size(role.size)
            .line_height(role.line_height)
            .font_weight(gpui::FontWeight(f32::from(role.weight)))
            .text_color(ui.text)
            .child(tr!("workbench-settings-title"));
        div()
            .flex_none()
            .flex()
            .flex_col()
            .gap_3()
            .px_6()
            .pt_4()
            .pb_3()
            .border_b_1()
            .border_color(ui.border)
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .gap_x_6()
                    .gap_y_2()
                    .child(title)
                    .child(div().flex_1())
                    .child(switch)
                    .child(open),
            )
            .child(
                div()
                    .debug_selector(|| "settings-search".into())
                    .w_full()
                    .max_w(layout::COLUMN_WIDTH)
                    .child(self.search.clone()),
            )
    }
}

/// The settings in the user's file and the project's, either of which may
/// be missing, with the number of the read, taken from `reads` as it
/// starts.
fn read_model(
    reads: &AtomicU64,
    user: Option<PathBuf>,
    project: Option<PathBuf>,
) -> (u64, SettingsModel) {
    let read = reads.fetch_add(1, Ordering::SeqCst);
    let user = user.and_then(|path| read_settings(&path));
    let project = project.and_then(|path| read_settings(&path));
    (
        read,
        SettingsModel::new(user.as_deref(), project.as_deref()),
    )
}

/// `path`'s text; `None` if it's missing, or unreadable (logged).
fn read_settings(path: &Path) -> Option<String> {
    match std::fs::read_to_string(path) {
        Ok(text) => Some(text),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => {
            log::warn!("can't read {}: {error}", path.display());
            None
        }
    }
}

/// The rows `query` finds in `model`: with nothing searched for, the
/// Common section first, its heading and its settings ([`COMMON`]); then
/// the top level's settings, then each group's under its heading, the
/// top-level tables in [`SECTION_ORDER`], each with its nested groups in
/// the schema's order, leaving out the groups it finds nothing in and the
/// settings the Common section shows. With them, how many rows are the
/// Common section's.
pub(crate) fn rows(model: &SettingsModel, query: &str) -> (Vec<Row>, usize) {
    let entries = model.entries();
    let common: Vec<usize> = match query.trim().is_empty() {
        true => (COMMON.iter())
            .filter_map(|key| entries.iter().position(|entry| entry.key_path() == *key))
            .collect(),
        false => Vec::new(),
    };
    let mut found = model.search(query).into_iter().peekable();
    // The search keeps the schema's order.
    let matched: Vec<bool> = (entries.iter())
        .map(|entry| {
            let hit = found.peek().is_some_and(|hit| std::ptr::eq(*hit, entry));
            if hit {
                found.next();
            }
            hit
        })
        .collect();
    let in_group = |key: &[String]| -> Vec<Row> {
        (entries.iter().enumerate())
            .filter(|(ix, entry)| {
                matched[*ix] && !common.contains(ix) && entry.key[..entry.key.len() - 1] == *key
            })
            .map(|(ix, _)| Row::Setting(ix))
            .collect()
    };
    let mut rows = Vec::new();
    if !common.is_empty() {
        rows.push(Row::Group(tr!("workbench-settings-common").to_string()));
        rows.extend(common.iter().map(|ix| Row::Setting(*ix)));
    }
    let common = rows.len();
    rows.extend(in_group(&[]));
    // Stable, so a table's nested groups stay after it.
    let mut groups: Vec<_> = model.groups().iter().collect();
    groups.sort_by_key(|group| {
        let top = group.key.first().map(String::as_str);
        (SECTION_ORDER.iter())
            .position(|name| Some(*name) == top)
            .unwrap_or(SECTION_ORDER.len())
    });
    for group in groups {
        let settings = in_group(&group.key);
        if !settings.is_empty() {
            rows.push(Row::Group(group.title.clone()));
            rows.extend(settings);
        }
    }
    (rows, common)
}

/// `entry`'s value as `scope` shows it: its file's, else, or if `reset`,
/// what it inherits (the user's, under Workspace) or the default; and
/// whether `scope`'s file sets it.
fn scope_value(
    model: &SettingsModel,
    entry: &SettingEntry,
    scope: Scope,
    reset: bool,
) -> (Json, bool) {
    let set = match scope {
        _ if reset => None,
        Scope::User => model.user_value(entry),
        Scope::Workspace => model.workspace_value(entry),
    };
    let modified = set.is_some();
    let inherited = match scope {
        Scope::User => None,
        Scope::Workspace => model.user_value(entry),
    };
    let value = (set.or(inherited)).unwrap_or_else(|| entry.default.clone());
    (value, modified)
}

/// A value and whether it's modified, as the tests compare them: a string
/// as itself, and null as `Not set`.
#[cfg(test)]
fn as_text((value, modified): (Json, bool)) -> (String, bool) {
    let text = match value.as_str() {
        Some(text) => text.to_owned(),
        None if value.is_null() => "Not set".to_owned(),
        None => value.to_string(),
    };
    (text, modified)
}

/// [`layout::help_line`]: the one line of help a row shows of `text`.
#[cfg(test)]
pub(crate) fn help_line_for_tests(text: &str) -> String {
    layout::help_line(text)
}

/// [`controls::table_row`], of a dotted key.
#[cfg(test)]
pub(crate) fn table_row_for_tests(text: &str, key: &str) -> Option<u32> {
    let key: Vec<String> = key.split('.').map(str::to_owned).collect();
    controls::table_row(text, &key)
}

/// A debug selector or element id of a row's part: `name`, a dash and
/// `key`, the setting's dotted key, say, as in `settings-row-editor.tab_size`.
pub(crate) fn selector(name: &str, key: &str) -> String {
    format!("{name}-{key}")
}

/// `name` as a title, as the model titles groups: `status_bar` is `Status
/// Bar`, and `lsp` is `LSP`.
fn title(name: &str) -> String {
    name.split(['_', '-'])
        .filter(|word| !word.is_empty())
        .map(|word| match word {
            "lsp" | "ai" | "ui" => word.to_uppercase(),
            _ => {
                let mut chars = word.chars();
                (chars.next())
                    .map(|first| first.to_uppercase().chain(chars).collect())
                    .unwrap_or_default()
            }
        })
        .collect::<Vec<String>>()
        .join(" ")
}

/// The list editors' buttons, Reset to Default and the You / This
/// project switch, by keyboard (A11Y-16b).
#[kiln::command(context = "settings_editor")]
impl SettingsEditor {
    /// Settings: Add List Item
    ///
    /// Adds an item to the list whose item has focus, else the selected
    /// row's list, as its Add Item button does, and focuses it.
    #[command(id = "settings.add_list_item")]
    fn add_list_item(&mut self, cx: &mut kiln::Cx<Self>) {
        self.add_focused_item(cx.window, cx.cx);
    }

    /// Settings: Remove List Item
    ///
    /// Removes the list item that has focus, as its remove button does,
    /// and focuses the item after it, else the one before.
    #[command(id = "settings.remove_list_item")]
    fn remove_list_item(&mut self, cx: &mut kiln::Cx<Self>) {
        self.remove_focused_item(cx.window, cx.cx);
    }

    /// Settings: Reset to Default
    ///
    /// Removes the selected setting from the shown scope's file, as its
    /// Reset button does.
    #[command(id = "settings.reset_to_default")]
    fn reset_to_default(&mut self, cx: &mut kiln::Cx<Self>) {
        self.reset_selected(cx.window, cx.cx);
    }

    /// Settings: Switch Scope
    ///
    /// Shows the other scope's values: This project's after You's, You's
    /// after This project's, with a folder open.
    #[command(id = "settings.switch_scope")]
    fn switch_scope(&mut self, cx: &mut kiln::Cx<Self>) {
        let scopes = self.scopes();
        let at = scopes.iter().position(|scope| *scope == self.scope());
        if let (Some(at), true) = (at, scopes.len() > 1) {
            self.switch_to(scopes[(at + 1) % scopes.len()], cx.window, cx.cx);
        }
    }
}

impl SettingsEditor {
    /// `settings.reset_to_default`: the selected row's Reset, where it
    /// shows one: its scope's file sets it, and can.
    fn reset_selected(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(entry) = self.selected_entry().cloned() else {
            return;
        };
        let modified = self.value(&entry, cx).is_some_and(|(_, set)| set);
        if modified && !self.restricted(&entry) {
            self.reset(&entry.key, window, cx);
        }
    }
}

impl Focusable for SettingsEditor {
    /// The search field's, so showing the tab focuses it.
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.search.focus_handle(cx)
    }
}

impl Render for SettingsEditor {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let ui = ui_theme(cx);
        // The cards drawn in this frame record themselves again.
        *self.card_bounds.borrow_mut() = [None; 3];
        let errors = (self.model.iter())
            .flat_map(|model| model.errors())
            .map(|error| {
                div()
                    .px_6()
                    .py_1()
                    .text_size(px(12.))
                    .text_color(ui.error_text)
                    .child(SharedString::from(error.clone()))
            })
            .collect::<Vec<_>>();
        let body = if self.model.is_some() && self.rows.is_empty() {
            div()
                .debug_selector(|| "settings-empty".into())
                .px_6()
                .py_4()
                .text_color(ui.muted_text)
                .child(tr!("workbench-settings-nothing-found"))
                .into_any_element()
        } else {
            list(
                self.list.clone(),
                cx.processor(|this, ix, window, cx| this.render_row(ix, window, cx)),
            )
            .size_full()
            .into_any_element()
        };
        // Once the list is laid out: the spacer and the anchor's heading.
        let settle = gpui::canvas(
            {
                let tab = cx.weak_entity();
                move |_, window, cx| {
                    tab.update(cx, |this, cx| this.settle_anchor(window, cx))
                        .ok();
                }
            },
            |_, _, _, _| {},
        )
        .absolute()
        .size_0();
        // Under an open dropdown, a click anywhere else in the tab closes
        // it, its button's too, and does nothing else.
        let backdrop = self.select.is_some().then(|| {
            div().absolute().inset_0().occlude().on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| {
                    this.select = None;
                    cx.notify();
                }),
            )
        });
        <Self as kiln::ViewCommands>::attach(div(), cx)
            .track_focus(&self.root_focus)
            .debug_selector(|| "settings-editor".into())
            .key_context(KEY_CONTEXT)
            .relative()
            .on_key_down(cx.listener(Self::on_key_down))
            .on_action(cx.listener(Self::on_input_command))
            .size_full()
            .flex()
            .flex_col()
            .bg(ui.background)
            .text_color(ui.text)
            .text_size(px(13.))
            .child(self.header(cx))
            .children(errors)
            // The list, and the navigation on its left; in a tab too narrow
            // for both, the list wraps to the first line, alone, and the
            // navigation to the second, out of view.
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .overflow_hidden()
                    .flex()
                    .flex_row_reverse()
                    .flex_wrap()
                    .child(
                        div()
                            .track_focus(&self.focus)
                            .debug_selector(|| "settings-list".into())
                            .h_full()
                            .flex_grow(1.)
                            .flex_basis(layout::NAV_MIN_TAB_WIDTH - layout::NAV_WIDTH)
                            .min_w_0()
                            .relative()
                            .flex()
                            .flex_col()
                            .child(body)
                            .child(settle),
                    )
                    .child(self.nav(cx)),
            )
            .children(backdrop)
    }
}

impl PaneItem for SettingsEditor {
    const KIND: &'static str = "fastedit.settings_editor";

    fn tab_title(&self, _: &App) -> SharedString {
        tr!("workbench-settings-title")
    }

    fn tab_icon(&self, _: &App) -> Option<Icon> {
        Some(Icon::FileCog)
    }

    fn window_title(&self, cx: &App) -> SharedString {
        format!("{} — {}", self.tab_title(cx), fe_config::APP_NAME).into()
    }
}

impl Workbench {
    /// Shows the Settings tab: the one open in any pane, activating that
    /// pane, or else a new one in the active pane. Either way its search
    /// field takes focus.
    pub(crate) fn open_settings_ui(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        for pane in self.panes(cx) {
            let open = (pane.read(cx).tabs().iter())
                .position(|tab| tab.item().downcast_ref::<SettingsEditor>().is_some());
            if let Some(ix) = open {
                pane.update(cx, |pane, cx| pane.activate(ix, window, cx));
                self.activate(pane, window, cx);
                return;
            }
        }
        let (workbench, root) = (cx.entity(), self.root().map(Path::to_owned));
        let editor = cx.new(|cx| SettingsEditor::new(&workbench, root, window, cx));
        self.pane().clone().update(cx, |pane, cx| {
            pane.insert_item(Box::new(editor), None, window, cx);
            cx.notify();
        });
    }

    /// Has each open Settings tab read the settings files again: they
    /// changed on disk.
    pub(crate) fn reload_settings_editors(&self, cx: &mut App) {
        for pane in self.panes(cx) {
            let editors: Vec<Entity<SettingsEditor>> = (pane.read(cx).tabs().iter())
                .filter_map(|tab| tab.item().downcast::<SettingsEditor>())
                .collect();
            for editor in editors {
                editor.update(cx, |editor, cx| editor.reload(cx));
            }
        }
    }
}
