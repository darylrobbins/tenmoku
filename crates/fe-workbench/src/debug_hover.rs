//! Hover evaluation while stopped (docs/decisions/r1b9dw3g0g-debugger.md §5
//! "Hover to evaluate", card D11).
//!
//! - **When.** Only while the session is stopped, and only where the adapter
//!   says `supportsEvaluateForHovers`: with no such capability no `evaluate`
//!   is ever sent (`repl`-style evaluation can have side effects).
//! - **What.** The identifier under the pointer, its extent from the syntax
//!   tree ([`fe_syntax::larger_syntax_node`], no guessing at word
//!   boundaries); a buffer with no tree gets nothing. It is evaluated with
//!   context `hover` in the topmost frame of the stopped thread that is in
//!   the hovered file (the call stack's selection is the panel's, which a
//!   hover can't see; a caller selected there shows its own file, whose
//!   frame is the one found).
//! - **Shown.** As markdown merged above the language server's hover
//!   ([`merged`]): the type, the value, and up to [`CHILD_CAP`] first-level
//!   children, fetched once. The popover can't expand them: Variables does.
//! - **Stale values.** A stop is a [`StopId`]: the session's id and the
//!   epoch, which is local to its session. The evaluation and the children's
//!   request carry the epoch, and the value is dropped unless the debugger
//!   is in the same stop when it lands, and again just before a popover
//!   publishes it (after the language server's wait too). A popover showing
//!   one is closed, by the frame hook ([`Workbench::sweep_debug_hover`]),
//!   once its stop is over. A command's answer is dropped if the buffer or
//!   the selection changed meanwhile or a later command has asked.
//! - **Text is shown as the adapter gave it**, each part in a fence longer
//!   than any run of backticks in it.
//! - **Commands.** `debug.hover` shows the value under the caret in a
//!   popover, `debug.evaluate` the selection's (or the caret's word's).

use std::ops::Range;

use fe_buffer::{BufferId, Version};
use fe_dap::{
    Epoch, PendingRequest, StopRef,
    protocol::{EvaluateArguments, EvaluateBody, Variable, VariablesBody},
};
use fe_gpui::HoverContent;
use gpui::{AsyncApp, Context, Entity, WeakEntity};
use kiln_l10n::{tr, verbatim};

use crate::{
    Workbench,
    debug_panel::frame_path,
    workbench::toast::{self, Toast},
};

/// The most children a popover lists.
pub(crate) const CHILD_CAP: usize = 20;

/// What to evaluate.
pub(crate) enum Probe {
    /// The identifier at this byte.
    Word(usize),
    /// This range's text as it is.
    Range(Range<usize>),
}

/// Why nothing is evaluated.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Refusal {
    /// No session, or it isn't stopped.
    NotStopped,
    /// The adapter doesn't say it can evaluate for hovers.
    Unsupported,
    /// No identifier (or text) there.
    NothingThere,
    /// The stopped thread has no frame in this file.
    NoFrame,
}

/// One stop of one session: epochs count from 1 in each session, so a
/// replacement session's stop can have an old one's epoch.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct StopId {
    pub(crate) session: u64,
    pub(crate) epoch: Epoch,
}

/// An evaluation sent.
pub(crate) struct Plan {
    expression: String,
    range: Range<usize>,
    stop: StopId,
    request: PendingRequest<EvaluateBody>,
}

impl Plan {
    #[cfg(test)]
    pub(crate) fn stop(&self) -> StopId {
        self.stop
    }
}

/// A value, and the stop it is of.
pub(crate) struct Value {
    pub(crate) content: HoverContent,
    pub(crate) stop: StopId,
}

/// `path` with its symlinks resolved, or as it is if that fails.
fn canonical(path: &std::path::Path) -> std::path::PathBuf {
    dunce::canonicalize(path).unwrap_or_else(|_| path.to_owned())
}

/// Whether `text` is an identifier: word characters, not starting with a
/// digit.
fn is_identifier(text: &str) -> bool {
    let mut chars = text.chars();
    chars
        .next()
        .is_some_and(|first| first == '_' || first.is_alphabetic())
        && chars.all(|c| c == '_' || c.is_alphanumeric())
}

