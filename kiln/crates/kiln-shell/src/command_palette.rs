// SPDX-License-Identifier: Apache-2.0

//! The command palette: every command that applies where focus was when
//! it opened, by title, with its key, run on a pick as if from there.
//!
//! [`Toggle`] (`command_palette.toggle`) shows it in the [`Shell`]'s modal
//! layer as [`COMMAND_PALETTE`], or dismisses it. It lists the commands
//! of the app's registry (`app.quit` and the rest), every typed command,
//! and those an app adds from its own registries with [`add_commands`],
//! leaving out those that take args and those whose context predicate
//! doesn't hold in the context stack focus was in. Each row shows the key
//! that runs it there, as this platform prints it. A pick gives focus back
//! to where it was and dispatches the command there, from
//! [`Source::Palette`] in that context stack.
//!
//! [`Shell`]: crate::Shell

use std::{cell::RefCell, collections::HashMap, ops::Range};

use gpui::{
    AnyElement, App, AppContext as _, Context, FocusHandle, Global, IntoElement as _,
    ParentElement as _, SharedString, Styled as _, Window, div,
};
use kiln_command::{CommandId, CommandMeta, ContextStack, Invocation, Source, Value};
use kiln_core::{CatalogError, CommandCatalog, shortcut_label, shortcuts};
use kiln_l10n::{text, tr};
use kiln_platform::{
    command_actions::{Command as _, TypedCommand, dispatch_invocation, typed_commands},
    keys,
};
use kiln_ui::{HighlightedLabel, UiTheme, char_ranges, ui_theme};

use crate::{ModalId, Picker, PickerDelegate};

kiln_platform::commands!(
    command_palette,
    [
        /// Show All Commands
        Toggle,
    ]
);

/// The palette's id in the modal layer.
pub const COMMAND_PALETTE: ModalId = ModalId("command_palette");

/// The palette's commands, to check keymap bindings against without an
/// app: `command_palette.toggle`.
pub fn catalog() -> Box<dyn CommandCatalog> {
    Box::new(Catalog)
}

struct Catalog;

impl CommandCatalog for Catalog {
    fn check(&self, command: &CommandId, args: &Value) -> Result<(), CatalogError> {
        if *command != Toggle::id() {
            return Err(CatalogError::UnknownCommand);
        }
        <Toggle as gpui::Action>::build(args.clone())
            .map(drop)
            .map_err(|error| CatalogError::BadArgs(error.to_string()))
    }
}

/// The commands apps added, beyond the app registry's and the typed ones.
#[derive(Default)]
struct Added(Vec<CommandMeta>);

impl Global for Added {}

/// Lists `commands` in the palette as well: those of an app's own
/// registries (`Registry::commands`), which the palette can't see. A
/// command listed twice shows once.
pub fn add_commands(cx: &mut App, commands: impl IntoIterator<Item = CommandMeta>) {
    cx.default_global::<Added>().0.extend(commands);
}

/// A command the palette lists.
#[derive(Clone, Debug, PartialEq)]
pub struct Entry {
    pub id: CommandId,
    pub title: SharedString,
}

/// Every command the palette would list in `stack`: those without args
/// whose context holds there, by title, each once. Each title is its
/// `label()` in the active locale ([`CommandMeta::label`],
/// [`TypedCommand::label`]), so the palette sorts and matches on the text
/// it shows.
pub fn commands_in(stack: &ContextStack, cx: &App) -> Vec<Entry> {
    let app = kiln_platform::commands::app_command_list(cx);
    let typed = typed_commands(cx).into_iter().filter_map(|typed| {
        if typed.unlisted {
            return None;
        }
        typed.build(&Value::Null).ok()?;
        Some(Listed::Typed(typed))
    });
    let added = cx.try_global::<Added>().map(|added| &added.0[..]);
    let listed = (app.iter().chain(added.unwrap_or_default()))
        .filter(|meta| meta.args_schema.is_none())
        .map(Listed::Meta)
        .chain(typed);
    let mut entries: Vec<Entry> = Vec::new();
    for command in listed {
        let (id, context) = match &command {
            Listed::Meta(meta) => (&meta.id, meta.context.as_ref()),
            Listed::Typed(typed) => (&typed.id, typed.context.as_ref()),
        };
        let applies = context.is_none_or(|context| context.eval(stack));
        if applies && *id != Toggle::id() && !entries.iter().any(|entry| entry.id == *id) {
            let id = id.clone();
            let title = match &command {
                Listed::Meta(meta) => meta.label(),
                Listed::Typed(typed) => typed.label(),
            };
            entries.push(Entry { id, title });
        }
    }
    entries.sort_by(|a, b| a.title.cmp(&b.title).then(a.id.as_str().cmp(b.id.as_str())));
    entries
}

/// A command `commands_in` may list: a registry's, or a typed one.
enum Listed<'a> {
    Meta(&'a CommandMeta),
    Typed(TypedCommand),
}

/// The command palette's delegate.
pub struct CommandPalette {
    entries: Vec<Entry>,
    /// The entries matching the query, with the byte ranges of their
    /// titles that matched.
    matches: Vec<(usize, Vec<Range<usize>>)>,
    /// Where focus was, which a pick gives it back to.
    previous: Option<FocusHandle>,
    /// The context stack it was in.
    stack: ContextStack,
    /// Each entry's key, looked up as its row first draws.
    keys: RefCell<HashMap<usize, Option<SharedString>>>,
}

