//! Changing a file's end of line sequence, as VS Code's
//! `workbench.action.editor.changeEOL`: a click on the status bar's LF/CRLF
//! item, or the palette's Change End of Line Sequence…
//! (`workbench.change_line_ending`), lists LF and CRLF, the active
//! editor's current one marked. Picking one runs the editor's own
//! `editor.set_end_of_line_lf` or `editor.set_end_of_line_crlf`, which
//! convert the buffer's save ending as one undo step.

use fe_buffer::LineEnding;
use fe_command::Invocation;
use fe_gpui::{EditorView, commands::RunCommand};
use gpui::{
    AnyElement, App, AppContext as _, Context, Entity, Focusable as _, IntoElement as _,
    ParentElement as _, SharedString, Styled as _, WeakEntity, Window, div,
};
use kiln_l10n::{text, tr};
use kiln_shell::ModalId;

use crate::{
    Workbench,
    picker::{Picker, PickerDelegate},
};

/// The modal layer's id for the picker.
pub(crate) const LINE_ENDING_PICKER: ModalId = ModalId("line_ending");

/// The endings offered, in order: label, and the editor command setting it.
/// The labels are the endings' own names, as VS Code shows them.
const OFFERED: [(&str, LineEnding, &str); 2] = [
    ("LF", LineEnding::Lf, "editor.set_end_of_line_lf"),
    ("CRLF", LineEnding::CrLf, "editor.set_end_of_line_crlf"),
];

pub(crate) struct LineEndingPicker {
    editor: WeakEntity<EditorView>,
    current: LineEnding,
    matches: Vec<usize>,
}

impl LineEndingPicker {
    fn new(editor: &Entity<EditorView>, cx: &App) -> Self {
        Self {
            editor: editor.downgrade(),
            current: editor.read(cx).model().buffer().line_ending(),
            matches: (0..OFFERED.len()).collect(),
        }
    }

    fn focus_editor(&self, window: &mut Window, cx: &mut App) {
        if let Some(editor) = self.editor.upgrade() {
            window.focus(&editor.focus_handle(cx), cx);
        }
    }
}

impl PickerDelegate for LineEndingPicker {
    fn update_matches(&mut self, query: String, cx: &mut Context<Picker<Self>>) {
        let query = query.to_lowercase();
        self.matches = (0..OFFERED.len())
            .filter(|&row| OFFERED[row].0.to_lowercase().contains(&query))
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
        OFFERED[self.matches[ix]].0.into()
    }

    fn render_match(&self, ix: usize, _: bool, cx: &App) -> AnyElement {
        let (label, ending, _) = OFFERED[self.matches[ix]];
        let muted = fe_ui::ui_theme(cx).muted_text;
        let current = (ending == self.current).then(|| {
            div()
                .ml_2()
                .text_color(muted)
                .child(tr!("workbench-setup-current"))
        });
        div()
            .flex()
            .items_center()
            .child(label)
            .children(current)
            .into_any_element()
    }

    /// Converts the editor to the ending, then focuses it. If the editor
    /// has closed, a pick only dismisses the picker.
    fn confirm(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Picker<Self>>) -> bool {
        let command = OFFERED[self.matches[ix]].2;
        self.focus_editor(window, cx);
        if let Some(editor) = self.editor.upgrade() {
            let run = RunCommand(Invocation::of(command).expect("a valid command ID"));
            editor.focus_handle(cx).dispatch_action(&run, window, cx);
        }
        true
    }

    fn dismissed(&mut self, window: &mut Window, cx: &mut Context<Picker<Self>>) {
        self.focus_editor(window, cx);
    }
}

impl Workbench {
    /// Shows LF and CRLF for the active editor, its ending marked, in place
    /// of any palette. Nothing without an active editor, or for a read-only
    /// one, which the conversion would edit.
    pub(crate) fn change_line_ending(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(editor) = self.active_editor(cx).cloned() else {
            return;
        };
        if editor.read(cx).is_read_only() {
            return;
        }
        let delegate = LineEndingPicker::new(&editor, cx);
        self.dismiss_palette(window, cx);
        let picker = cx.new(|cx| {
            Picker::new(
                delegate,
                text!("workbench-setup-line-ending-placeholder"),
                fe_ui::ui_theme(cx),
                cx,
            )
        });
        picker.update(cx, |picker, cx| picker.show(window, cx));
        (self.shell().clone()).update(cx, |shell, cx| {
            shell.show_modal(LINE_ENDING_PICKER, picker, window, cx)
        });
    }
}

#[cfg(test)]
impl LineEndingPicker {
    /// The matches' labels, each with whether it's marked current.
    pub(crate) fn labels(&self) -> Vec<(&'static str, bool)> {
        (self.matches.iter())
            .map(|&row| (OFFERED[row].0, OFFERED[row].1 == self.current))
            .collect()
    }
}

#[cfg(test)]
impl Workbench {
    /// The line ending picker in the modal layer, if it's the modal showing.
    pub(crate) fn line_ending_picker(&self, cx: &App) -> Option<Entity<Picker<LineEndingPicker>>> {
        let layer = self.shell().read(cx).modals().read(cx);
        layer.modal().filter(|id| *id == LINE_ENDING_PICKER)?;
        layer.modal_view()?.clone().downcast().ok()
    }
}

#[cfg(test)]
#[path = "line_ending_picker_tests.rs"]
mod tests;
