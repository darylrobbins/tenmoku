//! The editor's right-click menu (T1897): a right-click in the text (on
//! macOS a ctrl-click too, which GPUI's platform turns into a right press)
//! opens, at the pointer, the items the app's provider gives
//! ([`set_editor_menu_provider`]). fe-gpui draws the menu, owns its focus
//! and keys, and runs the chosen item; what's in it is the host's.
//!
//! - **The press.** On the line-number column with a debugger's handlers it
//!   opens the breakpoint menu, as before ([`EditorView::on_right_down`]).
//!   In the text (not the gutter, a lens row, the minimap, sticky scroll's
//!   band, nor anything drawn over the text) it focuses the editor, ends a
//!   composition typing nothing, and, outside every selection, puts one
//!   caret there as a plain click does (leaving vim's visual mode too); a
//!   press inside a selection keeps them all. Once the view is done with
//!   the press, the provider is asked ([`EditorMenuRequest`]). With no
//!   provider the press does nothing; an answer with no items opens
//!   nothing. Opening either menu closes the other.
//! - **Focus** is the menu's own while it's open ([`super::menu_focus`]),
//!   so no key typed reaches the buffer, vim's included. Up and Down move
//!   the highlight, wrapping and passing separators and disabled items;
//!   Home and End go to the first and last item; Enter runs the highlighted
//!   one; Escape closes. The pointer's hover highlights; a click runs an
//!   item; a press outside closes.
//! - **Running an item** closes the menu, gives focus back to the editor,
//!   then dispatches its invocation from the editor as a menu's
//!   ([`Source::Menu`]), in the editor's context as it was at the press
//!   when the item names none.
//! - **Closing.** Scrolling the editor, an edit, a Save As, and focus
//!   leaving the menu (the tab going away, another window) close it; a
//!   stale menu runs nothing.
//! - **Later items.** [`EditorView::extend_editor_menu`] adds items to the
//!   menu a request opened, for answers that come after it opens.

use std::{path::PathBuf, rc::Rc};

use fe_buffer::Version;
use fe_command::{ContextFrame, ContextStack, Invocation, Source};
use fe_ui::{MenuEntry, MenuPanel, Role};
use gpui::{
    AnyElement, App, Context, Global, InteractiveElement as _, IntoElement, Keystroke,
    MouseDownEvent, ParentElement as _, Pixels, Point, SharedString, WeakEntity, Window, anchored,
    deferred, div,
};
use kiln_l10n::tr;

use super::{
    EditorView, Press,
    menu_focus::{FocusReturn, MenuFocus},
};
use crate::{KEY_CONTEXT, commands::RunCommand};

/// The menu's key context while it has focus.
pub(crate) const MENU_KEY_CONTEXT: &str = "editor_menu";

/// One item of the menu.
#[derive(Clone, Debug, PartialEq)]
pub struct EditorMenuItem {
    /// Its title, which names it for assistive technology too.
    pub title: SharedString,
    /// The key that runs it, as the platform prints it.
    pub shortcut: Option<SharedString>,
    /// What choosing it runs, from the editor.
    pub invocation: Invocation,
    /// A disabled item is drawn muted, can't be chosen and is passed over
    /// by the arrows.
    pub enabled: bool,
}

/// Which opening of the menu a request is for: [`EditorView::extend_editor_menu`]
/// adds to that menu only.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct EditorMenuTicket(u64);

/// What the provider is asked for a right-click: the menu's entries for it.
#[derive(Clone)]
pub struct EditorMenuRequest {
    /// The editor right-clicked.
    pub editor: WeakEntity<EditorView>,
    pub ticket: EditorMenuTicket,
    /// The buffer offset clicked, and its buffer row from 0.
    pub offset: usize,
    pub row: u32,
    /// Whether the press was inside a selection, which it kept.
    pub inside_selection: bool,
    pub read_only: bool,
    /// The editor's key context stack at the press, its vim state and
    /// flags in its frame: the one its items' keys are read in.
    pub contexts: ContextStack,
}

/// What builds the menu for a request: its entries, `None` a separator.
pub type EditorMenuProvider =
    Rc<dyn Fn(&EditorMenuRequest, &mut Window, &mut App) -> Vec<Option<EditorMenuItem>>>;

struct Provider(EditorMenuProvider);

impl Global for Provider {}

