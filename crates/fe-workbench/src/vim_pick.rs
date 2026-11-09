//! The list a vim command leaves to pick from ([`fe_vim::Pick`], `:marks`)
//! as a picker in the modal layer: its title the placeholder, its header
//! the status line, each row its cells in aligned columns. The query keeps
//! the rows whose cells, joined by spaces, hold it as a case-insensitive
//! subsequence. Choosing a row focuses the editor the line was typed in and
//! runs the row's [`fe_vim::PickThen`] there; dismissing it focuses the
//! editor.

use fe_gpui::{EditorView, commands::RunCommand};
use gpui::{
    AnyElement, App, AppContext as _, Context, Entity, Focusable as _, IntoElement as _,
    ParentElement as _, SharedString, Styled as _, WeakEntity, Window, div, px,
};
use kiln_shell::ModalId;

use super::Workbench;
use crate::{
    picker::{Picker, PickerDelegate},
    toolbar_picker::subsequence,
};

/// The modal layer's id for the picker.
pub(super) const VIM_PICK: ModalId = ModalId("vim_pick");

/// Width of a character in the columns, in pixels.
const CHAR_WIDTH: f32 = 8.;

pub(crate) struct VimPicker {
    pick: fe_vim::Pick,
    /// The editor the line was typed in.
    editor: WeakEntity<EditorView>,
    /// The rows matching the query, as indices into `pick.rows`.
    matches: Vec<usize>,
    /// The width of each column, in characters.
    widths: Vec<usize>,
}

impl VimPicker {
    fn new(pick: fe_vim::Pick, editor: WeakEntity<EditorView>) -> Self {
        let columns = pick.rows.iter().map(|row| row.cells.len()).max();
        let widths = (0..columns.unwrap_or(0))
            .map(|column| {
                let cell = |row: &fe_vim::PickRow| row.cells.get(column).map(|c| c.chars().count());
                pick.rows.iter().filter_map(cell).max().unwrap_or(0)
            })
            .collect();
        Self {
            matches: (0..pick.rows.len()).collect(),
            pick,
            editor,
            widths,
        }
    }

    /// Focuses the editor, if it's still open.
    fn focus_editor(&self, window: &mut Window, cx: &mut App) -> Option<Entity<EditorView>> {
        let editor = self.editor.upgrade()?;
        window.focus(&editor.focus_handle(cx), cx);
        Some(editor)
    }
}

impl PickerDelegate for VimPicker {
    fn update_matches(&mut self, query: String, cx: &mut Context<Picker<Self>>) {
        let query: Vec<char> = query.chars().flat_map(char::to_lowercase).collect();
        self.matches = (self.pick.rows.iter().enumerate())
            .filter(|(_, row)| subsequence(&query, &row.cells.join(" ")).is_some())
            .map(|(ix, _)| ix)
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
        self.matches[ix].to_string().into()
    }

    fn render_match(&self, ix: usize, _: bool, cx: &App) -> AnyElement {
        let row = &self.pick.rows[self.matches[ix]];
        let last = row.cells.len().saturating_sub(1);
        let cells = row.cells.iter().enumerate().map(|(column, cell)| {
            let cell = div().child(cell.clone());
            match column == last {
                true => cell
                    .flex_1()
                    .min_w_0()
                    .overflow_hidden()
                    .whitespace_nowrap(),
                false => cell
                    .flex_none()
                    .w(px(CHAR_WIDTH * self.widths[column] as f32)),
            }
        });
        div()
            .flex()
            .flex_1()
            .min_w_0()
            .gap_2()
            .text_color(fe_ui::ui_theme(cx).text)
            .children(cells)
            .into_any_element()
    }

    /// Focuses the editor and runs the row's `then` in it. An editor closed
    /// by now only dismisses.
    fn confirm(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Picker<Self>>) -> bool {
        let then = self.pick.rows[self.matches[ix]].then.clone();
        if let Some(editor) = self.focus_editor(window, cx) {
            let focus = editor.focus_handle(cx);
            focus.dispatch_action(&RunCommand(then.invocation()), window, cx);
        }
        true
    }

    fn dismissed(&mut self, window: &mut Window, cx: &mut Context<Picker<Self>>) {
        self.focus_editor(window, cx);
    }

    fn rows_fill_width(&self) -> bool {
        true
    }

    fn status(&self) -> Option<SharedString> {
        Some(self.pick.header.clone().into())
    }
}

impl Workbench {
    /// Shows `pick`, typed in `editor`, as a focused picker in the modal
    /// layer, in place of any palette.
    pub(super) fn show_vim_pick(
        &mut self,
        editor: &Entity<EditorView>,
        pick: fe_vim::Pick,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.dismiss_palette(window, cx);
        let title = pick.title.clone();
        let delegate = VimPicker::new(pick, editor.downgrade());
        let picker = cx.new(|cx| Picker::new(delegate, &title, fe_ui::ui_theme(cx), cx));
        picker.update(cx, |picker, cx| picker.show(window, cx));
        self.modals.update(cx, |layer, cx| {
            layer.show_modal(VIM_PICK, picker, window, cx)
        });
    }
}

#[cfg(test)]
impl VimPicker {
    /// The matching rows' cells.
    pub(crate) fn rows(&self) -> Vec<Vec<String>> {
        let row = |ix: &usize| self.pick.rows[*ix].cells.clone();
        self.matches.iter().map(row).collect()
    }
}

#[cfg(test)]
impl Workbench {
    /// The vim picker in the modal layer, if it's the modal showing.
    pub(crate) fn vim_pick(&self, cx: &App) -> Option<Entity<Picker<VimPicker>>> {
        let layer = self.modals.read(cx);
        let shown = layer.modal().filter(|id| *id == VIM_PICK)?;
        _ = shown;
        layer.modal_view()?.clone().downcast().ok()
    }
}
