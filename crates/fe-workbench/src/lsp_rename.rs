//! Renaming the symbol at the caret through its language server
//! (`lsp.rename`).
//!
//! - **Target.** The server is asked what a rename at the active editor's
//!   primary caret would rename ([`LspManager::prepare_rename`]). Nothing
//!   there, a server's refusal or no server at all is an info prompt.
//! - **Name.** A [`RenamePicker`] shows as [`Palette::Rename`], its input
//!   holding the current name as if all selected: the first edit replaces
//!   it. Its one row, `Rename to <name>`, shows while the name is new.
//! - **Rename.** Confirming asks for the edits at the version the picker
//!   opened on, then applies them ([`Workbench::apply_file_edits`]). A
//!   buffer that changed since, a refused edit and a server's error are an
//!   info prompt, with nothing changed. One rename runs at a time.
//!
//! [`LspManager::prepare_rename`]: fe_lsp::LspManager::prepare_rename

use std::{ops::Range, rc::Rc};

use fe_buffer::{BufferId, Version};
use fe_gpui::EditorView;
use fe_lsp::{LspError, rename::RenameTarget};
use fe_ui::TextInput;
use gpui::{
    AnyElement, App, AppContext as _, Context, Entity, Focusable as _, IntoElement as _,
    ParentElement as _, PromptButton, PromptLevel, SharedString, Task, WeakEntity, Window, div,
};

use crate::{
    Workbench,
    lsp::{RenameAnswer, RenameEdits},
    picker::{Picker, PickerDelegate},
    workbench::Palette,
};

const STALE: &str = "The file changed since the rename began: nothing renamed";

/// What the picker asks about, as it opened.
struct Request {
    editor: WeakEntity<EditorView>,
    buffer: BufferId,
    version: Version,
    caret: usize,
    target: Range<usize>,
}

impl Workbench {
    /// Asks for a new name for the symbol at the active editor's caret,
    /// and renames it across the project.
    pub(crate) fn rename_symbol(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(editor) = self.active_editor(cx).cloned() else {
            return;
        };
        if self.lsp.rename.upgrade().is_some() {
            return self.say("A rename is already running", window, cx);
        }
        let (buffer, version, caret) = {
            let model = editor.read(cx).model();
            let buffer = model.buffer();
            (
                buffer.id(),
                buffer.version(),
                model.primary_selection().head(),
            )
        };
        let Some(prepared) = self.lsp_prepare_rename(buffer, version, caret) else {
            return self.say("No language server for this file", window, cx);
        };
        let guard = Rc::new(());
        self.lsp.rename = Rc::downgrade(&guard);
        let editor = editor.downgrade();
        cx.spawn_in(window, async move |workbench, cx| {
            let answer = prepared.await;
            // Held until the picker takes over, or the answer lands.
            workbench
                .update_in(cx, |workbench, window, cx| {
                    let request = |target: &RenameTarget| Request {
                        editor,
                        buffer,
                        version,
                        caret,
                        target: target.range.clone(),
                    };
                    workbench.prepared(answer, request, window, cx);
                })
                .ok();
            drop(guard);
        })
        .detach();
    }

    /// The server's answer to what to rename: the picker, or why not.
    fn prepared(
        &mut self,
        answer: RenameAnswer,
        request: impl FnOnce(&RenameTarget) -> Request,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let target = match answer {
            Ok(Some(target)) => target,
            Ok(None) => return self.say("Nothing to rename here", window, cx),
            Err(error) => return self.say(&reason(&error), window, cx),
        };
        self.dismiss_palette(window, cx);
        let request = request(&target);
        let workbench = cx.entity().downgrade();
        let delegate = RenamePicker::new(workbench, request, target.placeholder.clone());
        let picker = cx.new(|cx| Picker::new(delegate, "New name…", fe_ui::ui_theme(cx), cx));
        picker.update(cx, |picker, cx| {
            picker.show(window, cx);
            let input = picker.input().clone();
            picker.delegate.start(input, cx);
        });
        self.show_palette(Palette::Rename, picker, window, cx);
    }

