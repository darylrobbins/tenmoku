//! The Settings tab's controls, a function per [`SettingKind`], each
//! drawing the control for a row's value and committing a change through
//! [`SettingsEditor::write`]:
//!
//! - [`SettingKind::Bool`]: a switch (`setting-switch-<key>`), toggled by a
//!   click, or Space on its selected row.
//! - [`SettingKind::Enum`]: a dropdown (`setting-select-<key>`) of the
//!   schema's variants, anchored under its button, which a click or Enter
//!   on the selected row opens. Up and Down move through it, scrolling the
//!   highlighted variant into view of a list taller than the window
//!   allows, Enter or a click picks (`setting-option-<key>-<variant>`),
//!   and Escape or a click elsewhere closes it.
//! - [`SettingKind::Choice`]: the same dropdown, of the list the app gives
//!   now ([`choices`](super::choices)): the themes installed, say. One
//!   unset by default lists Default (built-in) first
//!   (`setting-option-default-<key>`), which resets it. A value not in the
//!   list, a theme since removed, shows with a "not installed" hint
//!   (`setting-hint-<key>`), and stays until another is picked; one the
//!   list has by another name (`theme.name`'s bundled theme by ID) shows
//!   as the file has it. An open list keeps the options it opened with.
//!   A model or font not listed shows with no hint, and the fonts are
//!   only listed when the dropdown opens. The model's list ends in Other…
//!   (`setting-option-other-<key>`), which swaps the row to an input
//!   (`setting-input-<key>`), focused, for an id the list hasn't; leaving
//!   it commits it and brings the dropdown back, unless it isn't a value
//!   the setting takes.
//! - [`SettingKind::Int`], [`SettingKind::Float`] and [`SettingKind::Text`]:
//!   an input (`setting-input-<key>`), committed on Enter or when it loses
//!   focus. Text that isn't a number, or is out of the schema's bounds,
//!   isn't written: why shows under the input (`setting-message-<key>`),
//!   in the words of the config problems for the file. Nothing typed into
//!   one unset by default resets it.
//! - [`SettingKind::TextList`]: an input per item
//!   (`setting-item-<key>-<n>`), each with a remove button
//!   (`setting-remove-<key>-<n>`), and Add Item (`setting-add-<key>`).
//!   Committing an item, or removing one, writes the whole list, an
//!   emptied item as an empty string; an item Add Item made isn't written
//!   while it's empty.
//! - [`SettingKind::Other`]: Edit in settings.toml (`setting-edit-<key>`),
//!   which opens the selected scope's file with the caret on the key's
//!   table, if the file has it.
//!
//! Every control is the design's 26 px control ([`CONTROL_HEIGHT`]): a
//! switch centred in that height, a dropdown's button, an input, a list's
//! inputs and Add Item, and Edit in settings.toml as a button.
//!
//! An input keeps what's typed into it across renders, and the file's
//! value replaces it only while it has no focus or holds nothing typed.
//! Reset to Default drops what's typed, so leaving the input after writes
//! nothing. A switch toggles from the value shown, a change still being
//! written included.

use std::{cell::Cell, fs::OpenOptions, io, path::Path, rc::Rc};

use fe_command::Value as Json;
use fe_config::{
    EnumVariant, Layer, SettingEditError, SettingEntry, SettingKind, SettingScope, SettingValue,
    parse_layer, reset_setting, set_setting, settings_schema,
};
use fe_ui::{Button, Icon, Switch, TextInput, TextInputEvent, ui_theme};
use gpui::{
    AnyElement, App, AppContext as _, Context, Entity, Focusable as _, InteractiveElement as _,
    IntoElement, MouseButton, ParentElement as _, ScrollHandle, SharedString,
    StatefulInteractiveElement as _, Styled as _, Subscription, Window, anchored, deferred, div,
    prelude::FluentBuilder as _, px, svg,
};
use kiln_l10n::{text, tr};

use super::{CONTROL_HEIGHT, Scope, SettingsEditor, choices, read_settings, selector};
use crate::{
    open,
    pane::Reveal,
    workbench::toast::{self, Toast},
};

/// The height of a dropdown's button.
const SELECT_HEIGHT: gpui::Pixels = CONTROL_HEIGHT;

/// The least height of each of a dropdown's variants.
const OPTION_HEIGHT: gpui::Pixels = px(24.);

/// The tallest a dropdown's list gets, the window allowing.
const MENU_HEIGHT: gpui::Pixels = px(400.);

/// What the Default (built-in) option of a choice unset by default says.
pub(super) fn default_choice() -> SharedString {
    tr!("workbench-settings-default-choice")
}

/// What a choice whose value isn't in its list says beside it.
pub(super) fn not_installed() -> SharedString {
    tr!("workbench-settings-not-installed")
}

/// A dropdown's options for `entry`, `None` if it has none: an enum's
/// variants, or the list a choice is one of now, after Default (built-in),
/// a `None` option, if it's unset by default.
pub(super) fn options(entry: &SettingEntry, cx: &App) -> Option<Vec<Option<EnumVariant>>> {
    match &entry.kind {
        SettingKind::Enum(variants) => Some(variants.iter().cloned().map(Some).collect()),
        SettingKind::Choice(source) => {
            let default = entry.default.is_null().then_some(None);
            let listed = choices::variants(*source, cx).into_iter().map(Some);
            Some(default.into_iter().chain(listed).collect())
        }
        _ => None,
    }
}

/// What picking `option` writes.
fn pick(option: &Option<EnumVariant>) -> Change {
    match option {
        Some(variant) => Change::Set(Value::Str(variant.value.clone())),
        None => Change::Reset,
    }
}

/// The index in `options`, `entry`'s, of the one `value` is: for a
/// choice, the one it means, as a bundled theme's ID means its title.
fn option_of(entry: &SettingEntry, options: &[Option<EnumVariant>], value: &Json) -> Option<usize> {
    let listed = match (&entry.kind, value.as_str()) {
        (SettingKind::Choice(source), Some(text)) => Some(choices::listed_as(*source, text)),
        (_, text) => text.map(str::to_owned),
    };
    options.iter().position(|option| match option {
        Some(variant) => listed.as_deref() == Some(variant.value.as_str()),
        None => value.is_null(),
    })
}

