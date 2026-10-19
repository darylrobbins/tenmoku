//! The completion menu (docs/decisions/lsp-client.md §7, "Completion"):
//! what the view's [`CompletionProvider`] (a language server, say) offers
//! at the caret, below the word being typed, filtered by it through
//! fe-editor's [`CompletionSession`].
//!
//! - **Opening.** `editor.trigger_completion` (`ctrl-space`) asks at the
//!   caret, and so does typing one of the provider's trigger characters.
//!   The menu shows once an answer with items lands; an answer with none
//!   (a timeout, say) closes it. Asking again replaces the request.
//! - **Typing** word characters narrows it. Cutting the word back past
//!   where it was asked, or typing more into an incomplete list, asks
//!   again, the menu staying as it was until the answer.
//! - **Keys**, in the `completion` key context while it shows:
//!   `completion.select_prev` and `select_next`, `completion.accept` and
//!   `completion.dismiss`. `escape`, or the caret leaving the word, closes
//!   it without editing.
//! - **Vim's keyword completion** (`<C-n>`, `<C-p>`) shows its candidates in
//!   the same rows, label only, the current one selected, at the typed
//!   text's start. It asks the provider nothing, closes a language
//!   server's menu, and sets no key context: its keys stay vim's.
//! - **Rows.** Each shows the item's label and label detail, its detail and
//!   its kind. It goes below the word, or above it where there's more room,
//!   and only the rows that fit there are built, [`MAX_ROWS`] at most,
//!   scrolled to keep the selection in view. The selected item is resolved, and its
//!   documentation shows beside the rows once it arrives.

use std::{collections::HashSet, future::Future, pin::Pin, rc::Rc};

use fe_buffer::{BufferId, Version};
use fe_command::ContextStack;
use fe_editor::completion::{CompletionItem, CompletionList, CompletionSession, CompletionTrigger};
use fe_vim::KeywordCompletion;
use gpui::{
    AnyElement, AvailableSpace, Context, FontWeight, InteractiveElement as _, IntoElement,
    ParentElement as _, Pixels, Point, SharedString, StatefulInteractiveElement as _, Styled as _,
    Task, anchored, canvas, deferred, div, px,
};

use crate::{
    EditorView,
    hover::{Block, UI_FONT, markdown_blocks},
};

/// `ctrl-space`: asks for completions at the caret.
pub const TRIGGER: &str = "editor.trigger_completion";
pub const SELECT_PREV: &str = "completion.select_prev";
pub const SELECT_NEXT: &str = "completion.select_next";
pub const ACCEPT: &str = "completion.accept";
pub const DISMISS: &str = "completion.dismiss";

/// The key context the menu's keys are bound in while it shows.
pub const KEY_CONTEXT: &str = "completion";

/// The most rows the menu shows, and builds, at once.
pub(crate) const MAX_ROWS: usize = 12;

/// A [`CompletionProvider`]'s answer to come: `None` for nothing to offer,
/// whatever the reason. Dropping it should cancel the work.
pub type CompletionFuture = Pin<Box<dyn Future<Output = Option<CompletionList>>>>;

/// An item, resolved, or as it was if it can't be.
pub type ResolveFuture = Pin<Box<dyn Future<Output = CompletionItem>>>;

/// Where a view's completions come from, set by its host
/// ([`EditorView::set_completion_provider`]): fe-gpui knows nothing of
/// language servers.
pub trait CompletionProvider {
    /// The characters that ask for completions when typed in `buffer`.
    fn trigger_characters(&self, buffer: BufferId) -> Vec<String>;

    /// What to offer at byte `offset` of `buffer`, at its `version`, with
    /// edits in that version.
    fn completions(
        &self,
        buffer: BufferId,
        version: Version,
        offset: usize,
        trigger: CompletionTrigger,
    ) -> CompletionFuture;

    /// `item` with its documentation and detail filled in.
    fn resolve(&self, item: CompletionItem) -> ResolveFuture;
}

