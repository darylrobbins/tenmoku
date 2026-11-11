//! The Keyboard Shortcuts editor, as VS Code's
//! (`workbench.action.openGlobalKeybindings`): a tab, opened by
//! `workbench.open_keyboard_shortcuts` (secondary-k secondary-s), that
//! lists every command with each of its bindings, a line per binding, as a
//! table of Command, Keybinding, When and Source. The rows are
//! [`ShortcutsModel`]'s, built from the keymap keys are matched against
//! ([`fe_gpui::current_keymap`]) and the commands the palette lists (the
//! typed ones, the app registry's and [`ListedCommands`]).
//!
//! - A search field on top, focused when the tab opens, filters the lines
//!   as it's typed ([`ShortcutsModel::search`]): by command title or ID,
//!   by when-text, or by keys, as `ctrl+k`, `cmd-k` or `"ctrl-k ctrl-s"`.
//! - A binding that clashes with another command's keys in the
//!   workbench or in an editor shows a warning icon, whose tooltip names
//!   the other commands ([`conflict_note`]); one a later entry with the
//!   same keys and context replaces is dimmed, its tooltip naming that
//!   entry's location ([`ShortcutsEditor::hidden_note`]).
//! - A right click on a line opens its menu: Copy Command ID, Add
//!   Keybinding (below), Remove Keybinding (also Delete, or
//!   secondary-backspace, on the selected line), Reset Keybinding on a
//!   command with entries in the user's keymap, and Show Same
//!   Keybindings, which searches for its keys. Up and Down move the
//!   menu's highlight and Enter chooses it; on the selected line, the
//!   palette's `shortcuts.copy_command_id`, `shortcuts.reset_binding` and
//!   `shortcuts.show_same_keybindings` do what those items do. Adding, changing,
//!   removing and resetting write the user's `keymap.toml`
//!   ([`fe_keymap::write_keymap`]) off the main thread, a failure shown in
//!   a toast; the watcher's reload of the file swaps the keymap in use,
//!   and the tab follows.
//! - It follows the keymap in use as the menu bar does: swapping it
//!   ([`fe_gpui::set_user_keymap`]) redraws every window, and the tab,
//!   drawing, rebuilds its rows if it changed. A tab in the background
//!   rebuilds when it's next shown.
//! - Up and Down move the selected line, scrolling it into view; Down in
//!   the search field moves into the list. `/` or secondary-f goes back
//!   to the search, and Escape closes the menu, else clears the search.
//!   The search takes the text inputs' editing keys.
//! - Keys for a binding, to add one or change it, are recorded in a box
//!   in the line's Keybinding cell: see [`recorder`]. Enter, a double
//!   click or a line's pencil (Add, on a line without a binding) opens it.
//! - Only the lines in view are built: the list is a `uniform_list`.
//!
//! The tab has no path and serializes to nothing, so it isn't kept in saved
//! layouts, and closes as any tab does.

use std::{
    collections::{BTreeMap, BTreeSet},
    fs, io,
    io::Write as _,
    ops::Range,
    path::{Path, PathBuf},
    sync::Arc,
};

use fe_command::{ContextFrame, ContextPredicate, ContextStack};
use fe_gpui::{EditorView, commands::RunCommand};
use fe_keymap::{
    Binding, CommandInfo, DEFAULT, DEFAULT_KEYMAP_PATH, DefaultKeymap, Entry, KEYMAP_FILE,
    KEYMAP_HEADER, Keymap, KeymapEdit, KeymapIssue, KeymapLayer, ShortcutBinding, ShortcutIssue,
    ShortcutRow, ShortcutSource, ShortcutsModel,
};
use fe_ui::{ActiveTheme as _, Icon, TextInput, TextInputEvent, Tooltip};
use gpui::{
    AnyElement, App, AppContext as _, ClickEvent, ClipboardItem, Context, Entity, FocusHandle,
    Focusable, InteractiveElement as _, IntoElement, KeyDownEvent, MouseButton, MouseDownEvent,
    ParentElement as _, Pixels, Point, Render, ScrollStrategy, SharedString,
    StatefulInteractiveElement as _, Styled as _, Subscription, Task, UniformListScrollHandle,
    Window, anchored, deferred, div, prelude::FluentBuilder as _, px, svg, uniform_list,
};
use kiln::{
    core::Os,
    platform::{
        config::{self, ChangedFiles},
        locale::ActiveLocale,
    },
};
use kiln_l10n::{text, tr, verbatim};
use kiln_shell::{PaneItem, PaneView as _};

use self::recorder::Recorder;
use crate::{
    ListedCommands, Workbench,
    picker::{self, PaletteOp},
    workbench::toast::{self, Toast},
};

/// The key context the editor's element sets.
pub(crate) const KEY_CONTEXT: &str = "shortcuts_editor";

/// Every line's height: the list is a `uniform_list`.
const LINE_HEIGHT: Pixels = px(26.);

/// The width of the column of each line's pencil or Add button.
const EDIT_WIDTH: Pixels = px(16.);

/// The Keybinding column's width.
const KEYS_WIDTH: Pixels = px(200.);

/// The Source column's width.
const SOURCE_WIDTH: Pixels = px(80.);

/// What a refused Reset Keybinding says, beside it in the menu: the tab
/// can't tell what the user's file does to the command's default on
/// another platform ([`ShortcutsEditor::reset_refused`]), which takes a
/// keymap store kept without every platform's defaults.
pub(crate) fn reset_refused_hint() -> SharedString {
    tr!("workbench-setup-shortcuts-reset-refused-hint")
}

/// What the list says when the search finds nothing.
pub(crate) fn nothing_found() -> SharedString {
    tr!("workbench-setup-shortcuts-nothing-found")
}

/// What a line shows, to find it again among new lines: its command, and
/// its binding's keys and location.
type Identity = (String, Option<(String, String)>);

/// A line of the table: a binding of one of the rows found, or a row
/// with no binding.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Line {
    /// An index into the rows found.
    pub(crate) row: usize,
    /// An index into the row's bindings; `None` for a row without any.
    pub(crate) binding: Option<usize>,
}

/// An item of a line's menu.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum MenuItem {
    CopyCommandId,
    AddKeybinding,
    RemoveKeybinding,
    ResetKeybinding,
    ShowSameKeybindings,
}

impl MenuItem {
    pub(crate) fn title(self) -> SharedString {
        match self {
            MenuItem::CopyCommandId => tr!("workbench-setup-shortcuts-copy-command-id"),
            MenuItem::AddKeybinding => tr!("workbench-setup-shortcuts-add"),
            MenuItem::RemoveKeybinding => tr!("workbench-setup-shortcuts-remove"),
            MenuItem::ResetKeybinding => tr!("workbench-setup-shortcuts-reset"),
            MenuItem::ShowSameKeybindings => tr!("workbench-setup-shortcuts-show-same"),
        }
    }
}

/// A line's open menu.
struct LineMenu {
    line: usize,
    items: Vec<MenuItem>,
    /// Those of `items` shown but refused, with [`reset_refused_hint`].
    disabled: Vec<MenuItem>,
    /// The item Up and Down moved to, or the pointer is over, which Enter
    /// chooses: none until either does.
    highlighted: Option<MenuItem>,
    /// Where it was asked for, in the window.
    position: Point<Pixels>,
}

/// The Keyboard Shortcuts tab: see the module docs.
pub(crate) struct ShortcutsEditor {
    /// The list's: where the keys go once Down leaves the search.
    focus: FocusHandle,
    search: Entity<TextInput>,
    /// The keymap the rows show.
    keymap: Arc<Keymap>,
    /// The user's `keymap.toml`, whose bindings are the User source.
    user_keymap: Option<PathBuf>,
    /// Its text as last read or written, which a reset edits; `None` if
    /// it's missing or not read yet.
    user_source: Option<String>,
    /// The read of it under way, dropped for a newer one.
    read: Task<()>,
    model: ShortcutsModel,
    /// The rows the search finds, each with only its matching bindings.
    found: Vec<ShortcutRow>,
    lines: Vec<Line>,
    /// The selected line.
    selected: Option<usize>,
    menu: Option<LineMenu>,
    /// The recorder open on a line.
    recorder: Option<Recorder>,
    scroll: UniformListScrollHandle,
    /// Each range of lines the list built, for the tests.
    #[cfg(test)]
    built: Vec<Range<usize>>,
    _subscriptions: Vec<Subscription>,
}