impl CommandPalette {
    /// A palette of the commands that apply where `window`'s focus is now,
    /// which a pick gives focus back to. Its rows draw in the active
    /// theme's colours ([`ui_theme`]); `_ui` is unused, left for callers
    /// until a later API cleanup.
    pub fn new(_ui: UiTheme, window: &Window, cx: &App) -> Self {
        let stack = keys::context_stack(&window.context_stack(), cx);
        let entries = commands_in(&stack, cx);
        Self {
            matches: (0..entries.len()).map(|ix| (ix, Vec::new())).collect(),
            entries,
            previous: window.focused(cx),
            stack,
            keys: RefCell::default(),
        }
    }

    /// The context stack it lists commands for.
    pub fn context_stack(&self) -> &ContextStack {
        &self.stack
    }

    /// The commands matching the query, in order.
    pub fn matches(&self) -> Vec<&Entry> {
        (self.matches.iter())
            .map(|(ix, _)| &self.entries[*ix])
            .collect()
    }

    /// The key that runs match `ix` where the palette opened, if any, as
    /// this platform prints it.
    pub fn key_label(&self, ix: usize, cx: &App) -> Option<SharedString> {
        let entry = self.matches.get(ix)?.0;
        let mut labels = self.keys.borrow_mut();
        labels
            .entry(entry)
            .or_insert_with(|| {
                let keymap = keys::current_keymap(cx);
                let id = &self.entries[entry].id;
                if keymap.bindings_for(id).is_empty() {
                    return None;
                }
                let found = shortcuts(&keymap, id, &Value::Null, &self.stack);
                found.first().map(|keys| shortcut_label(keys).into())
            })
            .clone()
    }
}

/// Where `query`'s chars appear in order in `title`, ignoring case: their
/// char indices, or `None` if they don't.
fn subsequence(title: &str, query: &str) -> Option<Vec<usize>> {
    let mut wanted = query.chars().filter(|c| !c.is_whitespace()).peekable();
    let mut found = Vec::new();
    for (ix, ch) in title.chars().enumerate() {
        let Some(want) = wanted.peek() else {
            break;
        };
        if ch.to_lowercase().eq(want.to_lowercase()) {
            found.push(ix);
            wanted.next();
        }
    }
    wanted.peek().is_none().then_some(found)
}

impl PickerDelegate for CommandPalette {
    fn update_matches(&mut self, query: String, cx: &mut Context<Picker<Self>>) {
        self.matches = (self.entries.iter().enumerate())
            .filter_map(|(ix, entry)| {
                let found = subsequence(&entry.title, &query)?;
                Some((ix, char_ranges(&entry.title, &found)))
            })
            .collect();
        cx.notify();
        let picker = cx.entity();
        // `Picker::matches_updated` borrows the picker, which is being
        // updated now.
        cx.defer(move |cx| picker.update(cx, |picker, cx| picker.matches_updated(true, cx)));
    }

    fn match_count(&self) -> usize {
        self.matches.len()
    }

    fn match_id(&self, ix: usize) -> SharedString {
        self.entries[self.matches[ix].0]
            .id
            .as_str()
            .to_owned()
            .into()
    }

    /// The command's title, not its id.
    fn match_label(&self, ix: usize, _: &App) -> SharedString {
        self.entries[self.matches[ix].0].title.clone()
    }

    /// The command that shows it: "Show All Commands".
    fn title(&self, _: &App) -> Option<SharedString> {
        let id = Toggle::id();
        Some(kiln_command::command_label(id.as_str(), id.as_str()))
    }

    fn render_match(&self, ix: usize, _: bool, cx: &App) -> AnyElement {
        let (entry, highlights) = &self.matches[ix];
        let title = self.entries[*entry].title.clone();
        let ui = ui_theme(cx);
        let label = HighlightedLabel::new(title, highlights.clone(), &ui);
        let key = self.key_label(ix, cx).map(|key| {
            div()
                .ml_auto()
                .pl_3()
                .flex_none()
                .text_color(ui.muted_text)
                .child(key)
        });
        div()
            .flex()
            .flex_1()
            .items_center()
            .child(label)
            .children(key)
            .into_any_element()
    }

    /// Gives focus back to where it was, then runs the command there, once
    /// the palette has gone.
    fn confirm(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Picker<Self>>) -> bool {
        let Some((entry, _)) = self.matches.get(ix) else {
            return false;
        };
        let invocation = Invocation::new(self.entries[*entry].id.clone())
            .source(Source::Palette)
            .context(self.stack.clone());
        if let Some(previous) = self.previous.take() {
            window.focus(&previous, cx);
        }
        window.defer(cx, move |window, cx| {
            if let Err(error) = dispatch_invocation(invocation, window, cx) {
                log::warn!("{error}");
            }
        });
        true
    }

    /// Gives focus back to where it was.
    fn dismissed(&mut self, window: &mut Window, cx: &mut Context<Picker<Self>>) {
        if let Some(previous) = self.previous.take() {
            window.focus(&previous, cx);
        }
    }

    fn status(&self) -> Option<SharedString> {
        (self.matches.is_empty()).then(|| tr!("kiln-shell-palette-no-matches"))
    }
}

/// Shows the command palette for `window`'s focus in `shell`'s modal
/// layer, or dismisses it if it shows.
pub(crate) fn toggle(
    shell: &mut crate::Shell,
    window: &mut Window,
    cx: &mut Context<crate::Shell>,
) {
    if shell.modal(cx) == Some(COMMAND_PALETTE) {
        return shell.dismiss_modal(window, cx);
    }
    let ui = ui_theme(cx);
    let palette = CommandPalette::new(ui, window, cx);
    let picker = cx.new(|cx| Picker::new(palette, text!("kiln-shell-palette-placeholder"), ui, cx));
    picker.update(cx, |picker, cx| picker.show(window, cx));
    shell.show_modal(COMMAND_PALETTE, picker, window, cx);
}