/// An open dropdown: the options it opened with, which it keeps while
/// open, so that a list changing meanwhile (an extension's themes
/// reloading) changes neither what it shows nor what a pick writes.
pub(super) struct Select {
    /// The setting's dotted key.
    key: String,
    options: Vec<Option<EnumVariant>>,
    /// Whether Other… follows the options, highlighted at their length.
    other: bool,
    highlighted: usize,
    /// The menu's scroll, which keeps the highlighted option in view.
    scroll: ScrollHandle,
}

/// What a dropdown of `options` showing `value` says, and its hint if it's
/// a choice whose value isn't one of them.
pub(super) fn select_label(
    entry: &SettingEntry,
    options: &[Option<EnumVariant>],
    value: &Json,
) -> (String, Option<SharedString>) {
    let choice = matches!(entry.kind, SettingKind::Choice(_));
    if choice && value.is_null() {
        return (default_choice().to_string(), None);
    }
    let text = value.as_str().unwrap_or_default().to_owned();
    let flags = match entry.kind {
        SettingKind::Choice(source) => !choices::takes_any(source),
        _ => false,
    };
    let missing = flags && option_of(entry, options, value).is_none();
    (text, missing.then(not_installed))
}

/// A value a control writes.
#[derive(Clone, Debug, PartialEq)]
pub(super) enum Value {
    Bool(bool),
    Str(String),
    Int(i64),
    Float(f64),
    List(Vec<String>),
}

impl Value {
    /// As the model gives values.
    pub(super) fn json(&self) -> Json {
        match self {
            Value::Bool(value) => Json::Bool(*value),
            Value::Str(text) => Json::String(text.clone()),
            Value::Int(value) => Json::from(*value),
            Value::Float(value) => Json::from(*value),
            Value::List(items) => Json::Array(items.iter().cloned().map(Json::String).collect()),
        }
    }

    /// As TOML, to check against the schema.
    fn toml(&self) -> toml::Value {
        match self {
            Value::Bool(value) => toml::Value::Boolean(*value),
            Value::Str(text) => toml::Value::String(text.clone()),
            Value::Int(value) => toml::Value::Integer(*value),
            Value::Float(value) => toml::Value::Float(*value),
            Value::List(items) => {
                toml::Value::Array(items.iter().cloned().map(toml::Value::String).collect())
            }
        }
    }
}

/// What a write does at a key.
#[derive(Clone, Debug, PartialEq)]
pub(super) enum Change {
    Set(Value),
    /// Removes the key: the setting goes back to its default.
    Reset,
}

impl Change {
    /// Makes the change at `key` in `scope`'s file, `root`'s for a
    /// workspace, unless that file is there and can't be written to.
    pub(super) fn write(
        &self,
        scope: SettingScope,
        root: &Path,
        key: &[&str],
    ) -> Result<(), SettingEditError> {
        let file = match scope {
            SettingScope::User => fe_config::user_settings_path(),
            SettingScope::Workspace => Some(fe_config::project_settings_path(root)),
        };
        if let Some(file) = file {
            writable(&file)?;
        }
        let value = match self {
            Change::Reset => return reset_setting(scope, root, key),
            Change::Set(value) => value,
        };
        let items: Vec<&str>;
        let value = match value {
            Value::Bool(value) => SettingValue::Bool(*value),
            Value::Str(text) => SettingValue::Str(text),
            Value::Int(value) => SettingValue::Int(*value),
            Value::Float(value) => SettingValue::Float(*value),
            Value::List(list) => {
                items = list.iter().map(String::as_str).collect();
                SettingValue::StrList(&items)
            }
        };
        set_setting(scope, root, key, value)
    }
}

/// Fails if the file at `path` is there and can't be opened for writing,
/// with a read-only mode, say: the writer, renaming a new file over it,
/// would replace it regardless. It's opened without blocking, as opening a
/// FIFO would block, and without changing it.
fn writable(path: &Path) -> io::Result<()> {
    let mut options = OpenOptions::new();
    options.write(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::custom_flags(&mut options, libc::O_NONBLOCK);
    match options.open(path) {
        Ok(_) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(io::Error::new(
            error.kind(),
            tr!(
                "workbench-settings-cant-write",
                path = path.display().to_string(),
                error = error.to_string()
            )
            .to_string(),
        )),
    }
}

/// A number or text input, or a list item's, kept across renders.
pub(super) struct Field {
    input: Entity<TextInput>,
    /// The file's value, as text, when the input last took it; `None` to
    /// take it again at the next render. Text that differs is typed.
    shown: Option<String>,
    /// The typed text that wasn't written, as it was committed.
    rejected: Option<String>,
    /// Why it wasn't ([`rejection`]), in the language as the files were
    /// last read: found again as each read lands, a change of language's
    /// among them ([`SettingsEditor::refresh_messages`]).
    message: Option<String>,
    /// A list's item that Add Item made, which isn't written while it's
    /// empty.
    added: bool,
    /// Whether the input changed since the last commit or sync from the
    /// file, even if its text is back to what it showed.
    edited: Rc<Cell<bool>>,
    _blur: Subscription,
    _edit: Subscription,
}

impl Field {
    /// Has the next render show the file's value, typed text or not.
    pub(super) fn forget_shown(&mut self) {
        self.shown = None;
        self.rejected = None;
        self.message = None;
        self.edited.set(false);
    }

    fn typed(&self, cx: &App) -> bool {
        self.edited.get() || self.shown.as_deref() != Some(self.input.read(cx).text())
    }
}

/// A list's editor: an input per item.
pub(super) struct ListField {
    /// The file's items when the inputs last took them; `None` to take them
    /// again at the next render.
    shown: Option<Vec<String>>,
    items: Vec<Field>,
    /// The items' sideways scroll, which keeps the item the keys moved to
    /// in view.
    scroll: ScrollHandle,
}

impl ListField {
    /// Has the next render show the file's items, typed text or not.
    pub(super) fn forget_shown(&mut self) {
        self.shown = None;
    }

    /// Whether an item has been edited since the last commit or sync.
    fn edited(&self) -> bool {
        self.items.iter().any(|item| item.edited.get())
    }

    fn clear_edited(&self) {
        for item in &self.items {
            item.edited.set(false);
        }
    }

    /// The items typed, less the ones Add Item made that are still empty.
    /// An item emptied stays, as an empty string.
    fn texts(&self, cx: &App) -> Vec<String> {
        (self.items.iter())
            .map(|item| (item.added, item.input.read(cx).text().to_owned()))
            .filter(|(added, text)| !(*added && text.is_empty()))
            .map(|(_, text)| text)
            .collect()
    }

    /// Has the items typed into stay, written.
    fn commit_added(&mut self, cx: &App) {
        for item in &mut self.items {
            item.added &= item.input.read(cx).text().is_empty();
        }
    }
}

/// The input that has focus in a row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Editing {
    /// The input of the setting at this dotted key.
    Field(String),
    /// An item of the list at this dotted key.
    Item(String, usize),
}