impl Workbench {
    /// The editor showing `buffer`.
    fn hover_editor(
        &self,
        buffer: BufferId,
        cx: &Context<Self>,
    ) -> Option<Entity<fe_gpui::EditorView>> {
        (self.open_editors(cx).into_iter())
            .find(|editor| editor.read(cx).model().snapshot().id() == buffer)
    }

    /// Sends the evaluation of `probe` in `buffer` (at `version`, when the
    /// caller names one), if there is one to send.
    pub(crate) fn debug_hover_plan(
        &self,
        buffer: BufferId,
        version: Option<Version>,
        probe: Probe,
        cx: &Context<Self>,
    ) -> Result<Plan, Refusal> {
        let debugger = self.debugger(cx).ok_or(Refusal::NotStopped)?;
        let debugger = debugger.read(cx);
        let location = debugger.stop_location().ok_or(Refusal::NotStopped)?;
        let session = debugger.session().ok_or(Refusal::NotStopped)?;
        let session_id = debugger.session_id().ok_or(Refusal::NotStopped)?;
        if session.capabilities().supports_evaluate_for_hovers != Some(true) {
            return Err(Refusal::Unsupported);
        }
        let editor = self.hover_editor(buffer, cx).ok_or(Refusal::NothingThere)?;
        let view = editor.read(cx);
        let model = view.model();
        let snapshot = model.snapshot();
        if version.is_some_and(|version| snapshot.version() != version) {
            return Err(Refusal::NothingThere);
        }
        let (range, expression) = match probe {
            Probe::Word(offset) => {
                let syntax = view.syntax().ok_or(Refusal::NothingThere)?;
                let range = fe_syntax::larger_syntax_node(syntax, offset..offset)
                    .filter(|range| range.end <= snapshot.len())
                    .ok_or(Refusal::NothingThere)?;
                let text = snapshot.text().slice_to_string(range.clone());
                if !is_identifier(&text) {
                    return Err(Refusal::NothingThere);
                }
                (range, text)
            }
            Probe::Range(range) => {
                let text = snapshot.text().slice_to_string(range.clone());
                if text.trim().is_empty() {
                    return Err(Refusal::NothingThere);
                }
                (range, text)
            }
        };
        let path = model.path().map(canonical).ok_or(Refusal::NoFrame)?;
        let stop = session.stop_data().ok_or(Refusal::NotStopped)?;
        if stop.epoch != location.epoch {
            return Err(Refusal::NotStopped);
        }
        let frame = (stop.frames.iter())
            .find(|frame| frame_path(frame).is_some_and(|file| canonical(&file) == path))
            .ok_or(Refusal::NoFrame)?;
        let arguments = EvaluateArguments {
            expression: expression.clone(),
            context: Some(verbatim!("hover").into()),
            ..Default::default()
        };
        let request = session.evaluate(&arguments, Some(stop.stop_ref(frame.id)));
        Ok(Plan {
            expression,
            range,
            stop: StopId {
                session: session_id,
                epoch: stop.epoch,
            },
            request,
        })
    }

    /// The children of `reference` of `stop`, asked for if the debugger is
    /// still in it.
    fn debug_hover_children(
        &self,
        stop: StopId,
        reference: i64,
        cx: &Context<Self>,
    ) -> Option<PendingRequest<VariablesBody>> {
        self.debug_hover_current(stop, cx).then_some(())?;
        let debugger = self.debugger(cx)?;
        let debugger = debugger.read(cx);
        Some(
            debugger
                .session()?
                .variables(StopRef::new(stop.epoch, reference)),
        )
    }

    /// Whether the debugger is still in `stop`: the same session, stopped at
    /// the same epoch.
    pub(crate) fn debug_hover_current(&self, stop: StopId, cx: &Context<Self>) -> bool {
        let Some(debugger) = self.debugger(cx) else {
            return false;
        };
        let debugger = debugger.read(cx);
        debugger.session_id() == Some(stop.session)
            && debugger.stop_location().map(|at| at.epoch) == Some(stop.epoch)
    }

