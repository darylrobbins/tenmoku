//! The status bar along the window's bottom edge: the active item's
//! items, for an editor its vim mode, with vim on, on the left (and
//! `recording @a` beside it while a macro records), and its
//! cursor, selections, indentation, encoding, line endings and language on
//! the right ([`fe_gpui::pane_item::status`]); and whatever items a host
//! sets.
//!
//! Items are data ([`StatusItem`], fastedit's name for kiln-shell's
//! [`kiln_shell::StatusItem`], with the built-in items' ids), kept and
//! drawn by kiln-shell's [`kiln_shell::status`]: the host's over the
//! active item's ([`kiln_shell::PaneItem::status_items`]), sorted. Those
//! are computed from the item each time the bar renders, so nothing new is
//! tracked: the bar observes the item the workbench says is active
//! ([`StatusBar::set_active_item`]), and re-renders when it notifies, as
//! an editor does on every caret move and edit. The workbench embeds the
//! bar cached, so its other redraws reuse the bar's last frame.
//!
//! While that editor has vim's command line open, the line shows in the
//! bar's place, even with the bar hidden: see [`vim_line`].
//!
//! A click on an editor's encoding item runs `workbench.change_encoding`
//! ([`crate::encoding_picker`]), which the editor's own items don't name.
//!
//! Its commands, `view.toggle_status_bar` and `language.select`, are those
//! of [`init`]'s registry, in the workbench's [`crate::KEY_CONTEXT`], with
//! `vim_line.cancel` in the line's. Their handlers only name an op: the
//! workbench does it, or the line.

use std::rc::Rc;

use fe_command::{
    Actor, CommandTarget, ContextPredicate, Dispatched, Effect, Invocation, Registry,
};
use fe_gpui::{EditorView, commands::RunCommand, pane_item::status as built_in};
use gpui::{
    App, Context, Entity, Global, IntoElement, Render, SharedString, Subscription, WeakEntity,
    Window,
};
use kiln_shell::{
    ItemHandle, ItemId,
    status::{self, StatusItems},
};

use crate::encoding_picker::CHANGE_ENCODING;

pub(crate) use built_in::{PLAIN_TEXT, language_label};

pub use kiln_shell::StatusSide;

/// One item on the bar: kiln-shell's [`kiln_shell::StatusItem`] by
/// fastedit's name, which holds the built-in items' ids
/// ([`StatusItem::CURSOR`] …). Items sort by side, then by `priority`,
/// lower first, so left to right; then in the order they were set, the
/// built-in ones first.
#[derive(Clone, Debug, PartialEq)]
pub struct StatusItem {
    /// Distinct among the bar's items. The built-in ones' are
    /// [`StatusItem::CURSOR`] and the other consts on `StatusItem`.
    pub id: SharedString,
    pub side: StatusSide,
    pub priority: i32,
    pub text: SharedString,
    /// Run when the item is clicked, as a key would run it: dispatched to
    /// the focused element, and up to the workbench. An invocation with no
    /// context gets the focused element's then.
    pub command: Option<Invocation>,
}

impl From<StatusItem> for kiln_shell::StatusItem {
    fn from(item: StatusItem) -> Self {
        let StatusItem {
            id,
            side,
            priority,
            text,
            command,
        } = item;
        let kiln = Self::new(id, side, priority, text);
        match command {
            Some(command) => kiln.command(command),
            None => kiln,
        }
    }
}

/// The built-in items' ids, an editor's ([`fe_gpui::pane_item::status`]):
/// the vim mode on the left, the others on the right, in this order. A host
/// item set with one of them shows in that item's place.
impl StatusItem {
    /// `NORMAL`, `INSERT`, `REPLACE`, `VISUAL`, `VISUAL LINE` or `VISUAL
    /// BLOCK`, only with vim on: on the left, priority 0.
    pub const VIM_MODE: &str = built_in::VIM_MODE;
    /// `recording @a` while vim records a macro into register `a`: on the
    /// left, priority 5, beside the mode.
    pub const VIM_RECORDING: &str = "vim_recording";
    /// Vim's last message, such as `E486: Pattern not found: zzz`, until
    /// the next key in the editor: on the left, priority 10.
    pub const VIM_MESSAGE: &str = built_in::VIM_MESSAGE;
    /// `Ln 12, Col 5`, then ` (17 selected)` or ` (3 selections)`: priority
    /// 100.
    pub const CURSOR: &str = built_in::CURSOR;
    /// `Spaces: 4`: priority 200.
    pub const INDENT: &str = built_in::INDENT;
    /// The file's encoding, such as `UTF-8`, `UTF-8 with BOM` or `windows-1252`;
    /// a click runs `workbench.change_encoding`: priority 300.
    pub const ENCODING: &str = built_in::ENCODING;
    /// `LF` or `CRLF`: priority 400.
    pub const LINE_ENDING: &str = built_in::LINE_ENDING;
    /// The language's name, `Plain Text`, or `…` while it loads; a click
    /// runs `language.select`: priority 500.
    pub const LANGUAGE: &str = built_in::LANGUAGE;
}

#[derive(Default)]
pub struct StatusBar {
    /// The item followed: the active pane's active tab's.
    item: Option<Box<dyn ItemHandle>>,
    /// That item as an editor, for vim's command line.
    editor: Option<WeakEntity<EditorView>>,
    /// The host's items.
    items: StatusItems,
    line: vim_line::VimLine,
    _observe: Option<Subscription>,
    /// Each item drawn by the last render, in order: its side, id and text.
    #[cfg(test)]
    drawn: Vec<(StatusSide, SharedString, SharedString)>,
    #[cfg(test)]
    renders: usize,
}

