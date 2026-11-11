//! Search Keymaps (`keymaps.search`, LazyVim's `<leader>sk`): a [`Picker`]
//! over the bindings of the keymap in use ([`fe_gpui::current_keymap`], the
//! one the command palette and the Keyboard Shortcuts editor read), one row
//! per binding as `keys  Command Title  context`, then the active editor's
//! vim normal-mode maps (the preset's, the vimrc's and `[[vim.remap]]`'s,
//! with vim on) the same way, their leader spelled out as the keys it is
//! (`<Space>ff`), their context `vim normal`.
//!
//! The bindings are listed by title, then keys; a binding a later one with
//! the same keys and context replaces never runs, so isn't listed. A map
//! that runs a command (`<Cmd>fe:{id}<CR>`) is titled with the command's
//! title, any other with its right side as written. Rows match the query by
//! subsequence over their keys and title, ignoring case.
//!
//! Enter runs the row's command where focus was when the picker opened, as
//! the command palette does: focus goes back there and the command is
//! dispatched in the context stack it was in, with the binding's args. A
//! map without a command has its keys typed instead, so it runs as if
//! typed: in the editor it's listed from, focused, in normal mode.

use std::collections::HashMap;

use fe_command::{CommandId, ContextStack, Invocation, Source, Value};
use fe_gpui::EditorView;
use fe_keymap::Keymap;
use fe_ui::{HighlightedLabel, char_ranges};
use fe_vim::{Remap, RemapMode, RemapSet, VimKey};
use gpui::{
    AnyElement, App, AppContext as _, Context, Entity, FocusHandle, Focusable as _,
    IntoElement as _, Keystroke, Modifiers, ParentElement as _, SharedString, Styled as _,
    WeakEntity, Window, div,
};
use kiln::platform::{command_actions::dispatch_invocation, keys};
use kiln_l10n::{text, tr, verbatim};

use crate::{
    ListedCommands, Workbench,
    picker::{Picker, PickerDelegate},
    toolbar_picker::subsequence,
    workbench::Palette,
};

/// The context a vim normal-mode map's row shows, in the column where the
/// bindings' contexts show as their predicates print, untranslated.
pub(crate) const VIM_NORMAL: &str = verbatim!("vim normal");

/// What picking a row does.
#[derive(Clone, Debug, PartialEq)]
enum Action {
    /// Runs the command with the args.
    Command(CommandId, Value),
    /// Types the keys of the map, as long as it's still the one they run.
    Keys(Vec<VimKey>, Remap),
}

/// A binding or a map.
struct Row {
    /// As the platform prints them (`Ctrl+P`), or in vim notation.
    keys: String,
    title: String,
    /// Where it applies, as its predicate prints; empty for everywhere.
    context: String,
    /// What the query matches: the keys, then the title.
    matched: String,
    action: Action,
}

impl Row {
    fn new(keys: String, title: String, context: String, action: Action) -> Self {
        let matched = format!("{keys} {title}");
        Self {
            keys,
            title,
            context,
            matched,
            action,
        }
    }
}

/// Every command's title the palette knows, in the active locale, resolved
/// now (its `label()`): the typed commands', the app registry's and the
/// workbench's [`ListedCommands`]', the first of an ID.
fn titles(cx: &App) -> HashMap<String, String> {
    let mut titles = HashMap::new();
    let typed = kiln::platform::command_actions::typed_commands(cx)
        .into_iter()
        .map(|typed| (typed.id.as_str().to_owned(), typed.label().into()));
    let listed = cx
        .try_global::<ListedCommands>()
        .map(|listed| &listed.0[..])
        .unwrap_or_default();
    let metas = kiln::platform::commands::app_command_list(cx)
        .into_iter()
        .chain(listed.iter().cloned())
        .map(|meta| (meta.id.as_str().to_owned(), meta.label().into()));
    for (id, title) in typed.chain(metas) {
        titles.entry(id).or_insert(title);
    }
    titles
}