/// The menu: its session, or one with no items while the first answer is
/// awaited.
pub(crate) struct Menu {
    session: CompletionSession,
    /// The version and caret last asked at.
    asked: (Version, usize),
    /// The first row shown: an index into the session's matches.
    top: usize,
    /// The items resolved so far, and the one resolving.
    resolved: HashSet<usize>,
    resolving: Option<(usize, Task<()>)>,
    /// The request in flight, if any: dropping it cancels it.
    request: Option<Task<()>>,
    /// Where it goes, as the frame being drawn lays its word out.
    place: Option<Place>,
    /// Its bounds, as the frame being drawn has laid it out.
    laid_out: Option<gpui::Bounds<Pixels>>,
    /// Vim's keyword completion this shows in place of an answer, if it
    /// does: its candidates are the rows, and the menu takes no keys.
    keyword: Option<KeywordCompletion>,
}

/// Where the menu goes: by its word's start, below it or, with more room
/// there, above it, with as many rows as fit.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Place {
    /// The corner by the word, in window coordinates: the menu's top-left,
    /// or its bottom-left `above` the word.
    pub(crate) at: Point<Pixels>,
    pub(crate) above: bool,
    pub(crate) rows: usize,
    pub(crate) row_height: Pixels,
}

/// What the last frame's menu showed.
#[cfg(test)]
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Painted {
    /// Each row's label (with its label detail), detail and kind.
    pub(crate) rows: Vec<(String, String, String)>,
    /// The selected row, among those.
    pub(crate) selected: Option<usize>,
    pub(crate) documentation: Option<String>,
    pub(crate) place: Place,
    /// Its bounds as laid out, in window coordinates.
    pub(crate) bounds: Option<gpui::Bounds<Pixels>>,
}

impl Menu {
    fn new(
        session: CompletionSession,
        asked: (Version, usize),
        keyword: Option<KeywordCompletion>,
    ) -> Self {
        Menu {
            session,
            asked,
            top: 0,
            resolved: HashSet::new(),
            resolving: None,
            request: None,
            place: None,
            laid_out: None,
            keyword,
        }
    }
}

impl EditorView {
    /// Sets where completions come from, or none. A menu open on the last
    /// one's answer goes.
    pub fn set_completion_provider(&mut self, provider: Option<Rc<dyn CompletionProvider>>) {
        self.completion_provider = provider;
        self.completion = None;
    }

    pub fn completion_provider(&self) -> Option<&Rc<dyn CompletionProvider>> {
        self.completion_provider.as_ref()
    }

    /// Whether the menu shows: there's an answer with items matching the
    /// word.
    pub fn completion_shown(&self) -> bool {
        (self.completion.as_ref())
            .is_some_and(|menu| menu.keyword.is_none() && !menu.session.matches().is_empty())
    }

    /// Asks the provider for completions at the caret, outside vim's
    /// modes that take no text.
    pub(crate) fn trigger_completion(
        &mut self,
        trigger: CompletionTrigger,
        cx: &mut Context<Self>,
    ) {
        self.sync_vim(cx);
        let Some(provider) = self.completion_provider.clone() else {
            return;
        };
        if !self.accepts_text() {
            return;
        }
        let snapshot = self.model.snapshot().clone();
        let offset = self.model.primary_selection().head();
        let version = snapshot.version();
        let reply = provider.completions(snapshot.id(), version, offset, trigger);
        let base = snapshot.clone();
        let request = cx.spawn(async move |view, cx| {
            let list = reply.await;
            view.update(cx, |view, cx| {
                view.completions_landed(base, offset, list, cx)
            })
            .ok();
        });
        if (self.completion.as_ref()).is_some_and(|menu| menu.keyword.is_some()) {
            self.completion = None;
        }
        let menu = self.completion.get_or_insert_with(|| {
            Menu::new(
                CompletionSession::new(snapshot, offset, CompletionList::default()),
                (version, offset),
                None,
            )
        });
        menu.asked = (version, offset);
        menu.request = Some(request);
    }