impl ShortcutsEditor {
    fn new(cx: &mut Context<Self>) -> Self {
        let ui = fe_ui::ui_theme(cx);
        // Keyed, so a change of language shows in it at once.
        let search = cx.new(|cx| {
            TextInput::new(text!("workbench-setup-shortcuts-search"), &ui, cx).bordered()
        });
        let subscriptions = vec![
            cx.subscribe(&search, |this, _, event, cx| {
                if *event == TextInputEvent::Changed {
                    this.refilter(cx);
                    cx.notify();
                }
            }),
            // The watcher saw the user's file change: a change that leaves
            // the keymap as it was, such as an entry for another platform,
            // still changes what a reset would do.
            cx.observe_global::<ChangedFiles>(|this, cx| {
                let changed = config::changed_files(cx);
                if (this.user_keymap.as_ref()).is_some_and(|path| changed.contains(path)) {
                    this.read_user_keymap(cx);
                }
            }),
            // The rows' titles are resolved as the model is built: an open
            // tab follows a change of language at once.
            cx.observe_global::<ActiveLocale>(|this, cx| this.follow_locale(cx)),
        ];
        let keymap = fe_gpui::current_keymap(cx);
        let mut this = Self {
            focus: cx.focus_handle(),
            search,
            model: build_model(&keymap, cx),
            keymap,
            user_keymap: keymap_path(cx),
            user_source: None,
            read: Task::ready(()),
            found: Vec::new(),
            lines: Vec::new(),
            selected: None,
            menu: None,
            recorder: None,
            scroll: UniformListScrollHandle::new(),
            #[cfg(test)]
            built: Vec::new(),
            _subscriptions: subscriptions,
        };
        this.refilter(cx);
        this.read_user_keymap(cx);
        this
    }

    /// Rebuilds the rows from the keymap in use if it isn't the one they
    /// show, reading the user's file again: it was reloaded.
    fn follow_keymap(&mut self, cx: &mut Context<Self>) {
        let live = fe_gpui::current_keymap(cx);
        if Arc::ptr_eq(&live, &self.keymap) {
            return;
        }
        self.model = build_model(&live, cx);
        self.keymap = live;
        self.refilter(cx);
        self.read_user_keymap(cx);
    }

    /// Rebuilds the rows from the keymap they show, their commands titled
    /// in the locale just set.
    fn follow_locale(&mut self, cx: &mut Context<Self>) {
        self.model = build_model(&self.keymap, cx);
        self.refilter(cx);
        cx.notify();
    }

    /// Reads the user's `keymap.toml` off the main thread, for a reset to
    /// edit, and offers the open menu's items again once read.
    fn read_user_keymap(&mut self, cx: &mut Context<Self>) {
        let Some(path) = self.user_keymap.clone() else {
            return;
        };
        let read = cx.background_spawn(async move {
            match fs::read_to_string(&path) {
                Ok(text) => Some(text),
                Err(error) if error.kind() == io::ErrorKind::NotFound => None,
                Err(error) => {
                    log::warn!("can't read {}: {error}", path.display());
                    None
                }
            }
        });
        self.read = cx.spawn(async move |this, cx| {
            let text = read.await;
            this.update(cx, |this, cx| {
                this.user_source = text;
                this.refresh_menu(cx);
            })
            .ok();
        });
    }

    /// The open menu's items as the user's file now has them.
    fn refresh_menu(&mut self, cx: &mut Context<Self>) {
        let Some(line) = self.menu.as_ref().map(|menu| menu.line) else {
            return;
        };
        let (items, disabled) = self.menu_items_for(line, cx);
        if let Some(menu) = &mut self.menu {
            // An item no longer offered, or now refused, isn't chosen.
            (menu.highlighted).take_if(|item| !items.contains(item) || disabled.contains(item));
            menu.items = items;
            menu.disabled = disabled;
        }
        cx.notify();
    }

    /// The lines for the search as it is now, keeping the selected
    /// binding selected if it's still found. Closes the menu.
    fn refilter(&mut self, cx: &App) {
        let selected = self.selected.and_then(|ix| self.identity(ix));
        // `@source:` is filtered here, by [`ShortcutsEditor::source`].
        let mut source = None;
        let query: Vec<&str> = (self.search.read(cx).text().split_whitespace())
            .filter(|word| match word.strip_prefix("@source:") {
                Some(name) if name.eq_ignore_ascii_case("user") => {
                    source = Some(ShortcutSource::User);
                    false
                }
                Some(name) if name.eq_ignore_ascii_case("default") => {
                    source = Some(ShortcutSource::Default);
                    false
                }
                _ => true,
            })
            .collect();
        let mut found = self.model.search(&query.join(" "), false);
        if let Some(source) = source {
            for row in &mut found {
                (row.bindings).retain(|binding| self.source(binding) == source);
            }
            found.retain(|row| !row.bindings.is_empty());
        }
        self.found = found;
        self.lines = (self.found.iter().enumerate())
            .flat_map(|(row, found)| match found.bindings.len() {
                0 => vec![Line { row, binding: None }],
                count => (0..count)
                    .map(|binding| Line {
                        row,
                        binding: Some(binding),
                    })
                    .collect(),
            })
            .collect();
        self.selected = selected.and_then(|selected| {
            (0..self.lines.len()).position(|ix| self.identity(ix).as_ref() == Some(&selected))
        });
        self.menu = None;
        // The recorder stays on its line while it's listed.
        if self.recorder.is_some() && self.recorder_line().is_none() {
            self.recorder = None;
        }
        self.refresh_recorder(cx);
    }

    /// What line `ix` shows, to find it again among new lines: its command,
    /// and its binding's keys and location.
    fn identity(&self, ix: usize) -> Option<Identity> {
        let (row, binding) = self.line(ix)?;
        let binding =
            binding.map(|binding| (binding.keys.to_string(), binding.location.to_string()));
        Some((row.command.clone(), binding))
    }

    /// Line `ix`'s row and binding.
    pub(crate) fn line(&self, ix: usize) -> Option<(&ShortcutRow, Option<&ShortcutBinding>)> {
        let line = self.lines.get(ix)?;
        let row = &self.found[line.row];
        Some((row, line.binding.map(|binding| &row.bindings[binding])))
    }

