//! The code actions at the active editor's caret or selection
//! (`lsp.code_actions`).
//!
//! - **Asking.** The server is asked for the actions at the active editor's
//!   primary selection (a caret is an empty range) and the diagnostics the
//!   editor shows overlapping it, at the buffer's version. Nothing shows
//!   while it answers; an answer that lands after the editor, its
//!   selection or its text moved on is dropped. No actions, a server's
//!   refusal or a file with no server is said in an info prompt.
//! - **Listing.** A [`CodeActionsPicker`] shows as [`Palette::CodeActions`]:
//!   a row per action, its title and its kind dimmed, in the order fe-lsp
//!   gives (quick fixes first, the preferred one first). A disabled action
//!   is greyed, with its reason, and refuses a pick. Typing filters the
//!   titles by subsequence, ignoring case.
//! - **Applying.** Confirming resolves the action, applies its edits
//!   ([`Workbench::apply_file_edits`]), then has the server run its command,
//!   if it has one: edits it sends meanwhile arrive as
//!   [`fe_lsp::LspEvent::ApplyEdit`]. The picker stays up, refusing another
//!   confirm, until that's done, and then closes with focus back in the
//!   editor. Edits that reach past the file are previewed instead, under
//!   the action's title, in the AI Changes panel
//!   ([`Workbench::preview_or_apply`]): the picker closes, and the command
//!   runs once they're applied, never if they're cancelled. A buffer changed since the list was asked for, a server error
//!   or a refused edit is an info prompt with the reason, and the edits
//!   not applied stay unapplied.

use std::ops::Range;

use fe_buffer::{BufferId, Version};
use fe_editor::diagnostics::{DiagnosticTag, Severity};
use fe_gpui::EditorView;
use fe_lsp::{CodeAction, LspError, code_action::CodeActionQuery};
use fe_ui::{HighlightedLabel, char_ranges};
use gpui::{
    AnyElement, App, AppContext as _, Context, Entity, Focusable as _, IntoElement as _,
    ParentElement as _, PromptButton, PromptLevel, SharedString, Styled as _, Task, WeakEntity,
    Window, div, prelude::FluentBuilder as _,
};

use crate::{
    Workbench,
    ai_changes_panel::OnApplied,
    lsp::CodeActioner,
    picker::{Picker, PickerDelegate},
    toolbar_picker::subsequence,
    workbench::Palette,
    workspace_edit::refactor_preview::RefactorPreview,
};

const STALE: &str = "The file changed; ask again";

/// An info prompt saying `message`, answered by its one button.
fn tell(message: &str, window: &mut Window, cx: &mut App) {
    drop(window.prompt(
        PromptLevel::Info,
        message,
        None,
        &[PromptButton::ok("OK")],
        cx,
    ));
}

/// Has the server run `action`'s command, through `actioner`, once its
/// previewed edits are applied, saying why if it fails.
fn command_after(actioner: CodeActioner, action: CodeAction) -> OnApplied {
    Box::new(move |_, _, window, cx| {
        let run = actioner.run(&action);
        cx.spawn_in(window, async move |_, cx| {
            if let Err(error) = run.await {
                cx.update(|window, cx| tell(&reason(&error), window, cx))
                    .ok();
            }
        })
        .detach();
    })
}

/// What a server's error says.
fn reason(error: &LspError) -> String {
    match error {
        LspError::Server { message, .. } => message.clone(),
        error => error.to_string(),
    }
}

impl Workbench {
    /// Lists the code actions `query` asks for at the active editor's
    /// selection and its diagnostics, to pick one to apply. With `direct`,
    /// the one enabled action there is, if there is exactly one, is applied
    /// at once instead. Without a kind, `source` actions are left out.
    pub(crate) fn show_code_actions(
        &mut self,
        query: CodeActionQuery,
        direct: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // One at a time, asking or listing.
        if self.palette == Some(Palette::CodeActions) || self.lsp.code_actioning.is_some() {
            return;
        }
        let Some(editor) = self.active_editor(cx).cloned() else {
            return;
        };
        let (buffer, version, range, diagnostics) = editor.update(cx, |view, _| {
            let model = view.model_mut();
            model.sync_display();
            let range = model.primary_selection().range;
            let diagnostics = (model.diagnostics().into_iter())
                .filter(|entry| overlaps(&entry.range, &range))
                .map(|entry| {
                    let mut sent = fe_lsp::Diagnostic::new(entry.range.clone(), &entry.message);
                    sent.severity = Some(match entry.severity {
                        Severity::Error => fe_lsp::Severity::Error,
                        Severity::Warning => fe_lsp::Severity::Warning,
                        Severity::Information => fe_lsp::Severity::Information,
                        Severity::Hint => fe_lsp::Severity::Hint,
                    });
                    sent.source = entry.source.clone();
                    sent.code = entry.code.clone();
                    sent.tags = (entry.tags.iter())
                        .map(|tag| match tag {
                            DiagnosticTag::Unnecessary => fe_lsp::DiagnosticTag::Unnecessary,
                            DiagnosticTag::Deprecated => fe_lsp::DiagnosticTag::Deprecated,
                        })
                        .collect();
                    sent
                })
                .collect();
            (
                model.buffer().id(),
                model.buffer().version(),
                range,
                diagnostics,
            )
        });
        let Some(actioner) = self.lsp_code_actioner(buffer) else {
            return tell("No language server for this file", window, cx);
        };
        let reply = actioner.ask(buffer, version, range.clone(), diagnostics, query.clone());
        self.lsp.code_actioning = Some(cx.spawn_in(window, async move |this, cx| {
            let answer = reply.await;
            let _ = this.update_in(cx, |workbench, window, cx| {
                if let Some(task) = workbench.lsp.code_actioning.take() {
                    task.detach();
                }
                let at = Asked {
                    editor,
                    buffer,
                    version,
                    range,
                };
                workbench.listed(at, actioner, (query, direct), answer, window, cx)
            });
        }));
    }

