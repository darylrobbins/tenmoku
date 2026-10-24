//! The Settings tab's controls, a function per [`SettingKind`], each
//! drawing the control for a row's value and committing a change through
//! [`SettingsEditor::write`]:
//!
//! - [`SettingKind::Bool`]: a switch (`setting-switch-<key>`), toggled by a
//!   click, or Space on its selected row.
//! - [`SettingKind::Enum`]: a dropdown (`setting-select-<key>`) of the
//!   schema's variants, anchored under its button, which a click or Enter
//!   on the selected row opens. Up and Down move through it, Enter or a
//!   click picks (`setting-option-<key>-<variant>`), and Escape or a click
//!   elsewhere closes it.
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
    IntoElement, MouseButton, ParentElement as _, StatefulInteractiveElement as _, Styled as _,
    Subscription, Window, anchored, deferred, div, prelude::FluentBuilder as _, px, svg,
};

use super::{CONTROL_HEIGHT, LINE_HEIGHT, Scope, SettingsEditor, read_settings};
use crate::{
    open,
    pane::Reveal,
    workbench::toast::{self, Toast},
};

/// The height of a dropdown's button, and of each of its variants.
const SELECT_HEIGHT: gpui::Pixels = px(24.);

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
            format!("{} can't be written to: {error}", path.display()),
        )),
    }
}

/// A number or text input, or a list item's, kept across renders.
pub(super) struct Field {
    input: Entity<TextInput>,
    /// The file's value, as text, when the input last took it; `None` to
    /// take it again at the next render. Text that differs is typed.
    shown: Option<String>,
    /// Why the typed text wasn't written.
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
            .ok_or_else(|| "must be a whole number".to_owned()),
        SettingKind::Float { .. } => (trimmed.parse::<f64>().ok())
            .filter(|value| value.is_finite())
            .map(|value| Some(Value::Float(value)))
            .ok_or_else(|| "must be a number".to_owned()),
        _ => Ok(Some(Value::Str(text.to_owned()))),
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
        let (control, height) = match &entry.kind {
            SettingKind::Bool => (self.switch(entry, value, disabled, cx), LINE_HEIGHT),
            SettingKind::Enum(variants) => (
                self.dropdown(ix, entry, variants, value, disabled, cx),
                SELECT_HEIGHT,
            ),
            SettingKind::Int { .. } | SettingKind::Float { .. } | SettingKind::Text { .. } => (
                self.input(ix, entry, value, disabled, window, cx),
                CONTROL_HEIGHT,
            ),
            SettingKind::TextList => (
                self.list(ix, entry, value, disabled, window, cx),
                CONTROL_HEIGHT,
            ),
            SettingKind::Other => (self.edit_link(ix, entry, cx), LINE_HEIGHT),
        };
        let key = entry.key_path();
        let fills = entry.kind == SettingKind::TextList;
        div()
            .debug_selector(move || format!("settings-value-{key}"))
            // A list takes the line's width, scrolling its items in it.
            .when(fills, |line| line.flex_1().min_w_0())
            .when(!fills, |line| line.flex_none())
            .h(height)
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

