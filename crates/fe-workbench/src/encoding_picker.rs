//! Changing a file's encoding, as VS Code's
//! `workbench.action.editor.changeEncoding`: a click on the status bar's
//! encoding item (`workbench.change_encoding`) lists two actions, Reopen
//! with Encoding and Save with Encoding, then the encodings, the active
//! editor's marked current. `workbench.reopen_with_encoding` and
//! `workbench.save_with_encoding` go straight to the encodings.
//!
//! Reopening reads the file again from disk in the encoding picked, as one
//! reload of the buffer ([`fe_editor::EditorModel::reload`]), each editor's
//! carets and selections mapped onto the new text ([`Remap`]). It's
//! refused while the buffer has unsaved changes, which it would lose, and
//! its read is dropped if the buffer changed while it ran.
//!
//! Saving writes the file in the encoding picked, which becomes the
//! buffer's once it's written ([`EditorView::save_in_encoding`]), unless
//! the text has a char that encoding can't represent: then nothing is
//! written, the encoding stays as it was, and a toast says which char, as
//! a save in the file's own encoding would (T249).
//!
//! One of these runs per buffer at a time ([`Running`]), and none starts
//! while the buffer is saving, so none sees another's encoding.
//!
//! The encodings are `encoding_rs`'s, which follow the WHATWG Encoding
//! Standard. Its ISO-8859-1 is windows-1252, so there's no row of its own.

use kiln_shell::picker::joined_label;
use std::{
    cell::RefCell,
    collections::{HashMap, HashSet},
    io,
    ops::Range,
    path::{Path, PathBuf},
    rc::Rc,
};

use fe_buffer::{
    Buffer, BufferId, DecodedFile, DiskStamp, Encoding, FileEncoding, ResolvedSelection, Text,
    Version,
};
use fe_gpui::EditorView;
use gpui::{
    AnyElement, App, AppContext as _, Context, Entity, Focusable as _, Global, IntoElement as _,
    ParentElement as _, SharedString, Styled as _, WeakEntity, Window, div,
};
use imara_diff::{Algorithm, Diff, InternedInput};
use kiln_l10n::{text, tr};
use kiln_shell::ModalId;

use crate::{
    Workbench, loader,
    picker::{Picker, PickerDelegate},
    workbench::toast::Toast,
};

/// The modal layer's id for the pickers.
pub(crate) const ENCODING_PICKER: ModalId = ModalId("encoding");

/// The command a click on the status bar's encoding item runs.
pub(crate) const CHANGE_ENCODING: &str = "workbench.change_encoding";

/// What the encoding picked is for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum EncodingAction {
    /// Read the file again in it.
    Reopen,
    /// Save the file in it.
    Save,
}

impl EncodingAction {
    /// The actions the status bar's item lists, in order.
    const ALL: [EncodingAction; 2] = [EncodingAction::Reopen, EncodingAction::Save];

    fn label(self) -> SharedString {
        match self {
            EncodingAction::Reopen => tr!("workbench-setup-encoding-reopen"),
            EncodingAction::Save => tr!("workbench-setup-encoding-save"),
        }
    }

    fn placeholder(self) -> kiln_l10n::Text {
        match self {
            EncodingAction::Reopen => text!("workbench-setup-encoding-reopen-placeholder"),
            EncodingAction::Save => text!("workbench-setup-encoding-save-placeholder"),
        }
    }
}

/// An encoding the picker offers: the WHATWG label `encoding_rs` knows it
/// by, and whether a save writes a BOM. [`EncodingChoice::label`] names it.
struct Offered {
    whatwg: &'static str,
    bom: bool,
}

const fn offered(whatwg: &'static str, bom: bool) -> Offered {
    Offered { whatwg, bom }
}

/// The encodings offered, in order. UTF-16 saves with a BOM, as VS Code's
/// does.
const OFFERED: [Offered; 21] = [
    offered("utf-8", false),
    offered("utf-8", true),
    offered("utf-16le", true),
    offered("utf-16be", true),
    offered("windows-1252", false),
    offered("iso-8859-15", false),
    offered("windows-1250", false),
    offered("iso-8859-2", false),
    offered("windows-1251", false),
    offered("koi8-r", false),
    offered("windows-1253", false),
    offered("iso-8859-7", false),
    offered("windows-1254", false),
    offered("windows-1255", false),
    offered("windows-1256", false),
    offered("shift_jis", false),
    offered("euc-jp", false),
    offered("gbk", false),
    offered("gb18030", false),
    offered("big5", false),
    offered("euc-kr", false),
];

