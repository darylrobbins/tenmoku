//! The Settings editor, as VS Code's (`workbench.action.openSettings2`):
//! a tab, opened by `workbench.open_settings_ui`, that lists every setting
//! [`SettingsModel`] knows, grouped as the schema nests them, with its
//! description and its value. Read-only for now: it shows values as text.
//!
//! - A search field on top, focused when the tab opens, filters the list
//!   as it's typed ([`SettingsModel::search`]); a group with nothing found
//!   loses its heading, and nothing found at all says so.
//! - Under it, the User and Workspace scopes, as VS Code's tabs: a row
//!   shows the selected scope's file's value, else what it inherits, and
//!   a 2px accent bar at its left edge if that file sets it. Workspace
//!   shows only with a folder open, and dims the rows a project's file
//!   can't set ([`SettingsModel::workspace_allowed`]).
//! - The files are read off the main thread when the tab opens, when the
//!   scope changes, when the root changes, when the config service's
//!   watcher sees either file change ([`ChangedFiles`]), even in a way
//!   that changes no effective setting, when the workbench reloads its
//!   settings ([`Workbench::reload_settings`]), and when the tab is
//!   activated or focus comes back into it from elsewhere. The watcher
//!   watches every workbench's root's project file, not only the launch
//!   root's.
//! - Up and Down move the selected row, scrolling it into view; Down in
//!   the search field moves into the list. `/` or secondary-f goes back to
//!   the search, and Escape clears it. The search takes the text inputs'
//!   editing keys (`text_input.backspace` and the like).
//! - Only the rows in view are built: the list is a `uniform_list`.
//!
//! It has no path and serializes to nothing, so it isn't kept in saved
//! layouts, and closes as any tab does.

use std::{
    ops::Range,
    path::{Path, PathBuf},
};

use fe_config::{SettingEntry, SettingKind, SettingsModel};
use fe_gpui::commands::RunCommand;
use fe_ui::{Icon, TextInput, TextInputEvent, ui_theme};
use gpui::{
    AnyElement, App, AppContext as _, Context, Entity, FocusHandle, Focusable,
    InteractiveElement as _, IntoElement, KeyDownEvent, ParentElement as _, Pixels, Render,
    ScrollStrategy, SharedString, StatefulInteractiveElement as _, Styled as _, Subscription, Task,
    UniformListScrollHandle, Window, div, prelude::FluentBuilder as _, px, uniform_list,
};
use kiln::platform::config::{self, ChangedFiles};
use kiln_shell::{PaneItem, PaneView as _};

use crate::{
    RootChanged, Workbench, open,
    picker::{self, PaletteOp},
};

/// The key context the editor's element sets.
pub(crate) const KEY_CONTEXT: &str = "settings_editor";

/// Every row's height, a group heading's as a setting's: the list is a
/// `uniform_list`. A setting's holds its title line, two lines of
/// description and its value line, with [`ROW_PADDING`] above and below
/// and [`ROW_GAP`] between.
const ROW_HEIGHT: Pixels = px(96.);

/// The height of a setting's title line and of its value line.
const LINE_HEIGHT: Pixels = px(18.);

/// The height of a line of a setting's description, which shows up to two.
const DESCRIPTION_LINE_HEIGHT: Pixels = px(16.);

const ROW_PADDING: Pixels = px(10.);

const ROW_GAP: Pixels = px(4.);

/// What a workspace-restricted row says under the Workspace scope, as VS
/// Code's.
pub(crate) const WORKSPACE_RESTRICTED: &str = "This setting cannot be changed in a workspace";

/// What the list says when the search finds nothing.
pub(crate) const NOTHING_FOUND: &str = "No settings found";