    /// An info prompt saying `message`.
    fn say(&self, message: &str, window: &mut Window, cx: &mut App) {
        // Answered by its one button; nothing waits on it.
        drop(window.prompt(
            PromptLevel::Info,
            message,
            None,
            &[PromptButton::ok("OK")],
            cx,
        ));
    }

    /// The server's edits for the rename `request` asked for: applied, the
    /// caret kept on the symbol, or said why not; the picker closes either
    /// way.
    fn renamed(
        &mut self,
        request: &Request,
        answer: RenameEdits,
        picker: &Entity<Picker<RenamePicker>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        picker.update(cx, |picker, _| {
            if let Some((task, _)) = picker.delegate.running.take() {
                task.detach();
            }
        });
        let caret = answer.as_ref().ok().and_then(|files| {
            let edits = files.iter().find_map(|file| match &file.target {
                fe_lsp::EditTarget::Open { buffer, edits, .. } if *buffer == request.buffer => {
                    Some(edits)
                }
                _ => None,
            })?;
            caret_after(edits, &request.target, request.caret)
        });
        // The server's edits were made for the version asked about: a
        // changed or closed source buffer discards all of them.
        let current = request
            .editor
            .upgrade()
            .map(|editor| editor.read(cx).model().buffer().version());
        let applied = answer.map_err(|error| reason(&error)).and_then(|files| {
            if current != Some(request.version) {
                return Err(STALE.to_owned());
            }
            self.apply_user_file_edits(files, cx)
        });
        match applied {
            Ok(_) => {
                let editor = request.editor.upgrade();
                if let (Some(editor), Some(caret)) = (editor, caret) {
                    editor.update(cx, |editor, cx| editor.select_and_reveal(caret..caret, cx));
                }
            }
            Err(message) => self.say(&message, window, cx),
        }
        picker.update(cx, |picker, cx| picker.dismiss(window, cx));
    }
}

/// What to tell the user of `error`.
fn reason(error: &LspError) -> String {
    match error {
        LspError::Server { message, .. } => message.clone(),
        LspError::Unavailable(why) if why == "rename" => {
            "The language server can't rename".to_owned()
        }
        error => error.to_string(),
    }
}

/// Where the caret goes once `edits` are made in its buffer: in the same
/// place in the symbol, clamped to its new name. `None` if no edit
/// renames the symbol, whose range was `target`.
fn caret_after(
    edits: &[(Range<usize>, String)],
    target: &Range<usize>,
    caret: usize,
) -> Option<usize> {
    let renamed = edits.iter().find(|(range, _)| range == target)?;
    let shift: isize = (edits.iter())
        .filter(|(range, _)| range.end <= target.start && range != target)
        .map(|(range, text)| text.len() as isize - range.len() as isize)
        .sum();
    let start = target.start.checked_add_signed(shift)?;
    let within = caret.clamp(target.start, target.end) - target.start;
    Some(start + within.min(renamed.1.len()))
}

/// The new name's picker: the symbol's name to edit, and one row to
/// confirm it.
pub(crate) struct RenamePicker {
    workbench: WeakEntity<Workbench>,
    request: Request,
    placeholder: String,
    /// The name as typed.
    name: String,
    /// Its rename, asked for and not answered, and the guard keeping
    /// others out.
    running: Option<(Task<()>, Rc<()>)>,
}

impl RenamePicker {
    fn new(workbench: WeakEntity<Workbench>, request: Request, placeholder: String) -> Self {
        Self {
            workbench,
            request,
            name: String::new(),
            placeholder,
            running: None,
        }
    }

    /// The picker has opened, on `input`: it holds the placeholder.
    fn start(&mut self, input: Entity<TextInput>, cx: &mut Context<Picker<Self>>) {
        let placeholder = self.placeholder.clone();
        // Selected in the input, so its first edit replaces it.
        input.update(cx, |input, cx| {
            input.set_text(placeholder.clone(), cx);
            input.select_all(cx);
        });
        self.name = placeholder;
        cx.notify();
    }