/// The keymap's bindings that can run, by title, then keys.
fn binding_rows(keymap: &Keymap, titles: &HashMap<String, String>) -> Vec<Row> {
    let bindings = keymap.bindings();
    let mut rows: Vec<Row> = (bindings.iter().enumerate())
        .filter(|(ix, binding)| {
            // A later binding with the same keys and context replaces it.
            !(bindings[ix + 1..].iter())
                .any(|later| later.keys == binding.keys && later.context == binding.context)
        })
        .map(|(_, binding)| {
            let id = binding.command.as_str();
            let title = titles.get(id).cloned().unwrap_or_else(|| id.to_owned());
            let context = (binding.context.as_ref()).map_or_else(String::new, ToString::to_string);
            let keys = fe_keymap::shortcut_label(&binding.keys);
            let action = Action::Command(binding.command.clone(), binding.args.clone());
            Row::new(keys, title, context, action)
        })
        .collect();
    rows.sort_by_cached_key(|row| (row.title.to_lowercase(), row.keys.clone()));
    rows
}

/// `set`'s normal-mode maps, in the order of their keys.
fn map_rows(set: &RemapSet, titles: &HashMap<String, String>) -> Vec<Row> {
    let mut rows = Vec::new();
    // Depth first from no keys: each map's keys go on from a shorter prefix.
    let mut prefixes = vec![Vec::new()];
    while let Some(prefix) = prefixes.pop() {
        for continuation in set.continuations(RemapMode::Normal, &prefix) {
            let mut keys = prefix.clone();
            keys.push(continuation.key);
            if continuation.longer > 0 {
                prefixes.push(keys.clone());
            }
            let Some(map) = continuation.map else {
                continue;
            };
            let shown: String = keys.iter().map(|&key| notation(key)).collect();
            let (title, action) = match continuation.command {
                Some(id) => {
                    let title = (titles.get(id.as_str()).cloned())
                        .unwrap_or_else(|| id.as_str().to_owned());
                    (title, Action::Command(id.clone(), Value::Null))
                }
                None => (map.to.clone(), Action::Keys(keys, map.clone())),
            };
            rows.push(Row::new(shown, title, VIM_NORMAL.to_owned(), action));
        }
    }
    rows.sort_by(|a, b| a.keys.cmp(&b.keys));
    rows
}

/// Whether `editor` has vim on, with `map` still the normal-mode map that
/// `keys` run, as when its row was listed.
fn maps(editor: &Entity<EditorView>, keys: &[VimKey], map: &Remap, cx: &App) -> bool {
    let Some(((last, prefix), state)) = keys.split_last().zip(editor.read(cx).vim_state()) else {
        return false;
    };
    (state
        .remaps()
        .continuations(RemapMode::Normal, prefix)
        .iter())
    .any(|continuation| continuation.key == *last && continuation.map == Some(map))
}

/// One key in vim notation, as a map's keys are written.
fn notation(key: VimKey) -> String {
    match key {
        VimKey::Char(' ') => "<Space>".to_owned(),
        VimKey::Char('<') => "<lt>".to_owned(),
        VimKey::Char('|') => "<Bar>".to_owned(),
        VimKey::Char(ch) => ch.to_string(),
        VimKey::Ctrl(ch) => format!("<C-{ch}>"),
        VimKey::Esc => "<Esc>".to_owned(),
        VimKey::Enter => "<CR>".to_owned(),
        VimKey::Backspace => "<BS>".to_owned(),
        VimKey::Tab => "<Tab>".to_owned(),
        VimKey::Left => "<Left>".to_owned(),
        VimKey::Right => "<Right>".to_owned(),
        VimKey::Up => "<Up>".to_owned(),
        VimKey::Down => "<Down>".to_owned(),
    }
}

/// The keystroke that types `key` into an editor in vim's normal mode.
fn keystroke(key: VimKey) -> Keystroke {
    let named = |key: &str| Keystroke {
        modifiers: Modifiers::default(),
        key: key.to_owned(),
        key_char: None,
    };
    match key {
        VimKey::Char(' ') => Keystroke {
            key_char: Some(" ".to_owned()),
            ..named("space")
        },
        VimKey::Char(ch) => Keystroke {
            modifiers: Modifiers {
                shift: ch.is_uppercase(),
                ..Modifiers::default()
            },
            key: ch.to_lowercase().to_string(),
            key_char: Some(ch.to_string()),
        },
        VimKey::Ctrl(ch) => Keystroke {
            modifiers: Modifiers::control(),
            ..named(&ch.to_string())
        },
        VimKey::Esc => named("escape"),
        VimKey::Enter => named("enter"),
        VimKey::Backspace => named("backspace"),
        VimKey::Tab => named("tab"),
        VimKey::Left => named("left"),
        VimKey::Right => named("right"),
        VimKey::Up => named("up"),
        VimKey::Down => named("down"),
    }
}

