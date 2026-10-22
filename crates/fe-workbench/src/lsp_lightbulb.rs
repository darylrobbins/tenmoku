//! The lightbulb by the caret's row, as VS Code shows it: once the active
//! editor's caret (or selection) has rested [`REST`] on a buffer whose
//! server provides code actions, the server is asked for the actions there
//! (`triggerKind` automatic, no kind named), and the gutter shows a
//! lightbulb on the caret's row if any enabled action other than a
//! `source` one comes back: the blue one when one is a preferred quick
//! fix. A click on it runs Quick Fix (`lsp.code_actions`).
//!
//! Moving or editing hides it at once and restarts the wait, and an answer
//! for an old selection or version is dropped. Another editor becoming
//! active, `editor.lightbulb` turning off, or the buffer leaving its server
//! clears the glyph and the pending request.

use std::{ops::Range, rc::Rc, time::Duration};

use fe_buffer::Version;
use fe_editor::diagnostics::{DiagnosticTag, Severity};
use fe_gpui::{EditorView, GlyphKind, GutterGlyph};
use fe_lsp::{CodeAction, code_action::CodeActionQuery};
use gpui::{App, Context, Entity, EntityId, Task, Window};

use crate::Workbench;

/// How long the caret rests before the actions are asked for: VS Code's
/// delay for the lightbulb.
const REST: Duration = Duration::from_millis(250);

/// What clicking the glyph does.
type Click = Rc<dyn Fn(&mut Window, &mut App)>;

/// Where the lightbulb is asked for: the editor, its text's version and
/// its primary selection.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Wanted {
    editor: EntityId,
    /// The server the buffer is on: another one asks again.
    server: fe_lsp::ServerId,
    version: Version,
    range: Range<usize>,
    /// The end of the selection the caret is at: its row gets the glyph.
    head: usize,
}

/// The active editor's lightbulb.
#[derive(Default)]
pub(super) struct Lightbulb {
    wanted: Option<Wanted>,
    /// The editor the glyph is on, while it's shown.
    shown_on: Option<Entity<EditorView>>,
    /// The wait for the caret to rest, then the request.
    task: Option<Task<()>>,
    /// What clicking the glyph does.
    #[cfg(test)]
    click: Option<Click>,
}

/// Whether a diagnostic's range is one `selection` is at: a caret is at the
/// ends of a range too, a selection must share some of it.
fn overlaps(diagnostic: &Range<usize>, selection: &Range<usize>) -> bool {
    match selection.is_empty() {
        true => diagnostic.start <= selection.start && selection.start <= diagnostic.end,
        false => diagnostic.start < selection.end && selection.start < diagnostic.end,
    }
}

/// Whether to show a lightbulb for `actions`, and which: none for only
/// disabled or `source` ones.
fn glyph_for(actions: &[CodeAction]) -> Option<GlyphKind> {
    let mut found = None;
    for action in actions {
        if action.disabled.is_some() || action.kind_is("source") {
            continue;
        }
        if action.is_preferred && action.is_quick_fix() {
            return Some(GlyphKind::LightbulbAutofix);
        }
        found = Some(GlyphKind::Lightbulb);
    }
    found
}

impl Workbench {
    /// What clicking the glyph does, while it's shown.
    #[cfg(test)]
    pub(crate) fn lightbulb_click(&self) -> Option<Click> {
        self.lsp.lightbulb.click.clone()
    }

    /// Where `editor` asks for the lightbulb now: nothing unless its
    /// buffer is on a server with a code action provider.
    fn lightbulb_wanted(&self, editor: &Entity<EditorView>, cx: &App) -> Option<Wanted> {
        let model = editor.read(cx).model();
        let buffer = model.buffer();
        let manager = super::lock(self.lsp.manager.as_ref()?);
        let server = manager.server_of(buffer.id())?;
        // `Simple(false)` says there is none. (`lsp_types` isn't this
        // crate's dependency to name the variants with.)
        let provider = manager.capabilities(server)?.code_action_provider?;
        if format!("{provider:?}") == "Simple(false)" {
            return None;
        }
        let selection = model.primary_selection();
        Some(Wanted {
            editor: editor.entity_id(),
            server,
            version: buffer.version(),
            head: selection.head(),
            range: selection.range,
        })
    }