    /// The server's answer to what it offers at `at`: said, or the picker
    /// of its actions. Dropped if the editor isn't the active one, as it
    /// was, any more.
    fn listed(
        &mut self,
        at: Asked,
        actioner: CodeActioner,
        (query, direct): (CodeActionQuery, bool),
        answer: Result<Vec<CodeAction>, LspError>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let current = self.active_editor(cx) == Some(&at.editor) && {
            let model = at.editor.read(cx).model();
            model.buffer().version() == at.version && model.primary_selection().range == at.range
        };
        if !current || self.palette == Some(Palette::CodeActions) {
            return;
        }
        let mut actions = match answer {
            Ok(actions) => actions,
            Err(error) => return tell(&reason(&error), window, cx),
        };
        // Quick Fix leaves the source actions to their own commands.
        if query.only.is_none() {
            actions.retain(|action| !action.kind_is("source"));
        }
        if actions.is_empty() {
            return match kind_name(&query) {
                Some(kind) => tell(&format!("No {kind} actions available"), window, cx),
                None => tell("No code actions available", window, cx),
            };
        }
        if direct {
            let mut enabled = actions.iter().filter(|action| action.disabled.is_none());
            if let (Some(action), None) = (enabled.next(), enabled.next()) {
                return self.apply_alone(at, actioner, action.clone(), window, cx);
            }
        }
        // fe-lsp ranks the quick fixes for the diagnostics first: the
        // preferred one leads the list all the same. Stable.
        actions.sort_by_key(|action| {
            !(action.is_preferred && action.is_quick_fix() && action.disabled.is_none())
        });
        self.dismiss_palette(window, cx);
        let delegate = CodeActionsPicker {
            workbench: cx.weak_entity(),
            editor: at.editor.downgrade(),
            actioner,
            buffer: at.buffer,
            version: at.version,
            actions,
            matches: Vec::new(),
            refused: None,
            running: None,
        };
        let picker =
            cx.new(|cx| Picker::new(delegate, "Filter code actions…", fe_ui::ui_theme(cx), cx));
        picker.update(cx, |picker, cx| picker.show(window, cx));
        #[cfg(test)]
        LAST.with(|last| *last.borrow_mut() = Some(picker.downgrade()));
        self.show_palette(Palette::CodeActions, picker, window, cx);
    }

    /// Resolves `action`, applies its edits and has the server run its
    /// command, as confirming it in the picker would, without one.
    fn apply_alone(
        &mut self,
        at: Asked,
        actioner: CodeActioner,
        action: CodeAction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let resolving = actioner.resolve(&action);
        self.lsp.code_actioning = Some(cx.spawn_in(window, async move |this, cx| {
            let resolved = resolving.await;
            let run = this
                .update_in(cx, |workbench, window, cx| {
                    let unchanged = at.editor.read(cx).model().buffer().version() == at.version;
                    let applied = match resolved {
                        Ok(_) if !unchanged => Err(STALE.to_owned()),
                        Ok(action) => (actioner.edits(&action))
                            .map_err(|error| reason(&error))
                            .and_then(|edits| {
                                if RefactorPreview::needs_preview(&edits, at.buffer) {
                                    let title = action.title.clone();
                                    let after = command_after(actioner.clone(), action);
                                    (workbench.preview_or_apply(
                                        title, at.buffer, edits, after, window, cx,
                                    ))
                                    .map(|()| None)
                                } else {
                                    (workbench.apply_user_file_edits(edits, cx))
                                        .map(|_| Some(actioner.run(&action)))
                                }
                            }),
                        Err(error) => Err(reason(&error)),
                    };
                    match applied {
                        Ok(Some(run)) => Some(run),
                        Ok(None) => {
                            // Previewed: its command runs once it's applied.
                            if let Some(task) = workbench.lsp.code_actioning.take() {
                                task.detach();
                            }
                            None
                        }
                        Err(message) => {
                            if let Some(task) = workbench.lsp.code_actioning.take() {
                                task.detach();
                            }
                            tell(&message, window, cx);
                            None
                        }
                    }
                })
                .ok()
                .flatten();
            let Some(run) = run else { return };
            let ran = run.await;
            let _ = this.update_in(cx, |workbench, window, cx| {
                if let Some(task) = workbench.lsp.code_actioning.take() {
                    task.detach();
                }
                if let Err(error) = ran {
                    tell(&reason(&error), window, cx);
                }
            });
        }));
    }
}