pub(crate) struct KeymapsPicker {
    rows: Vec<Row>,
    query: String,
    /// The rows matching the query, each with the chars of `matched` it
    /// matched.
    matches: Vec<(usize, Vec<usize>)>,
    /// Where focus was, which a pick gives it back to.
    previous_focus: Option<FocusHandle>,
    /// The context stack focus was in.
    stack: ContextStack,
    /// The active editor as it opened, whose maps are listed: a map's keys
    /// are typed there.
    editor: Option<WeakEntity<EditorView>>,
}

impl KeymapsPicker {
    fn new(
        rows: Vec<Row>,
        previous_focus: Option<FocusHandle>,
        stack: ContextStack,
        editor: Option<WeakEntity<EditorView>>,
    ) -> Self {
        Self {
            matches: (0..rows.len()).map(|ix| (ix, Vec::new())).collect(),
            rows,
            query: String::new(),
            previous_focus,
            stack,
            editor,
        }
    }

    fn refilter(&mut self, cx: &mut Context<Picker<Self>>) {
        let query: Vec<char> = self.query.chars().flat_map(char::to_lowercase).collect();
        self.matches = (self.rows.iter().enumerate())
            .filter_map(|(ix, row)| Some((ix, subsequence(&query, &row.matched)?)))
            .collect();
        let picker = cx.entity();
        // The picker is being updated now.
        cx.defer(move |cx| picker.update(cx, |picker, cx| picker.matches_updated(true, cx)));
    }
}

#[cfg(test)]
impl KeymapsPicker {
    /// The matches: each one's keys, title and context.
    pub(crate) fn labels(&self) -> Vec<(String, String, String)> {
        (self.matches.iter())
            .map(|(row, _)| {
                let row = &self.rows[*row];
                (row.keys.clone(), row.title.clone(), row.context.clone())
            })
            .collect()
    }
}

impl PickerDelegate for KeymapsPicker {
    fn update_matches(&mut self, query: String, cx: &mut Context<Picker<Self>>) {
        self.query = query;
        self.refilter(cx);
    }

    fn match_count(&self) -> usize {
        self.matches.len()
    }

    fn match_id(&self, ix: usize) -> SharedString {
        self.matches[ix].0.to_string().into()
    }

    fn rows_fill_width(&self) -> bool {
        true
    }

    fn render_match(&self, ix: usize, _: bool, cx: &App) -> AnyElement {
        let ui = fe_ui::ui_theme(cx);
        let (row, matched) = &self.matches[ix];
        let row = &self.rows[*row];
        // `matched` is the keys, a space, then the title.
        let keys_len = row.keys.chars().count();
        let in_keys: Vec<usize> = (matched.iter().copied())
            .filter(|&ix| ix < keys_len)
            .collect();
        let in_title: Vec<usize> = (matched.iter())
            .filter_map(|&ix| ix.checked_sub(keys_len + 1))
            .collect();
        let keys = HighlightedLabel::new(row.keys.clone(), char_ranges(&row.keys, &in_keys), &ui);
        let title =
            HighlightedLabel::new(row.title.clone(), char_ranges(&row.title, &in_title), &ui);
        div()
            .flex()
            .flex_1()
            .min_w_0()
            .items_center()
            .gap_3()
            .child(
                div()
                    .flex_none()
                    .min_w(gpui::px(140.))
                    .whitespace_nowrap()
                    .child(keys),
            )
            .child(
                div()
                    .min_w_0()
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .child(title),
            )
            .child(
                div()
                    .ml_auto()
                    .min_w_0()
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .text_color(ui.muted_text)
                    .child(row.context.clone()),
            )
            .into_any_element()
    }