impl Editing {
    fn key(&self) -> &str {
        match self {
            Editing::Field(key) | Editing::Item(key, _) => key,
        }
    }
}

/// `value` as an input shows it: nothing for a value unset by default.
fn input_text(value: &Json) -> String {
    match value {
        Json::String(text) => text.clone(),
        Json::Null => String::new(),
        value => value.to_string(),
    }
}

/// `value`'s items, as a list editor shows them.
fn list_items(value: &Json) -> Vec<String> {
    (value.as_array().into_iter().flatten())
        .filter_map(|item| item.as_str().map(str::to_owned))
        .collect()
}

/// The value `text`, typed into `entry`'s input, sets; `Ok(None)` for
/// nothing typed into one unset by default, which resets it; or why it
/// isn't a value of the setting's kind.
pub(super) fn parse(entry: &SettingEntry, text: &str) -> Result<Option<Value>, String> {
    let trimmed = text.trim();
    if trimmed.is_empty() && entry.default.is_null() {
        return Ok(None);
    }
    match entry.kind {
        SettingKind::Int { .. } => (trimmed.parse().ok())
            .map(|value| Some(Value::Int(value)))
            .ok_or_else(|| tr!("workbench-settings-not-whole-number").to_string()),
        SettingKind::Float { .. } => (trimmed.parse::<f64>().ok())
            .filter(|value| value.is_finite())
            .map(|value| Some(Value::Float(value)))
            .ok_or_else(|| tr!("workbench-settings-not-number").to_string()),
        _ => Ok(Some(Value::Str(text.to_owned()))),
    }
}

/// Why `text`, typed into `entry`'s input, isn't a value it takes, if it
/// isn't: [`parse`]'s error, else the schema's [`problem`].
fn rejection(entry: &SettingEntry, text: &str) -> Option<String> {
    match parse(entry, text) {
        Err(message) => Some(message),
        Ok(Some(value)) => problem(entry, &value),
        Ok(None) => None,
    }
}

/// Why `entry` can't take `value`, as the config problems for a file
/// setting it would say: out of the schema's range, say.
pub(super) fn problem(entry: &SettingEntry, value: &Value) -> Option<String> {
    let (name, tables) = entry.key.split_last()?;
    let mut table = toml::Table::from_iter([(name.clone(), value.toml())]);
    for part in tables.iter().rev() {
        table = toml::Table::from_iter([(part.clone(), toml::Value::Table(table))]);
    }
    let mut layer = Layer::default();
    layer.table = table;
    (layer.check_schema(&settings_schema()).into_iter())
        .find(|diagnostic| diagnostic.is_error())
        .map(|diagnostic| diagnostic.message)
}

/// The 0-based row of `key` in `text`, a settings file: where its table's
/// header names it (`[[vim.remap]]`, or `[lsp.servers.rust]` for
/// `lsp.servers`), or where it's set. `None` if the file doesn't parse or
/// hasn't it.
pub(super) fn table_row(text: &str, key: &[String]) -> Option<u32> {
    if key.is_empty() {
        return None;
    }
    let layer = parse_layer(Path::new(""), text.to_owned());
    let offset = layer.key_offset(key)?;
    let row = text.get(..offset)?.matches('\n').count();
    u32::try_from(row).ok()
}

impl SettingsEditor {
    /// The control for row `ix`'s `entry`, showing `value`; off if
    /// `disabled`.
    pub(super) fn control(
        &mut self,
        ix: usize,
        entry: &SettingEntry,
        value: &Json,
        disabled: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let control = match &entry.kind {
            SettingKind::Bool => self.switch(entry, value, disabled, cx),
            SettingKind::Choice(_) if self.typing.contains(&entry.key_path()) => {
                self.input(ix, entry, value, disabled, window, cx)
            }
            SettingKind::Enum(_) | SettingKind::Choice(_) => {
                // A list that flags no value isn't needed to draw the
                // button: the fonts aren't listed at every render.
                let options = match entry.kind {
                    SettingKind::Choice(source) if choices::takes_any(source) => Vec::new(),
                    _ => options(entry, cx).unwrap_or_default(),
                };
                self.dropdown(ix, entry, &options, value, disabled, window, cx)
            }
            SettingKind::Int { .. } | SettingKind::Float { .. } | SettingKind::Text { .. } => {
                self.input(ix, entry, value, disabled, window, cx)
            }
            SettingKind::TextList => self.list(ix, entry, value, disabled, window, cx),
            SettingKind::Other => self.edit_link(ix, entry, cx),
        };
        let key = entry.key_path();
        let fills = entry.kind == SettingKind::TextList;
        div()
            .debug_selector(move || selector("settings-value", &key))
            // A list takes the line's width, scrolling its items in it.
            .when(fills, |line| line.flex_1().min_w_0())
            .when(!fills, |line| line.flex_none().max_w_full())
            .h(CONTROL_HEIGHT)
            .flex()
            .items_center()
            .child(control)
            .into_any_element()
    }

    /// Why the typed value of the setting at dotted `key` wasn't written,
    /// if it wasn't.
    pub(super) fn message(&self, key: &str) -> Option<String> {
        self.fields.get(key)?.message.clone()
    }

    /// Finds again why each input's refused text wasn't written, in the
    /// language now, as the model just read has its settings.
    pub(super) fn refresh_messages(&mut self) {
        let Some(model) = &self.model else {
            return;
        };
        for (key, field) in &mut self.fields {
            let entry = model
                .entries()
                .iter()
                .find(|entry| entry.key_path() == *key);
            if let (Some(rejected), Some(entry)) = (&field.rejected, entry) {
                field.message = rejection(entry, rejected);
            }
        }
    }

    /// What the input of the setting at dotted `key` holds, and why it
    /// wasn't written, if it wasn't; `None` until the input is drawn.
    #[cfg(test)]
    pub(crate) fn field(&self, key: &str, cx: &App) -> Option<(String, Option<String>)> {
        let field = self.fields.get(key)?;
        Some((field.input.read(cx).text().to_owned(), self.message(key)))
    }