/// Makes `provider` build every editor's right-click menu in the app; with
/// none, a right-click in the text does nothing.
pub fn set_editor_menu_provider(cx: &mut App, provider: EditorMenuProvider) {
    cx.set_global(Provider(provider));
}

/// The menu, open or being asked for.
#[derive(Default)]
pub(crate) struct EditorMenu {
    open: Option<OpenMenu>,
    /// The last ticket given out.
    tickets: u64,
    /// The request the provider is being asked for, which opens once it
    /// answers unless a newer press or a close came first.
    asking: Option<EditorMenuTicket>,
    /// Focus a menu closed without a window gives back at the next render.
    give_back: Option<FocusReturn>,
}

struct OpenMenu {
    ticket: EditorMenuTicket,
    /// Where it was asked for, in the window.
    position: Point<Pixels>,
    entries: Vec<Option<EditorMenuItem>>,
    /// The item Up and Down moved to, or the pointer is over: none until
    /// either does.
    highlighted: Option<usize>,
    /// The buffer's version and file then: an edit or a Save As makes it
    /// stale.
    version: Version,
    /// Where the view was scrolled once a frame had settled the scroll
    /// (revealing the caret the press put): a scroll from anything since,
    /// such as the other side of a diff following its own, makes it stale.
    /// `None` until a frame has painted it.
    scroll: Option<Point<Pixels>>,
    path: Option<PathBuf>,
    /// The editor's context at the press, which an item's invocation that
    /// names none runs in.
    contexts: ContextStack,
    focus: MenuFocus,
}

impl EditorView {
    /// A right press on the view: the breakpoint menu on the line-number
    /// column with a debugger's handlers, the editor menu in the text; see
    /// the [module](self).
    pub(crate) fn on_right_press(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.breakpoint_row_at(event.position).is_some() {
            self.close_editor_menu_in(window, cx);
            self.on_right_down(event, window, cx);
            return;
        }
        if !cx.has_global::<Provider>() || !self.editor_menu_target(event.position, window) {
            return;
        }
        cx.stop_propagation();
        window.focus(&self.focus, cx);
        if !self.settle_press(event, Press::Right, window, cx) {
            return;
        }
        self.close_breakpoint_menu(cx);
        self.close_editor_menu_in(window, cx);
        self.cancel_composition(cx);
        self.drag = None;
        let vim = self.vim_owns_mouse(cx);
        let offset = match vim {
            true => self.vim_offset_for_position(event.position),
            false => self.offset_for_position(event.position),
        };
        // Half-open, as selections are: the character after one isn't in it.
        let inside = (self.model.selections().iter())
            .any(|selection| selection.range.start <= offset && offset < selection.range.end);
        // Followed through the caret's move, which may edit the text: in
        // vim's insert mode, leaving a blank line takes out its unused
        // autoindent.
        let clicked = self.model.snapshot().anchor_before(offset);
        if !inside {
            // One caret, as a plain click puts it, out of vim's visual mode.
            self.cancel_vim(cx);
            match vim {
                true => self.model.set_cursor(offset),
                false => {
                    let place = |model: &mut fe_editor::EditorModel| model.set_cursor(offset);
                    self.split_session(fe_vim::Splits::Moves, place, cx);
                }
            }
            self.set_hover(None, cx);
            self.sync_completion(cx);
            self.changed(cx);
        }
        let offset = self.model.snapshot().resolve(&clicked);
        let row = self.model.text().offset_to_point(offset).row;
        let contexts = self.menu_contexts(window, cx);
        self.editor_menu.tickets += 1;
        let ticket = EditorMenuTicket(self.editor_menu.tickets);
        self.editor_menu.asking = Some(ticket);
        let request = EditorMenuRequest {
            editor: cx.entity().downgrade(),
            ticket,
            offset,
            row,
            inside_selection: inside,
            read_only: self.is_read_only(),
            contexts,
        };
        let position = event.position;
        // Asked once this view is done with the press, as the provider
        // reads it.
        window.defer(cx, move |window, cx| {
            let Some(provider) = cx.try_global::<Provider>().map(|p| p.0.clone()) else {
                return;
            };
            let entries = provider(&request, window, cx);
            let Some(view) = request.editor.upgrade() else {
                return;
            };
            view.update(cx, |view, cx| {
                view.open_editor_menu(&request, entries, position, window, cx)
            });
        });
    }