    /// The entry's keys, then its title and context.
    fn match_label(&self, ix: usize, _: &App) -> SharedString {
        let row = &self.rows[self.matches[ix].0];
        let parts = [&row.keys, &row.title, &row.context];
        let parts: Vec<&str> = (parts.into_iter())
            .map(String::as_str)
            .filter(|part| !part.is_empty())
            .collect();
        parts.join(", ").into()
    }

    /// Gives focus back to where it was, then runs the row's command there,
    /// once the picker has gone. A map of keys types them in the editor its
    /// row is listed from, focused, back in normal mode first if it's in
    /// another (a typed `<Esc>`), wherever focus was: typed elsewhere, they
    /// could do anything. Its editor closed meanwhile, or vim turned off in
    /// it or the map changed since the picker opened, it does nothing: the
    /// keys would go in as text, or run something else.
    fn confirm(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Picker<Self>>) -> bool {
        let Some(&(row, _)) = self.matches.get(ix) else {
            return false;
        };
        let action = self.rows[row].action.clone();
        let stack = self.stack.clone();
        let previous = self.previous_focus.take();
        let action = match action {
            Action::Keys(mut keys, map) => {
                let editor = self.editor.as_ref().and_then(WeakEntity::upgrade);
                let Some(editor) = editor.filter(|editor| maps(editor, &keys, &map, cx)) else {
                    if let Some(previous) = previous {
                        window.focus(&previous, cx);
                    }
                    return true;
                };
                window.focus(&editor.focus_handle(cx), cx);
                if editor.read(cx).vim_mode() != Some("normal") {
                    keys.insert(0, VimKey::Esc);
                }
                Action::Keys(keys, map)
            }
            command => {
                if let Some(previous) = previous {
                    window.focus(&previous, cx);
                }
                command
            }
        };
        window.defer(cx, move |window, cx| match action {
            Action::Command(id, args) => {
                let invocation = Invocation::new(id)
                    .args(args)
                    .source(Source::Palette)
                    .context(stack);
                if let Err(error) = dispatch_invocation(invocation, window, cx) {
                    log::warn!("{error}");
                }
            }
            Action::Keys(keys, _) => {
                for key in keys {
                    window.dispatch_keystroke(keystroke(key), cx);
                }
            }
        });
        true
    }

    fn dismissed(&mut self, window: &mut Window, cx: &mut Context<Picker<Self>>) {
        if let Some(previous) = self.previous_focus.take() {
            window.focus(&previous, cx);
        }
    }

    fn status(&self) -> Option<SharedString> {
        if self.rows.is_empty() {
            Some(tr!("workbench-keymaps-none"))
        } else if self.matches.is_empty() {
            Some(tr!("workbench-keymaps-no-matches"))
        } else {
            None
        }
    }
}

impl Workbench {
    /// The keymaps picker, while it shows.
    #[cfg(test)]
    pub(crate) fn keymaps_picker(&self, cx: &App) -> Option<Entity<Picker<KeymapsPicker>>> {
        self.shown_palette(Palette::Keymaps, cx)
    }

    /// Shows the keymaps picker, focused, over the keymap's bindings and the
    /// active editor's vim maps, in place of any other palette, or afresh
    /// if it shows. A pick runs where focus is now.
    pub(crate) fn show_keymaps(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.dismiss_palette(window, cx);
        let previous = window.focused(cx);
        let stack = keys::context_stack(&window.context_stack(), cx);
        let titles = titles(cx);
        let mut rows = binding_rows(&fe_gpui::current_keymap(cx), &titles);
        let editor = self.active_editor(cx).cloned();
        let vim = editor.as_ref().and_then(|editor| {
            let state = editor.read(cx).vim_state()?;
            Some(map_rows(state.remaps(), &titles))
        });
        rows.extend(vim.unwrap_or_default());
        let editor = editor.as_ref().map(Entity::downgrade);
        let delegate = KeymapsPicker::new(rows, previous, stack, editor);
        let picker = cx.new(|cx| {
            Picker::new(
                delegate,
                text!("workbench-keymaps-placeholder"),
                fe_ui::ui_theme(cx),
                cx,
            )
        });
        picker.update(cx, |picker, cx| picker.show(window, cx));
        self.show_palette(Palette::Keymaps, picker, window, cx);
    }
}

#[cfg(test)]
#[path = "keymaps_picker_tests.rs"]
mod keymaps_picker_tests;