    /// Typing `text` asks for completions if it ends with a trigger
    /// character.
    pub(crate) fn completion_typed(&mut self, text: &str, cx: &mut Context<Self>) {
        let Some(provider) = &self.completion_provider else {
            return;
        };
        let buffer = self.model.buffer().id();
        let trigger = (provider.trigger_characters(buffer).into_iter())
            .find(|trigger| !trigger.is_empty() && text.ends_with(trigger.as_str()));
        if let Some(trigger) = trigger {
            self.trigger_completion(CompletionTrigger::Character(trigger), cx);
        }
    }

    fn completions_landed(
        &mut self,
        base: fe_buffer::BufferSnapshot,
        offset: usize,
        list: Option<CompletionList>,
        cx: &mut Context<Self>,
    ) {
        self.sync_vim(cx);
        let accepts_text = self.accepts_text();
        let Some(menu) = &mut self.completion else {
            return;
        };
        if menu.keyword.is_some() {
            return;
        }
        menu.request = None;
        // Nothing, a timeout among others, closes it, even over items an
        // earlier answer gave.
        let Some(list) = list.filter(|list| accepts_text && !list.items.is_empty()) else {
            self.close_completion(cx);
            return;
        };
        let mut session = CompletionSession::new(base, offset, list);
        if !session.update(&self.model) {
            self.close_completion(cx);
            return;
        }
        // The selected item stays selected if it's offered again.
        let selected = (menu.session.selected()).map(|ix| menu.session.items()[ix].label.clone());
        let again = (session.matches().iter())
            .find(|&&ix| Some(&session.items()[ix].label) == selected.as_ref())
            .copied();
        if let Some(ix) = again {
            session.select(ix);
        }
        menu.session = session;
        menu.top = 0;
        menu.resolved.clear();
        menu.resolving = None;
        // Typed into while it was awaited: an incomplete list asks again.
        self.sync_completion(cx);
    }

    /// After anything that may have moved the caret or changed the text:
    /// narrows the menu to the word typed, closes it once the session is
    /// over ([`CompletionSession::update`]) or vim has left insert mode,
    /// and asks again for more as an incomplete list is typed into.
    pub(crate) fn sync_completion(&mut self, cx: &mut Context<Self>) {
        self.sync_vim(cx);
        let accepts_text = self.accepts_text();
        let Some(menu) = &mut self.completion else {
            return;
        };
        if menu.keyword.is_some() {
            cx.notify();
            return;
        }
        if !accepts_text || !menu.session.update(&self.model) {
            // Cut back, or moved within the word: ask again there.
            let again = accepts_text && menu.session.may_ask_again(&self.model);
            self.close_completion(cx);
            if again {
                self.trigger_completion(CompletionTrigger::Invoked, cx);
            }
            return;
        }
        let now = (
            self.model.snapshot().version(),
            self.model.primary_selection().head(),
        );
        if menu.asked != now && menu.session.is_incomplete() {
            self.trigger_completion(CompletionTrigger::Incomplete, cx);
        }
        self.scroll_to_selected();
        self.resolve_selected(cx);
        cx.notify();
    }

    /// Drops a menu whose session is over, for a frame about to set the
    /// key context: whatever moved the caret (a jump, a vim command, a
    /// drag) may not have synced it.
    pub(crate) fn drop_stale_completion(&mut self) {
        // Vim's keyword completion under way shows its candidates, over a
        // language server's menu, which it closes.
        if let Some(keyword) = self.keyword_completion() {
            let (top, place) = match &self.completion {
                Some(menu) if menu.keyword.is_some() => (menu.top, menu.place),
                _ => (0, None),
            };
            let snapshot = self.model.snapshot().clone();
            let at = self.model.primary_selection().head();
            let mut menu = Menu::new(
                CompletionSession::new(snapshot.clone(), at, CompletionList::default()),
                (snapshot.version(), at),
                Some(keyword),
            );
            (menu.top, menu.place) = (top, place);
            self.completion = Some(menu);
            self.scroll_to_selected();
            return;
        }
        let accepts_text = self.accepts_text();
        if (self.completion.as_ref()).is_some_and(|menu| {
            menu.keyword.is_some() || !accepts_text || !menu.session.is_live(&self.model)
        }) {
            self.completion = None;
        }
    }

