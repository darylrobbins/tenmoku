//! Go to Symbol in File (`nav.document_symbols`, and quick open's `@`): a
//! [`Picker`] over the active editor's outline, the one the Outline panel and
//! the breadcrumbs show ([`OutlinePanel::shown_for`]), so the language
//! server's symbols where it gave some and tree-sitter's otherwise.
//!
//! Rows match the query by subsequence, ignoring case, in outline order.
//! Moving the selection scrolls the editor to the symbol, opening a fold that
//! hides it until the selection moves on, and leaves its caret alone;
//! dismissing puts the scroll and the folds back. Opening scrolls nothing, as
//! the first row isn't one moved to. A click elsewhere, which hides the picker
//! without dismissing it, leaves them: that click may be in the editor, on
//! what the preview shows.
//!
//! A pick is checked against the outline it was listed from, as the toolbar
//! picker's are: if the outline changed since, or is behind the editor's
//! text, the list is made again and the pick refused, so it never lands on a
//! moved symbol. A list with no row to pick has no pick to refuse, so it's
//! made again as soon as another outline shows.

use std::ops::{Range, RangeInclusive};

use fe_buffer::{ResolvedSelection, Version};
use fe_gpui::EditorView;
use fe_syntax::{MAX_OUTLINE_ITEMS, OutlineItem};
use fe_ui::{HighlightedLabel, char_ranges};
use gpui::{
    AnyElement, App, AppContext as _, Context, Entity, FocusHandle, Focusable as _,
    InteractiveElement as _, IntoElement as _, ParentElement as _, SharedString, Styled as _,
    Subscription, WeakEntity, Window, div, px, svg,
};
use kiln_l10n::{text, tr};

use crate::{
    OutlinePanel, Workbench,
    outline_panel::symbol_icon,
    picker::{Picker, PickerDelegate},
    toolbar_picker::subsequence,
    workbench::Palette,
};

/// Between the names of a symbol's parents.
const CHAIN_SEPARATOR: &str = " › ";

/// The folds before a preview opened some, and what says they still apply.
struct FoldsBefore {
    folds: Vec<RangeInclusive<u32>>,
    /// The text they're rows of.
    version: Version,
    /// None of them hid a caret.
    selections: Vec<ResolvedSelection>,
}

pub(crate) struct SymbolPicker {
    /// Records the caret before a pick jumps.
    workbench: WeakEntity<Workbench>,
    outline: WeakEntity<OutlinePanel>,
    /// The active editor as it opened, if any.
    editor: Option<WeakEntity<EditorView>>,
    /// The outline's generation `items` are listed from: `None` if no outline
    /// showed for the editor then.
    generation: Option<u64>,
    /// The editor can have an outline, and the first is still to land.
    parsing: bool,
    /// The outline stopped at its cap.
    truncated: bool,
    items: Vec<OutlineItem>,
    query: String,
    /// The items matching the query, with the byte ranges matched.
    matches: Vec<(usize, Vec<Range<usize>>)>,
    /// Moves on with each list, so a row pressed in one can't pick from the
    /// next.
    listing: u64,
    /// The item last scrolled to, or the one not to scroll to: the first
    /// row of a list that only just opened.
    previewed: Option<usize>,
    /// The editor's scroll before the first preview, to go back to.
    scroll_before: Option<f32>,
    /// What a preview that opened folds found.
    folds_before: Option<FoldsBefore>,
    /// Focused before it opened, and again once it's dismissed.
    previous_focus: Option<FocusHandle>,
    _follows: Vec<Subscription>,
}

impl SymbolPicker {
    fn new(
        workbench: WeakEntity<Workbench>,
        outline: WeakEntity<OutlinePanel>,
        editor: Option<WeakEntity<EditorView>>,
        previous_focus: Option<FocusHandle>,
    ) -> Self {
        Self {
            workbench,
            outline,
            editor,
            generation: None,
            parsing: false,
            truncated: false,
            items: Vec::new(),
            query: String::new(),
            matches: Vec::new(),
            listing: 0,
            previewed: None,
            scroll_before: None,
            folds_before: None,
            previous_focus,
            _follows: Vec::new(),
        }
    }