    #[cfg(test)]
    pub(crate) fn lines(&self) -> &[Line] {
        &self.lines
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

    /// The open menu's items.
    #[cfg(test)]
    pub(crate) fn menu_items(&self) -> Option<&[MenuItem]> {
        self.menu.as_ref().map(|menu| &menu.items[..])
    }

    /// The open menu's items shown disabled.
    #[cfg(test)]
    pub(crate) fn disabled_menu_items(&self) -> Option<&[MenuItem]> {
        self.menu.as_ref().map(|menu| &menu.disabled[..])
    }

    /// Each range of lines the list built, dropping them.
    #[cfg(test)]
    pub(crate) fn take_built(&mut self) -> Vec<Range<usize>> {
        std::mem::take(&mut self.built)
    }

    /// The binding a later one with the same keys and context replaces
    /// `binding` with, in the keymap the rows show.
    fn overrider(&self, binding: &ShortcutBinding) -> Option<&Binding> {
        let bindings = self.keymap.bindings();
        let ix = bindings
            .iter()
            .position(|ours| ours.location == binding.location && ours.keys == binding.keys)?;
        let ours = &bindings[ix];
        (bindings[ix + 1..].iter())
            .rev()
            .find(|later| later.keys == ours.keys && later.context == ours.context)
    }

    /// What a hidden binding's tooltip says: where the entry that replaces
    /// it is. `None` if `binding` isn't hidden.
    pub(crate) fn hidden_note(&self, binding: &ShortcutBinding) -> Option<String> {
        let hidden =
            (binding.issues.iter()).any(|issue| matches!(issue, ShortcutIssue::Hidden { .. }));
        if !hidden {
            return None;
        }
        Some(match self.overrider(binding) {
            Some(by) => {
                let location = by.location.to_string();
                tr!("workbench-setup-shortcuts-hidden-by", location = location).to_string()
            }
            None => tr!("workbench-setup-shortcuts-hidden-by-unbind").to_string(),
        })
    }

    /// Where `binding` comes from: the user's `keymap.toml`, or else a
    /// keymap the app ships. Not [`ShortcutBinding::source`], which takes
    /// only the first layer for the defaults, where the app's keymap may
    /// lie over kiln's and its modules'.
    pub(crate) fn source(&self, binding: &ShortcutBinding) -> ShortcutSource {
        match &self.user_keymap {
            Some(user) if binding.location.file == *user => ShortcutSource::User,
            _ => ShortcutSource::Default,
        }
    }

    /// Whether the user's keymap has entries for `row`'s command that a
    /// reset would delete: a reset of the file as last read changes it.
    /// That counts entries in effect or not: one for another platform, or
    /// one a later entry unbinds, and an unbind entry that removed one of
    /// the command's defaults, in any layer the app ships.
    fn resettable(&self, row: &ShortcutRow, cx: &App) -> bool {
        let Some(source) = &self.user_source else {
            return false;
        };
        let defaults = default_keymaps(cx);
        let edit = KeymapEdit::ResetCommand {
            command: row.command.clone(),
        };
        fe_keymap::edit_keymap_on(&borrowed(&defaults), source, &edit, Os::current())
            .is_ok_and(|edited| edited != *source)
    }

    /// Whether a reset of `row`'s command is refused, fail-closed: its
    /// default comes from a layer the app ships other than fastedit's
    /// (kiln's, a module's, an extension's), the store holds the layers
    /// only as read on this platform (kiln keeps every platform's with it,
    /// [`config::SharedKeymap::platform_defaults`], so only a store made
    /// without kiln's setup), and the user's file has an entry gated on
    /// another platform that names the command or unbinds keys, whose
    /// effect on that default there the tab can't tell. fastedit's own
    /// layer is passed to an edit as shipped, with every platform's keys,
    /// so its defaults reset either way.
    fn reset_refused(&self, row: &ShortcutRow, cx: &App) -> bool {
        let (Some(source), Some(guard)) = (&self.user_source, self.reset_guard(row, cx)) else {
            return false;
        };
        guard.refuses(source)
    }

    /// The refusal of a reset of `row`'s command, to run on the file's
    /// text: none if the store or the defaults can't make it refuse.
    fn reset_guard(&self, row: &ShortcutRow, cx: &App) -> Option<ResetGuard> {
        let store = config::keymap_store(cx)?;
        if store.platform_defaults().is_some() {
            return None;
        }
        let elsewhere = (store.defaults().iter())
            .filter(|layer| layer.path != Path::new(DEFAULT_KEYMAP_PATH))
            .flat_map(|layer| &layer.entries)
            .any(|entry| (entry.command.as_ref()).is_some_and(|id| id.as_str() == row.command));
        elsewhere.then(|| ResetGuard {
            path: (self.user_keymap.clone()).unwrap_or_else(|| PathBuf::from(KEYMAP_FILE)),
            command: row.command.clone(),
        })
    }

    /// The items line `ix`'s menu offers, and those of them it shows
    /// disabled.
    fn menu_items_for(&self, ix: usize, cx: &App) -> (Vec<MenuItem>, Vec<MenuItem>) {
        let Some((row, binding)) = self.line(ix) else {
            return (Vec::new(), Vec::new());
        };
        let mut items = vec![MenuItem::CopyCommandId, MenuItem::AddKeybinding];
        let mut disabled = Vec::new();
        if let Some(binding) = binding
            && self.hidden_note(binding).is_none()
        {
            items.push(MenuItem::RemoveKeybinding);
        }
        if self.reset_refused(row, cx) {
            items.push(MenuItem::ResetKeybinding);
            disabled.push(MenuItem::ResetKeybinding);
        } else if self.resettable(row, cx) {
            items.push(MenuItem::ResetKeybinding);
        }
        if binding.is_some() {
            items.push(MenuItem::ShowSameKeybindings);
        }
        (items, disabled)
    }

    /// Opens line `ix`'s menu at `position`, selecting the line.
    pub(crate) fn open_menu(
        &mut self,
        ix: usize,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.recorder = None;
        self.selected = Some(ix);
        let (items, disabled) = self.menu_items_for(ix, cx);
        self.menu = Some(LineMenu {
            line: ix,
            items,
            disabled,
            highlighted: None,
            position,
        });
        window.focus(&self.focus, cx);
        cx.notify();
    }

    fn close_menu(&mut self, cx: &mut Context<Self>) {
        if self.menu.take().is_some() {
            cx.notify();
        }
    }

    /// Runs the open menu's `item`, closing it; a disabled one does
    /// nothing.
    pub(crate) fn run_menu_item(
        &mut self,
        item: MenuItem,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if (self.menu.as_ref()).is_none_or(|menu| menu.disabled.contains(&item)) {
            return;
        }
        let Some(menu) = self.menu.take() else {
            return;
        };
        cx.notify();
        self.run_line_item(menu.line, item, window, cx);
    }

    /// Moves the open menu's highlight to the next item it can choose, or
    /// with `down` false the one before, wrapping; from none, the first or
    /// the last.
    fn move_highlight(&mut self, down: bool, cx: &mut Context<Self>) {
        let Some(menu) = &mut self.menu else {
            return;
        };
        let enabled: Vec<MenuItem> = (menu.items.iter())
            .filter(|item| !menu.disabled.contains(item))
            .copied()
            .collect();
        let Some(last) = enabled.len().checked_sub(1) else {
            return;
        };
        let at = (menu.highlighted).and_then(|item| enabled.iter().position(|it| *it == item));
        let next = match (at, down) {
            (None, true) => 0,
            (None, false) => last,
            (Some(at), true) if at == last => 0,
            (Some(at), true) => at + 1,
            (Some(at), false) => at.checked_sub(1).unwrap_or(last),
        };
        menu.highlighted = Some(enabled[next]);
        cx.notify();
    }

    /// Enter in the open menu: chooses the highlighted item, if any.
    fn choose_highlighted(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(item) = self.menu.as_ref().and_then(|menu| menu.highlighted) {
            self.run_menu_item(item, window, cx);
        }
    }

    /// Runs `item` of the selected line's menu, as the menu would, if the
    /// menu offers it there and doesn't refuse it: the palette's
    /// `shortcuts.*` commands. Closes an open menu.
    fn run_on_selected(&mut self, item: MenuItem, window: &mut Window, cx: &mut Context<Self>) {
        let Some(ix) = self.selected else {
            return;
        };
        let (items, disabled) = self.menu_items_for(ix, cx);
        if !items.contains(&item) || disabled.contains(&item) {
            return;
        }
        self.close_menu(cx);
        self.run_line_item(ix, item, window, cx);
    }

    /// Runs `item` on line `ix`.
    fn run_line_item(
        &mut self,
        ix: usize,
        item: MenuItem,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some((row, binding)) = self.line(ix) else {
            return;
        };
        match item {
            MenuItem::CopyCommandId => {
                cx.write_to_clipboard(ClipboardItem::new_string(row.command.clone()));
            }
            MenuItem::AddKeybinding => self.start_recording(ix, true, window, cx),
            MenuItem::RemoveKeybinding => self.remove(ix, cx),
            MenuItem::ResetKeybinding => {
                let command = row.command.clone();
                let guard = self.reset_guard(row, cx);
                self.edit_guarded(KeymapEdit::ResetCommand { command }, guard, cx);
            }
            MenuItem::ShowSameKeybindings => {
                let Some(binding) = binding else {
                    return;
                };
                let query = format!("\"{}\"", binding.keys);
                self.search
                    .update(cx, |search, cx| search.set_text(query, cx));
                self.refilter(cx);
                window.focus(&self.search.focus_handle(cx), cx);
            }
        }
    }

    /// Removes line `ix`'s binding, unless it's hidden or there's none.
    pub(crate) fn remove(&mut self, ix: usize, cx: &mut Context<Self>) {
        let Some((_, Some(binding))) = self.line(ix) else {
            return;
        };
        if self.hidden_note(binding).is_some() {
            return;
        }
        let edit = KeymapEdit::Remove {
            keys: binding.keys.to_string(),
            context: binding.when.clone(),
        };
        self.edit(edit, cx);
    }

    /// Makes `edit` to the user's keymap off the main thread, in order with
    /// the edits made before it ([`queue_write`]), showing a toast if it
    /// fails. The watcher reloads the file it writes.
    fn edit(&mut self, edit: KeymapEdit, cx: &mut Context<Self>) {
        self.edit_guarded(edit, None, cx);
    }

    /// [`Self::edit`], refused if `guard` refuses the text edited.
    fn edit_guarded(
        &mut self,
        edit: KeymapEdit,
        guard: Option<ResetGuard>,
        cx: &mut Context<Self>,
    ) {
        let Some(path) = keymap_path(cx) else {
            toast::show(
                cx,
                Toast::error(tr!("workbench-setup-shortcuts-change-failed"))
                    .detail(tr!("workbench-setup-shortcuts-no-config-dir")),
            );
            return;
        };
        let defaults = default_keymaps(cx);
        let write = queue_write(path, defaults, edit, guard, cx);
        cx.spawn(async move |this, cx| {
            let written = (write.recv().await).unwrap_or_else(|_| {
                Err(tr!("workbench-setup-shortcuts-write-dropped").to_string())
            });
            match written {
                Ok(written) => {
                    this.update(cx, |this, cx| {
                        this.user_source = Some(written.source);
                        this.refresh_menu(cx);
                    })
                    .ok();
                }
                Err(error) => {
                    log::warn!("can't change the keymap: {error}");
                    cx.update(|cx| {
                        toast::show(
                            cx,
                            Toast::error(tr!("workbench-setup-shortcuts-change-failed"))
                                .detail(error),
                        )
                    });
                }
            }
        })
        .detach();
    }

    /// Selects the line below the selected one, or with `down` false the
    /// one above, or the first with none selected, and scrolls it into
    /// view.
    fn move_selection(&mut self, down: bool, cx: &mut Context<Self>) {
        if self.lines.is_empty() {
            return;
        }
        let last = self.lines.len() - 1;
        let next = match (self.selected, down) {
            (None, _) => 0,
            (Some(at), true) => (at + 1).min(last),
            (Some(at), false) => at.saturating_sub(1),
        };
        self.selected = Some(next);
        self.scroll.scroll_to_item(next, ScrollStrategy::Nearest);
        cx.notify();
    }

    /// The search field's editing commands (`text_input.backspace` and the
    /// like, which the input leaves to its host); others go on.
    fn on_input_command(&mut self, run: &RunCommand, _: &mut Window, cx: &mut Context<Self>) {
        if !input_command(&self.search, run, cx) {
            cx.propagate();
        }
    }

    /// Its keys: see the module docs.
    fn on_key_down(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let keystroke = &event.keystroke;
        let m = keystroke.modifiers;
        let searching = self.search.focus_handle(cx).is_focused(window);
        // The recorder's box takes its own keys; its When field's Enter
        // and Escape come here.
        if self.when_focused(window, cx) {
            match keystroke.key.as_str() {
                "enter" if !m.modified() => self.save_recording(window, cx),
                "escape" if !m.modified() => self.cancel_recording(window, cx),
                _ => return,
            }
            return cx.stop_propagation();
        }
        match keystroke.key.as_str() {
            // The open menu's: Up and Down move its highlight, Enter
            // chooses it.
            "up" | "down" if !m.modified() && self.menu.is_some() => {
                self.move_highlight(keystroke.key == "down", cx);
            }
            "enter" if !m.modified() && self.menu.is_some() => self.choose_highlighted(window, cx),
            "enter" if !m.modified() && !searching => {
                if let Some(ix) = self.selected {
                    self.start_recording(ix, false, window, cx);
                }
            }
            "up" | "down" if !m.modified() => {
                let down = keystroke.key == "down";
                self.close_menu(cx);
                if searching && down {
                    window.focus(&self.focus, cx);
                }
                self.move_selection(down, cx);
            }
            "/" if !m.modified() && !searching => {
                window.focus(&self.search.focus_handle(cx), cx);
            }
            "f" if m.secondary() && !m.alt && !m.shift => {
                window.focus(&self.search.focus_handle(cx), cx);
            }
            "delete" if !m.modified() && !searching => {
                if let Some(ix) = self.selected {
                    self.close_menu(cx);
                    self.remove(ix, cx);
                }
            }
            "backspace" if m.secondary() && !m.alt && !m.shift && !searching => {
                if let Some(ix) = self.selected {
                    self.close_menu(cx);
                    self.remove(ix, cx);
                }
            }
            "escape" if !m.modified() && self.menu.is_some() => self.close_menu(cx),
            "escape" if !m.modified() && !self.search.read(cx).text().is_empty() => {
                self.search.update(cx, |search, cx| search.clear(cx));
            }
            _ => return,
        }
        cx.stop_propagation();
    }

    fn render_lines(
        &mut self,
        range: Range<usize>,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        #[cfg(test)]
        self.built.push(range.clone());
        let ui = fe_ui::ui_theme(cx);
        let selected_bg = if self.focus.is_focused(window) {
            ui.row_selected
        } else {
            ui.row_selected_inactive
        };
        range
            .filter_map(|ix| {
                let line = self.render_line(ix, cx)?;
                Some(
                    line.when(self.selected == Some(ix), |line| line.bg(selected_bg))
                        .into_any_element(),
                )
            })
            .collect()
    }

    /// Line `ix`: its pencil, or Add without a binding, its command's
    /// title and ID, its keys with a warning if they clash, or the recorder
    /// open on it, its when-text and its source; dimmed if it's hidden.
    fn render_line(&self, ix: usize, cx: &mut Context<Self>) -> Option<gpui::Stateful<gpui::Div>> {
        let ui = fe_ui::ui_theme(cx);
        let warning = cx.theme().colors.status.warning;
        let (row, binding) = self.line(ix)?;
        let command = row.command.clone();
        // A command's first line is `shortcuts-row-<command>`, its others
        // `-<n>` after.
        let nth = self.lines[ix].binding.unwrap_or(0);
        let selector = match nth {
            0 => format!("shortcuts-row-{command}"),
            nth => format!("shortcuts-row-{command}-{nth}"),
        };
        let hidden = binding.and_then(|binding| self.hidden_note(binding));
        let conflict = binding.and_then(conflict_note);
        let keys = binding.map_or_else(String::new, |binding| binding.label.clone());
        let when = binding
            .and_then(|binding| binding.when.clone())
            .unwrap_or_default();
        let source = match binding.map(|binding| self.source(binding)) {
            Some(ShortcutSource::Default) => tr!("workbench-setup-shortcuts-source-default"),
            Some(ShortcutSource::User) => tr!("workbench-setup-shortcuts-source-user"),
            None => SharedString::default(),
        };
        let cell = || {
            div()
                .flex_none()
                .flex()
                .items_center()
                .gap_1()
                .overflow_hidden()
                .whitespace_nowrap()
        };
        let hidden_selector = format!("shortcuts-hidden-{command}-{nth}");
        let recording = self.recorder_line() == Some(ix);
        let recorder = recording.then(|| self.render_recorder_keys(cx)).flatten();
        let when_input = recording.then(|| self.render_recorder_when(cx)).flatten();
        // A line's pencil is `shortcuts-edit-<command>`, `-<n>` after for
        // its others, as the line's selector; Add, on a line without a
        // binding, `shortcuts-add-<command>`.
        let button = match (binding, &hidden) {
            (_, Some(_)) => None,
            (Some(_), None) => Some((
                match nth {
                    0 => format!("shortcuts-edit-{command}"),
                    nth => format!("shortcuts-edit-{command}-{nth}"),
                },
                "✎",
                false,
            )),
            (None, None) => Some((format!("shortcuts-add-{command}"), "+", true)),
        };
        let button = button.map(|(selector, glyph, add)| {
            div()
                .id(SharedString::from(selector.clone()))
                .debug_selector(move || selector)
                .flex_none()
                .text_color(ui.muted_text)
                .hover(|style| style.text_color(ui.text))
                .child(glyph)
                .on_click(cx.listener(move |this, _, window, cx| {
                    cx.stop_propagation();
                    this.start_recording(ix, add, window, cx);
                }))
        });
        let edit_cell = cell().w(EDIT_WIDTH).children(button);
        let keys_cell = cell()
            .w(KEYS_WIDTH)
            .when(hidden.is_some(), |cell| {
                cell.debug_selector(move || hidden_selector)
            })
            .map(|cell| match recorder {
                Some(recorder) => cell.child(recorder),
                None => cell.child(div().text_color(ui.text).child(keys)),
            })
            .when_some(conflict.filter(|_| !recording), |cell, note| {
                let selector = format!("shortcuts-conflict-{command}-{nth}");
                cell.child(
                    div()
                        .id(SharedString::from(selector.clone()))
                        .debug_selector(move || selector)
                        .flex_none()
                        .child(
                            svg()
                                .path(Icon::TriangleAlert.path())
                                .size(px(14.))
                                .text_color(warning),
                        )
                        .tooltip(move |_, cx| {
                            let tooltip = Tooltip::new(
                                note.clone(),
                                None::<SharedString>,
                                &fe_ui::ui_theme(cx),
                            );
                            cx.new(|_| tooltip).into()
                        }),
                )
            });
        let line = div()
            .id(("shortcuts-line", ix))
            .debug_selector(move || selector)
            .w_full()
            .h(LINE_HEIGHT)
            .px_6()
            .flex()
            .items_center()
            .gap_4()
            .when_some(hidden, |line, note| {
                line.opacity(0.5).tooltip(move |_, cx| {
                    let tooltip =
                        Tooltip::new(note.clone(), None::<SharedString>, &fe_ui::ui_theme(cx));
                    cx.new(|_| tooltip).into()
                })
            })
            .on_click(cx.listener(move |this, event: &ClickEvent, window, cx| {
                if event.click_count() == 2 {
                    return this.start_recording(ix, false, window, cx);
                }
                this.selected = Some(ix);
                this.close_menu(cx);
                match (this.recorder_line() == Some(ix), &this.recorder) {
                    (true, Some(recorder)) => window.focus(recorder.focus(), cx),
                    _ => {
                        this.recorder = None;
                        window.focus(&this.focus, cx);
                    }
                }
                cx.notify();
            }))
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                    cx.stop_propagation();
                    this.open_menu(ix, event.position, window, cx);
                }),
            )
            .child(edit_cell)
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .items_baseline()
                    .gap_2()
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .child(div().text_color(ui.text).child(row.title.clone()))
                    .child(
                        div()
                            .text_color(ui.muted_text)
                            .text_size(px(12.))
                            .child(row.command.clone()),
                    ),
            )
            .child(keys_cell)
            .child(match when_input {
                Some(input) => div().flex_1().min_w_0().flex().child(input),
                None => div()
                    .flex_1()
                    .min_w_0()
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .text_color(ui.muted_text)
                    .child(when),
            })
            .child(
                cell()
                    .w(SOURCE_WIDTH)
                    .text_color(ui.muted_text)
                    .child(source),
            );
        Some(line)
    }

    /// The table's column headings.
    fn header(&self, cx: &App) -> impl IntoElement + use<> {
        let ui = fe_ui::ui_theme(cx);
        let heading = |title: SharedString| {
            div()
                .overflow_hidden()
                .whitespace_nowrap()
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .child(title)
        };
        div()
            .debug_selector(|| "shortcuts-header".into())
            .flex_none()
            .h(LINE_HEIGHT)
            .px_6()
            .flex()
            .items_center()
            .gap_4()
            .border_b_1()
            .border_color(ui.border)
            .text_color(ui.muted_text)
            .child(div().flex_none().w(EDIT_WIDTH))
            .child(
                heading(tr!("workbench-setup-shortcuts-column-command"))
                    .flex_1()
                    .min_w_0(),
            )
            .child(
                heading(tr!("workbench-setup-shortcuts-column-keybinding"))
                    .flex_none()
                    .w(KEYS_WIDTH),
            )
            .child(
                heading(tr!("workbench-setup-shortcuts-column-when"))
                    .flex_1()
                    .min_w_0(),
            )
            .child(
                heading(tr!("workbench-setup-shortcuts-column-source"))
                    .flex_none()
                    .w(SOURCE_WIDTH),
            )
    }

    /// The open menu, drawn over the window where it was asked for, as
    /// kiln-ui's `MenuPanel` draws one, with its disabled items muted and
    /// their hint beside them.
    fn render_menu(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let menu = self.menu.as_ref()?;
        let colors = cx.theme().colors;
        let hover = colors.list.row_hover;
        let selected = colors.list.row_selected;
        let selected_text = colors.tab.active_text;
        let rows = (menu.items.iter().enumerate()).map(|(ix, &item)| {
            let disabled = menu.disabled.contains(&item);
            let highlighted = menu.highlighted == Some(item);
            let selector = format!("menu-item:{}", item.title());
            div()
                .id(("shortcuts-menu-item", ix))
                .debug_selector(move || selector)
                .flex()
                .items_center()
                .justify_between()
                .gap_6()
                .h(px(24.))
                .px_3()
                .rounded_sm()
                .child(item.title())
                .map(|row| match disabled {
                    true => row.text_color(colors.text.muted).child(
                        div()
                            .debug_selector(|| "shortcuts-menu-hint".into())
                            .child(reset_refused_hint()),
                    ),
                    false => row
                        .hover(move |style| style.bg(hover))
                        .when(highlighted, |row| {
                            row.debug_selector(|| "shortcuts-menu-highlighted".into())
                                .bg(selected)
                                .text_color(selected_text)
                        })
                        .on_mouse_move(cx.listener(move |this, _, _, cx| {
                            if let Some(menu) = &mut this.menu
                                && menu.highlighted != Some(item)
                            {
                                menu.highlighted = Some(item);
                                cx.notify();
                            }
                        }))
                        .on_click(cx.listener(move |this, _, window, cx| {
                            cx.stop_propagation();
                            this.run_menu_item(item, window, cx);
                        })),
                })
        });
        let panel = div()
            .debug_selector(|| "shortcuts-menu".into())
            .occlude()
            .on_mouse_down_out(cx.listener(|this, _, _, cx| this.close_menu(cx)))
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .min_w(px(220.))
            .flex()
            .flex_col()
            .p_1()
            .bg(colors.surface.panel)
            .border_1()
            .border_color(colors.border.default)
            .rounded_md()
            .shadow_lg()
            .text_color(colors.text.primary)
            .children(rows);
        Some(
            deferred(
                anchored()
                    .position(menu.position)
                    .snap_to_window()
                    .child(panel),
            )
            .with_priority(1)
            .into_any_element(),
        )
    }
}