    /// Has `escape` close the menu, or drop the request a menu waits on,
    /// wherever it's pressed, before any binding takes it, and losing
    /// focus close it too.
    pub(crate) fn close_completion_on_escape_and_blur(
        view: &mut EditorView,
        window: &mut gpui::Window,
        cx: &mut Context<EditorView>,
    ) {
        let this = cx.weak_entity();
        let escape = cx.intercept_keystrokes(move |event, _, cx| {
            if event.keystroke.key == "escape" {
                this.update(cx, |view, cx| view.close_completion(cx)).ok();
            }
        });
        let blur = cx.on_blur(&view.focus, window, |view, _, cx| view.close_completion(cx));
        view.completion_subscriptions = vec![escape, blur];
    }

    pub(crate) fn close_completion(&mut self, cx: &mut Context<Self>) {
        if self.completion.take().is_some() {
            cx.notify();
        }
    }

    /// Runs one of the menu's commands, which the registry has dispatched.
    /// `true` if `command` is one.
    pub(crate) fn run_completion_command(&mut self, command: &str, cx: &mut Context<Self>) -> bool {
        match command {
            TRIGGER => self.trigger_completion(CompletionTrigger::Invoked, cx),
            SELECT_PREV | SELECT_NEXT => {
                self.drop_stale_completion();
                if let Some(menu) = &mut self.completion
                    && menu.keyword.is_none()
                {
                    menu.session.select_next(command == SELECT_PREV);
                    self.scroll_to_selected();
                    self.resolve_selected(cx);
                    cx.notify();
                }
            }
            ACCEPT => {
                self.drop_stale_completion();
                if let Some(menu) = self.completion.take()
                    && menu.keyword.is_none()
                    && let Err(error) = menu.session.accept(&mut self.model)
                {
                    log::debug!("completion not applied: {error}");
                }
                self.changed(cx);
            }
            DISMISS => self.close_completion(cx),
            _ => return false,
        }
        true
    }

    /// Scrolls the rows so the selected one shows.
    fn scroll_to_selected(&mut self) {
        let Some(menu) = &mut self.completion else {
            return;
        };
        let rows = menu.place.map_or(MAX_ROWS, |place| place.rows);
        let (count, selected) = match &menu.keyword {
            Some(keyword) => (keyword.candidates.len(), keyword.selected.unwrap_or(0)),
            None => (
                menu.session.matches().len(),
                menu.session.selected_match().unwrap_or(0),
            ),
        };
        menu.top = (menu.top.min(selected))
            .max((selected + 1).saturating_sub(rows))
            .min(count.saturating_sub(rows));
    }

    /// Asks the provider to resolve the selected item, once.
    fn resolve_selected(&mut self, cx: &mut Context<Self>) {
        let (Some(provider), Some(menu)) = (self.completion_provider.clone(), &mut self.completion)
        else {
            return;
        };
        let Some(ix) = menu.session.selected() else {
            return;
        };
        if menu.resolved.contains(&ix) || menu.resolving.as_ref().is_some_and(|(at, _)| *at == ix) {
            return;
        }
        let reply = provider.resolve(menu.session.items()[ix].clone());
        // Replaces the last one's, which dropping cancels.
        let task = cx.spawn(async move |view, cx| {
            let item = reply.await;
            view.update(cx, |view, cx| {
                let Some(menu) = &mut view.completion else {
                    return;
                };
                menu.resolving = None;
                menu.resolved.insert(ix);
                menu.session.set_item(ix, item);
                cx.notify();
            })
            .ok();
        });
        menu.resolving = Some((ix, task));
    }

    /// Where the word starts, for the frame being laid out to place the
    /// menu below, if it shows.
    pub(crate) fn completion_word_start(&self) -> Option<usize> {
        let menu = self.completion.as_ref()?;
        if let Some(keyword) = &menu.keyword {
            return (!keyword.candidates.is_empty()).then_some(keyword.typed.start);
        }
        (!menu.session.matches().is_empty()).then(|| menu.session.word_start_in(&self.model))
    }