    /// What the inputs of the list at dotted `key` hold.
    #[cfg(test)]
    pub(crate) fn list_texts(&self, key: &str, cx: &App) -> Vec<String> {
        (self.lists.get(key).into_iter())
            .flat_map(|list| &list.items)
            .map(|item| item.input.read(cx).text().to_owned())
            .collect()
    }

    /// A switch, on for a true `value`.
    fn switch(
        &self,
        entry: &SettingEntry,
        value: &Json,
        disabled: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let path = entry.key_path();
        let id = selector("setting-switch", &path);
        Switch::new(gpui::SharedString::from(id.clone()))
            .debug_selector(move || id)
            .on(value.as_bool().unwrap_or(false))
            .disabled(disabled)
            // From the value as it is when clicked, a change still being
            // written included, not as it was drawn.
            .on_change(cx.listener(move |this, _: &bool, _, cx| {
                this.toggle(&path, cx);
            }))
            .into_any_element()
    }

    /// Toggles the selected setting if it's a switch's; whether it was.
    pub(super) fn toggle_selected(&mut self, cx: &mut Context<Self>) -> bool {
        match self.selected_entry().map(SettingEntry::key_path) {
            Some(path) => self.toggle(&path, cx),
            None => false,
        }
    }

    /// Toggles the switch of the setting at dotted `path`, from the value
    /// shown, a change waiting to be written included; whether it's a
    /// switch the selected scope can set.
    fn toggle(&mut self, path: &str, cx: &mut Context<Self>) -> bool {
        let Some(entry) = self.entry(path).cloned() else {
            return false;
        };
        if entry.kind != SettingKind::Bool || self.restricted(&entry) {
            return false;
        }
        let on = self
            .value(&entry, cx)
            .is_some_and(|(value, _)| value.as_bool() == Some(true));
        self.write(&entry.key, Change::Set(Value::Bool(!on)), cx);
        true
    }

    /// Resets the setting at `key` to its default, dropping what's typed
    /// into its inputs, so that leaving them writes nothing after.
    pub(super) fn reset(&mut self, key: &[String], window: &mut Window, cx: &mut Context<Self>) {
        let editing = self.editing(window, cx);
        self.reset_keeping_focus(key, window, cx);
        // A list item that had focus, gone with the items the default
        // hasn't: focus goes to an item left, else the list of rows.
        if let Some(Editing::Item(path, item)) = editing
            && self.editing(window, cx).is_none()
        {
            self.focus_item_or_list(&path, item, window, cx);
        }
    }

    /// [`SettingsEditor::reset`], leaving focus where it is.
    fn reset_keeping_focus(&mut self, key: &[String], window: &mut Window, cx: &mut Context<Self>) {
        self.write(key, Change::Reset, cx);
        let path = key.join(".");
        let Some(entry) = self.entry(&path).cloned() else {
            return;
        };
        let Some((value, _)) = self.value(&entry, cx) else {
            return;
        };
        if let Some(field) = self.fields.get_mut(&path) {
            field.forget_shown();
            Self::sync_field(field, &input_text(&value), cx);
        }
        if self.lists.contains_key(&path) {
            let list = self.lists.get_mut(&path).expect("checked above");
            list.forget_shown();
            list.items.retain(|item| !item.added);
            self.sync_list(&path, list_items(&value), window, cx);
        }
    }