    /// Whether a right press at `position` is in the text: over the text
    /// the last frame drew, uncovered, and not on a lens row, the minimap
    /// or sticky scroll's band.
    fn editor_menu_target(&self, position: Point<Pixels>, window: &Window) -> bool {
        let Some(layout) = &self.layout else {
            return false;
        };
        let on_lens = (self.lens_rows.iter()).any(|row| row.bounds.contains(&position));
        let on_minimap = self
            .minimap_bounds()
            .is_some_and(|bounds| bounds.contains(&position));
        let on_band = crate::element::sticky_scroll::row_at(layout, position, window).is_some();
        layout.over_text(position, window) && !on_lens && !on_minimap && !on_band
    }

    /// The editor's key context stack now, as a key typed in it would be
    /// matched in: the frames around it, then its own, stamped with vim's
    /// live state, carrying its flags.
    fn menu_contexts(&mut self, window: &Window, cx: &mut App) -> ContextStack {
        let stack = self.context_stack(window, cx).unwrap_or_else(|| {
            let own = ContextStack::new().with(ContextFrame::new(KEY_CONTEXT));
            crate::vim::stamp(own, self.vim.as_ref())
        });
        let mut flags: Vec<&str> = self.key_flags.clone();
        if self.shows_open_hunk() {
            flags.push("proposal");
        }
        let own = (stack.frames().iter()).rposition(|frame| frame.name == KEY_CONTEXT);
        (stack.frames().iter().cloned().enumerate())
            .map(|(ix, frame)| match Some(ix) == own {
                true => flags.iter().fold(frame, |frame, &flag| frame.flag(flag)),
                false => frame,
            })
            .collect()
    }

    /// Opens the menu `request` asked for with `entries`, unless a newer
    /// press or a close came first, or nothing in them is an item.
    fn open_editor_menu(
        &mut self,
        request: &EditorMenuRequest,
        entries: Vec<Option<EditorMenuItem>>,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.editor_menu.asking != Some(request.ticket) {
            return;
        }
        self.editor_menu.asking = None;
        let entries = tidy(entries);
        if entries.is_empty() {
            return;
        }
        // Settled here, not left to the editor's blur.
        self.cancel_composition(cx);
        self.close_breakpoint_menu(cx);
        let ticket = request.ticket;
        let focus = MenuFocus::take(&self.focus, window, cx, move |view, window, cx| {
            if (view.editor_menu.open.as_ref()).is_some_and(|menu| menu.ticket == ticket) {
                view.close_editor_menu_in(window, cx);
                // Told in the draw's focus phase, whose notify asks for no
                // frame: the next one, once it's over, takes the menu away.
                cx.defer_in(window, |_, _, cx| cx.notify());
            }
        });
        self.editor_menu.open = Some(OpenMenu {
            ticket,
            position,
            entries,
            highlighted: None,
            version: self.model.snapshot().version(),
            scroll: None,
            path: self.model.path().map(Into::into),
            contexts: request.contexts.clone(),
            focus,
        });
        cx.notify();
    }

    /// Inserts `items` into the open menu right after the item running
    /// command `after`, or as a new last group if there's none, if
    /// `ticket` is the open menu's; otherwise does nothing. The highlight
    /// stays on the item it was on.
    pub fn extend_editor_menu(
        &mut self,
        ticket: EditorMenuTicket,
        after: &str,
        items: Vec<Option<EditorMenuItem>>,
        cx: &mut Context<Self>,
    ) {
        let Some(menu) = (self.editor_menu.open.as_mut()).filter(|menu| menu.ticket == ticket)
        else {
            return;
        };
        if items.iter().all(Option::is_none) {
            return;
        }
        let anchor = (menu.entries.iter()).position(|entry| {
            entry
                .as_ref()
                .is_some_and(|item| item.invocation.command.as_str() == after)
        });
        let at = match anchor {
            Some(ix) => ix + 1,
            None => {
                menu.entries.push(None);
                menu.entries.len()
            }
        };
        let count = items.len();
        menu.entries.splice(at..at, items);
        if let Some(highlighted) = menu.highlighted.filter(|&ix| ix >= at) {
            menu.highlighted = Some(highlighted + count);
        }
        cx.notify();
    }

    /// Whether the menu is open, holding the editor's input.
    pub(crate) fn editor_menu_open(&self) -> bool {
        self.editor_menu.open.is_some()
    }

    /// The open menu's entries, `None` a separator. For tests outside the
    /// crate too.
    #[doc(hidden)]
    pub fn editor_menu_entries(&self) -> Option<&[Option<EditorMenuItem>]> {
        Some(&self.editor_menu.open.as_ref()?.entries)
    }