    /// Lists the outline, and follows the picker's selection and the outline
    /// from now on.
    fn follow(&mut self, outline: &Entity<OutlinePanel>, cx: &mut Context<Picker<Self>>) {
        self.list(cx);
        self._follows = vec![
            cx.observe(outline, |picker, _, cx| picker.delegate.outline_changed(cx)),
            // `Picker::select` is the only way the selection moves, and it
            // notifies; the delegate has no hook for it.
            cx.observe_self(|picker, cx| {
                let selected = picker.selected();
                picker.delegate.preview(selected, cx);
            }),
        ];
    }

    /// Makes the list from the outline now shown for the editor.
    fn list(&mut self, cx: &App) {
        self.listing += 1;
        self.matches.clear();
        self.previewed = self.query.is_empty().then_some(0);
        self.generation = None;
        self.parsing = false;
        self.truncated = false;
        self.items.clear();
        let id = self.editor.as_ref().map(|editor| editor.entity_id());
        let outline = self.outline.upgrade();
        let followed = outline
            .as_ref()
            .zip(id)
            .and_then(|(outline, id)| outline.read(cx).shown_for(id));
        match followed {
            Some(Some((outline, generation))) => {
                self.generation = Some(generation);
                self.truncated = outline.truncated;
                self.items.clone_from(&outline.items);
            }
            Some(None) => self.parsing = true,
            None => {}
        }
    }

    /// The generation of the outline now shown for the editor, if any.
    fn shown_generation(&self, cx: &App) -> Option<u64> {
        let id = self.editor.as_ref()?.entity_id();
        let outline = self.outline.upgrade()?;
        let shown = outline.read(cx).shown_for(id)??;
        Some(shown.1)
    }

    /// The outline changed: a list with a row to pick stands, for a pick to
    /// find it stale. One without is made again.
    fn outline_changed(&mut self, cx: &mut Context<Picker<Self>>) {
        if self.generation.is_some()
            && (!self.matches.is_empty() || self.shown_generation(cx) == self.generation)
        {
            return;
        }
        let was = self.parsing;
        let listed = self.generation.is_some();
        self.list(cx);
        if self.generation.is_some() {
            self.refilter(cx);
        } else if listed || self.parsing != was {
            cx.notify();
        }
    }

    /// Matches the items against the query, or those of the outline now shown
    /// if another shows and they have no match, then selects the first match.
    /// The selection moves last: until then it may point at another row of
    /// these matches, which mustn't be previewed.
    fn refilter(&mut self, cx: &mut Context<Picker<Self>>) {
        self.filter();
        if self.matches.is_empty() && self.shown_generation(cx) != self.generation {
            self.list(cx);
            self.filter();
        }
        let picker = cx.entity();
        // The picker is being updated now.
        cx.defer(move |cx| picker.update(cx, |picker, cx| picker.matches_updated(true, cx)));
    }

    fn filter(&mut self) {
        let query: Vec<char> = self.query.chars().flat_map(char::to_lowercase).collect();
        self.matches = (self.items.iter().enumerate())
            .filter_map(|(ix, item)| {
                let indices = subsequence(&query, &item.name)?;
                Some((ix, char_ranges(&item.name, &indices)))
            })
            .collect();
    }

    /// Scrolls the editor to the symbol of match `ix` if it hasn't been yet,
    /// a third of the way down the view, opening a fold that hides it and
    /// closing what the last preview opened. The caret stays where it is.
    fn preview(&mut self, ix: usize, cx: &mut App) {
        let Some(&(item, _)) = self.matches.get(ix) else {
            return;
        };
        if self.previewed.replace(item) == Some(item) {
            return;
        }
        let Some(editor) = self.editor.as_ref().and_then(WeakEntity::upgrade) else {
            return;
        };
        let row = self.items[item].row;
        editor.update(cx, |editor, cx| {
            // No frame yet: nothing to scroll from.
            let Some((_, rows)) = editor.viewport_rows() else {
                return;
            };
            self.scroll_before.get_or_insert(editor.scroll_top());
            self.close_folds(editor);
            if editor.model().display().hidden_rows(row).is_some() {
                let model = editor.model();
                let folds = model.display().folds();
                self.folds_before = Some(FoldsBefore {
                    folds: folds.iter().map(|fold| fold.start..=fold.end).collect(),
                    version: model.snapshot().version(),
                    selections: model.selections(),
                });
                editor.model_mut().unfold_rows(row..=row);
            }
            // The rows above it are screen rows: a row of the text may wrap
            // to more than the view holds.
            editor.scroll_to_buffer_line(row as f32, cx);
            let top = editor.scroll_top() - (rows / 3) as f32;
            editor.set_scroll_top(top, cx);
        });
    }