impl StatusBar {
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds `item`, or replaces the item with its id where it was. An item
    /// with a built-in item's id shows in its place.
    pub fn set_item(&mut self, item: impl Into<kiln_shell::StatusItem>, cx: &mut Context<Self>) {
        self.items.set(item.into());
        cx.notify();
    }

    /// Removes the host's item `id`; a built-in item it replaced comes back.
    pub fn remove_item(&mut self, id: &str, cx: &mut Context<Self>) {
        self.items.remove(id);
        cx.notify();
    }

    /// Follows `item`, the active pane's active tab's, or none. The
    /// workbench calls it as it renders, as it does
    /// [`crate::OutlinePanel::set_editor`].
    pub fn set_active_item(
        &mut self,
        item: Option<&dyn ItemHandle>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if item.map(|item| item.id()) == self.item_id() {
            return;
        }
        // Now, while the last frame still draws the item left.
        self.leave_line(item.map(|item| item.id().0), window, cx);
        self.editor = item
            .and_then(|item| item.downcast_ref::<EditorView>())
            .map(Entity::downgrade);
        let bar = cx.entity().downgrade();
        self._observe = item.map(|item| {
            let changed = move |window: &mut Window, cx: &mut App| {
                bar.update(cx, |bar, cx| {
                    bar.sync_line(window, cx);
                    cx.notify();
                })
                .ok();
            };
            item.observe_in(window, Box::new(changed), cx)
        });
        self.item = item.map(ItemHandle::boxed_clone);
        // Not while the workbench draws.
        cx.defer_in(window, Self::sync_line);
        cx.notify();
    }

    /// The item followed.
    pub(crate) fn item_id(&self) -> Option<ItemId> {
        self.item.as_ref().map(|item| item.id())
    }

    /// Every item to draw, sorted: the item's, with the editor's macro
    /// recording, and the host's over them.
    pub(crate) fn items(&self, cx: &App) -> Vec<kiln_shell::StatusItem> {
        let Some(item) = self.item.as_deref() else {
            return self.items.items_for(None, cx);
        };
        let recording = self
            .editor
            .as_ref()
            .and_then(|editor| editor.upgrade())
            .and_then(|editor| editor.read(cx).vim_state()?.recording())
            .map(|register| {
                StatusItem {
                    id: StatusItem::VIM_RECORDING.into(),
                    side: StatusSide::Left,
                    priority: 5,
                    text: format!("recording @{register}").into(),
                    command: None,
                }
                .into()
            });
        let mut own = item.status_items(cx);
        if self.editor.is_some() {
            for item in &mut own {
                if item.id == StatusItem::ENCODING && item.command.is_none() {
                    let change = Invocation::of(CHANGE_ENCODING).expect("a valid command ID");
                    item.command = Some(change);
                    item.tooltip = Some("Select Encoding".into());
                }
            }
        }
        self.items.merged(own.into_iter().chain(recording))
    }
}

impl Render for StatusBar {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if let Some(line) = self.render_line(cx) {
            #[cfg(test)]
            {
                self.renders += 1;
                self.drawn.clear();
            }
            return line.into_any_element();
        }
        let items = self.items(cx);
        #[cfg(test)]
        {
            self.renders += 1;
            self.drawn = items
                .iter()
                .map(|item| (item.side, item.id.clone(), item.text.clone()))
                .collect();
        }
        status::status_bar(items, cx).into_any_element()
    }
}

#[cfg(test)]
impl StatusBar {
    /// The texts of the items the last render drew on `side`, left to
    /// right. Whether that frame showed the bar is the window's to say: see
    /// the `status-item:{id}` debug selectors.
    pub(crate) fn drawn(&self, side: StatusSide) -> Vec<(SharedString, SharedString)> {
        self.drawn
            .iter()
            .filter(|(item_side, ..)| *item_side == side)
            .map(|(_, id, text)| (id.clone(), text.clone()))
            .collect()
    }

    /// How many times the bar has rendered.
    pub(crate) fn renders(&self) -> usize {
        self.renders
    }
}

/// The [`CommandTarget`] of the status bar's one registry command: whether
/// the vim command line's cancel ran. The bar's other commands are typed
/// methods of the workbench.
pub(crate) struct StatusCommands;

impl CommandTarget for StatusCommands {
    type Tx = ();
    type Target<'a> = bool;
}

struct Commands(Rc<Registry<StatusCommands>>);

impl Global for Commands {}

/// Registers the status bar's registry commands.
pub(crate) fn init(cx: &mut App) {
    cx.set_global(Commands(Rc::new(registry())));
}

pub(crate) fn registry() -> Registry<StatusCommands> {
    let mut registry = Registry::new();
    registry
        .register(
            vim_line::CANCEL,
            "Cancel Command Line",
            |target: &mut bool, _: &Actor| {
                *target = true;
                Ok(Dispatched::new(Effect::Notify))
            },
        )
        .expect("status bar command IDs are valid and distinct")
        .context(ContextPredicate::identifier(vim_line::KEY_CONTEXT));
    registry
}

/// Whether `run` is the vim command line's cancel, in the line's context.
pub(crate) fn is_cancel_line(run: &RunCommand, cx: &App) -> bool {
    let Some(commands) = cx.try_global::<Commands>() else {
        return false;
    };
    let registry = commands.0.clone();
    if registry.get(&run.0.command).is_none() {
        return false;
    }
    let mut cancel = false;
    if let Err(error) = registry.dispatch(&mut cancel, &run.0) {
        log::warn!("{error}");
    }
    cancel
}

#[path = "vim_line.rs"]
mod vim_line;

#[cfg(test)]
#[path = "status_bar_vim_tests.rs"]
mod vim_tests;

#[cfg(test)]
#[path = "status_bar_visual_tests.rs"]
mod visual_tests;