/// One encoding the picker offers, as a choice.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct EncodingChoice {
    /// The WHATWG label it's offered by.
    whatwg: &'static str,
    pub(crate) encoding: FileEncoding,
}

impl EncodingChoice {
    /// What the picker lists it as.
    pub(crate) fn label(self) -> SharedString {
        match (self.whatwg, self.encoding.bom) {
            ("utf-8", false) => tr!("workbench-setup-encoding-utf-8"),
            ("utf-8", true) => tr!("workbench-setup-encoding-utf-8-bom"),
            ("utf-16le", true) => tr!("workbench-setup-encoding-utf-16le"),
            ("utf-16be", true) => tr!("workbench-setup-encoding-utf-16be"),
            ("windows-1252", false) => tr!("workbench-setup-encoding-windows-1252"),
            ("iso-8859-15", false) => tr!("workbench-setup-encoding-iso-8859-15"),
            ("windows-1250", false) => tr!("workbench-setup-encoding-windows-1250"),
            ("iso-8859-2", false) => tr!("workbench-setup-encoding-iso-8859-2"),
            ("windows-1251", false) => tr!("workbench-setup-encoding-windows-1251"),
            ("koi8-r", false) => tr!("workbench-setup-encoding-koi8-r"),
            ("windows-1253", false) => tr!("workbench-setup-encoding-windows-1253"),
            ("iso-8859-7", false) => tr!("workbench-setup-encoding-iso-8859-7"),
            ("windows-1254", false) => tr!("workbench-setup-encoding-windows-1254"),
            ("windows-1255", false) => tr!("workbench-setup-encoding-windows-1255"),
            ("windows-1256", false) => tr!("workbench-setup-encoding-windows-1256"),
            ("shift_jis", false) => tr!("workbench-setup-encoding-shift-jis"),
            ("euc-jp", false) => tr!("workbench-setup-encoding-euc-jp"),
            ("gbk", false) => tr!("workbench-setup-encoding-gbk"),
            ("gb18030", false) => tr!("workbench-setup-encoding-gb18030"),
            ("big5", false) => tr!("workbench-setup-encoding-big5"),
            ("euc-kr", false) => tr!("workbench-setup-encoding-euc-kr"),
            _ => self.encoding.label().into(),
        }
    }

    /// What a toast calls it: the encoding's label.
    fn name(self) -> String {
        self.encoding.label()
    }

    /// Whether it's `current`, the editor's encoding: UTF-16 with or
    /// without a BOM is UTF-16.
    fn is(self, current: FileEncoding) -> bool {
        let utf16 = self.encoding.name().starts_with("UTF-16");
        self.encoding.encoding == current.encoding && (utf16 || self.encoding.bom == current.bom)
    }
}

/// The encodings offered, in order.
pub(crate) fn choices() -> Vec<EncodingChoice> {
    OFFERED
        .iter()
        .map(|offered| EncodingChoice {
            whatwg: offered.whatwg,
            encoding: FileEncoding {
                encoding: Encoding::for_label(offered.whatwg.as_bytes())
                    .expect("encoding_rs knows the encodings offered"),
                bom: offered.bom,
            },
        })
        .collect()
}

/// The choice labelled `label`.
#[cfg(test)]
pub(crate) fn choice(label: &str) -> Option<EncodingChoice> {
    choices().into_iter().find(|choice| choice.label() == label)
}

/// A picker's rows: the two actions, or the encodings for one.
enum Rows {
    Actions,
    Encodings {
        action: EncodingAction,
        choices: Vec<EncodingChoice>,
        /// The editor's encoding when it opened.
        current: FileEncoding,
    },
}

pub(crate) struct EncodingPicker {
    rows: Rows,
    /// The rows matching the query, as indices.
    matches: Vec<usize>,
    /// The editor it's opened for.
    editor: WeakEntity<EditorView>,
    workbench: WeakEntity<Workbench>,
}