/// What `query`'s kind is called in "No … actions available": `source.fixAll`
/// is "fix all". `None` without a kind.
fn kind_name(query: &CodeActionQuery) -> Option<String> {
    let kind = query.only.as_ref()?.first()?;
    let last = kind.rsplit('.').next()?;
    let mut name = String::new();
    for c in last.chars() {
        if c.is_uppercase() {
            name.push(' ');
        }
        name.extend(c.to_lowercase());
    }
    Some(name)
}

/// Whether a diagnostic's range is one `selection` is at: a caret is at
/// the ends of a range too, a selection must share some of it.
fn overlaps(diagnostic: &Range<usize>, selection: &Range<usize>) -> bool {
    match selection.is_empty() {
        true => diagnostic.start <= selection.start && selection.start <= diagnostic.end,
        false => diagnostic.start < selection.end && selection.start < diagnostic.end,
    }
}

#[cfg(test)]
thread_local! {
    /// The picker last shown, for the tests.
    static LAST: std::cell::RefCell<Option<WeakEntity<Picker<CodeActionsPicker>>>> = const {
        std::cell::RefCell::new(None)
    };
}

/// Where the actions were asked for.
struct Asked {
    editor: Entity<EditorView>,
    buffer: BufferId,
    version: Version,
    range: Range<usize>,
}

/// A server's code actions, and the one confirmed.
pub(crate) struct CodeActionsPicker {
    workbench: WeakEntity<Workbench>,
    editor: WeakEntity<EditorView>,
    actioner: CodeActioner,
    buffer: BufferId,
    /// The buffer's version the server was asked about.
    version: Version,
    actions: Vec<CodeAction>,
    /// The actions whose titles match the query, with the byte ranges
    /// matched, in the server's order.
    matches: Vec<(usize, Vec<Range<usize>>)>,
    /// Why the last pick was refused.
    refused: Option<String>,
    /// The action being applied.
    running: Option<Task<()>>,
}

impl CodeActionsPicker {
    /// Focuses the editor, if it's still open.
    fn focus_editor(&self, window: &mut Window, cx: &mut App) {
        if let Some(editor) = self.editor.upgrade() {
            window.focus(&editor.focus_handle(cx), cx);
        }
    }

    /// Whether the buffer is as the actions were asked for.
    fn unchanged(&self, cx: &App) -> bool {
        (self.editor.upgrade()).is_some_and(|editor| {
            let buffer = editor.read(cx).model().buffer();
            buffer.id() == self.buffer && buffer.version() == self.version
        })
    }

    /// `action` resolved: its edits applied, and its command to run if
    /// those were. Said, with `None`, if not; `None` too if they're
    /// previewed instead, once the picker has closed, the command to run
    /// once they're applied.
    fn apply(
        &mut self,
        action: Result<CodeAction, LspError>,
        window: &mut Window,
        cx: &mut Context<Picker<Self>>,
    ) -> Option<crate::lsp::Answer<()>> {
        let source = self.buffer;
        let applied = match action {
            Ok(_) if !self.unchanged(cx) => Err(STALE.to_owned()),
            Ok(action) => match (self.actioner.edits(&action), self.workbench.upgrade()) {
                (Err(error), _) => Err(reason(&error)),
                (Ok(_), None) => Err("the window has closed".to_owned()),
                (Ok(edits), Some(workbench)) if RefactorPreview::needs_preview(&edits, source) => {
                    // After the picker closes, so the panel keeps focus.
                    let title = action.title.clone();
                    let after = command_after(self.actioner.clone(), action);
                    window.defer(cx, move |window, cx| {
                        workbench.update(cx, |workbench, cx| {
                            let shown =
                                workbench.preview_or_apply(title, source, edits, after, window, cx);
                            if let Err(message) = shown {
                                tell(&message, window, cx);
                            }
                        })
                    });
                    Ok(None)
                }
                (Ok(edits), Some(workbench)) => workbench
                    .update(cx, |workbench, cx| {
                        workbench.apply_user_file_edits(edits, cx)
                    })
                    .map(|_| Some(self.actioner.run(&action))),
            },
            Err(error) => Err(reason(&error)),
        };
        match applied {
            Ok(run) => run,
            Err(message) => {
                tell(&message, window, cx);
                None
            }
        }
    }