    /// Places the menu by `word`, the bounds of its word's start in the
    /// frame being drawn, in a window of `viewport`, rows `row_height`
    /// apart: below the word, or above it with more room there, with as
    /// many rows as fit, up to [`MAX_ROWS`]. Closes it if the frame shows
    /// no word (scrolled out, or folded away), or no row fits: a menu not
    /// drawn mustn't take keys.
    pub(crate) fn place_completion(
        &mut self,
        word: Option<gpui::Bounds<Pixels>>,
        viewport: gpui::Size<Pixels>,
        row_height: Pixels,
        cx: &mut Context<Self>,
    ) {
        let Some(word) = word else {
            if self.completion_shown() {
                self.close_completion(cx);
            }
            return;
        };
        // The list's border, above and below.
        let border = px(2.);
        let (below, above) = (
            viewport.height - word.bottom() - border,
            word.top() - border,
        );
        let fit = |space: Pixels| (space / row_height).floor().max(0.) as usize;
        let place_above = fit(below) < MAX_ROWS && fit(above) > fit(below);
        let rows = fit(if place_above { above } else { below }).min(MAX_ROWS);
        // Not one row fits by the word: it can't show, nor take keys.
        if rows == 0 {
            self.close_completion(cx);
            return;
        }
        let Some(menu) = &mut self.completion else {
            return;
        };
        let at = match place_above {
            true => gpui::point(word.left(), word.top()),
            false => gpui::point(word.left(), word.bottom()),
        };
        menu.place = Some(Place {
            at,
            above: place_above,
            rows,
            row_height,
        });
        self.scroll_to_selected();
    }

    /// The bounds the frame being drawn has laid the menu out at, its
    /// documentation's included, in window coordinates, once its layer has
    /// laid it out ([`menu_layer`]).
    pub(crate) fn completion_bounds(&self) -> Option<gpui::Bounds<Pixels>> {
        self.completion.as_ref()?.laid_out
    }