impl EncodingPicker {
    fn new(rows: Rows, editor: &Entity<EditorView>, workbench: WeakEntity<Workbench>) -> Self {
        let mut picker = Self {
            rows,
            matches: Vec::new(),
            editor: editor.downgrade(),
            workbench,
        };
        picker.matches = (0..picker.row_count()).collect();
        picker
    }

    fn row_count(&self) -> usize {
        match &self.rows {
            Rows::Actions => EncodingAction::ALL.len(),
            Rows::Encodings { choices, .. } => choices.len(),
        }
    }

    /// Row `row`'s label, and its WHATWG name for an encoding.
    fn row(&self, row: usize) -> (SharedString, Option<&'static str>) {
        match &self.rows {
            Rows::Actions => (EncodingAction::ALL[row].label(), None),
            Rows::Encodings { choices, .. } => {
                (choices[row].label(), Some(choices[row].encoding.name()))
            }
        }
    }

    /// Whether match `ix` is the editor's encoding, as it was on opening.
    fn is_current(&self, ix: usize) -> bool {
        match &self.rows {
            Rows::Actions => false,
            Rows::Encodings {
                choices, current, ..
            } => choices[self.matches[ix]].is(*current),
        }
    }

    /// Focuses the editor it's opened for, if it's still open.
    fn focus_editor(&self, window: &mut Window, cx: &mut App) {
        if let Some(editor) = self.editor.upgrade() {
            window.focus(&editor.focus_handle(cx), cx);
        }
    }
}

#[cfg(test)]
impl EncodingPicker {
    /// The matches' labels, each with whether it's marked current.
    pub(crate) fn labels(&self) -> Vec<(SharedString, bool)> {
        (0..self.matches.len())
            .map(|ix| (self.row(self.matches[ix]).0, self.is_current(ix)))
            .collect()
    }

    /// What the encodings listed are for; `None` while it lists the actions.
    pub(crate) fn action(&self) -> Option<EncodingAction> {
        match &self.rows {
            Rows::Actions => None,
            Rows::Encodings { action, .. } => Some(*action),
        }
    }
}