    /// The open menu's ticket.
    #[doc(hidden)]
    pub fn editor_menu_ticket(&self) -> Option<EditorMenuTicket> {
        Some(self.editor_menu.open.as_ref()?.ticket)
    }

    /// The open menu's highlighted entry. For tests.
    #[doc(hidden)]
    pub fn editor_menu_highlighted(&self) -> Option<usize> {
        self.editor_menu.open.as_ref()?.highlighted
    }

    /// The handle the open menu's element tracks, which has focus while it
    /// does. For tests.
    #[cfg(test)]
    pub(crate) fn editor_menu_focus(&self) -> Option<gpui::FocusHandle> {
        Some(self.editor_menu.open.as_ref()?.focus.handle().clone())
    }

    /// Closes the menu, if it's open. Focus, if the menu has it, goes back
    /// to the editor at its next render.
    pub fn close_editor_menu(&mut self, cx: &mut Context<Self>) {
        self.editor_menu.asking = None;
        if let Some(menu) = self.editor_menu.open.take() {
            self.editor_menu.give_back = Some(menu.focus.give_back_later());
            cx.notify();
        }
    }

    /// Closes the menu, giving focus back to the editor if the menu has it.
    pub(crate) fn close_editor_menu_in(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.editor_menu.asking = None;
        if let Some(menu) = self.editor_menu.open.take() {
            menu.focus.give_back(window, cx);
            cx.notify();
        }
    }

    /// At render: gives back the focus of a menu closed without a window.
    pub(crate) fn settle_editor_menu_focus(&mut self, window: &mut Window, cx: &mut App) {
        if let Some(give_back) = self.editor_menu.give_back.take() {
            give_back.finish(window, cx);
        }
    }

    /// A key while the menu has focus: see the [module](self). Every key
    /// is the menu's; the editor sees none.
    fn editor_menu_key(
        &mut self,
        keystroke: &Keystroke,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(menu) = self.editor_menu.open.as_mut() else {
            return;
        };
        let enabled: Vec<bool> = (menu.entries.iter())
            .map(|entry| entry.as_ref().is_some_and(|item| item.enabled))
            .collect();
        let first = enabled.iter().position(|&on| on);
        let last = enabled.iter().rposition(|&on| on);
        match keystroke.key.as_str() {
            "escape" => return self.close_editor_menu_in(window, cx),
            "down" => menu.highlighted = next_highlight(&enabled, menu.highlighted, true),
            "up" => menu.highlighted = next_highlight(&enabled, menu.highlighted, false),
            "home" => menu.highlighted = first,
            "end" => menu.highlighted = last,
            "enter" => {
                if let Some(ix) = menu.highlighted {
                    self.choose_editor_menu_item(ix, window, cx);
                }
                return;
            }
            _ => return,
        }
        cx.notify();
    }

    /// Chooses entry `ix` of the open menu: closes it, gives focus back to
    /// the editor, and runs the item from the editor once this listener is
    /// done, unless it's disabled or the menu was stale. For the menu's
    /// rows and keys, and tests outside the crate.
    #[doc(hidden)]
    pub fn choose_editor_menu_item(
        &mut self,
        ix: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(menu) = self.editor_menu.open.take() else {
            return;
        };
        let current = self.editor_menu_is_current(&menu);
        cx.notify();
        let OpenMenu {
            entries,
            contexts,
            focus,
            ..
        } = menu;
        focus.give_back(window, cx);
        let Some(Some(item)) = entries.into_iter().nth(ix).filter(|_| current) else {
            return;
        };
        if !item.enabled {
            return;
        }
        let mut invocation = item.invocation.source(Source::Menu);
        if invocation.context.is_empty() {
            invocation.context = contexts;
        }
        let action = RunCommand(invocation);
        let editor = self.focus.clone();
        window.defer(cx, move |window, cx| {
            editor.dispatch_action(&action, window, cx)
        });
    }

    /// Whether `menu` was opened on the text and file the view has now,
    /// scrolled where it settled.
    fn editor_menu_is_current(&self, menu: &OpenMenu) -> bool {
        menu.version == self.model.snapshot().version()
            && menu.path.as_deref() == self.model.path()
            && menu.scroll.is_none_or(|scroll| scroll == self.scroll)
    }