/// Which settings file the editor shows values from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Scope {
    /// The user's `settings.toml`.
    User,
    /// The root's `.fastedit/settings.toml`.
    Workspace,
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
    scope: Scope,
    /// The workbench's root, whose settings file the Workspace scope shows.
    root: Option<PathBuf>,
    /// `None` until the files are first read.
    model: Option<SettingsModel>,
    /// What the search finds, with its groups' headings.
    rows: Vec<Row>,
    /// The selected row, a setting's.
    selected: Option<usize>,
    scroll: UniformListScrollHandle,
    /// The read of the files under way, dropped for a newer one.
    load: Task<()>,
    /// Each range of rows the list built, for the tests.
    #[cfg(test)]
    built: Vec<Range<usize>>,
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
        let search = cx.new(|cx| TextInput::new("Search settings", &ui, cx).bordered());
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
            cx.subscribe(workbench, |this, _, RootChanged(root), cx| {
                this.root = Some(root.clone());
                this.reload(cx);
            }),
        ];
        let mut this = Self {
            root_focus,
            focus: cx.focus_handle(),
            search,
            scope: Scope::User,
            root,
            model: None,
            rows: Vec::new(),
            selected: None,
            scroll: UniformListScrollHandle::new(),
            load: Task::ready(()),
            #[cfg(test)]
            built: Vec::new(),
            _subscriptions: subscriptions,
        };
        this.reload(cx);
        this
    }

    /// Reads the user's and the root's settings files again, off the main
    /// thread, and shows what they hold once read.
    pub(crate) fn reload(&mut self, cx: &mut Context<Self>) {
        let (user, project) = self.files(cx);
        let read = cx.background_spawn(async move {
            let user = user.and_then(|path| read_settings(&path));
            let project = project.and_then(|path| read_settings(&path));
            SettingsModel::new(user.as_deref(), project.as_deref())
        });
        self.load = cx.spawn(async move |this, cx| {
            let model = read.await;
            this.update(cx, |this, cx| {
                this.model = Some(model);
                this.refilter(cx);
            })
            .ok();
        });
    }

    /// The user's settings file and the root's.
    fn files(&self, cx: &App) -> (Option<PathBuf>, Option<PathBuf>) {
        let project = self.root.as_deref().map(fe_config::project_settings_path);
        (open::settings_path(cx), project)
    }

    /// The rows for the search as it is now, keeping the selected setting
    /// selected if it's still found.
    fn refilter(&mut self, cx: &mut Context<Self>) {
        let selected = self.selected_setting();
        self.rows = match &self.model {
            Some(model) => rows(model, self.search.read(cx).text()),
            None => Vec::new(),
        };
        self.selected = selected
            .and_then(|entry| (self.rows.iter()).position(|row| *row == Row::Setting(entry)));
        cx.notify();
    }

    /// The selected setting's index in the model's entries.
    fn selected_setting(&self) -> Option<usize> {
        match self.rows.get(self.selected?)? {
            Row::Setting(entry) => Some(*entry),
            Row::Group(_) => None,
        }
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
        self.reload(cx);
        cx.notify();
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
    pub(crate) fn take_built(&mut self) -> Vec<Range<usize>> {
        std::mem::take(&mut self.built)
    }

    /// `entry`'s value as the selected scope shows it, as text, and
    /// whether that scope's file sets it; `None` until the files are read.
    #[cfg(test)]
    pub(crate) fn shown_value(&self, entry: &SettingEntry) -> Option<(String, bool)> {
        let model = self.model.as_ref()?;
        Some(shown_value(model, entry, self.scope()))
    }

    /// Selects the setting below the selected one, or with `down` false the
    /// one above, skipping headings, or the first with none selected, and
    /// scrolls it into view.
    fn move_selection(&mut self, down: bool, cx: &mut Context<Self>) {
        let settings: Vec<usize> = (self.rows.iter().enumerate())
            .filter(|(_, row)| matches!(row, Row::Setting(_)))
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
        // Going up to a group's first setting, its heading comes into view
        // with it, the heading at the top, if both fit and either is out
        // of view; else the setting alone comes into view.
        let (height, scrolled) = {
            let handle = &self.scroll.0.borrow().base_handle;
            (handle.bounds().size.height, -handle.offset().y)
        };
        let in_view = |ix: usize| {
            let top = ROW_HEIGHT * ix as f32 - scrolled;
            top >= Pixels::ZERO && top + ROW_HEIGHT <= height
        };
        let heading = (next.checked_sub(1)).filter(|ix| {
            !down && height >= ROW_HEIGHT * 2. && matches!(self.rows[*ix], Row::Group(_))
        });
        match heading {
            Some(heading) if !(in_view(heading) && in_view(next)) => {
                (self.scroll).scroll_to_item_strict(heading, ScrollStrategy::Top)
            }
            _ => (self.scroll).scroll_to_item(next, ScrollStrategy::Nearest),
        }
        cx.notify();
    }

    /// The search field's editing commands (`text_input.backspace` and the
    /// like, which the input leaves to its host); others go on.
    fn on_input_command(&mut self, run: &RunCommand, _: &mut Window, cx: &mut Context<Self>) {
        let edit: fn(&mut TextInput, &mut Context<TextInput>) = match picker::op(run, cx) {
            Some(PaletteOp::Backspace) => TextInput::backspace,
            Some(PaletteOp::DeleteWordLeft) => TextInput::delete_word_left,
            Some(PaletteOp::Clear) => TextInput::clear,
            Some(PaletteOp::Paste) => TextInput::paste,
            _ => return cx.propagate(),
        };
        self.search.update(cx, edit);
    }

    /// Its keys: see the module docs.
    fn on_key_down(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let keystroke = &event.keystroke;
        let m = keystroke.modifiers;
        let searching = self.search.focus_handle(cx).is_focused(window);
        match keystroke.key.as_str() {
            "up" | "down" if !m.modified() => {
                let down = keystroke.key == "down";
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
            "escape" if !m.modified() && !self.search.read(cx).text().is_empty() => {
                self.search.update(cx, |search, cx| search.clear(cx));
            }
            _ => return,
        }
        cx.stop_propagation();
    }

    fn render_rows(
        &mut self,
        range: Range<usize>,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        #[cfg(test)]
        self.built.push(range.clone());
        let ui = ui_theme(cx);
        let Some(model) = &self.model else {
            return Vec::new();
        };
        let scope = self.scope();
        let selected_bg = if self.focus.is_focused(window) {
            ui.row_selected
        } else {
            ui.row_selected_inactive
        };
        range
            .filter_map(|ix| {
                let row = match self.rows.get(ix)? {
                    Row::Group(title) => div()
                        .debug_selector({
                            let title = title.clone();
                            move || format!("settings-group-{title}")
                        })
                        .h(ROW_HEIGHT)
                        .px_6()
                        .pb_2()
                        .flex()
                        .flex_col()
                        .justify_end()
                        .text_size(px(16.))
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .text_color(ui.text)
                        .child(title.clone())
                        .into_any_element(),
                    Row::Setting(entry) => {
                        let entry = &model.entries()[*entry];
                        let restricted =
                            scope == Scope::Workspace && !model.workspace_allowed(entry);
                        self.setting_row(ix, entry, restricted, model, cx)
                            .when(self.selected == Some(ix), |row| row.bg(selected_bg))
                            .into_any_element()
                    }
                };
                Some(row)
            })
            .collect()
    }

    /// Row `ix`, `entry`'s: its title and key path, its description, and
    /// its value, dimmed with a note if `restricted`.
    fn setting_row(
        &self,
        ix: usize,
        entry: &SettingEntry,
        restricted: bool,
        model: &SettingsModel,
        cx: &mut Context<Self>,
    ) -> gpui::Stateful<gpui::Div> {
        let ui = ui_theme(cx);
        let key = entry.key_path();
        let (value, modified) = shown_value(model, entry, self.scope());
        let value = match entry.kind {
            SettingKind::Other => "Edit in settings.toml".to_owned(),
            _ => value,
        };
        let name = entry
            .key
            .last()
            .map_or_else(String::new, |name| title(name));
        div()
            .id(("settings-row", ix))
            .debug_selector({
                let key = key.clone();
                move || format!("settings-row-{key}")
            })
            .relative()
            .h(ROW_HEIGHT)
            .px_6()
            .py(ROW_PADDING)
            .flex()
            .flex_col()
            .gap(ROW_GAP)
            .overflow_hidden()
            .when(restricted, |row| row.opacity(0.5))
            .on_click(cx.listener(move |this, _, window, cx| {
                this.selected = Some(ix);
                window.focus(&this.focus, cx);
                cx.notify();
            }))
            .when(modified, |row| {
                row.child(
                    div()
                        .debug_selector({
                            let key = key.clone();
                            move || format!("settings-modified-{key}")
                        })
                        .absolute()
                        .left_0()
                        .top_0()
                        .bottom_0()
                        .w(px(2.))
                        .bg(ui.accent),
                )
            })
            .child(
                div()
                    .flex_none()
                    .h(LINE_HEIGHT)
                    .line_height(LINE_HEIGHT)
                    .flex()
                    .items_baseline()
                    .gap_2()
                    .whitespace_nowrap()
                    .overflow_hidden()
                    .child(
                        div()
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .text_color(ui.text)
                            .child(name),
                    )
                    .child(
                        div()
                            .text_color(ui.muted_text)
                            .text_size(px(12.))
                            .child(key.clone()),
                    ),
            )
            .child(
                div()
                    .debug_selector({
                        let key = key.clone();
                        move || format!("settings-description-{key}")
                    })
                    .flex_none()
                    .h(DESCRIPTION_LINE_HEIGHT * 2.)
                    .line_height(DESCRIPTION_LINE_HEIGHT)
                    .text_size(px(12.))
                    .text_color(ui.muted_text)
                    .line_clamp(2)
                    .child(entry.description.clone()),
            )
            .child(
                div()
                    .debug_selector({
                        let key = key.clone();
                        move || format!("settings-value-{key}")
                    })
                    .flex_none()
                    .h(LINE_HEIGHT)
                    .line_height(LINE_HEIGHT)
                    .flex()
                    .gap_3()
                    .whitespace_nowrap()
                    .overflow_hidden()
                    .text_size(px(12.))
                    .child(div().text_color(ui.text).child(value))
                    .when(restricted, |line| {
                        line.child(
                            div()
                                .debug_selector(move || format!("settings-restricted-{key}"))
                                .text_color(ui.muted_text)
                                .child(WORKSPACE_RESTRICTED),
                        )
                    }),
            )
    }

    /// The User and Workspace tabs.
    fn scope_bar(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let ui = ui_theme(cx);
        let shown = self.scope();
        div()
            .flex_none()
            .flex()
            .gap_4()
            .px_6()
            .border_b_1()
            .border_color(ui.border)
            .children(self.scopes().iter().map(|&scope| {
                let (label, selector) = match scope {
                    Scope::User => ("User", "settings-scope-user"),
                    Scope::Workspace => ("Workspace", "settings-scope-workspace"),
                };
                let selected = scope == shown;
                div()
                    .id(selector)
                    .debug_selector(move || selector.into())
                    .py_1p5()
                    .border_b_2()
                    .border_color(if selected {
                        ui.accent
                    } else {
                        gpui::transparent_black()
                    })
                    .text_color(if selected { ui.text } else { ui.muted_text })
                    .cursor_pointer()
                    .on_click(cx.listener(move |this, _, _, cx| this.set_scope(scope, cx)))
                    .child(label)
            }))
    }
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

/// The rows `query` finds in `model`: the top level's settings, then each
/// group's under its heading, in the schema's order, leaving out the
/// groups it finds nothing in.
pub(crate) fn rows(model: &SettingsModel, query: &str) -> Vec<Row> {
    let entries = model.entries();
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
            .filter(|(ix, entry)| matched[*ix] && entry.key[..entry.key.len() - 1] == *key)
            .map(|(ix, _)| Row::Setting(ix))
            .collect()
    };
    let mut rows = in_group(&[]);
    for group in model.groups() {
        let settings = in_group(&group.key);
        if !settings.is_empty() {
            rows.push(Row::Group(group.title.clone()));
            rows.extend(settings);
        }
    }
    rows
}

/// `entry`'s value as `scope` shows it, as text: its file's, else what it
/// inherits (the user's, under Workspace) or the default; and whether
/// `scope`'s file sets it.
fn shown_value(model: &SettingsModel, entry: &SettingEntry, scope: Scope) -> (String, bool) {
    let set = match scope {
        Scope::User => model.user_value(entry),
        Scope::Workspace => model.workspace_value(entry),
    };
    let modified = set.is_some();
    let inherited = match scope {
        Scope::User => None,
        Scope::Workspace => model.user_value(entry),
    };
    let value = (set.or(inherited)).unwrap_or_else(|| entry.default.clone());
    let text = match value.as_str() {
        Some(text) => text.to_owned(),
        None if value.is_null() => "Not set".to_owned(),
        None => value.to_string(),
    };
    (text, modified)
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

impl Focusable for SettingsEditor {
    /// The search field's, so showing the tab focuses it.
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.search.focus_handle(cx)
    }
}

impl Render for SettingsEditor {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let ui = ui_theme(cx);
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
                .child(NOTHING_FOUND)
                .into_any_element()
        } else {
            uniform_list(
                "settings-rows",
                self.rows.len(),
                cx.processor(|this, range, window, cx| this.render_rows(range, window, cx)),
            )
            .track_scroll(&self.scroll)
            .flex_1()
            .into_any_element()
        };
        div()
            .track_focus(&self.root_focus)
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
                    .debug_selector(|| "settings-search".into())
                    .on_action(cx.listener(Self::on_input_command))
                    .flex_none()
                    .px_6()
                    .pt_3()
                    .pb_2()
                    .child(self.search.clone()),
            )
            .child(self.scope_bar(cx))
            .children(errors)
            .child(
                div()
                    .track_focus(&self.focus)
                    .debug_selector(|| "settings-list".into())
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .flex_col()
                    .child(body),
            )
    }
}

impl PaneItem for SettingsEditor {
    const KIND: &'static str = "fastedit.settings_editor";

    fn tab_title(&self, _: &App) -> SharedString {
        "Settings".into()
    }

    fn tab_icon(&self, _: &App) -> Option<Icon> {
        Some(Icon::FileCog)
    }

    fn window_title(&self, cx: &App) -> SharedString {
        format!("{} — fastedit", self.tab_title(cx)).into()
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