impl PickerDelegate for EncodingPicker {
    /// Rows whose label or encoding name holds `query`, ignoring case.
    fn update_matches(&mut self, query: String, cx: &mut Context<Picker<Self>>) {
        let query = query.to_lowercase();
        self.matches = (0..self.row_count())
            .filter(|&row| {
                let (label, name) = self.row(row);
                label.to_lowercase().contains(&query)
                    || name.is_some_and(|name| name.to_lowercase().contains(&query))
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
        self.row(self.matches[ix]).0
    }

    fn render_match(&self, ix: usize, _: bool, cx: &App) -> AnyElement {
        let (label, name) = self.row(self.matches[ix]);
        let muted = fe_ui::ui_theme(cx).muted_text;
        let name = name.map(|name| div().ml_2().text_color(muted).child(name));
        let current = (self.is_current(ix)).then(|| {
            div()
                .ml_2()
                .text_color(muted)
                .child(tr!("workbench-setup-current"))
        });
        div()
            .flex()
            .items_center()
            .child(label)
            .children(name)
            .children(current)
            .into_any_element()
    }

    /// The row's label, then current if it's the editor's encoding.
    fn match_label(&self, ix: usize, _: &App) -> SharedString {
        let (label, _) = self.row(self.matches[ix]);
        let current = self.is_current(ix).then(|| tr!("workbench-setup-current"));
        joined_label([Some(label), current].into_iter().flatten())
    }

    /// An action lists the encodings for it, in a picker of their own; an
    /// encoding reopens or saves the editor in it, then focuses the editor.
    /// If the editor has closed, a pick only dismisses the picker.
    fn confirm(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Picker<Self>>) -> bool {
        let row = self.matches[ix];
        let (editor, workbench) = (self.editor.clone(), self.workbench.clone());
        match &self.rows {
            Rows::Actions => {
                let action = EncodingAction::ALL[row];
                // Kept until the next picker shows in its place: the
                // layer dismisses it then, once it's no longer updating.
                window.defer(cx, move |window, cx| {
                    let Some(editor) = editor.upgrade() else {
                        return;
                    };
                    workbench
                        .update(cx, |workbench, cx| {
                            workbench.pick_encoding(action, &editor, window, cx)
                        })
                        .ok();
                });
                false
            }
            Rows::Encodings {
                action, choices, ..
            } => {
                let (action, choice) = (*action, choices[row]);
                self.focus_editor(window, cx);
                if let Some(editor) = editor.upgrade() {
                    workbench
                        .update(cx, |workbench, cx| match action {
                            EncodingAction::Reopen => {
                                workbench.reopen_with_encoding(&editor, choice, cx)
                            }
                            EncodingAction::Save => {
                                workbench.save_with_encoding(&editor, choice, window, cx)
                            }
                        })
                        .ok();
                }
                true
            }
        }
    }

    fn dismissed(&mut self, window: &mut Window, cx: &mut Context<Picker<Self>>) {
        self.focus_editor(window, cx);
    }
}

impl Workbench {
    /// Shows the two actions, Reopen with Encoding and Save with Encoding,
    /// for the active editor, in place of any palette. Nothing without an
    /// active editor.
    pub(crate) fn change_encoding(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(editor) = self.active_editor(cx).cloned() else {
            return;
        };
        let delegate = EncodingPicker::new(Rows::Actions, &editor, cx.entity().downgrade());
        let placeholder = text!("workbench-setup-encoding-actions-placeholder");
        self.show_encoding_picker(delegate, placeholder, window, cx);
    }

    /// Shows the encodings to `action` the active editor's file in, its
    /// encoding marked, in place of any palette. Nothing without an active
    /// editor.
    pub(crate) fn pick_encoding_for_active(
        &mut self,
        action: EncodingAction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(editor) = self.active_editor(cx).cloned() {
            self.pick_encoding(action, &editor, window, cx);
        }
    }

    /// Shows the encodings to `action` `editor`'s file in, its encoding
    /// marked, in place of any palette.
    fn pick_encoding(
        &mut self,
        action: EncodingAction,
        editor: &Entity<EditorView>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let current = editor.read(cx).model().buffer().encoding();
        let rows = Rows::Encodings {
            action,
            choices: choices(),
            current,
        };
        let delegate = EncodingPicker::new(rows, editor, cx.entity().downgrade());
        self.show_encoding_picker(delegate, action.placeholder(), window, cx);
    }

    fn show_encoding_picker(
        &mut self,
        delegate: EncodingPicker,
        placeholder: kiln_l10n::Text,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.dismiss_palette(window, cx);
        let picker = cx.new(|cx| Picker::new(delegate, placeholder, fe_ui::ui_theme(cx), cx));
        picker.update(cx, |picker, cx| picker.show(window, cx));
        (self.shell().clone()).update(cx, |shell, cx| {
            shell.show_modal(ENCODING_PICKER, picker, window, cx)
        });
    }

    /// Why an encoding operation on `editor`'s buffer can't start now: the
    /// buffer is saving, or another one is running for it.
    fn busy(&self, editor: &Entity<EditorView>, name: &str, cx: &App) -> Option<Toast> {
        let view = editor.read(cx);
        let buffer = view.model().buffer().id();
        if view.is_saving() {
            Some(
                Toast::info(tr!("workbench-setup-encoding-saving", name = name))
                    .detail(tr!("workbench-setup-encoding-saving-detail")),
            )
        } else if Running::holds(buffer, cx) {
            Some(
                Toast::info(tr!("workbench-setup-encoding-running", name = name))
                    .detail(tr!("workbench-setup-encoding-running-detail")),
            )
        } else {
            None
        }
    }

    /// Reads `editor`'s file again from disk, decoded in `choice`'s
    /// encoding, in place of its text, as one reload: each editor's carets
    /// and selections are mapped onto the new text ([`Remap`]). With
    /// unsaved changes, with no file, while saving or while another
    /// encoding operation runs for the buffer, it changes nothing and a
    /// toast says why. So it does if the read fails, or if the buffer's
    /// text, path, encoding or file changed while it ran.
    pub(crate) fn reopen_with_encoding(
        &mut self,
        editor: &Entity<EditorView>,
        choice: EncodingChoice,
        cx: &mut Context<Self>,
    ) {
        let model = editor.read(cx).model();
        let Some(path) = model.path().map(Path::to_owned) else {
            let toast = Toast::info(tr!("workbench-setup-encoding-reopen-no-file"));
            return self.show_toast(toast, cx);
        };
        let name = fe_gpui::pane_item::file_name(Some(&path));
        let (dirty, state) = {
            let buffer = model.buffer();
            (buffer.is_dirty(), BufferState::of(&buffer, &path))
        };
        if dirty {
            let toast = Toast::warning(tr!("workbench-setup-encoding-reopen-dirty", name = &name))
                .detail(tr!(
                    "workbench-setup-encoding-reopen-dirty-detail",
                    encoding = choice.name()
                ));
            return self.show_toast(toast, cx);
        }
        if let Some(toast) = self.busy(editor, &name, cx) {
            return self.show_toast(toast, cx);
        }
        let running = Running::start(state.buffer, cx);
        let encoding = choice.encoding;
        let read =
            (cx.background_executor()).spawn(async move { loader::read_with(&path, encoding) });
        let editor = editor.downgrade();
        cx.spawn(async move |workbench, cx| {
            let read = read.await;
            workbench
                .update(cx, |workbench, cx| {
                    drop(running);
                    let Some(editor) = editor.upgrade() else {
                        return;
                    };
                    let (file, stamp) = match read {
                        Ok(read) => read,
                        Err(error) => {
                            let toast = Toast::error(tr!(
                                "workbench-setup-encoding-reopen-failed",
                                name = &name
                            ))
                            .detail(error.to_string());
                            return workbench.show_toast(toast, cx);
                        }
                    };
                    let unchanged = {
                        let view = editor.read(cx);
                        let model = view.model();
                        let buffer = model.buffer();
                        !view.is_saving()
                            && !buffer.is_dirty()
                            && model
                                .path()
                                .is_some_and(|path| state == BufferState::of(&buffer, path))
                    };
                    if !unchanged {
                        let toast = Toast::warning(tr!(
                            "workbench-setup-encoding-reopen-changed",
                            name = &name
                        ))
                        .detail(tr!(
                            "workbench-setup-encoding-reopen-changed-detail",
                            encoding = choice.name()
                        ));
                        return workbench.show_toast(toast, cx);
                    }
                    workbench.reload_keeping_selections(&editor, file, stamp, cx);
                })
                .ok();
        })
        .detach();
    }

    /// Reloads `editor`'s buffer from `file`, mapping the carets and
    /// selections of every editor over the buffer, in every window, onto the new text row by
    /// row ([`Remap`]), rather than leaving those inside the text that
    /// changed at its edges.
    fn reload_keeping_selections(
        &mut self,
        editor: &Entity<EditorView>,
        file: DecodedFile,
        stamp: DiskStamp,
        cx: &mut Context<Self>,
    ) {
        let buffer = editor.read(cx).model().buffer().id();
        let others = Self::other_workbenches(cx);
        let mut editors: Vec<_> = (self.and_others(&others, cx))
            .flat_map(|workbench| workbench.file_editors(cx))
            .filter(|other| other.read(cx).model().buffer().id() == buffer)
            .collect();
        if !editors.contains(editor) {
            editors.push(editor.clone());
        }
        let before: Vec<_> = (editors.iter())
            .map(|editor| {
                editor.update(cx, |editor, _| {
                    editor.model_mut().sync_display();
                    // Vim's visual ends, which it paints the selection from.
                    // Also while a `/` or `?` command line suspends it.
                    let visual = editor.vim_state().and_then(|vim| {
                        let model = editor.model();
                        Some((vim.visual_anchor(model)?, vim.visual_cursor(model)?))
                    });
                    let top = editor.scroll_top();
                    let model = editor.model();
                    (model.text().clone(), model.selections(), visual, top)
                })
            })
            .collect();
        editor.update(cx, |editor, cx| {
            editor.model_mut().reload(file, Some(stamp));
            cx.notify();
        });
        for (editor, (old, selections, visual, top)) in editors.iter().zip(before) {
            editor.update(cx, |editor, cx| {
                let model = editor.model_mut();
                model.sync_display();
                let new = model.text().clone();
                let mut remap = Remap::new(&old, &new);
                let selections: Vec<_> = (selections.into_iter())
                    .map(|selection| ResolvedSelection {
                        range: remap.offset(selection.range.start)
                            ..remap.offset(selection.range.end),
                        reversed: selection.reversed,
                    })
                    .collect();
                model.set_selections(selections);
                if let Some((anchor, cursor)) = visual {
                    let (anchor, cursor) = (remap.offset(anchor), remap.offset(cursor));
                    editor.update_vim(|vim, model| vim.remap_visual(model, anchor, cursor), cx);
                }
                // The remap asks to reveal the caret; the scroll stays.
                editor.set_scroll_top(top, cx);
                cx.notify();
            });
        }
    }

    /// Saves `editor`'s buffer in `choice`'s encoding, which it saves in
    /// from then on. A char the encoding can't represent, or bytes the file
    /// had that became U+FFFD, refuse it: nothing is written, the encoding
    /// stays as it was, and a toast says why, as does any other failure of
    /// the save ([`EditorView::save_in_encoding`]). While the buffer saves, or
    /// another encoding operation runs for it, it changes nothing and a
    /// toast says why. An untitled buffer only takes the encoding, which
    /// its first save writes in, unless its text changed while it was
    /// checked.
    pub(crate) fn save_with_encoding(
        &mut self,
        editor: &Entity<EditorView>,
        choice: EncodingChoice,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let model = editor.read(cx).model();
        let name = fe_gpui::pane_item::file_name(model.path());
        let (buffer, snapshot) = {
            let buffer = model.buffer();
            (
                buffer.id(),
                buffer.save_snapshot().in_encoding(choice.encoding),
            )
        };
        let checked_version = snapshot.version();
        if let Some(toast) = self.busy(editor, &name, cx) {
            return self.show_toast(toast, cx);
        }
        let running = Running::start(buffer, cx);
        // Encoding the whole text takes a while for a large file.
        let checked =
            (cx.background_executor()).spawn(async move { snapshot.write_to(io::sink()) });
        let editor = editor.downgrade();
        cx.spawn_in(window, async move |workbench, cx| {
            // Held to the end: no other encoding operation starts meanwhile.
            let _running = running;
            let refused = |error: &dyn std::fmt::Display| {
                let message = tr!(
                    "workbench-setup-encoding-save-failed",
                    name = &name,
                    encoding = choice.name()
                );
                Toast::error(message).detail(error.to_string())
            };
            let show = |toast, cx: &mut gpui::AsyncWindowContext| {
                (workbench.update(cx, |workbench, cx| workbench.show_toast(toast, cx))).ok();
            };
            if let Err(error) = checked.await {
                return show(refused(&error), cx);
            }
            let Some(editor) = editor.upgrade() else {
                return;
            };
            let started = editor.update_in(cx, |editor, window, cx| {
                if editor.model().path().is_some() {
                    return Ok(Some(editor.save_in_encoding(choice.encoding, window, cx)));
                }
                // A Save As started meanwhile writes what it prepared, in
                // the encoding then, and only then gives the buffer a file.
                if editor.is_saving() {
                    return Err(Box::new(
                        Toast::info(tr!("workbench-setup-encoding-saving", name = &name))
                            .detail(tr!("workbench-setup-encoding-saving-detail")),
                    ));
                }
                // Nothing writes it now to check it again.
                if editor.model().buffer().version() != checked_version {
                    return Err(Box::new(
                        Toast::info(tr!("workbench-setup-encoding-save-changed", name = &name))
                            .detail(tr!(
                                "workbench-setup-encoding-save-changed-detail",
                                encoding = choice.name()
                            )),
                    ));
                }
                (editor.model_mut().buffer_mut()).set_encoding(choice.encoding);
                cx.notify();
                Ok(None)
            });
            let saved = match started {
                Ok(Ok(Some(saved))) => saved,
                Ok(Err(toast)) => return show(*toast, cx),
                Ok(Ok(None)) | Err(_) => return,
            };
            if let Err(error) = saved.await {
                show(refused(&format!("{error:#}")), cx);
            }
        })
        .detach();
    }

    fn show_toast(&mut self, toast: Toast, cx: &mut Context<Self>) {
        (self.toasts.clone()).update(cx, |toasts, cx| {
            toasts.push(toast, cx);
        });
    }
}

/// The buffers an encoding operation runs for, each until its
/// [`RunningGuard`] drops: one at a time per buffer.
#[derive(Clone, Default)]
struct Running(Rc<RefCell<HashSet<BufferId>>>);

impl Global for Running {}

impl Running {
    fn holds(buffer: BufferId, cx: &App) -> bool {
        cx.try_global::<Running>()
            .is_some_and(|running| running.0.borrow().contains(&buffer))
    }

    /// Notes an operation running for `buffer`, until the guard drops.
    fn start(buffer: BufferId, cx: &mut App) -> RunningGuard {
        let running = cx.default_global::<Running>().clone();
        running.0.borrow_mut().insert(buffer);
        RunningGuard { running, buffer }
    }
}

struct RunningGuard {
    running: Running,
    buffer: BufferId,
}

impl Drop for RunningGuard {
    fn drop(&mut self) {
        self.running.0.borrow_mut().remove(&self.buffer);
    }
}

/// What a reopen's read must still match to be applied: the buffer, its
/// text's version, its file and encoding, and the file's stamp.
#[derive(Clone, Debug, PartialEq)]
struct BufferState {
    buffer: BufferId,
    version: Version,
    path: PathBuf,
    encoding: FileEncoding,
    stamp: Option<DiskStamp>,
}

impl BufferState {
    fn of(buffer: &Buffer, path: &Path) -> Self {
        Self {
            buffer: buffer.id(),
            version: buffer.version(),
            path: path.to_owned(),
            encoding: buffer.encoding(),
            stamp: buffer.disk_stamp(),
        }
    }
}

/// How many chars of each side one char diff takes in: rows are diffed a
/// window at a time, so a long row costs time in proportion to its length.
const DIFF_WINDOW: usize = 4 * 1024;

/// Where `offset` in `old` lands in `new`: see [`Remap`].
#[cfg(test)]
pub(crate) fn map_offset(old: &Text, new: &Text, offset: usize) -> usize {
    Remap::new(old, new).offset(offset)
}

/// Maps offsets in `old` to `new`, the same file decoded another way: each
/// to the same row, at the column [`RowRemap::column`] takes it to. A row
/// `new` lacks clips to its end. Each row is diffed once, however many
/// offsets on it are mapped.
struct Remap<'a> {
    old: &'a Text,
    new: &'a Text,
    rows: HashMap<u32, RowRemap>,
}

impl<'a> Remap<'a> {
    fn new(old: &'a Text, new: &'a Text) -> Self {
        Self {
            old,
            new,
            rows: HashMap::new(),
        }
    }