    /// What the input of the setting at dotted `key` holds, and why it
    /// wasn't written, if it wasn't; `None` until the input is drawn.
    #[cfg(test)]
    pub(crate) fn field(&self, key: &str, cx: &App) -> Option<(String, Option<String>)> {
        let field = self.fields.get(key)?;
        Some((
            field.input.read(cx).text().to_owned(),
            field.message.clone(),
        ))
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
        let id = format!("setting-switch-{path}");
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

    /// A dropdown of `variants`, showing `value`, with its list open under
    /// it while it's [`SettingsEditor::select`].
    fn dropdown(
        &self,
        ix: usize,
        entry: &SettingEntry,
        variants: &[EnumVariant],
        value: &Json,
        disabled: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let ui = ui_theme(cx);
        let path = entry.key_path();
        let current = value.as_str().unwrap_or_default().to_owned();
        let open = (self.select.as_ref())
            .filter(|(key, _)| *key == path)
            .map(|(_, highlighted)| *highlighted);
        let button = div()
            .id(("setting-select", ix))
            .debug_selector({
                let path = path.clone();
                move || format!("setting-select-{path}")
            })
            .h(SELECT_HEIGHT)
            .min_w(px(140.))
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
            .child(current)
            .child(
                svg()
                    .path(Icon::ChevronDown.path())
                    .size(px(12.))
                    .text_color(ui.muted_text),
            )
            .when(!disabled, |button| {
                let path = path.clone();
                button
                    .cursor_pointer()
                    .on_click(cx.listener(move |this, _, _, cx| this.toggle_dropdown(&path, cx)))
            });
        let menu = open.map(|highlighted| {
            let options = variants.iter().enumerate().map(|(option, variant)| {
                let selector = format!("setting-option-{path}-{}", variant.value);
                let key = entry.key.clone();
                let picked = Value::Str(variant.value.clone());
                div()
                    .id(("setting-option", option))
                    .debug_selector(move || selector)
                    .min_h(SELECT_HEIGHT)
                    .px_2()
                    .py_1()
                    .flex()
                    .flex_col()
                    .cursor_pointer()
                    .when(option == highlighted, |row| row.bg(ui.row_selected))
                    .hover(move |style| style.bg(ui.row_hover))
                    .child(div().text_color(ui.text).child(variant.value.clone()))
                    // Under it, on one line: one that's too long ends in
                    // an ellipsis.
                    .when(!variant.description.is_empty(), |row| {
                        row.child(
                            div()
                                .w_full()
                                .overflow_hidden()
                                .whitespace_nowrap()
                                .text_ellipsis()
                                .text_size(px(11.))
                                .text_color(ui.muted_text)
                                .child(variant.description.clone()),
                        )
                    })
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.select = None;
                        this.write(&key, Change::Set(picked.clone()), cx);
                        cx.stop_propagation();
                    }))
            });
            div()
                .occlude()
                .w(px(360.))
                .py_1()
                .rounded_md()
                .border_1()
                .border_color(ui.border)
                .bg(ui.panel_background)
                .shadow_md()
                .text_size(px(12.))
                .children(options)
                .on_mouse_down_out(cx.listener(|this, _, _, cx| {
                    this.select = None;
                    cx.notify();
                }))
        });
        div()
            .relative()
            .child(button)
            .when_some(menu, |dropdown, menu| {
                dropdown.child(
                    div().absolute().top(SELECT_HEIGHT + px(2.)).left_0().child(
                        deferred(anchored().snap_to_window_with_margin(px(8.)).child(menu))
                            .with_priority(1),
                    ),
                )
            })
            .into_any_element()
    }

    /// Opens the dropdown of the setting at dotted `path`, its value
    /// highlighted, or closes it if it's open.
    fn toggle_dropdown(&mut self, path: &str, cx: &mut Context<Self>) {
        if self.select.as_ref().is_some_and(|(key, _)| key == path) {
            self.select = None;
        } else if let Some(entry) = self.entry(path).cloned() {
            let SettingKind::Enum(variants) = &entry.kind else {
                return;
            };
            let value = self.value(&entry, cx).map(|(value, _)| value);
            let current = value.as_ref().and_then(Json::as_str);
            let highlighted = (variants.iter())
                .position(|variant| Some(variant.value.as_str()) == current)
                .unwrap_or(0);
            self.select = Some((path.to_owned(), highlighted));
        }
        cx.notify();
    }

    /// Moves the open dropdown's highlight down, or with `down` false up,
    /// stopping at its ends.
    pub(super) fn move_highlight(&mut self, down: bool, cx: &mut Context<Self>) {
        let Some((key, highlighted)) = self.select.clone() else {
            return;
        };
        let count = match self.entry(&key).map(|entry| &entry.kind) {
            Some(SettingKind::Enum(variants)) => variants.len(),
            _ => return,
        };
        let next = match down {
            true => (highlighted + 1).min(count.saturating_sub(1)),
            false => highlighted.saturating_sub(1),
        };
        self.select = Some((key, next));
        cx.notify();
    }

    /// Enter on the selected row: picks the open dropdown's highlighted
    /// variant, opens a closed one, goes into an input (a list's first
    /// item, or a new one), opens `settings.toml` for a table, or toggles
    /// a switch.
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
            SettingKind::Enum(variants) => match self.select.take() {
                Some((key, highlighted)) if key == path => {
                    if let Some(variant) = variants.get(highlighted) {
                        let picked = Value::Str(variant.value.clone());
                        self.write(&entry.key, Change::Set(picked), cx);
                    }
                    cx.notify();
                }
                _ => self.toggle_dropdown(&path, cx),
            },
            SettingKind::Int { .. } | SettingKind::Float { .. } | SettingKind::Text { .. } => {
                if let Some(field) = self.fields.get(&path) {
                    window.focus(&field.input.focus_handle(cx), cx);
                }
            }
            SettingKind::TextList => {
                let first = (self.lists.get(&path)).and_then(|list| list.items.first());
                match first {
                    Some(item) => window.focus(&item.input.focus_handle(cx), cx),
                    None => self.add_item(&path, window, cx),
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
        placeholder: &str,
        commit: impl Fn(&mut Self, &mut Context<Self>) + 'static,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Field {
        let ui = ui_theme(cx);
        let placeholder = placeholder.to_owned();
        let input = cx.new(|cx| {
            let mut input = TextInput::new(placeholder, &ui, cx).bordered();
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
        field.message = None;
        field.edited.set(false);
    }

    /// The input of a number or text setting: its typed text, else its
    /// value.
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
        if !self.fields.contains_key(&path) {
            let placeholder = if entry.default.is_null() {
                "Not set"
            } else {
                ""
            };
            let commit = {
                let path = path.clone();
                move |this: &mut Self, cx: &mut Context<Self>| {
                    this.commit(&Editing::Field(path.clone()), false, cx)
                }
            };
            let field = self.new_field(text.clone(), placeholder, commit, window, cx);
            self.fields.insert(path.clone(), field);
        }
        let field = self.fields.get_mut(&path).expect("the field was just made");
        Self::sync_field(field, &text, cx);
        let input = field.input.clone();
        input.update(cx, |input, cx| input.set_typing(!disabled, cx));
        let width = match entry.kind {
            SettingKind::Text { .. } => px(320.),
            _ => px(120.),
        };
        div()
            .debug_selector(move || format!("setting-input-{path}"))
            .flex_none()
            .w(width)
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
                    move || format!("setting-item-{path}-{item}")
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
                            move || format!("setting-remove-{path}-{item}")
                        })
                        .flex_none()
                        .size(px(20.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded_sm()
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
        let add = Button::new(("setting-add", ix), "Add Item")
            .small()
            .disabled(disabled)
            .debug_selector({
                let path = path.clone();
                move || format!("setting-add-{path}")
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
                    .debug_selector(move || format!("setting-items-{path}"))
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
        self.new_field(text, "", commit, window, cx)
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
        self.lists
            .get_mut(path)
            .expect("checked above")
            .items
            .push(item);
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
            Err(message) => field.message = Some(message),
            Ok(change) => {
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

    /// Edit in settings.toml, for a table.
    fn edit_link(&self, ix: usize, entry: &SettingEntry, cx: &mut Context<Self>) -> AnyElement {
        let ui = ui_theme(cx);
        let (key, path) = (entry.key.clone(), entry.key_path());
        div()
            .id(("setting-edit", ix))
            .debug_selector(move || format!("setting-edit-{path}"))
            .text_color(ui.accent)
            .cursor_pointer()
            .hover(|style| style.underline())
            .child("Edit in settings.toml")
            .on_click(cx.listener(move |this, _, window, cx| this.edit_in_file(&key, window, cx)))
            .into_any_element()
    }

    /// Opens the selected scope's settings file in the active pane, as
    /// `workbench.open_settings` does the user's, creating the user's from
    /// the template if it's missing, with the caret on `key`'s table if
    /// the file has it.
    pub(super) fn edit_in_file(
        &mut self,
        key: &[String],
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let (user, project) = self.files(cx);
        let (path, create) = match self.scope() {
            Scope::User => (user, true),
            Scope::Workspace => (project, false),
        };
        let Some(path) = path else {
            toast::show(
                cx,
                Toast::error("Can't open settings").detail("There's no config directory"),
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
                if create {
                    open::create_settings(&path)?;
                }
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
                            Toast::error(format!("Can't create {}", path.display()))
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