    /// The frame hook: a popover showing the debugger's value of a stop that
    /// is over is closed (what is left of the hover isn't worth keeping: it
    /// would be asked again).
    pub(crate) fn sweep_debug_hover(
        &mut self,
        editors: &[Entity<fe_gpui::EditorView>],
        cx: &mut Context<Self>,
    ) {
        let host = self.hover_host().clone();
        let listed = std::mem::take(&mut *host.debug_shown.borrow_mut());
        let mut kept = Vec::new();
        for (buffer, stop) in listed {
            let showing: Vec<_> = (editors.iter())
                .filter(|editor| editor.read(cx).model().snapshot().id() == buffer)
                .filter(|editor| editor.read(cx).popover_text().is_some())
                .collect();
            if showing.is_empty() {
                continue;
            }
            if self.debug_hover_current(stop, cx) {
                kept.push((buffer, stop));
                continue;
            }
            host.debug_command.borrow_mut().take();
            for editor in showing {
                // Setting a provider drops the popover it was showing.
                editor.update(cx, |view, _| {
                    let provider = view.hover_provider().cloned();
                    view.set_hover_provider(provider);
                });
            }
        }
        host.debug_shown.borrow_mut().extend(kept);
    }
}

/// `text` in a fence one backtick longer than its longest run of them (at
/// least three), as the adapter gave it.
fn fenced(text: &str) -> String {
    let mut longest = 0;
    let mut run = 0;
    for c in text.chars() {
        run = if c == '`' { run + 1 } else { 0 };
        longest = longest.max(run);
    }
    let fence = "`".repeat((longest + 1).max(3));
    let mut out = String::new();
    out.push_str(&fence);
    out.push('\n');
    out.push_str(text);
    out.push('\n');
    out.push_str(&fence);
    out
}

/// The value as the popover shows it: the type, the value and the
/// children, each in a fence, `total` of them if the adapter said.
fn markdown(
    expression: &str,
    body: &EvaluateBody,
    children: &[Variable],
    total: Option<usize>,
) -> String {
    let mut parts = Vec::new();
    if let Some(kind) = body.r#type.as_deref().filter(|kind| !kind.is_empty()) {
        parts.push(fenced(kind));
    }
    parts.push(fenced(&[expression, " = ", body.result.as_str()].concat()));
    if !children.is_empty() {
        let mut lines: Vec<String> = (children.iter().take(CHILD_CAP))
            .map(|child| format!("{} = {}", child.name, child.value))
            .collect();
        let more = total.unwrap_or(children.len()).saturating_sub(CHILD_CAP);
        if more > 0 {
            lines.push(tr!("workbench-debug-hover-more", count = more).to_string());
        }
        parts.push(fenced(&lines.join("\n")));
    }
    parts.join("\n\n")
}

/// Whether the debugger is still in `stop`.
pub(crate) async fn current(
    workbench: &WeakEntity<Workbench>,
    cx: &mut AsyncApp,
    stop: StopId,
) -> bool {
    workbench
        .update(cx, |workbench, cx| workbench.debug_hover_current(stop, cx))
        .unwrap_or(false)
}

/// Evaluates `plan`, and fetches its children if it has them, once. An
/// error is the adapter's message if it refused; `None` means the stop
/// ended first, and nothing shows.
pub(crate) async fn evaluate(
    workbench: &WeakEntity<Workbench>,
    cx: &mut AsyncApp,
    plan: Plan,
) -> Result<Value, Option<String>> {
    let body = plan.request.await.map_err(|error| {
        Some(match error.message() {
            Some(message) if !message.is_empty() => message.to_owned(),
            _ => error.to_string(),
        })
    })?;
    let mut children = Vec::new();
    let mut total = None;
    if body.variables_reference > 0 {
        let request = workbench
            .update(cx, |workbench, cx| {
                workbench.debug_hover_children(plan.stop, body.variables_reference, cx)
            })
            .ok()
            .flatten();
        if let Some(request) = request
            && let Ok(reply) = request.await
        {
            let counted = body.named_variables.unwrap_or(0) + body.indexed_variables.unwrap_or(0);
            total = Some(
                usize::try_from(counted)
                    .unwrap_or(0)
                    .max(reply.variables.len()),
            );
            children = reply.variables;
            children.truncate(CHILD_CAP);
        }
    }
    if !current(workbench, cx, plan.stop).await {
        return Err(None);
    }
    Ok(Value {
        content: HoverContent {
            text: markdown(&plan.expression, &body, &children, total),
            range: Some(plan.range),
        },
        stop: plan.stop,
    })
}