    fn offset(&mut self, offset: usize) -> usize {
        let (old, new) = (self.old, self.new);
        let row = old.offset_to_point(offset).row;
        if row >= new.row_count() {
            return new.len();
        }
        let column = old.offset_to_char(offset) - old.offset_to_char(old.row_start(row));
        let remap = (self.rows.entry(row))
            .or_insert_with(|| RowRemap::new(&old.row_to_string(row), &new.row_to_string(row)));
        let column = remap.column(column);
        let within: usize = remap.new[..column]
            .iter()
            .copied()
            .map(char::len_utf8)
            .sum();
        new.row_start(row) + within
    }
}

/// One row's old and new text, and the runs they share ([`shared_runs`]).
struct RowRemap {
    old_len: usize,
    new: Vec<char>,
    runs: Vec<(Range<usize>, usize)>,
}

impl RowRemap {
    fn new(old: &str, new: &str) -> Self {
        let (old, new): (Vec<char>, Vec<char>) = (old.chars().collect(), new.chars().collect());
        let runs = shared_runs(&old, &new);
        Self {
            old_len: old.len(),
            new,
            runs,
        }
    }

    /// Where char column `column` of the old row lands in the new. A column
    /// in text both have keeps its place: at the start of a run, or at the
    /// row's end, it goes past any text new before it. One in a run that
    /// decoded differently goes as far into the new run, at most to its end.
    fn column(&self, column: usize) -> usize {
        let column = column.min(self.old_len);
        // The end of the last shared run before the column, in both.
        let (mut old_end, mut new_end) = (0, 0);
        for (run, new_start) in &self.runs {
            if column < run.start {
                // In the changed run between that one and this.
                return new_end + (column - old_end).min(new_start - new_end);
            }
            if column < run.end {
                return new_start + (column - run.start);
            }
            (old_end, new_end) = (run.end, new_start + run.len());
        }
        if column == self.old_len {
            return self.new.len();
        }
        new_end + (column - old_end).min(self.new.len() - new_end)
    }
}

/// The runs of chars `old` and `new` share, in order: each an old range and
/// where it starts in `new`. What they share at either end is a run of its
/// own; between, a char diff ([`imara_diff`]) finds them a window of
/// [`DIFF_WINDOW`] chars at a time, each window starting where what the one
/// before shared in its first half ended, or at its first shared run.
fn shared_runs(old: &[char], new: &[char]) -> Vec<(Range<usize>, usize)> {
    let prefix = old.iter().zip(new).take_while(|(a, b)| a == b).count();
    let suffix = (old[prefix..].iter().rev())
        .zip(new[prefix..].iter().rev())
        .take_while(|(a, b)| a == b)
        .count();
    let (old_end, new_end) = (old.len() - suffix, new.len() - suffix);
    let mut runs = Vec::new();
    if prefix > 0 {
        runs.push((0..prefix, 0));
    }
    let (mut a, mut b) = (prefix, prefix);
    while a < old_end && b < new_end {
        let last = old_end - a <= DIFF_WINDOW && new_end - b <= DIFF_WINDOW;
        let (to_a, to_b) = (old_end.min(a + DIFF_WINDOW), new_end.min(b + DIFF_WINDOW));
        let mut input = InternedInput::default();
        input.update_before(old[a..to_a].iter().copied());
        input.update_after(new[b..to_b].iter().copied());
        let mut diff = Diff::compute(Algorithm::Histogram, &input);
        // A change that could sit anywhere in a run of one repeated char
        // goes to its end: one near the window's end is left to the next.
        diff.postprocess_no_heuristic(&input);
        // The shared runs between the window's hunks, and after the last.
        let mut found = Vec::new();
        let (mut at_a, mut at_b) = (0, 0);
        for hunk in diff.hunks().chain([imara_diff::Hunk {
            before: (to_a - a) as u32..(to_a - a) as u32,
            after: (to_b - b) as u32..(to_b - b) as u32,
        }]) {
            let before = hunk.before.start as usize;
            if before > at_a {
                found.push((a + at_a..a + before, b + at_b));
            }
            (at_a, at_b) = (hunk.before.end as usize, hunk.after.end as usize);
        }
        if last {
            runs.extend(found);
            break;
        }
        // Runs near the window's end may pair up differently with what's
        // past it: keep what's in the first half on both sides, a run
        // crossing it cut there, and go on from where the last kept ends.
        let (half_a, half_b) = (a + DIFF_WINDOW / 2, b + DIFF_WINDOW / 2);
        let first = found.first().cloned();
        let kept: Vec<_> = (found.into_iter())
            .filter(|(run, new_start)| run.start < half_a && *new_start < half_b)
            .map(|(run, new_start)| {
                let len = run.len().min(half_a - run.start).min(half_b - new_start);
                (run.start..run.start + len, new_start)
            })
            .collect();
        match (kept.last(), first) {
            (Some((run, new_start)), _) => (a, b) = (run.end, new_start + run.len()),
            // Nothing shared in the first half: on from the first shared run.
            (None, Some((run, new_start))) => (a, b) = (run.start, new_start),
            // Nothing shared in the window at all: past it, as changed text.
            (None, None) => (a, b) = (to_a, to_b),
        }
        runs.extend(kept);
    }
    if suffix > 0 {
        runs.push((old_end..old.len(), new_end));
    }
    runs
}

#[cfg(test)]
impl Workbench {
    /// The encoding picker in the modal layer, if it's the modal showing.
    pub(crate) fn encoding_picker(&self, cx: &App) -> Option<Entity<Picker<EncodingPicker>>> {
        let layer = self.shell().read(cx).modals().read(cx);
        layer.modal().filter(|id| *id == ENCODING_PICKER)?;
        layer.modal_view()?.clone().downcast().ok()
    }
}

#[cfg(test)]
#[path = "encoding_picker_tests.rs"]
mod tests;