/// Runs `run` on `input` if it's one of a text input's editing commands
/// (`text_input.backspace` and the like, which the input leaves to its
/// host), returning whether it was.
fn input_command(input: &Entity<TextInput>, run: &RunCommand, cx: &mut App) -> bool {
    let Some(edit) = picker::op(run, cx).and_then(PaletteOp::edit) else {
        return false;
    };
    input.update(cx, edit);
    true
}

/// The model of the rows `keymap` gives the commands the palette lists,
/// marked with the bindings that clash ([`clashes`]).
fn build_model(keymap: &Keymap, cx: &App) -> ShortcutsModel {
    ShortcutsModel::new(keymap, &commands(keymap, cx), Os::current()).with_issues(&clashes(keymap))
}

/// [`Keymap::report_with`] in the contexts the workbench has, as the
/// keymap's own contexts tell them: the workbench alone, an editor in it,
/// and for each context a binding names, a context where each of its
/// alternatives holds ([`witness`]), as `[workbench, terminal]` for `terminal`.
/// An alternative that needs one of the editor's flags or keys
/// ([`EditorView::key_context_frames`]), or rules some out with no other
/// view named, holds in each frame an editor can have, innermost: an editor
/// is in a view only where a binding says so (`DiffView && editor`), so
/// `project_panel && !vim.enabled` holds in the panel alone. A binding for
/// `workbench` and one for `terminal` with the same keys clash there, where
/// the terminal's wins; one for `project_panel` and one for `editor` never
/// hold in one context, so don't, nor one for `vim.visual` and one for
/// `vim.mode == normal`.
fn clashes(keymap: &Keymap) -> Vec<KeymapIssue> {
    let mut predicates: Vec<&ContextPredicate> = Vec::new();
    for context in (keymap.bindings().iter()).filter_map(|binding| binding.context.as_ref()) {
        if !predicates.contains(&context) {
            predicates.push(context);
        }
    }
    let mut mentions = Mentions::default();
    for predicate in &predicates {
        mentions.add(predicate);
    }
    // The editor's keys, from every frame it can have: `vim.recording`, a
    // key, is the editor's though a context names it as a flag.
    let frames = EditorView::key_context_frames();
    let mut editor_keys = BTreeSet::from([fe_gpui::KEY_CONTEXT]);
    for frame in frames {
        editor_keys.extend(frame.flags.iter().map(String::as_str));
        editor_keys.extend(frame.values.keys().map(String::as_str));
    }
    let is_editor = |name: &str| editor_keys.contains(name);
    let editors = mentions.project(frames);
    let workbench = ContextStack::new().with(ContextFrame::new(crate::KEY_CONTEXT));
    let editor = workbench
        .clone()
        .with(ContextFrame::new(fe_gpui::KEY_CONTEXT));
    let mut contexts = vec![workbench.clone(), editor];
    // Every view that holds editors, with each frame an editor can have in
    // it: a condition that only rules editor keys out (`DiffView &&
    // !vim.enabled`) holds there though no binding names the editor.
    for host in hosting_contexts() {
        let host = workbench.clone().with(ContextFrame::new(host));
        contexts.extend(
            editors
                .iter()
                .map(|editor| host.clone().with(editor.clone())),
        );
    }
    for context in predicates {
        // A witness for each alternative: `settings_editor ||
        // shortcuts_editor` holds in either tab.
        for term in terms(context, false) {
            let Some(stack) = witness(&term, is_editor) else {
                continue;
            };
            let needs_editor = term.iter().any(|literal| {
                matches!(literal, Literal::Has(_) | Literal::Is(..)) && is_editor(literal.name())
            });
            let names_editor = term.iter().any(|literal| is_editor(literal.name()));
            let mut stacks = vec![stack.clone()];
            if needs_editor || (names_editor && stack.frames().len() == 1) {
                stacks.extend((editors.iter()).map(|editor| stack.clone().with(editor.clone())));
            }
            for stack in stacks.into_iter().filter(|stack| context.eval(stack)) {
                if !contexts.contains(&stack) {
                    contexts.push(stack);
                }
            }
        }
    }
    keymap.report_with(&contexts)
}