    #[cfg(test)]
    pub(crate) fn status_text(&self) -> String {
        self.status().unwrap_or_default().to_string()
    }

    /// The listed rows' titles, with their kind and reason.
    #[cfg(test)]
    pub(crate) fn rows(&self) -> Vec<(String, Option<String>, Option<String>)> {
        (self.matches.iter())
            .map(|(ix, _)| {
                let action = &self.actions[*ix];
                (
                    action.title.clone(),
                    action.kind.clone(),
                    action.disabled.clone(),
                )
            })
            .collect()
    }
}

impl PickerDelegate for CodeActionsPicker {
    fn update_matches(&mut self, query: String, cx: &mut Context<Picker<Self>>) {
        let wanted: Vec<char> = query.chars().flat_map(char::to_lowercase).collect();
        self.refused = None;
        self.matches = (self.actions.iter().enumerate())
            .filter_map(|(ix, action)| {
                let found = subsequence(&wanted, &action.title)?;
                Some((ix, char_ranges(&action.title, &found)))
            })
            .collect();
        cx.notify();
        // `Picker::matches_updated` borrows the picker, which is being
        // updated now.
        let picker = cx.entity();
        cx.defer(move |cx| picker.update(cx, |picker, cx| picker.matches_updated(true, cx)));
    }

    fn match_count(&self) -> usize {
        self.matches.len()
    }

    fn match_id(&self, ix: usize) -> SharedString {
        self.matches[ix].0.to_string().into()
    }

    fn render_match(&self, ix: usize, _: bool, cx: &App) -> AnyElement {
        let ui = fe_ui::ui_theme(cx);
        let (row, highlights) = &self.matches[ix];
        let action = &self.actions[*row];
        let label = match action.disabled {
            // Highlighted labels take their colour from the theme.
            Some(_) => div()
                .text_color(ui.muted_text)
                .child(action.title.clone())
                .into_any_element(),
            None => HighlightedLabel::new(action.title.clone(), highlights.clone(), &ui)
                .into_any_element(),
        };
        let detail = match (&action.kind, &action.disabled) {
            (Some(kind), Some(why)) => Some(format!("{kind} · {why}")),
            (kind, why) => kind.clone().or_else(|| why.clone()),
        };
        div()
            .flex()
            .flex_1()
            .items_center()
            .gap_2()
            .when(action.disabled.is_some(), |row| {
                row.text_color(ui.muted_text)
            })
            .child(label)
            .children(detail.map(|detail| div().ml_auto().text_color(ui.muted_text).child(detail)))
            .into_any_element()
    }

    /// Applies the action, and keeps the picker up until it's done.
    fn confirm(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Picker<Self>>) -> bool {
        if self.running.is_some() {
            return false;
        }
        let action = &self.actions[self.matches[ix].0];
        if let Some(why) = &action.disabled {
            self.refused = Some(why.clone());
            cx.notify();
            return false;
        }
        if !self.unchanged(cx) {
            tell(STALE, window, cx);
            self.focus_editor(window, cx);
            return true;
        }
        let resolving = self.actioner.resolve(action);
        self.running = Some(cx.spawn_in(window, async move |picker, cx| {
            let resolved = resolving.await;
            let Ok(Some(run)) = picker.update_in(cx, |picker, window, cx| {
                let run = picker.delegate.apply(resolved, window, cx);
                if run.is_none() {
                    picker.delegate.running = None;
                    picker.dismiss(window, cx);
                }
                run
            }) else {
                return;
            };
            let ran = run.await;
            let _ = picker.update_in(cx, |picker, window, cx| {
                if let Err(error) = ran {
                    tell(&reason(&error), window, cx);
                }
                if let Some(task) = picker.delegate.running.take() {
                    task.detach();
                }
                picker.dismiss(window, cx);
            });
        }));
        false
    }

    /// Cancels an action still running, and focuses the editor.
    fn dismissed(&mut self, window: &mut Window, cx: &mut Context<Picker<Self>>) {
        self.running = None;
        self.focus_editor(window, cx);
    }

    fn status(&self) -> Option<SharedString> {
        match &self.refused {
            Some(why) => Some(format!("Disabled: {why}").into()),
            None => Some("Enter to apply, Escape to cancel".into()),
        }
    }
}

#[cfg(test)]
#[path = "lsp_code_actions_tests.rs"]
mod tests;