    /// A dropdown of `options`, showing `value`, with its list open under
    /// it while it's [`SettingsEditor::select`], of the options it opened
    /// with. A `None` option is Default (built-in). The list is no taller
    /// than the window, and scrolls.
    #[allow(clippy::too_many_arguments)]
    fn dropdown(
        &self,
        ix: usize,
        entry: &SettingEntry,
        options: &[Option<EnumVariant>],
        value: &Json,
        disabled: bool,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let ui = ui_theme(cx);
        let path = entry.key_path();
        let (current, hint) = select_label(entry, options, value);
        let hint = hint.map(|hint| {
            let path = path.clone();
            div()
                .debug_selector(move || selector("setting-hint", &path))
                .min_w_0()
                .truncate()
                .text_color(ui.muted_text)
                .child(hint)
        });
        let open = self.select.as_ref().filter(|select| select.key == path);
        let margin = px(8.);
        let max_height = (window.viewport_size().height - margin * 2.)
            .min(MENU_HEIGHT)
            .max(OPTION_HEIGHT);
        let button = div()
            .id(("setting-select", ix))
            .debug_selector({
                let path = path.clone();
                move || selector("setting-select", &path)
            })
            .h(SELECT_HEIGHT)
            .min_w(px(120.))
            // In a narrow column, it's as wide as the column, its label cut
            // short.
            .max_w_full()
            .px_2()
            .flex()
            .items_center()
            .justify_between()
            .gap_2()
            .rounded_md()
            .border_1()
            .border_color(ui.border)
            .bg(ui.panel_background)
            .text_color(ui.text)
            .child(
                div()
                    .min_w_0()
                    .flex()
                    .gap_2()
                    .child(div().min_w_0().truncate().child(current))
                    .children(hint),
            )
            .child(
                svg()
                    .flex_none()
                    .path(Icon::ChevronDown.path())
                    .size(px(12.))
                    .text_color(ui.muted_text),
            )
            .when(!disabled, |button| {
                let path = path.clone();
                button.cursor_pointer().on_click(
                    cx.listener(move |this, _, window, cx| this.toggle_dropdown(&path, window, cx)),
                )
            });
        let menu = open.map(|select| {
            let highlighted = select.highlighted;
            let options = select.options.iter().enumerate().map(|(option, variant)| {
                let selector = match variant {
                    Some(variant) => {
                        selector("setting-option", &format!("{path}-{}", variant.value))
                    }
                    None => selector("setting-option-default", &path),
                };
                let key = entry.key.clone();
                let picked = pick(variant);
                let (value, description) = match variant {
                    Some(variant) => (variant.value.clone(), variant.description.clone()),
                    None => (default_choice().to_string(), String::new()),
                };
                div()
                    .id(("setting-option", option))
                    .debug_selector(move || selector)
                    .min_h(OPTION_HEIGHT)
                    .px_2()
                    .py_1()
                    .flex()
                    .flex_col()
                    .cursor_pointer()
                    .when(option == highlighted, |row| row.bg(ui.row_selected))
                    .hover(move |style| style.bg(ui.row_hover))
                    .child(div().text_color(ui.text).child(value))
                    // Under it, on one line: one that's too long ends in
                    // an ellipsis.
                    .when(!description.is_empty(), |row| {
                        row.child(
                            div()
                                .w_full()
                                .overflow_hidden()
                                .whitespace_nowrap()
                                .text_ellipsis()
                                .text_size(px(11.))
                                .text_color(ui.muted_text)
                                .child(description),
                        )
                    })
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.select = None;
                        this.write(&key, picked.clone(), cx);
                        cx.stop_propagation();
                    }))
            });
            let other = select.other.then(|| {
                let option = select.options.len();
                let path = path.clone();
                div()
                    .id(("setting-option", option))
                    .debug_selector({
                        let path = path.clone();
                        move || selector("setting-option-other", &path)
                    })
                    .min_h(OPTION_HEIGHT)
                    .px_2()
                    .py_1()
                    .flex()
                    .items_center()
                    .cursor_pointer()
                    .when(option == highlighted, |row| row.bg(ui.row_selected))
                    .hover(move |style| style.bg(ui.row_hover))
                    .text_color(ui.text)
                    .child(tr!("workbench-settings-other-choice"))
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.select = None;
                        this.type_other(&path, window, cx);
                        cx.stop_propagation();
                    }))
            });
            div()
                .id(("setting-menu", ix))
                .debug_selector({
                    let path = path.clone();
                    move || selector("setting-menu", &path)
                })
                .occlude()
                .w(px(360.))
                .max_h(max_height)
                .overflow_y_scroll()
                .track_scroll(&select.scroll)
                .py_1()
                .rounded_md()
                .border_1()
                .border_color(ui.border)
                .bg(ui.panel_background)
                .shadow_md()
                .text_size(px(12.))
                .children(options)
                .children(other)
                .on_mouse_down_out(cx.listener(|this, _, _, cx| {
                    this.select = None;
                    cx.notify();
                }))
        });
        div()
            .relative()
            .max_w_full()
            .child(button)
            .when_some(menu, |dropdown, menu| {
                dropdown.child(
                    div().absolute().top(SELECT_HEIGHT + px(2.)).left_0().child(
                        deferred(anchored().snap_to_window_with_margin(margin).child(menu))
                            .with_priority(1),
                    ),
                )
            })
            .into_any_element()
    }

    /// The open dropdown's setting, the options it shows and the one
    /// highlighted: Other…, if it has it, at their length.
    #[cfg(test)]
    pub(super) fn open_select(&self) -> Option<(&str, &[Option<EnumVariant>], usize)> {
        let select = self.select.as_ref()?;
        Some((&select.key, &select.options, select.highlighted))
    }

    /// Swaps the dropdown of the setting at dotted `path` for an input,
    /// showing its value, focused: Other….
    fn type_other(&mut self, path: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Some(entry) = self.entry(path).cloned() else {
            return;
        };
        let Some((value, _)) = self.value(&entry, cx) else {
            return;
        };
        self.typing.insert(path.to_owned());
        let field = self.field_of(&entry, &input_text(&value), window, cx);
        field.forget_shown();
        Self::sync_field(field, &input_text(&value), cx);
        let focus = field.input.focus_handle(cx);
        window.focus(&focus, cx);
        cx.notify();
    }

    /// Opens the dropdown of the setting at dotted `path`, its value
    /// highlighted, or closes it if it's open.
    fn toggle_dropdown(&mut self, path: &str, window: &mut Window, cx: &mut Context<Self>) {
        if self
            .select
            .as_ref()
            .is_some_and(|select| select.key == path)
        {
            self.select = None;
        } else if let Some(entry) = self.entry(path).cloned() {
            let Some(options) = options(&entry, cx) else {
                return;
            };
            let other = match entry.kind {
                SettingKind::Choice(source) => choices::other(source),
                _ => false,
            };
            let value = self.value(&entry, cx).map(|(value, _)| value);
            let listed = (value.as_ref()).and_then(|value| option_of(&entry, &options, value));
            // A value the list hasn't was typed in: Other….
            let typed = other && value.as_ref().is_some_and(Json::is_string);
            let highlighted = match listed {
                Some(listed) => listed,
                None if typed => options.len(),
                None => 0,
            };
            self.select = Some(Select {
                key: path.to_owned(),
                options,
                other,
                highlighted,
                scroll: ScrollHandle::new(),
            });
            // The list has nothing to scroll in until it's drawn once.
            cx.on_next_frame(window, |this, _, cx| {
                if let Some(select) = &this.select {
                    select.scroll.scroll_to_item(select.highlighted);
                    cx.notify();
                }
            });
        }
        cx.notify();
    }

    /// Moves the open dropdown's highlight down, or with `down` false up,
    /// stopping at its ends.
    pub(super) fn move_highlight(&mut self, down: bool, cx: &mut Context<Self>) {
        let Some(select) = self.select.as_mut() else {
            return;
        };
        let count = select.options.len() + usize::from(select.other);
        select.highlighted = match down {
            true => (select.highlighted + 1).min(count.saturating_sub(1)),
            false => select.highlighted.saturating_sub(1),
        };
        select.scroll.scroll_to_item(select.highlighted);
        cx.notify();
    }

    /// Enter on the selected row: picks the open dropdown's highlighted
    /// variant, opens a closed one, goes into an input (a list's first
    /// item, a new one, or a dropdown's after Other…), opens
    /// `settings.toml` for a table, or toggles a switch.
    pub(super) fn activate(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(entry) = self.selected_entry().cloned() else {
            return;
        };
        if self.restricted(&entry) && entry.kind != SettingKind::Other {
            return;
        }
        let path = entry.key_path();
        match &entry.kind {
            SettingKind::Bool => {
                self.toggle_selected(cx);
            }
            SettingKind::Choice(_) if self.typing.contains(&path) => {
                if let Some(field) = self.fields.get(&path) {
                    window.focus(&field.input.focus_handle(cx), cx);
                }
            }
            SettingKind::Enum(_) | SettingKind::Choice(_) => match self.select.take() {
                Some(select) if select.key == path => {
                    match select.options.get(select.highlighted) {
                        Some(option) => self.write(&entry.key, pick(option), cx),
                        None if select.other => self.type_other(&path, window, cx),
                        None => {}
                    }
                    cx.notify();
                }
                _ => self.toggle_dropdown(&path, window, cx),
            },
            SettingKind::Int { .. } | SettingKind::Float { .. } | SettingKind::Text { .. } => {
                if let Some(field) = self.fields.get(&path) {
                    window.focus(&field.input.focus_handle(cx), cx);
                }
            }
            SettingKind::TextList => {
                let empty = (self.lists.get(&path)).is_none_or(|list| list.items.is_empty());
                match empty {
                    // Scrolled back to it, wherever the keys left the list.
                    false => self.focus_item_or_list(&path, 0, window, cx),
                    true => self.add_item(&path, window, cx),
                }
            }
            SettingKind::Other => self.edit_in_file(&entry.key, window, cx),
        }
    }

    /// A new input showing `text`, which `commit` commits when it loses
    /// focus.
    fn new_field(
        &self,
        text: String,
        placeholder: Option<kiln_l10n::Text>,
        commit: impl Fn(&mut Self, &mut Context<Self>) + 'static,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Field {
        let ui = ui_theme(cx);
        let input = cx.new(|cx| {
            let placeholder = placeholder.unwrap_or_else(|| kiln_l10n::Text::from(""));
            let mut input = TextInput::new(placeholder, &ui, cx)
                .bordered()
                .height(CONTROL_HEIGHT);
            input.set_text(text.clone(), cx);
            input
        });
        let blur = cx.on_blur(&input.focus_handle(cx), window, move |this, _, cx| {
            commit(this, cx)
        });
        let edited = Rc::new(Cell::new(false));
        let edit = cx.subscribe(&input, {
            let edited = edited.clone();
            move |_, _, event: &TextInputEvent, _| {
                if *event == TextInputEvent::Changed {
                    edited.set(true);
                }
            }
        });
        Field {
            input,
            shown: Some(text),
            rejected: None,
            message: None,
            added: false,
            edited,
            _blur: blur,
            _edit: edit,
        }
    }

    /// Gives `field` the file's value, `text`, unless something is typed
    /// that its blur hasn't committed; `forget_shown` forces it.
    fn sync_field(field: &mut Field, text: &str, cx: &mut App) {
        if field.shown.as_deref() == Some(text) {
            return;
        }
        if field.shown.is_some() && field.typed(cx) {
            return;
        }
        (field.input).update(cx, |input, cx| input.set_text(text.to_owned(), cx));
        field.shown = Some(text.to_owned());
        field.rejected = None;
        field.message = None;
        field.edited.set(false);
    }

    /// The input of `entry`, made showing `text` if it isn't yet. Leaving
    /// it commits it, and has a dropdown's row after Other… show the
    /// dropdown again, unless what it holds isn't a value the setting
    /// takes.
    fn field_of(
        &mut self,
        entry: &SettingEntry,
        text: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> &mut Field {
        let path = entry.key_path();
        if !self.fields.contains_key(&path) {
            let placeholder =
                (entry.default.is_null()).then(|| text!("workbench-settings-not-set"));
            let commit = {
                let path = path.clone();
                move |this: &mut Self, cx: &mut Context<Self>| {
                    this.commit(&Editing::Field(path.clone()), false, cx);
                    let refused =
                        (this.fields.get(&path)).is_some_and(|field| field.message.is_some());
                    if !refused && this.typing.remove(&path) {
                        cx.notify();
                    }
                }
            };
            let field = self.new_field(text.to_owned(), placeholder, commit, window, cx);
            self.fields.insert(path.clone(), field);
        }
        self.fields.get_mut(&path).expect("the field was just made")
    }

    /// The input of a number or text setting, or of a dropdown's after
    /// Other…: its typed text, else its value.
    fn input(
        &mut self,
        ix: usize,
        entry: &SettingEntry,
        value: &Json,
        disabled: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let path = entry.key_path();
        let text = input_text(value);
        let field = self.field_of(entry, &text, window, cx);
        Self::sync_field(field, &text, cx);
        let input = field.input.clone();
        input.update(cx, |input, cx| input.set_typing(!disabled, cx));
        let width = match entry.kind {
            SettingKind::Text { .. } | SettingKind::Choice(_) => px(260.),
            _ => px(96.),
        };
        div()
            .debug_selector(move || selector("setting-input", &path))
            .flex_none()
            .w(width)
            // In a narrow column, as wide as the column.
            .max_w_full()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, _, _, cx| {
                    // The input takes focus, not the list.
                    this.selected = Some(ix);
                    cx.stop_propagation();
                    cx.notify();
                }),
            )
            .child(input)
            .into_any_element()
    }

    /// A list's editor: an input per item, each with a remove button, and
    /// Add Item.
    fn list(
        &mut self,
        ix: usize,
        entry: &SettingEntry,
        value: &Json,
        disabled: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let ui = ui_theme(cx);
        let path = entry.key_path();
        self.sync_list(&path, list_items(value), window, cx);
        let list = &self.lists[&path];
        let scroll = list.scroll.clone();
        let items: Vec<Entity<TextInput>> =
            (list.items.iter()).map(|item| item.input.clone()).collect();
        for input in &items {
            input.update(cx, |input, cx| input.set_typing(!disabled, cx));
        }
        let items = items.into_iter().enumerate().map(|(item, input)| {
            let path = path.clone();
            div()
                .debug_selector({
                    let path = path.clone();
                    move || selector("setting-item", &format!("{path}-{item}"))
                })
                .flex_none()
                .flex()
                .items_center()
                .gap_1()
                .child(
                    div()
                        .w(px(160.))
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(move |this, _, _, cx| {
                                this.selected = Some(ix);
                                cx.stop_propagation();
                                cx.notify();
                            }),
                        )
                        .child(input),
                )
                .child(
                    div()
                        .id(("setting-remove", item))
                        .debug_selector({
                            let path = path.clone();
                            move || selector("setting-remove", &format!("{path}-{item}"))
                        })
                        .flex_none()
                        .size(CONTROL_HEIGHT)
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded_sm()
                        .role(gpui::Role::Button)
                        .aria_label(tr!("workbench-settings-remove-item"))
                        .when(!disabled, |button| {
                            button
                                .cursor_pointer()
                                .hover(move |style| style.bg(ui.row_hover))
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.remove_item(&path, item, cx)
                                }))
                        })
                        .child(
                            svg()
                                .path(Icon::X.path())
                                .size(px(12.))
                                .text_color(ui.muted_text),
                        ),
                )
        });
        let items: Vec<_> = items.collect();
        let add = Button::new(("setting-add", ix), text!("workbench-settings-add-item"))
            .disabled(disabled)
            .debug_selector({
                let path = path.clone();
                move || selector("setting-add", &path)
            })
            .on_click(cx.listener({
                let path = path.clone();
                move |this, _, window, cx| {
                    this.selected = Some(ix);
                    this.add_item(&path, window, cx);
                    cx.stop_propagation();
                }
            }));
        // The items scroll sideways in what Add Item leaves of the line.
        div()
            .w_full()
            .flex()
            .items_center()
            .gap_2()
            .child(
                div()
                    .id(("setting-list", ix))
                    .track_scroll(&scroll)
                    .debug_selector(move || selector("setting-items", &path))
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .items_center()
                    .gap_2()
                    .overflow_x_scroll()
                    .children(items),
            )
            .child(add)
            .into_any_element()
    }

    /// Gives the list editor of the setting at dotted `path`, made if it
    /// isn't yet, the file's `items`, unless the items have something typed
    /// that no blur has committed; `forget_shown` forces it.
    fn sync_list(
        &mut self,
        path: &str,
        items: Vec<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let list = self.lists.entry(path.to_owned()).or_insert(ListField {
            shown: None,
            items: Vec::new(),
            scroll: ScrollHandle::new(),
        });
        if list.shown.as_ref() == Some(&items) {
            return;
        }
        let typed = list.edited() || list.shown.as_ref() != Some(&list.texts(cx));
        if list.shown.is_some() && typed {
            return;
        }
        // The inputs there are take the items in place, keeping focus.
        let have = list.items.len();
        list.items.truncate(items.len());
        for (item, text) in list.items.iter_mut().zip(&items) {
            item.shown = None;
            item.added = false;
            Self::sync_field(item, text, cx);
        }
        list.shown = Some(items.clone());
        for text in items.into_iter().skip(have) {
            let item = self.new_item(path, text, window, cx);
            self.lists
                .get_mut(path)
                .expect("made above")
                .items
                .push(item);
        }
    }

    /// A new item's input, for the list at dotted `path`.
    fn new_item(
        &self,
        path: &str,
        text: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Field {
        // Any item commits the whole list.
        let commit = {
            let path = path.to_owned();
            move |this: &mut Self, cx: &mut Context<Self>| {
                this.commit(&Editing::Item(path.clone(), 0), false, cx)
            }
        };
        self.new_field(text, None, commit, window, cx)
    }

    /// Adds an empty item to the list at dotted `path`, focused; it's
    /// written once something typed into it is committed.
    fn add_item(&mut self, path: &str, window: &mut Window, cx: &mut Context<Self>) {
        if !self.lists.contains_key(path) {
            return;
        }
        let mut item = self.new_item(path, String::new(), window, cx);
        item.added = true;
        window.focus(&item.input.focus_handle(cx), cx);
        let list = self.lists.get_mut(path).expect("checked above");
        list.items.push(item);
        // In view, past the items already there.
        list.scroll.scroll_to_item(list.items.len() - 1);
        cx.notify();
    }

    /// Removes item `item` from the list at dotted `path`, and writes the
    /// list.
    fn remove_item(&mut self, path: &str, item: usize, cx: &mut Context<Self>) {
        let Some(entry) = self.entry(path).cloned() else {
            return;
        };
        let Some(list) = self.lists.get_mut(path) else {
            return;
        };
        if item >= list.items.len() {
            return;
        }
        list.items.remove(item);
        let items = list.texts(cx);
        list.commit_added(cx);
        list.clear_edited();
        list.shown = Some(items.clone());
        self.write(&entry.key, Change::Set(Value::List(items)), cx);
        cx.stop_propagation();
    }

    /// Up or Down in item `item` of the list at dotted `path`: focuses the
    /// item after it, with `down`, else the one before, if there's one.
    pub(super) fn focus_item(
        &mut self,
        path: &str,
        item: usize,
        down: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let next = match down {
            true => item.checked_add(1),
            false => item.checked_sub(1),
        };
        let Some(list) = self.lists.get(path) else {
            return;
        };
        if let Some(next) = next.filter(|next| *next < list.items.len()) {
            window.focus(&list.items[next].input.focus_handle(cx), cx);
            list.scroll.scroll_to_item(next);
            cx.notify();
        }
    }

    /// `settings.add_list_item`: Add Item on the list whose item has focus,
    /// else on the selected row's list, unless its scope's file can't set
    /// it.
    pub(super) fn add_focused_item(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let path = match self.editing(window, cx) {
            Some(Editing::Item(path, _)) => path,
            _ => match self.selected_entry() {
                Some(entry) if entry.kind == SettingKind::TextList => entry.key_path(),
                _ => return,
            },
        };
        let entry = self.entry(&path).cloned();
        if entry.is_some_and(|entry| !self.restricted(&entry)) {
            self.add_item(&path, window, cx);
        }
    }

    /// `settings.remove_list_item`: removes the list item that has focus,
    /// as its remove button does, and focuses the item now in its place,
    /// else the one before it, else the list.
    pub(super) fn remove_focused_item(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(Editing::Item(path, item)) = self.editing(window, cx) else {
            return;
        };
        if (self.entry(&path).cloned()).is_none_or(|entry| self.restricted(&entry)) {
            return;
        }
        self.remove_item(&path, item, cx);
        self.focus_item_or_list(&path, item, window, cx);
    }

    /// Focuses item `item` of the list at dotted `path`, else its last,
    /// scrolled into view, else the list of rows: where focus goes when the
    /// item that had it is gone.
    pub(super) fn focus_item_or_list(
        &mut self,
        path: &str,
        item: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let list = self.lists.get(path);
        let count = list.map_or(0, |list| list.items.len());
        match (list, count.checked_sub(1)) {
            (Some(list), Some(last)) => {
                let item = item.min(last);
                window.focus(&list.items[item].input.focus_handle(cx), cx);
                list.scroll.scroll_to_item(item);
            }
            _ => window.focus(&self.focus, cx),
        }
        cx.notify();
    }

    /// Gives the open dropdown's options the text of the model just read,
    /// in the language now: each of the options it opened with that the
    /// setting still lists, by value, takes its description now.
    pub(super) fn refresh_select(&mut self, cx: &App) {
        let Some(select) = &self.select else {
            return;
        };
        let Some(now) = (self.entry(&select.key)).and_then(|entry| options(entry, cx)) else {
            return;
        };
        let select = self.select.as_mut().expect("checked above");
        for option in select.options.iter_mut().flatten() {
            let found = now.iter().flatten().find(|now| now.value == option.value);
            if let Some(found) = found {
                option.description = found.description.clone();
            }
        }
    }

    /// The row input that has focus, if one has.
    pub(super) fn editing(&self, window: &Window, cx: &App) -> Option<Editing> {
        let focused = |input: &Entity<TextInput>| input.focus_handle(cx).is_focused(window);
        let field = (self.fields.iter())
            .find(|(_, field)| focused(&field.input))
            .map(|(key, _)| Editing::Field(key.clone()));
        field.or_else(|| {
            self.lists.iter().find_map(|(key, list)| {
                let item = list.items.iter().position(|item| focused(&item.input))?;
                Some(Editing::Item(key.clone(), item))
            })
        })
    }

    /// `editing`'s input.
    pub(super) fn input_of(&self, editing: &Editing) -> Option<Entity<TextInput>> {
        let field = match editing {
            Editing::Field(key) => self.fields.get(key)?,
            Editing::Item(key, item) => self.lists.get(key)?.items.get(*item)?,
        };
        Some(field.input.clone())
    }

    /// Writes what `editing`'s input holds (for a list's item, the whole
    /// list), unless it's what the file holds and not `always`, or it isn't
    /// a value the setting takes: then why shows under the input.
    pub(super) fn commit(&mut self, editing: &Editing, always: bool, cx: &mut Context<Self>) {
        let Some(entry) = self.entry(editing.key()).cloned() else {
            return;
        };
        if self.restricted(&entry) {
            return;
        }
        if let Editing::Item(key, _) = editing {
            let Some(list) = self.lists.get_mut(key) else {
                return;
            };
            let items = list.texts(cx);
            if !always && !list.edited() && list.shown.as_ref() == Some(&items) {
                return;
            }
            list.commit_added(cx);
            list.clear_edited();
            list.shown = Some(items.clone());
            return self.write(&entry.key, Change::Set(Value::List(items)), cx);
        }
        let Some(field) = self.fields.get_mut(editing.key()) else {
            return;
        };
        let text = field.input.read(cx).text().to_owned();
        if !always && !field.typed(cx) {
            field.rejected = None;
            field.message = None;
            return cx.notify();
        }
        let change = match parse(&entry, &text) {
            Err(message) => Err(message),
            Ok(None) => Ok(Change::Reset),
            Ok(Some(value)) => match problem(&entry, &value) {
                Some(message) => Err(message),
                None => Ok(Change::Set(value)),
            },
        };
        match change {
            Err(message) => {
                field.rejected = Some(text);
                field.message = Some(message);
            }
            Ok(change) => {
                field.rejected = None;
                field.message = None;
                field.shown = Some(text);
                field.edited.set(false);
                self.write(&entry.key, change, cx);
            }
        }
        cx.notify();
    }

    /// Puts back the file's value in `editing`'s input, dropping what's
    /// typed.
    pub(super) fn revert(&mut self, editing: &Editing, cx: &mut Context<Self>) {
        match editing {
            Editing::Field(key) => {
                if let Some(field) = self.fields.get_mut(key) {
                    field.forget_shown();
                }
            }
            Editing::Item(key, _) => {
                if let Some(list) = self.lists.get_mut(key) {
                    list.forget_shown();
                }
            }
        }
        cx.notify();
    }

    /// Edit in settings.toml, for a table: a button, which opens the file
    /// even where the selected scope's file can't set the table.
    fn edit_link(&self, ix: usize, entry: &SettingEntry, cx: &mut Context<Self>) -> AnyElement {
        let (key, path) = (entry.key.clone(), entry.key_path());
        Button::new(
            ("setting-edit", ix),
            text!("workbench-settings-edit-in-file"),
        )
        .debug_selector(move || selector("setting-edit", &path))
        .on_click(cx.listener(move |this, _, window, cx| {
            this.edit_in_file(&key, window, cx);
            cx.stop_propagation();
        }))
        .into_any_element()
    }

    /// Opens the selected scope's settings file in the active pane, as
    /// `workbench.open_settings` does the user's, creating it from
    /// the template if it's missing, with the caret on `key`'s table if
    /// the file has it.
    pub(super) fn edit_in_file(
        &mut self,
        key: &[String],
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let (user, project) = self.files(cx);
        let path = match self.scope() {
            Scope::User => user,
            Scope::Workspace => project,
        };
        let Some(path) = path else {
            toast::show(
                cx,
                Toast::error(tr!("workbench-settings-cant-open"))
                    .detail(tr!("workbench-settings-no-config-dir")),
            );
            return;
        };
        let key = key.to_vec();
        // An open editor's text, unsaved edits included, is what `open_at`
        // will show, so the row comes from it rather than from disk.
        let open_text = self.workbench.upgrade().and_then(|workbench| {
            let editors = workbench.read(cx).file_editors(cx);
            // The exact path, as `open_at` finds a tab by.
            let editor = editors
                .iter()
                .find(|editor| editor.read(cx).model().path() == Some(path.as_path()))?;
            let model = editor.read(cx).model();
            Some(model.buffer().text().to_string())
        });
        let read = cx.background_spawn({
            let path = path.clone();
            async move {
                open::create_settings(&path)?;
                let text = match open_text {
                    Some(text) => text,
                    None => read_settings(&path).unwrap_or_default(),
                };
                Ok::<_, std::io::Error>(table_row(&text, &key))
            }
        });
        let workbench = self.workbench.clone();
        cx.spawn_in(window, async move |_, cx| {
            let row = match read.await {
                Ok(row) => row,
                Err(error) => {
                    log::error!("can't create {}: {error}", path.display());
                    cx.update(|_, cx| {
                        toast::show(
                            cx,
                            Toast::error(tr!(
                                "workbench-settings-cant-create",
                                path = path.display().to_string()
                            ))
                            .detail(error.to_string()),
                        )
                    })
                    .ok();
                    return;
                }
            };
            let reveal = row.map(|row| Reveal {
                row,
                columns: 0..0,
                within_row: true,
            });
            workbench
                .update_in(cx, |workbench, window, cx| {
                    let pane = workbench.pane().clone();
                    pane.update(cx, |pane, cx| pane.open_at(path, reveal, window, cx));
                })
                .ok();
        })
        .detach();
    }
}

#[cfg(test)]
#[path = "settings_editor_controls_blur_tests.rs"]
mod blur_tests;