    /// The open menu, over the window where it was asked for; closed if
    /// the text or the file changed under it.
    pub(crate) fn editor_menu_layer(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let stale =
            (self.editor_menu.open.as_ref()).is_some_and(|m| !self.editor_menu_is_current(m));
        if stale {
            self.close_editor_menu_in(window, cx);
        }
        let menu = self.editor_menu.open.as_ref()?;
        // Where this frame settles the scroll, once it's painted.
        let ticket = menu.ticket;
        cx.defer_in(window, move |view, _, _| {
            let scroll = view.scroll;
            if let Some(menu) = (view.editor_menu.open.as_mut()).filter(|m| m.ticket == ticket) {
                menu.scroll.get_or_insert(scroll);
            }
        });
        let shown = (menu.entries.iter())
            .map(|entry| match entry {
                Some(item) => MenuEntry::Item {
                    title: item.title.clone(),
                    shortcut: item.shortcut.clone(),
                },
                None => MenuEntry::Separator,
            })
            .collect();
        let disabled = (menu.entries.iter().enumerate())
            .filter(|(_, entry)| entry.as_ref().is_some_and(|item| !item.enabled))
            .map(|(ix, _)| ix);
        let (chooser, hover) = (cx.entity().downgrade(), cx.entity().downgrade());
        let ui = fe_ui::ui_theme(cx);
        let panel = MenuPanel::new(shown, &ui)
            .highlighted(menu.highlighted)
            .disabled(disabled)
            .item_nodes()
            .on_click(move |ix, window, cx| {
                let choose = |view: &mut EditorView, cx: &mut Context<EditorView>| {
                    view.choose_editor_menu_item(ix, window, cx)
                };
                (chooser.update(cx, choose)).ok();
            })
            .on_hover(move |ix, _, cx| {
                (hover.update(cx, |view, cx| {
                    if let Some(menu) = &mut view.editor_menu.open
                        && menu.highlighted != Some(ix)
                    {
                        menu.highlighted = Some(ix);
                        cx.notify();
                    }
                }))
                .ok();
            });
        let wrapper = div()
            .id("editor-menu")
            .debug_selector(|| "editor-menu".into())
            .track_focus(menu.focus.handle())
            .key_context(super::menu_focus::key_context(MENU_KEY_CONTEXT))
            .on_key_down(cx.listener(|view, event: &gpui::KeyDownEvent, window, cx| {
                cx.stop_propagation();
                view.editor_menu_key(&event.keystroke, window, cx);
            }))
            .on_mouse_down_out(
                cx.listener(|view, _, window, cx| view.close_editor_menu_in(window, cx)),
            )
            .child(panel);
        let wrapper = fe_ui::node(wrapper, window, cx, Role::Menu, tr!("gpui-editor-menu"));
        Some(
            deferred(
                anchored()
                    .position(menu.position)
                    .snap_to_window()
                    .child(wrapper),
            )
            .with_priority(1)
            .into_any_element(),
        )
    }
}

/// `entries` without separators at either end or two together.
fn tidy(entries: Vec<Option<EditorMenuItem>>) -> Vec<Option<EditorMenuItem>> {
    let mut tidied: Vec<Option<EditorMenuItem>> = Vec::with_capacity(entries.len());
    for entry in entries {
        if entry.is_some() || tidied.last().is_some_and(Option::is_some) {
            tidied.push(entry);
        }
    }
    if tidied.last().is_some_and(Option::is_none) {
        tidied.pop();
    }
    tidied
}

/// Where Down (or with `down` false, Up) moves the highlight from
/// `highlighted`, among entries `enabled` says can be chosen: wrapping,
/// passing separators and disabled items; from none, the first or the
/// last. None if nothing can be chosen.
fn next_highlight(enabled: &[bool], highlighted: Option<usize>, down: bool) -> Option<usize> {
    let items: Vec<usize> = (enabled.iter().enumerate())
        .filter(|(_, on)| **on)
        .map(|(ix, _)| ix)
        .collect();
    let (first, last) = (*items.first()?, *items.last()?);
    let Some(from) = highlighted else {
        return Some(if down { first } else { last });
    };
    Some(match down {
        true => items.iter().copied().find(|&ix| ix > from).unwrap_or(first),
        false => items
            .iter()
            .rev()
            .copied()
            .find(|&ix| ix < from)
            .unwrap_or(last),
    })
}

#[cfg(test)]
#[path = "context_menu_tests.rs"]
mod tests;