    /// Hides the glyph, if shown.
    fn hide_lightbulb(&mut self, cx: &mut Context<Self>) {
        if let Some(editor) = self.lsp.lightbulb.shown_on.take() {
            editor.update(cx, |view, cx| {
                view.set_gutter_glyph(None, None);
                cx.notify();
            });
        }
    }

    /// Brings the lightbulb up to date with the active editor's caret: run
    /// by [`Self::sync_lsp`] after each draw.
    pub(super) fn sync_lightbulb(&mut self, cx: &mut Context<Self>) {
        let now = match self.lsp.lightbulb_on.get() {
            true => self
                .active_editor(cx)
                .cloned()
                .and_then(|editor| self.lightbulb_wanted(&editor, cx)),
            false => None,
        };
        if self.lsp.lightbulb.wanted == now {
            return;
        }
        self.hide_lightbulb(cx);
        self.lsp.lightbulb.wanted = now.clone();
        self.lsp.lightbulb.task = None;
        let Some(wanted) = now else {
            return;
        };
        let task = cx.spawn(async move |workbench, cx| {
            cx.background_executor().timer(REST).await;
            let Ok(Some(reply)) =
                workbench.update(cx, |workbench, cx| workbench.ask_lightbulb(&wanted, cx))
            else {
                return;
            };
            let answer = reply.await;
            workbench
                .update(cx, |workbench, cx| {
                    workbench.lightbulb_landed(&wanted, answer, cx)
                })
                .ok();
        });
        self.lsp.lightbulb.task = Some(task);
    }

    /// The request for `wanted`, sent now: nothing if it isn't current.
    fn ask_lightbulb(&self, wanted: &Wanted, cx: &App) -> Option<super::Answer<Vec<CodeAction>>> {
        let editor = self.active_editor(cx)?;
        if self.lightbulb_wanted(editor, cx).as_ref() != Some(wanted) {
            return None;
        }
        let model = editor.read(cx).model();
        let buffer = model.buffer().id();
        let diagnostics = (model.diagnostics().into_iter())
            .filter(|entry| overlaps(&entry.range, &wanted.range))
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
        let query = CodeActionQuery {
            only: None,
            automatic: true,
        };
        let actioner = self.lsp_code_actioner(buffer)?;
        Some(actioner.ask(
            buffer,
            wanted.version,
            wanted.range.clone(),
            diagnostics,
            query,
        ))
    }

    /// Shows the lightbulb `answer` calls for, if its editor is still
    /// active with the same selection and version.
    fn lightbulb_landed(
        &mut self,
        wanted: &Wanted,
        answer: Result<Vec<CodeAction>, fe_lsp::LspError>,
        cx: &mut Context<Self>,
    ) {
        let Some(editor) = self.active_editor(cx).cloned() else {
            return;
        };
        if self.lightbulb_wanted(&editor, cx).as_ref() != Some(wanted) {
            return;
        }
        let kind = match answer {
            Ok(actions) => glyph_for(&actions),
            Err(error) => {
                log::debug!("no code actions for the lightbulb: {error}");
                None
            }
        };
        let Some(kind) = kind else {
            return;
        };
        let row = editor
            .read(cx)
            .model()
            .text()
            .offset_to_point(wanted.head)
            .row;
        let workbench = cx.weak_entity();
        let click: Click = Rc::new(move |window, cx| {
            workbench
                .update(cx, |workbench, cx| {
                    workbench.show_code_actions(CodeActionQuery::default(), false, window, cx)
                })
                .ok();
        });
        #[cfg(test)]
        {
            self.lsp.lightbulb.click = Some(click.clone());
        }
        editor.update(cx, |view, cx| {
            view.set_gutter_glyph(Some(GutterGlyph { row, kind }), Some(click));
            cx.notify();
        });
        self.lsp.lightbulb.shown_on = Some(editor);
    }
}