/// The named key contexts an editor sits in, apart from the workbench's own
/// frame (where the file viewer's and the panes' editors are): fe-gpui's
/// views', the chat composer's and the Changes panel's commit message.
fn hosting_contexts() -> impl Iterator<Item = &'static str> {
    (EditorView::host_key_contexts().iter().copied()).chain([
        fe_chat_view::COMPOSER_CONTEXT,
        crate::git_panel::KEY_CONTEXT,
    ])
}

/// The flags and the values of keys some binding's context names: an
/// editor's frames that differ only in others hold for the same bindings.
#[derive(Default)]
struct Mentions<'a> {
    flags: BTreeSet<&'a str>,
    values: BTreeMap<&'a str, BTreeSet<&'a str>>,
}

impl<'a> Mentions<'a> {
    fn add(&mut self, predicate: &'a ContextPredicate) {
        use ContextPredicate as P;
        match predicate {
            P::Identifier(name) => {
                self.flags.insert(name);
            }
            P::Equal(key, value) | P::NotEqual(key, value) => {
                self.values.entry(key).or_default().insert(value);
            }
            P::Not(inner) => self.add(inner),
            P::And(a, b) | P::Or(a, b) => {
                self.add(a);
                self.add(b);
            }
        }
    }

    /// `frames` without the flags and keys no context names, and with the
    /// value of a key named only for other values as the first such value
    /// seen, each once.
    fn project(&self, frames: &[ContextFrame]) -> Vec<ContextFrame> {
        let mut others: BTreeMap<&str, &str> = BTreeMap::new();
        let mut seen = BTreeSet::new();
        let mut projected = Vec::new();
        for frame in frames {
            let flags: Vec<&str> = (frame.flags.iter())
                .map(String::as_str)
                .filter(|flag| self.flags.contains(flag))
                .collect();
            let values: Vec<(&str, &str)> = (frame.values.iter())
                .filter_map(|(key, value)| {
                    let named = self.values.get(key.as_str())?;
                    let value = match named.contains(value.as_str()) {
                        true => value.as_str(),
                        false => *others.entry(key).or_insert(value),
                    };
                    Some((key.as_str(), value))
                })
                .collect();
            let shape = (flags, values);
            if !seen.contains(&shape) {
                let (flags, values) = &shape;
                let frame = flags
                    .iter()
                    .fold(ContextFrame::new(&frame.name), |frame, flag| {
                        frame.flag(*flag)
                    });
                projected.push(
                    (values.iter()).fold(frame, |frame, (key, value)| frame.set(*key, *value)),
                );
                seen.insert(shape);
            }
        }
        projected
    }
}