    /// Closes the folds a preview opened, unless the text or a caret moved
    /// since: their rows may no longer say which they were, or may now hide
    /// the caret. The selections are left as they are.
    fn close_folds(&mut self, editor: &mut EditorView) {
        let Some(before) = self.folds_before.take() else {
            return;
        };
        let model = editor.model();
        if model.snapshot().version() == before.version && model.selections() == before.selections {
            editor.model_mut().restore_folds(before.folds);
        }
    }

    /// The names of `item`'s parents, from the top level.
    fn chain(&self, item: &OutlineItem) -> String {
        let parents = std::iter::successors(item.parent, |&parent| {
            self.items.get(parent as usize)?.parent
        });
        let mut names: Vec<&str> = parents
            // Its depth bounds the walk, whatever a server's outline says.
            .take(item.depth as usize)
            .filter_map(|parent| Some(self.items.get(parent as usize)?.name.as_str()))
            .collect();
        names.reverse();
        names.join(CHAIN_SEPARATOR)
    }

    /// A pick is a jump: back returns to the caret as it is now. A running
    /// go-to-definition is dropped first, as `nav_jump` does, so its answer
    /// can't land over the pick.
    fn push_history(&self, cx: &mut Context<Picker<Self>>) {
        if let Some(workbench) = self.workbench.upgrade() {
            workbench.update(cx, |workbench, cx| {
                workbench.nav_state.goto.next();
                workbench.push_history(cx)
            });
        }
    }
}

#[cfg(test)]
impl SymbolPicker {
    /// The matching rows' names.
    pub(crate) fn row_names(&self) -> Vec<String> {
        let rows = self.matches.iter();
        rows.map(|(item, _)| self.items[*item].name.clone())
            .collect()
    }
}

impl PickerDelegate for SymbolPicker {
    fn update_matches(&mut self, query: String, cx: &mut Context<Picker<Self>>) {
        self.query = query;
        self.refilter(cx);
    }

    fn match_count(&self) -> usize {
        self.matches.len()
    }

    fn match_id(&self, ix: usize) -> SharedString {
        format!("{}:{}", self.listing, self.matches[ix].0).into()
    }

    fn rows_fill_width(&self) -> bool {
        true
    }

    fn render_match(&self, ix: usize, _: bool, cx: &App) -> AnyElement {
        let ui = fe_ui::ui_theme(cx);
        let (item, highlights) = &self.matches[ix];
        let item = &self.items[*item];
        let label = HighlightedLabel::new(item.name.clone(), highlights.clone(), &ui);
        let path = symbol_icon(item.kind).path();
        let muted = |text: String| {
            div()
                .min_w_0()
                .overflow_hidden()
                .whitespace_nowrap()
                .text_color(ui.muted_text)
                .child(text)
        };
        div()
            .flex()
            .flex_1()
            .min_w_0()
            .items_center()
            .gap_2()
            .child(
                svg()
                    .debug_selector(|| format!("symbol-picker-icon:{}:{path}", item.name))
                    .path(path)
                    .size(px(14.))
                    .flex_none()
                    .text_color(ui.icon),
            )
            .child(div().flex_none().child(label))
            .child(muted(item.detail.clone()))
            .child(muted(self.chain(item)).ml_auto().flex_none())
            .into_any_element()
    }

    /// The symbol's name, then its detail and what holds it, as shown.
    fn match_label(&self, ix: usize, _: &App) -> SharedString {
        let item = &self.items[self.matches[ix].0];
        let chain = self.chain(item);
        let parts = [item.name.as_str(), item.detail.as_str(), chain.as_str()];
        let parts: Vec<&str> = (parts.into_iter())
            .filter(|part| !part.is_empty())
            .collect();
        parts.join(", ").into()
    }