/// The hover provider's part: the value of the identifier at `offset`, if
/// there is one to show.
pub(crate) async fn hover(
    workbench: WeakEntity<Workbench>,
    mut cx: AsyncApp,
    buffer: BufferId,
    version: Version,
    offset: usize,
) -> Option<Value> {
    let plan = workbench
        .update(&mut cx, |workbench, cx| {
            workbench.debug_hover_plan(buffer, Some(version), Probe::Word(offset), cx)
        })
        .ok()?
        .ok()?;
    evaluate(&workbench, &mut cx, plan).await.ok()
}

/// What a hover shows of the debugger's value and the rest (the language
/// server's, an ignore file's): the value first, over the identifier's
/// extent.
pub(crate) fn merged(
    value: Option<HoverContent>,
    rest: Option<HoverContent>,
) -> Option<HoverContent> {
    match (value, rest) {
        (Some(value), Some(rest)) => Some(HoverContent {
            text: format!("{}\n\n{}", value.text, rest.text),
            range: value.range.or(rest.range),
        }),
        (value, rest) => value.or(rest),
    }
}

impl Workbench {
    /// `debug.hover` and `debug.evaluate`: the value of the caret's
    /// identifier, or of the selection if `selection` and there is one, in a
    /// popover at the caret.
    pub fn debug_show_value(&mut self, selection: bool, cx: &mut Context<Self>) {
        let Some(editor) = self.active_editor(cx).cloned() else {
            return toast::show(cx, Toast::info(tr!("workbench-debug-no-file")));
        };
        let (buffer, probe) = {
            let model = editor.read(cx).model();
            let primary = model.primary_selection();
            let probe = match selection && !primary.range.is_empty() {
                true => Probe::Range(primary.range.clone()),
                false => Probe::Word(primary.head()),
            };
            (model.snapshot().id(), probe)
        };
        let plan = match self.debug_hover_plan(buffer, None, probe, cx) {
            Ok(plan) => plan,
            Err(refusal) => {
                let message = match refusal {
                    Refusal::NotStopped => tr!("workbench-debug-hover-not-stopped"),
                    Refusal::Unsupported => tr!("workbench-debug-hover-unsupported"),
                    Refusal::NothingThere => tr!("workbench-debug-hover-nothing"),
                    Refusal::NoFrame => tr!("workbench-debug-hover-no-frame"),
                };
                return toast::show(cx, Toast::info(message));
            }
        };
        let (version, selected) = {
            let model = editor.read(cx).model();
            (model.snapshot().version(), model.primary_selection().range)
        };
        let task = cx.spawn(async move |workbench, cx| {
            let shown = evaluate(&workbench, cx, plan).await;
            workbench
                .update(cx, |workbench, cx| {
                    // Only for what was asked about: the text and the
                    // selection are as they were.
                    let view = editor.read(cx);
                    let model = view.model();
                    let fresh = model.snapshot().version() == version
                        && model.primary_selection().range == selected;
                    if !fresh {
                        return;
                    }
                    match shown {
                        Ok(value) => {
                            let stop = value.stop;
                            editor.update(cx, |view, cx| view.show_popover(value.content, cx));
                            let host = workbench.hover_host();
                            host.debug_shown.borrow_mut().push((buffer, stop));
                        }
                        Err(Some(error)) => toast::show(
                            cx,
                            Toast::info(tr!(
                                "workbench-debug-hover-failed",
                                error = error.as_str()
                            )),
                        ),
                        Err(None) => {}
                    }
                })
                .ok();
        });
        // A later command replaces this one, which drops it unanswered.
        *self.hover_host().debug_command.borrow_mut() = Some(task);
    }
}