/// The most terms [`terms`] expands a predicate to.
const MAX_TERMS: usize = 64;

/// A literal of a term of a predicate in disjunctive normal form.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Literal {
    Has(String),
    Lacks(String),
    Is(String, String),
    IsNot(String, String),
}

/// `predicate`, or its negation if `negated`, as alternatives of
/// conjunctions of literals: at most [`MAX_TERMS`] of them.
fn terms(predicate: &ContextPredicate, negated: bool) -> Vec<Vec<Literal>> {
    use ContextPredicate as P;
    match (predicate, negated) {
        (P::Identifier(name), false) => vec![vec![Literal::Has(name.clone())]],
        (P::Identifier(name), true) => vec![vec![Literal::Lacks(name.clone())]],
        (P::Equal(key, value), false) | (P::NotEqual(key, value), true) => {
            vec![vec![Literal::Is(key.clone(), value.clone())]]
        }
        (P::Equal(key, value), true) | (P::NotEqual(key, value), false) => {
            vec![vec![Literal::IsNot(key.clone(), value.clone())]]
        }
        (P::Not(inner), negated) => terms(inner, !negated),
        (P::And(a, b), false) | (P::Or(a, b), true) => {
            let (a, b) = (terms(a, negated), terms(b, negated));
            let mut product = Vec::new();
            for left in &a {
                for right in &b {
                    product.push(left.iter().chain(right).cloned().collect());
                }
            }
            product.truncate(MAX_TERMS);
            product
        }
        (P::Or(a, b), false) | (P::And(a, b), true) => {
            let mut either = terms(a, negated);
            either.extend(terms(b, negated));
            either.truncate(MAX_TERMS);
            either
        }
    }
}

impl Literal {
    /// The flag or key it reads.
    fn name(&self) -> &str {
        match self {
            Self::Has(name) | Self::Lacks(name) | Self::Is(name, _) | Self::IsNot(name, _) => name,
        }
    }
}

/// A context stack where every literal of `term` holds but those on an
/// editor's keys (`is_editor`), whose frame goes in after it, under the
/// workbench's frame, or `None` if some contradict each other.
fn witness(term: &[Literal], is_editor: impl Fn(&str) -> bool) -> Option<ContextStack> {
    let term: Vec<_> = (term.iter())
        .filter(|literal| !is_editor(literal.name()))
        .collect();
    let mut names = Vec::new();
    let mut values: Vec<(&str, &str)> = Vec::new();
    for literal in &term {
        match literal {
            Literal::Has(name) if !names.contains(&name.as_str()) => names.push(name.as_str()),
            Literal::Is(key, value) => {
                if values.iter().any(|(k, v)| k == key && v != value) {
                    return None;
                }
                values.push((key, value));
            }
            _ => {}
        }
    }
    let contradicted = term.iter().any(|literal| match literal {
        Literal::Lacks(name) => name == crate::KEY_CONTEXT || names.contains(&name.as_str()),
        Literal::IsNot(key, value) => values.contains(&(key.as_str(), value.as_str())),
        _ => false,
    });
    if contradicted {
        return None;
    }
    let mut frames = vec![ContextFrame::new(crate::KEY_CONTEXT)];
    frames.extend(
        (names.iter())
            .filter(|name| **name != crate::KEY_CONTEXT)
            .map(|name| ContextFrame::new(*name)),
    );
    let last = frames.pop().expect("the workbench's frame");
    let last = (values.iter()).fold(last, |frame, (key, value)| frame.set(*key, *value));
    frames.push(last);
    Some(frames.into_iter().collect())
}

/// The commands the palette lists, with the typed ones it leaves out that
/// `keymap` binds, each once: the typed commands, the app registry's and
/// the workbench's [`ListedCommands`]. Each is titled in the active locale,
/// resolved now (its `label()`).
fn commands(keymap: &Keymap, cx: &App) -> Vec<CommandInfo> {
    let bound = |id: &fe_command::CommandId| (keymap.bindings().iter()).any(|b| b.command == *id);
    let typed = kiln::platform::command_actions::typed_commands(cx)
        .into_iter()
        .filter(|typed| !typed.unlisted || bound(&typed.id))
        .map(|typed| CommandInfo {
            id: typed.id.as_str().to_owned(),
            title: typed.label().into(),
            context: typed.context.map(|context| context.to_string()),
        });
    let listed = cx
        .try_global::<ListedCommands>()
        .map(|listed| &listed.0[..])
        .unwrap_or_default();
    let metas = kiln::platform::commands::app_command_list(cx)
        .into_iter()
        .chain(listed.iter().cloned())
        .map(|meta| CommandInfo {
            id: meta.id.as_str().to_owned(),
            title: meta.label().into(),
            context: meta.context.map(|context| context.to_string()),
        });
    // The model keeps the first of an ID.
    typed.chain(metas).collect()
}