    /// Selects the symbol's name and focuses its editor, unless the outline
    /// changed since it was listed, or is behind the editor's text (edited
    /// since, its outline still to land): then it's listed again and the pick
    /// refused.
    fn confirm(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Picker<Self>>) -> bool {
        let Some(&(item, _)) = self.matches.get(ix) else {
            return false;
        };
        // Its editor closed meanwhile: nothing to select in.
        let Some(editor) = self.editor.as_ref().and_then(WeakEntity::upgrade) else {
            return true;
        };
        let version = editor.read(cx).model().snapshot().version();
        let current = self.outline.upgrade().and_then(|outline| {
            let shown = outline.read(cx).shown_for(editor.entity_id()).flatten();
            shown
                .filter(|(outline, _)| outline.version == version)
                .map(|(_, generation)| generation)
        });
        if current.is_none() || current != self.generation {
            self.list(cx);
            self.refilter(cx);
            return false;
        }
        let name = self.items[item].name_range.clone();
        self.push_history(cx);
        editor.update(cx, |editor, cx| {
            // Selecting it opens the folds around it, and no others.
            self.close_folds(editor);
            editor.select_and_reveal(name, cx)
        });
        window.focus(&editor.focus_handle(cx), cx);
        // The preview's scroll stands: the name is in view there, or the
        // reveal brings it.
        self.scroll_before = None;
        self.previous_focus = None;
        true
    }

    fn dismissed(&mut self, window: &mut Window, cx: &mut Context<Picker<Self>>) {
        let editor = self.editor.as_ref().and_then(WeakEntity::upgrade);
        if let (Some(top), Some(editor)) = (self.scroll_before.take(), editor) {
            editor.update(cx, |editor, cx| {
                // Folds first: the scroll is in rows as they were folded.
                self.close_folds(editor);
                editor.set_scroll_top(top, cx)
            });
        }
        if let Some(focus) = self.previous_focus.take() {
            window.focus(&focus, cx);
        }
    }

    fn status(&self) -> Option<SharedString> {
        if self.parsing {
            Some(tr!("workbench-navigation-symbols-parsing"))
        } else if self.items.is_empty() {
            Some(tr!("workbench-navigation-symbols-none"))
        } else if self.matches.is_empty() && !self.query.is_empty() {
            Some(tr!("workbench-navigation-symbols-no-matches"))
        } else if self.truncated {
            Some(tr!(
                "workbench-navigation-symbols-truncated",
                count = MAX_OUTLINE_ITEMS
            ))
        } else {
            None
        }
    }
}

impl Workbench {
    /// Shows the symbol picker, focused, for the active editor, in place of
    /// any other palette, or dismisses it.
    pub(crate) fn toggle_document_symbols(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.palette == Some(Palette::DocumentSymbols) {
            return self.dismiss_palette(window, cx);
        }
        self.show_document_symbols("", window, cx);
    }

    /// Shows the symbol picker, focused, for the active editor, in place of
    /// any other palette, with `query` as its input's text. With no editor,
    /// it says it has no symbols.
    pub(crate) fn show_document_symbols(
        &mut self,
        query: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.dismiss_palette(window, cx);
        // Quick open handing over has nothing to go back to if nothing was
        // focused before it: its input is hidden now.
        let handing_over = self.quick_open().focus_handle(cx);
        let previous = window
            .focused(cx)
            .filter(|focused| *focused != handing_over);
        let editor = self.active_editor(cx).map(Entity::downgrade);
        let outline = self.dock().read(cx).outline().clone();
        let delegate = SymbolPicker::new(
            cx.entity().downgrade(),
            outline.downgrade(),
            editor,
            previous,
        );
        let placeholder = text!("workbench-navigation-file-symbols-placeholder");
        let picker = cx.new(|cx| Picker::new(delegate, placeholder, fe_ui::ui_theme(cx), cx));
        picker.update(cx, |picker, cx| {
            picker.delegate.follow(&outline, cx);
            picker.show(window, cx);
            if !query.is_empty() {
                // Setting the text doesn't say it changed.
                let input = picker.input().clone();
                input.update(cx, |input, cx| input.set_text(query, cx));
                picker.delegate.update_matches(query.to_owned(), cx);
            }
        });
        self.show_palette(Palette::DocumentSymbols, picker, window, cx);
    }
}

#[cfg(test)]
#[path = "symbol_picker_tests.rs"]
mod tests;