    /// The menu, below the start of the word, if it shows and the frame
    /// being drawn has placed it.
    pub(crate) fn render_completion(&mut self, cx: &mut Context<Self>) -> Option<AnyElement> {
        #[cfg(test)]
        {
            self.painted_completion = None;
        }
        let menu = self.completion.as_mut()?;
        menu.laid_out = None;
        let menu = &*menu;
        let session = &menu.session;
        let place = menu.place?;
        let theme = &self.theme;
        let (rows, selected_row, documentation): (Vec<(String, String, String)>, _, _) =
            if let Some(keyword) = &menu.keyword {
                let end = (menu.top + place.rows).min(keyword.candidates.len());
                let shown = keyword.candidates.get(menu.top..end)?;
                let rows = (shown.iter())
                    .map(|label| (label.clone(), String::new(), String::new()))
                    .collect();
                let selected = keyword.selected.and_then(|ix| ix.checked_sub(menu.top));
                (rows, selected.filter(|&row| row < shown.len()), None)
            } else {
                if session.matches().is_empty() {
                    return None;
                }
                let selected = session.selected();
                let shown = &session.matches()
                    [menu.top..(menu.top + place.rows).min(session.matches().len())];
                let rows = (shown.iter())
                    .map(|&ix| {
                        let item = &session.items()[ix];
                        let label = item.label.clone() + item.label_detail.as_deref().unwrap_or("");
                        let detail = item.detail.clone().unwrap_or_default();
                        (label, detail, item.kind.clone().unwrap_or_default())
                    })
                    .collect();
                let selected_row = shown.iter().position(|&ix| Some(ix) == selected);
                let documentation = selected
                    .and_then(|ix| session.items()[ix].documentation.clone())
                    .filter(|text| !text.trim().is_empty());
                (rows, selected_row, documentation)
            };
        #[cfg(test)]
        {
            self.painted_completion = Some(Painted {
                rows: rows.clone(),
                selected: selected_row,
                documentation: documentation.clone(),
                place,
                bounds: None,
            });
        }
        let dim = theme.gutter_foreground;
        let list = div()
            .flex()
            .flex_col()
            .min_w(px(240.))
            .max_w(px(480.))
            .children(
                rows.into_iter()
                    .enumerate()
                    .map(|(row, (label, detail, kind))| {
                        let mut row_div = div()
                            .h(place.row_height)
                            .flex()
                            .items_center()
                            .gap_2()
                            .px_2()
                            .overflow_hidden()
                            .whitespace_nowrap();
                        if Some(row) == selected_row {
                            row_div = row_div.bg(theme.selection);
                        }
                        row_div
                            .child(div().font_weight(FontWeight::MEDIUM).child(label))
                            .child(
                                div()
                                    .flex_1()
                                    .overflow_hidden()
                                    .text_color(dim)
                                    .child(detail),
                            )
                            .child(div().text_color(dim).font_family(UI_FONT).child(kind))
                    }),
            );
        let docs = documentation.map(|text| {
            let blocks = markdown_blocks(&text).into_iter().map(|block| {
                let font: SharedString = match &block {
                    Block::Text(_) => UI_FONT.into(),
                    Block::Code(_) => theme.font_family.clone(),
                };
                div()
                    .font_family(font)
                    .child(SharedString::from(block.text().to_owned()))
            });
            div()
                .id("completion-documentation")
                .max_w(px(420.))
                .max_h(place.row_height * place.rows as f32)
                .overflow_y_scroll()
                .px_2()
                .flex()
                .flex_col()
                .gap_1()
                .border_l_1()
                .border_color(theme.popover_border)
                .children(blocks)
        });
        let menu = div()
            .flex()
            .items_start()
            .bg(theme.popover_background)
            .border_1()
            .border_color(theme.popover_border)
            .rounded_sm()
            .text_color(theme.foreground)
            .text_size(theme.font_size)
            .font_family(theme.font_family.clone())
            .child(list)
            .children(docs);
        let menu = {
            // Records the bounds it's laid out at, for signature help to
            // keep clear of.
            let view = cx.weak_entity();
            menu.child(
                canvas(
                    move |bounds, _, cx| {
                        view.update(cx, |view, _| {
                            if let Some(menu) = &mut view.completion {
                                menu.laid_out = Some(bounds);
                            }
                            #[cfg(test)]
                            if let Some(painted) = &mut view.painted_completion {
                                painted.bounds = Some(bounds);
                            }
                        })
                        .ok();
                    },
                    |_, _, _, _| {},
                )
                .absolute()
                .size_full(),
            )
        };
        let corner = match place.above {
            true => gpui::Anchor::BottomLeft,
            false => gpui::Anchor::TopLeft,
        };
        Some(
            anchored()
                .anchor(corner)
                .position(place.at)
                .snap_to_window()
                .child(menu)
                .into_any_element(),
        )
    }
}

/// Where the view's menu is drawn: over everything else in the window,
/// built once the frame's layout has settled, as the hover popover is
/// ([`crate::hover::popover_layer`]).
pub(crate) fn menu_layer(cx: &mut Context<EditorView>) -> impl IntoElement {
    let view = cx.entity().downgrade();
    let layer = canvas(
        move |_, window, cx| {
            let mut menu = view
                .update(cx, |view, cx| view.render_completion(cx))
                .ok()??;
            menu.prepaint_as_root(
                Point::default(),
                window.viewport_size().map(AvailableSpace::Definite),
                window,
                cx,
            );
            Some(menu)
        },
        |_, menu, window, cx| {
            if let Some(mut menu) = menu {
                menu.paint(window, cx);
            }
        },
    );
    deferred(layer.absolute())
}

/// `stack` with its innermost editor frame's [`KEY_CONTEXT`] flag set as
/// `shown` says: keys are matched against the menu as it is when they
/// come, as vim's are ([`crate::vim::live_stack`]).
pub(crate) fn stamp(stack: ContextStack, shown: bool) -> ContextStack {
    let frames = stack.frames();
    let editor = (frames.iter()).rposition(|frame| frame.name == fe_editor::commands::CONTEXT);
    (frames.iter().enumerate())
        .map(|(ix, frame)| {
            let mut frame = frame.clone();
            if Some(ix) == editor {
                frame.flags.remove(KEY_CONTEXT);
                if shown {
                    frame.flags.insert(KEY_CONTEXT.to_owned());
                }
            }
            frame
        })
        .collect()
}