/// What a clashing binding's warning says: the other commands bound to
/// its keys where both apply, and those whose longer keys it starts.
/// `None` if it clashes with none.
pub(crate) fn conflict_note(binding: &ShortcutBinding) -> Option<String> {
    let mut also = Vec::new();
    let mut starts = Vec::new();
    for issue in &binding.issues {
        match issue {
            ShortcutIssue::Conflict { command, .. } if !also.contains(command) => {
                also.push(command.clone())
            }
            ShortcutIssue::Shadowed { by, keys } => {
                let keys = fe_keymap::shortcut_label(keys);
                let command = by.to_string();
                let starting = tr!(
                    "workbench-setup-shortcuts-command-keys",
                    command = command,
                    keys = keys
                );
                starts.push(starting.to_string())
            }
            _ => {}
        }
    }
    // The lists are joined here, as one argument each.
    let (also, starts) = (also.join(verbatim!(", ")), starts.join(verbatim!(", ")));
    let note = match (also.is_empty(), starts.is_empty()) {
        (true, true) => return None,
        (false, true) => tr!("workbench-setup-shortcuts-also-bound", commands = also),
        (true, false) => tr!("workbench-setup-shortcuts-starts-keys", commands = starts),
        (false, false) => tr!(
            "workbench-setup-shortcuts-also-bound-and-starts-keys",
            also = also,
            starts = starts
        ),
    };
    Some(note.to_string())
}

/// The keymaps the user's lies over, each file's path and text, for an
/// edit to read it on: the default layers of the keymap store the config
/// service reloads, kiln's and its modules' and extensions' with
/// fastedit's, or fastedit's alone without one. fastedit's is its file as
/// shipped, with every platform's keys; the others are written out with
/// each platform's entries as kiln read them there ([`layer_text`]), a
/// layer kept on any platform included, though not on this one (an
/// extension's file with an error here alone); or, from a store kept
/// without every platform's, as read on this one.
fn default_keymaps(cx: &App) -> Vec<(PathBuf, String)> {
    let Some(store) = kiln::platform::config::keymap_store(cx) else {
        return vec![(PathBuf::from(DEFAULT_KEYMAP_PATH), DEFAULT.to_owned())];
    };
    let text = |path: &Path, entries: &mut dyn Iterator<Item = (&Entry, Option<Os>)>| match path
        == Path::new(DEFAULT_KEYMAP_PATH)
    {
        true => DEFAULT.to_owned(),
        false => layer_text(entries),
    };
    let Some(platforms) = store.platform_defaults() else {
        return (store.defaults().iter())
            .map(|layer| {
                let mut entries = layer.entries.iter().map(|entry| (entry, None));
                (layer.path.clone(), text(&layer.path, &mut entries))
            })
            .collect();
    };
    // Every platform's layers in one order, each in its place among those
    // it lies over and under there.
    let mut order: Vec<(&Path, usize)> = Vec::new();
    for (_, layers) in &platforms {
        let mut at = 0;
        for (layer, _) in keyed(layers) {
            match order.iter().position(|other| *other == layer) {
                Some(ix) => at = at.max(ix + 1),
                None => {
                    order.insert(at, layer);
                    at += 1;
                }
            }
        }
    }
    (order.into_iter())
        .map(|key| {
            let mut entries = (platforms.iter()).flat_map(|(os, layers)| {
                (keyed(layers).find(|(other, _)| *other == key))
                    .into_iter()
                    .flat_map(move |(_, layer)| {
                        layer.entries.iter().map(move |entry| (entry, Some(*os)))
                    })
            });
            (key.0.to_owned(), text(key.0, &mut entries))
        })
        .collect()
}

/// Each of `layers` by its file and how many of that file's lie under
/// it, which tell the same layer apart on each platform.
fn keyed(layers: &[KeymapLayer]) -> impl Iterator<Item = ((&Path, usize), &KeymapLayer)> {
    (layers.iter().enumerate()).map(|(ix, layer)| {
        let under = (layers[..ix].iter())
            .filter(|under| under.path == layer.path)
            .count();
        ((layer.path.as_path(), under), layer)
    })
}

/// `entries` as a keymap file writes them, each as it was read on the
/// platform it's paired with, its keys for that one alone, or with
/// `None` on this one, its keys for any. Args TOML can't hold (a JSON
/// `null`) are left out: an edit matches defaults by keys, context and
/// command.
fn layer_text(entries: &mut dyn Iterator<Item = (&Entry, Option<Os>)>) -> String {
    let bindings = entries
        .map(|(entry, os)| {
            let mut table = toml::Table::new();
            let keys = entry.keys.to_string();
            let keys = match os {
                Some(os) => {
                    toml::Value::Table(toml::Table::from_iter([(os.name().into(), keys.into())]))
                }
                None => keys.into(),
            };
            table.insert("keys".into(), keys);
            if let Some(context) = &entry.context {
                table.insert("context".into(), context.to_string().into());
            }
            let command = entry
                .command
                .as_ref()
                .map_or("", |command| command.as_str());
            table.insert("command".into(), command.into());
            if !entry.args.is_null()
                && let Ok(args) = toml::Value::try_from(&entry.args)
            {
                table.insert("args".into(), args);
            }
            toml::Value::Table(table)
        })
        .collect();
    let mut file = toml::Table::new();
    file.insert("bindings".into(), toml::Value::Array(bindings));
    toml::to_string(&file).unwrap_or_default()
}

/// `keymaps` as [`fe_keymap::edit_keymap_on`] takes them.
fn borrowed(keymaps: &[(PathBuf, String)]) -> Vec<DefaultKeymap<'_>> {
    (keymaps.iter())
        .map(|(path, text)| (path.as_path(), text.as_str()))
        .collect()
}

/// Reset's refusal for a command whose default comes from a layer other
/// than fastedit's, which the queued write re-runs on the text it edits.
#[derive(Clone)]
struct ResetGuard {
    path: PathBuf,
    command: String,
}

impl ResetGuard {
    /// Whether the user's `source` has an entry gated on another platform
    /// that names the command or unbinds keys, whose effect on that
    /// default there can't be told.
    fn refuses(&self, source: &str) -> bool {
        let entries = |os: Os| {
            let (layer, _) = KeymapLayer::parse_for(&self.path, source, None, os);
            layer.map_or_else(Vec::new, |layer| layer.entries)
        };
        let here: Vec<_> = (entries(Os::current()).into_iter())
            .map(|entry| entry.location)
            .collect();
        (Os::ALL.into_iter())
            .filter(|os| *os != Os::current())
            .flat_map(entries)
            .any(|entry| {
                !here.contains(&entry.location)
                    && (entry.command.as_ref()).is_none_or(|id| id.as_str() == self.command)
            })
    }
}

/// What a queued write answers: the file written, or why it failed.
type WriteResult = Result<fe_keymap::KeymapWrite, String>;

/// An edit waiting for its file's worker.
struct QueuedWrite {
    defaults: Vec<(PathBuf, String)>,
    edit: KeymapEdit,
    /// Refuses the edit on the text it would edit, if it does.
    guard: Option<ResetGuard>,
    done: async_channel::Sender<WriteResult>,
    /// Asks the worker whether to wait before the write: for tests.
    #[cfg(test)]
    before: Option<BeforeFn>,
}

/// Each keymap file's queue of edits, whose worker writes them in the
/// order they were queued. The worker is detached from the tab and its
/// window, so an edit made before the tab closes still lands before one
/// made after.
#[derive(Default)]
struct KeymapWrites(BTreeMap<PathBuf, async_channel::Sender<QueuedWrite>>);

impl gpui::Global for KeymapWrites {}

/// Test-only: called with each edit in the worker before it's written; the
/// worker waits on the channel it returns, if any, so a test can hold an
/// edit up without blocking other tasks.
#[cfg(test)]
type BeforeFn = Arc<dyn Fn(&KeymapEdit) -> Option<async_channel::Receiver<()>> + Send + Sync>;