    /// The name to rename to, if it's a new one.
    fn new_name(&self) -> Option<&str> {
        let name = self.name.trim();
        (!name.is_empty() && name != self.placeholder).then_some(name)
    }

    fn label(&self) -> String {
        format!("Rename to {}", self.name.trim())
    }

    /// Focuses the editor it was opened for, if it's still open.
    fn focus_editor(&self, window: &mut Window, cx: &mut App) {
        if let Some(editor) = self.request.editor.upgrade() {
            window.focus(&editor.focus_handle(cx), cx);
        }
    }

    #[cfg(test)]
    pub(crate) fn status_text(&self) -> String {
        self.status().unwrap_or_default().to_string()
    }
}

impl PickerDelegate for RenamePicker {
    fn update_matches(&mut self, query: String, cx: &mut Context<Picker<Self>>) {
        self.name = query;
        cx.notify();
        let picker = cx.entity();
        // `Picker::matches_updated` borrows the picker, which is being
        // updated now.
        cx.defer(move |cx| picker.update(cx, |picker, cx| picker.matches_updated(true, cx)));
    }

    fn match_count(&self) -> usize {
        usize::from(self.new_name().is_some())
    }

    fn match_id(&self, _: usize) -> SharedString {
        self.label().into()
    }

    fn render_match(&self, _: usize, _: bool, _: &App) -> AnyElement {
        div().child(self.label()).into_any_element()
    }

    /// Asks for the rename; the picker stays until it's answered.
    fn confirm(&mut self, _: usize, window: &mut Window, cx: &mut Context<Picker<Self>>) -> bool {
        if self.running.is_some() {
            return false;
        }
        let Some(new_name) = self.new_name().map(str::to_owned) else {
            return false;
        };
        let (Some(workbench), Some(editor)) =
            (self.workbench.upgrade(), self.request.editor.upgrade())
        else {
            return true;
        };
        let request = &self.request;
        let current = editor.read(cx).model().buffer().version();
        let asked = if current == request.version {
            workbench.update(cx, |workbench, cx| {
                let asked = workbench.lsp_rename(
                    request.buffer,
                    request.version,
                    request.caret,
                    new_name.clone(),
                );
                // A rename running keeps the next out.
                asked.map(|asked| {
                    let guard = Rc::new(());
                    workbench.lsp.rename = Rc::downgrade(&guard);
                    cx.notify();
                    (asked, guard)
                })
            })
        } else {
            workbench.update(cx, |workbench, cx| workbench.say(STALE, window, cx));
            return true;
        };
        let Some((asked, guard)) = asked else {
            let message = "No language server for this file";
            workbench.update(cx, |workbench, cx| workbench.say(message, window, cx));
            return true;
        };
        let request = Request {
            editor: self.request.editor.clone(),
            buffer: self.request.buffer,
            version: self.request.version,
            caret: self.request.caret,
            target: self.request.target.clone(),
        };
        let task = cx.spawn_in(window, async move |picker, cx| {
            let answer = asked.await;
            let Some(picker) = picker.upgrade() else {
                return;
            };
            workbench
                .update_in(cx, |workbench, window, cx| {
                    workbench.renamed(&request, answer, &picker, window, cx)
                })
                .ok();
        });
        self.running = Some((task, guard));
        false
    }

    fn dismissed(&mut self, window: &mut Window, cx: &mut Context<Picker<Self>>) {
        // A rename still running is dropped, unanswered.
        self.running = None;
        self.focus_editor(window, cx);
    }

    fn status(&self) -> Option<SharedString> {
        Some("Enter to rename, Escape to cancel".into())
    }
}

#[cfg(test)]
#[path = "lsp_rename_tests.rs"]
mod tests;