#[cfg(test)]
#[derive(Clone)]
pub(crate) struct BeforeWrite(pub(crate) BeforeFn);

#[cfg(test)]
impl gpui::Global for BeforeWrite {}

/// Queues `edit` to the keymap at `path` after the edits queued to it
/// before, and returns what answers once it's been written.
fn queue_write(
    path: PathBuf,
    defaults: Vec<(PathBuf, String)>,
    edit: KeymapEdit,
    guard: Option<ResetGuard>,
    cx: &mut App,
) -> async_channel::Receiver<WriteResult> {
    let (done, result) = async_channel::bounded(1);
    let mut job = QueuedWrite {
        defaults,
        edit,
        guard,
        done,
        #[cfg(test)]
        before: cx.try_global::<BeforeWrite>().map(|hook| hook.0.clone()),
    };
    loop {
        let writes = cx.default_global::<KeymapWrites>();
        let sender = writes.0.get(&path).filter(|sender| !sender.is_closed());
        if let Some(sender) = sender {
            match sender.try_send(job) {
                Ok(()) => return result,
                Err(async_channel::TrySendError::Closed(back)) => job = back,
                Err(async_channel::TrySendError::Full(_)) => unreachable!("unbounded"),
            }
        }
        let (sender, queued) = async_channel::unbounded::<QueuedWrite>();
        let at = path.clone();
        cx.background_spawn(async move {
            while let Ok(job) = queued.recv().await {
                #[cfg(test)]
                if let Some(gate) = job.before.as_ref().and_then(|before| before(&job.edit)) {
                    gate.recv().await.ok();
                }
                let written = fe_keymap::write_keymap_checked(
                    &at,
                    &borrowed(&job.defaults),
                    &job.edit,
                    Os::current(),
                    |source| match &job.guard {
                        // What a refused Reset's toast says when the file
                        // changed after the menu offered it.
                        Some(guard) if guard.refuses(source) => {
                            Err(tr!("workbench-setup-shortcuts-reset-refused").to_string())
                        }
                        _ => Ok(()),
                    },
                    || {},
                )
                .map_err(|error| error.to_string());
                job.done.try_send(written).ok();
            }
        })
        .detach();
        cx.global_mut::<KeymapWrites>()
            .0
            .insert(path.clone(), sender);
    }
}

/// The user's `keymap.toml`, if there's a config directory.
pub(crate) fn keymap_path(cx: &App) -> Option<PathBuf> {
    #[cfg(test)]
    if let Some(over) = cx.try_global::<crate::open::ConfigDirOverride>() {
        return over.0.as_ref().map(|dir| dir.join(fe_keymap::KEYMAP_FILE));
    }
    // A test with no override has no user config, not the developer's.
    #[cfg(test)]
    return None;
    #[cfg(not(test))]
    {
        let _ = cx;
        fe_keymap::user_keymap_path()
    }
}

/// Writes [`KEYMAP_HEADER`] to `path`, with its directory, unless the file
/// exists, which is never overwritten.
pub(crate) fn create_keymap(path: &Path) -> io::Result<()> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let mut file = match fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
    {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => return Ok(()),
        Err(error) => return Err(error),
    };
    file.write_all(KEYMAP_HEADER.as_bytes())
}

/// The selected line's menu items, by keyboard (A11Y-16b): each does what
/// the item does in the line's menu, where the menu offers it.
#[kiln::command(context = "shortcuts_editor")]
impl ShortcutsEditor {
    /// Keyboard Shortcuts: Copy Command ID
    ///
    /// Puts the selected line's command ID on the clipboard.
    #[command(id = "shortcuts.copy_command_id")]
    fn copy_command_id(&mut self, cx: &mut kiln::Cx<Self>) {
        self.run_on_selected(MenuItem::CopyCommandId, cx.window, cx.cx);
    }

    /// Keyboard Shortcuts: Reset Keybinding
    ///
    /// Removes the user's keymap entries for the selected line's command,
    /// putting back its default keys.
    #[command(id = "shortcuts.reset_binding")]
    fn reset_binding(&mut self, cx: &mut kiln::Cx<Self>) {
        self.run_on_selected(MenuItem::ResetKeybinding, cx.window, cx.cx);
    }

    /// Keyboard Shortcuts: Show Same Keybindings
    ///
    /// Searches for the selected line's keys.
    #[command(id = "shortcuts.show_same_keybindings")]
    fn show_same_keybindings(&mut self, cx: &mut kiln::Cx<Self>) {
        self.run_on_selected(MenuItem::ShowSameKeybindings, cx.window, cx.cx);
    }
}

impl Focusable for ShortcutsEditor {
    /// The search field's, so showing the tab focuses it.
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.search.focus_handle(cx)
    }
}

impl Render for ShortcutsEditor {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.follow_keymap(cx);
        let ui = fe_ui::ui_theme(cx);
        let body = if self.lines.is_empty() {
            div()
                .debug_selector(|| "shortcuts-empty".into())
                .px_6()
                .py_4()
                .text_color(ui.muted_text)
                .child(nothing_found())
                .into_any_element()
        } else {
            uniform_list(
                "shortcuts-lines",
                self.lines.len(),
                cx.processor(|this, range, window, cx| this.render_lines(range, window, cx)),
            )
            .track_scroll(&self.scroll)
            .flex_1()
            .into_any_element()
        };
        <Self as kiln::ViewCommands>::attach(div(), cx)
            .key_context(KEY_CONTEXT)
            .on_key_down(cx.listener(Self::on_key_down))
            .size_full()
            .flex()
            .flex_col()
            .bg(ui.background)
            .text_color(ui.text)
            .text_size(px(13.))
            .child(
                div()
                    .debug_selector(|| "shortcuts-search".into())
                    .on_action(cx.listener(Self::on_input_command))
                    .flex_none()
                    .px_6()
                    .pt_3()
                    .pb_2()
                    .child(self.search.clone()),
            )
            .child(self.header(cx))
            .child(
                div()
                    .track_focus(&self.focus)
                    .debug_selector(|| "shortcuts-list".into())
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .flex_col()
                    .child(body),
            )
            .children(self.render_menu(cx))
    }
}

impl PaneItem for ShortcutsEditor {
    const KIND: &'static str = "fastedit.shortcuts_editor";

    fn tab_title(&self, _: &App) -> SharedString {
        tr!("workbench-setup-shortcuts-title")
    }

    fn tab_icon(&self, _: &App) -> Option<Icon> {
        Some(Icon::FileCog)
    }

    fn window_title(&self, cx: &App) -> SharedString {
        format!("{} — {}", self.tab_title(cx), fe_config::APP_NAME).into()
    }
}

impl Workbench {
    /// Shows the Keyboard Shortcuts tab: the one open in any pane,
    /// activating that pane, or else a new one in the active pane. Either
    /// way its search field takes focus.
    pub(crate) fn open_keyboard_shortcuts(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        for pane in self.panes(cx) {
            let open = (pane.read(cx).tabs().iter())
                .position(|tab| tab.item().downcast_ref::<ShortcutsEditor>().is_some());
            if let Some(ix) = open {
                pane.update(cx, |pane, cx| pane.activate(ix, window, cx));
                self.activate(pane, window, cx);
                return;
            }
        }
        let editor = cx.new(ShortcutsEditor::new);
        self.pane().clone().update(cx, |pane, cx| {
            pane.insert_item(Box::new(editor), None, window, cx);
            cx.notify();
        });
    }

    /// The rows an open Keyboard Shortcuts tab lists for its search, each
    /// command's ID and title, as its lines show them; `None` if none is
    /// open. For tests outside this crate that follow a command's title,
    /// as one in a fake locale.
    #[doc(hidden)]
    pub fn shortcuts_editor_rows(&self, cx: &App) -> Option<Vec<(String, String)>> {
        self.panes(cx).iter().find_map(|pane| {
            (pane.read(cx).tabs().iter()).find_map(|tab| {
                let editor = tab.item().downcast_ref::<ShortcutsEditor>()?;
                let rows = (editor.read(cx).found.iter())
                    .map(|row| (row.command.clone(), row.title.clone()))
                    .collect();
                Some(rows)
            })
        })
    }
}

#[path = "shortcuts_recorder.rs"]
pub(crate) mod recorder;

#[cfg(test)]
#[path = "shortcuts_editor_queue_tests.rs"]
mod queue_tests;
