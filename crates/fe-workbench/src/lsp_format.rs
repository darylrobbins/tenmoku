//! Formatting the active editor's document or selection through its
//! language server or, for a document, the language's external formatter
//! (`lsp.format_document`, `lsp.format_selection`).
//!
//! - **The request.** For the buffer at its version now: the whole
//!   document, or the primary selection widened to whole rows (a caret
//!   formats its row). The options are the editor's: its tab size and
//!   whether it indents with spaces (its detected or configured style, or
//!   spaces with none).
//! - **The edits.** Applied to the buffer as one transaction of the user,
//!   as accepting a completion is, so the editor's undo takes it back in one
//!   step (it undoes only its own origin, which `Origin::Format` isn't);
//!   selections stay anchored. If the buffer has changed since the request
//!   (the user typed meanwhile, or saved it under another name) they're
//!   dropped, and nothing is shown.
//! - **No result.** A server with no formatter shows "No formatter for
//!   <language>"; a timeout or an error shows itself. Both as info toasts.
//! - **External formatters.** A document whose language has a
//!   `[formatters.<language>]` table is formatted by that command as well as
//!   by the server: `prefer` says which goes first, and the other runs if the
//!   first has no formatter, can't be found, fails or times out. A selection
//!   is always the server's. The output goes through the same checks as the
//!   server's edits, the buffer's version first, and is applied by lines, so a
//!   caret on a line the formatter left alone stays on it. A failure shows
//!   itself even if the other source then formats, but for a program that
//!   isn't there, which shows only if nothing formatted.
//! - **The folder's trust.** The user chose the program, but it reads its
//!   config, which can be code, from the folder. So just before each run the
//!   folder it runs in and the file's real folder are resolved
//!   ([`formatter_folders`], off the main thread) and must both be inside a
//!   folder trusted now; if not, or if one can't be resolved, the language has
//!   no external formatter for this format.
//! - **[`Workbench::format_buffer`]** is the same for the document, for
//!   format on save, which reports what happened instead of showing it. A
//!   save gives its hook a second ([`SaveHook`]), so a formatter that takes
//!   longer is stopped at that, whatever its `timeout_ms`, and the text is
//!   saved as it was.
//! - **Code actions on save.** [`FormatOnSave`] first applies the actions of
//!   each kind in `editor.code_actions_on_save`, started within 750 ms.

use std::{
    cell::{Cell, RefCell},
    fs,
    ops::Range,
    path::{Path, PathBuf},
    rc::Rc,
    time::{Duration, Instant},
};

use fe_buffer::{BufferSnapshot, Origin, ProposedChange, Version};
use fe_config::{FormatterPrefer, FormatterSettings, Settings};
use fe_editor::IndentStyle;
use fe_gpui::{EditorView, SaveHook};
use fe_lsp::{
    CodeAction, LspError,
    formatting::{Edits, FormatOptions},
};
use gpui::{
    App, AppContext as _, AsyncApp, BackgroundExecutor, Context, Entity, FutureExt as _, Task,
    WeakEntity, Window,
};

use crate::{
    Workbench,
    external_format::{BufferPath, FormatError, Runner, format_text},
    lsp::{Answer, CodeActioner, resolve, routed_language},
    status_bar::{PLAIN_TEXT, language_label},
    workbench::toast::{self, Toast},
};

/// What formatting a buffer did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum FormatOutcome {
    /// Its text changed.
    Formatted,
    /// The formatter had nothing to change, or the buffer changed while it
    /// answered, so its edits were dropped.
    Unchanged,
    /// The buffer's server has no formatter, or it has no server, and its
    /// language has no external formatter to run.
    NoFormatter,
}

impl Workbench {
    /// Formats the active editor's document, or only its selection if
    /// `selection`.
    pub(crate) fn format_active(
        &mut self,
        selection: bool,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(editor) = self.active_editor(cx).cloned() else {
            return;
        };
        let language = (editor.read(cx).syntax())
            .map(|syntax| language_label(syntax.language().name()))
            .unwrap_or_else(|| PLAIN_TEXT.into());
        let task = self.format_range(&editor, selection, false, cx);
        cx.spawn(async move |_, cx| {
            let message = match task.await {
                Ok(FormatOutcome::Formatted | FormatOutcome::Unchanged) => return,
                Ok(FormatOutcome::NoFormatter) => format!("No formatter for {language}"),
                Err(error) => error,
            };
            cx.update(|cx| toast::show(cx, Toast::info(message)));
        })
        .detach();
    }

    /// Formats `editor`'s whole buffer: see the [module](self).
    pub(crate) fn format_buffer(
        &mut self,
        editor: &Entity<EditorView>,
        cx: &mut Context<Self>,
    ) -> Task<Result<FormatOutcome, String>> {
        self.format_range(editor, false, true, cx)
    }

    /// Formats `editor`'s buffer, or its selection's rows if `selection`.
    /// `for_save` if a save asked: only its reply answers to the save's
    /// rules for editing the text, and it shows nothing.
    fn format_range(
        &mut self,
        editor: &Entity<EditorView>,
        selection: bool,
        for_save: bool,
        cx: &mut Context<Self>,
    ) -> Task<Result<FormatOutcome, String>> {
        let manager = self.lsp.manager();
        let external = match selection {
            true => None,
            false => self.external_formatter(editor, cx),
        };
        if manager.is_none() && external.is_none() {
            return Task::ready(Ok(FormatOutcome::NoFormatter));
        }
        let (buffer, version, path, range, options) = editor.update(cx, |view, _| {
            // Caught up first, so the rows, the selection and the version
            // describe the same text.
            let model = view.model_mut();
            model.sync_display();
            let range = selection.then(|| rows_of(model, model.primary_selection().range));
            let insert_spaces = !matches!(model.indent_style(), Some(IndentStyle::Tabs));
            let buffer = model.buffer();
            let options = FormatOptions {
                tab_size: model.tab_size() as u32,
                insert_spaces,
            };
            let path = model.path().map(Path::to_owned);
            (buffer.id(), buffer.version(), path, range, options)
        });
        let ask_server = move |executor: &BackgroundExecutor| {
            let manager = manager.as_ref()?;
            let reply = manager
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .format(buffer, version, range.clone(), options);
            Some(executor.spawn(reply))
        };
        let external_first = (external.as_ref())
            .is_some_and(|external| external.settings.prefer == FormatterPrefer::External);
        // A server that goes first is asked now, at the version just read.
        let reply = (!external_first)
            .then(|| ask_server(cx.background_executor()))
            .flatten();
        let target = FormatTarget {
            editor: editor.clone(),
            version,
            path,
            for_save,
        };
        cx.spawn(async move |_, cx| {
            let mut failures = Failures::default();
            let (first_external, second_external) = match external_first {
                true => (external, None),
                false => (None, external),
            };
            let step = match first_external {
                Some(external) => external.run(&target, cx).await,
                None => server_step(reply, &target, cx).await,
            };
            let mut outcome = failures.note(step);
            if outcome.is_none() && (external_first || second_external.is_some()) {
                // The other's answer would be dropped too: don't ask for it.
                if !target.current(cx) {
                    outcome = Some(FormatOutcome::Unchanged);
                } else {
                    let step = match second_external {
                        Some(external) => external.run(&target, cx).await,
                        None => {
                            let reply = ask_server(cx.background_executor());
                            server_step(reply, &target, cx).await
                        }
                    };
                    outcome = failures.note(step);
                }
            }
            match outcome {
                Some(outcome) => {
                    failures.settle(for_save, cx);
                    Ok(outcome)
                }
                None => failures.answer(),
            }
        })
    }

    /// The external formatter for `editor`'s document, if its language has one.
    /// Whether it may run is asked when it would: see [`ExternalFormatter::run`].
    fn external_formatter(
        &self,
        editor: &Entity<EditorView>,
        cx: &Context<Self>,
    ) -> Option<ExternalFormatter> {
        let view = editor.read(cx);
        let language = routed_language(view)?;
        let settings = (kiln::platform::config::settings::<Settings>(cx).formatters)
            .get(&*language)?
            .clone();
        Some(ExternalFormatter {
            settings,
            runner: self.format_runner.clone(),
            root: self.root.clone(),
            extension: (view.syntax()).and_then(|syntax| syntax.language().first_extension()),
        })
    }
}

/// The folders a formatter for the file at `path`, or for an unsaved buffer
/// under `root`, reads its config from, resolved as the language servers'
/// paths are ([`resolve`]): where it runs, the process's own folder if the
/// buffer gives none, and where the file really is, past a symlink. `None` if
/// one can't be resolved. Touches the disk.
pub(crate) fn formatter_folders(path: Option<&Path>, root: Option<&Path>) -> Option<Vec<PathBuf>> {
    let working = match path {
        Some(path) => path.parent(),
        None => root,
    };
    let working = match working.filter(|folder| !folder.as_os_str().is_empty()) {
        Some(folder) => fs::canonicalize(folder).ok()?,
        None => fs::canonicalize(std::env::current_dir().ok()?).ok()?,
    };
    let mut folders = vec![working];
    if let Some(path) = path {
        folders.push(resolve(path)?.parent()?.to_owned());
    }
    Some(folders)
}

/// What one formatter did, for [`Workbench::format_range`] to go on from.
enum Step {
    /// It answered: nothing further runs.
    Done(FormatOutcome),
    /// There is none for the buffer.
    NoFormatter,
    /// Its program doesn't exist: this says so.
    NotFound(String),
    /// It failed: this says why, on one line.
    Failed(String),
}

/// What's gone wrong so far in a format.
#[derive(Default)]
struct Failures {
    failed: Vec<String>,
    not_found: Option<String>,
}

impl Failures {
    /// Keeps what `step` says went wrong; its outcome if it formatted.
    fn note(&mut self, step: Step) -> Option<FormatOutcome> {
        match step {
            Step::Done(outcome) => return Some(outcome),
            Step::NoFormatter => {}
            Step::NotFound(message) => self.not_found = Some(message),
            Step::Failed(message) => self.failed.push(message),
        }
        None
    }

    /// Reports these when something else answered. A program that isn't there
    /// is no news then, unlike one that ran and failed.
    fn settle(self, for_save: bool, cx: &mut AsyncApp) {
        if let Some(message) = &self.not_found {
            log::debug!("format: {message}");
        }
        for message in self.failed {
            match for_save {
                true => log::debug!("format on save: {message}"),
                false => cx.update(|cx| toast::show(cx, Toast::info(message))),
            }
        }
    }

    /// The answer when nothing formatted: why, or that nothing could.
    fn answer(self) -> Result<FormatOutcome, String> {
        if !self.failed.is_empty() {
            return Err(self.failed.join("; "));
        }
        match self.not_found {
            Some(message) => Err(message),
            None => Ok(FormatOutcome::NoFormatter),
        }
    }
}

/// The document a format is for, as it was when asked.
struct FormatTarget {
    editor: Entity<EditorView>,
    version: Version,
    path: Option<PathBuf>,
    for_save: bool,
}

impl FormatTarget {
    /// Whether its buffer is still as it was asked about.
    fn current(&self, cx: &mut AsyncApp) -> bool {
        self.editor.update(cx, |view, _| {
            let model = view.model_mut();
            model.sync_display();
            model.path() == self.path.as_deref() && model.buffer().version() == self.version
        })
    }

    /// Applies what `change` makes of the buffer's text, unless it has moved
    /// on since it was asked about.
    fn apply(
        &self,
        change: impl FnOnce(&BufferSnapshot) -> ProposedChange,
        cx: &mut AsyncApp,
    ) -> FormatOutcome {
        self.editor.update(cx, |view, cx| {
            let model = view.model_mut();
            model.sync_display();
            // Saved under another name since: the edits were for the
            // document under the old one.
            if model.path() != self.path.as_deref() {
                return FormatOutcome::Unchanged;
            }
            // Format on save, with a vim insert opened since the request.
            if self.for_save && !view.save_edits_allowed(cx) {
                return FormatOutcome::Unchanged;
            }
            let model = view.model_mut();
            let mut buffer = model.buffer_mut();
            if buffer.version() != self.version {
                return FormatOutcome::Unchanged;
            }
            // Aside from any transaction the user has open (a Vim
            // insert), so undo takes back the format alone.
            let mut change = change(buffer.current());
            let (applied, _) = change.accept_all(&mut buffer);
            drop(buffer);
            cx.notify();
            match applied.is_some() {
                true => FormatOutcome::Formatted,
                false => FormatOutcome::Unchanged,
            }
        })
    }
}

/// A server's answer to a format request: its edits, `None` if it has no
/// formatter.
type ServerReply = Task<Result<Option<Edits>, LspError>>;

/// Takes the server's `reply`, if it was asked, and applies its edits.
async fn server_step(reply: Option<ServerReply>, target: &FormatTarget, cx: &mut AsyncApp) -> Step {
    let Some(reply) = reply else {
        return Step::NoFormatter;
    };
    let edits = match reply.await {
        Ok(Some(edits)) => edits,
        Ok(None) => return Step::NoFormatter,
        Err(error) => {
            return match failure(error) {
                Ok(outcome) => Step::Done(outcome),
                Err(message) => Step::Failed(message),
            };
        }
    };
    let change = |base: &BufferSnapshot| {
        ProposedChange::from_edits(base, merge_inserts(edits), Origin::User)
    };
    Step::Done(target.apply(change, cx))
}

/// A language's external formatter, and what to run it with.
struct ExternalFormatter {
    settings: FormatterSettings,
    runner: Rc<dyn Runner>,
    root: Option<PathBuf>,
    extension: Option<&'static str>,
}

impl ExternalFormatter {
    /// Whether the folders it would read from are trusted now, whatever they
    /// were when the format was asked for.
    async fn may_run(&self, target: &FormatTarget, cx: &mut AsyncApp) -> bool {
        let (path, root) = (target.path.clone(), self.root.clone());
        let folders = cx
            .background_spawn(async move { formatter_folders(path.as_deref(), root.as_deref()) })
            .await;
        let Some(folders) = folders else {
            return false;
        };
        cx.update(|cx| {
            (folders.iter()).all(|folder| kiln_shell::trust::decided(folder, cx) == Some(true))
        })
    }

    /// Runs the formatter over the text at `target`'s version and applies what
    /// it gives, if it may run.
    async fn run(self, target: &FormatTarget, cx: &mut AsyncApp) -> Step {
        if !self.may_run(target, cx).await {
            log::debug!("format: a formatter doesn't run in an untrusted folder");
            return Step::NoFormatter;
        }
        let text = target.editor.update(cx, |view, _| {
            let model = view.model_mut();
            model.sync_display();
            (model.buffer().version() == target.version).then(|| model.text().to_string())
        });
        let Some(text) = text else {
            return Step::Done(FormatOutcome::Unchanged);
        };
        let buffer = match &target.path {
            Some(path) => BufferPath::Saved(path),
            None => BufferPath::Unsaved {
                root: self.root.as_deref(),
                extension: self.extension,
            },
        };
        let executor = cx.background_executor().clone();
        let run = format_text(&*self.runner, &executor, &self.settings, buffer, &text);
        let (range, new_text) = match run.await {
            Ok(Some(edit)) => edit,
            Ok(None) => return Step::Done(FormatOutcome::Unchanged),
            Err(error @ FormatError::NotFound(_)) => return Step::NotFound(error.to_string()),
            Err(error) => return Step::Failed(error.to_string()),
        };
        // By lines over the whole text, not the one edit given: a caret between
        // two changes stays on its line, and an output with other line endings
        // is compared as the buffer's text is held, newlines normalized.
        let formatted = format!("{}{new_text}{}", &text[..range.start], &text[range.end..]);
        let change = |base: &BufferSnapshot| {
            ProposedChange::from_text(base, 0..text.len(), &formatted, Origin::User)
        };
        Step::Done(target.apply(change, cx))
    }
}

/// A request's failure: the buffer having changed before the server took
/// the request is the user typing, which drops the format like a stale
/// reply, not an error to show.
fn failure(error: LspError) -> Result<FormatOutcome, String> {
    match error {
        LspError::Unavailable(why) if why == "buffer changed" => Ok(FormatOutcome::Unchanged),
        error => Err(error.to_string()),
    }
}

/// How long into the hook code actions are still started (VS Code's
/// `editor.codeActionsOnSaveTimeout`), leaving formatting the rest of the
/// save's time.
const CODE_ACTIONS_LIMIT: Duration = Duration::from_millis(750);

impl Workbench {
    /// Asks for the actions of `kind` over `editor`'s whole buffer, with
    /// the buffer's version they were asked at.
    fn ask_on_save(
        &mut self,
        editor: &Entity<EditorView>,
        kind: &str,
        cx: &mut Context<Self>,
    ) -> Option<(CodeActioner, Version, Answer<Vec<CodeAction>>)> {
        let (buffer, version, len) = editor.update(cx, |view, _| {
            let model = view.model_mut();
            model.sync_display();
            let buffer = model.buffer();
            (buffer.id(), buffer.version(), model.text().len())
        });
        let actioner = self.lsp_code_actioner(buffer)?;
        let reply = actioner.ask_kind(buffer, version, 0..len, kind);
        Some((actioner, version, reply))
    }

    /// Applies the enabled actions of `kind` to `editor`, once each and in
    /// the server's order, as the code actions picker applies one. Nothing
    /// is asked, resolved, applied or run at or after `deadline` on the
    /// executor's clock. The buffer is as the answer was asked for, but for
    /// the save's own edits: typing meanwhile, or an action gone stale,
    /// ends the kind.
    async fn run_kind_on_save(
        this: &WeakEntity<Workbench>,
        editor: &Entity<EditorView>,
        kind: &str,
        deadline: Instant,
        cx: &mut AsyncApp,
    ) -> Result<(), String> {
        let executor = cx.background_executor().clone();
        let in_time = || match executor.now() < deadline {
            true => Ok(()),
            false => Err("out of time".to_owned()),
        };
        in_time()?;
        let asked = this
            .update(cx, |workbench, cx| workbench.ask_on_save(editor, kind, cx))
            .map_err(|error| error.to_string())?;
        let Some((actioner, mut version, reply)) = asked else {
            return Err("no language server".into());
        };
        let actions = cx
            .background_spawn(reply)
            .await
            .map_err(|error| error.to_string())?;
        for action in actions
            .into_iter()
            .filter(|action| action.disabled.is_none())
        {
            in_time()?;
            let current = |cx: &mut AsyncApp| {
                Ok::<_, String>(editor.read_with(cx, |view, _| view.model().buffer().version()))
            };
            if current(cx)? != version {
                return Err("the buffer changed".into());
            }
            let action = cx
                .background_spawn(actioner.resolve(&action))
                .await
                .map_err(|error| error.to_string())?;
            let edits = actioner.edits(&action).map_err(|error| error.to_string())?;
            in_time()?;
            if current(cx)? != version {
                return Err("the buffer changed".into());
            }
            version = this
                .update(cx, |workbench, cx| {
                    if !editor.update(cx, |view, cx| view.save_edits_allowed(cx)) {
                        return Err("the buffer changed".to_owned());
                    }
                    workbench.apply_file_edits(edits, cx)?;
                    Ok(editor.read(cx).model().buffer().version())
                })
                .map_err(|error| error.to_string())??;
            in_time()?;
            cx.background_spawn(actioner.run(&action))
                .await
                .map_err(|error| error.to_string())?;
        }
        Ok(())
    }
}

/// Readies an editor to be saved: runs the code actions of the kinds in
/// `editor.code_actions_on_save`, then formats it if `editor.format_on_save`
/// is on. A timeout, an error or no action or formatter is logged, never
/// shown.
pub(crate) struct FormatOnSave {
    workbench: WeakEntity<Workbench>,
    enabled: Rc<Cell<bool>>,
    kinds: Rc<RefCell<Vec<String>>>,
}

impl FormatOnSave {
    pub(crate) fn new(
        workbench: WeakEntity<Workbench>,
        enabled: Rc<Cell<bool>>,
        kinds: Rc<RefCell<Vec<String>>>,
    ) -> Self {
        Self {
            workbench,
            enabled,
            kinds,
        }
    }
}

impl SaveHook for FormatOnSave {
    fn before_save(&self, editor: &Entity<EditorView>, cx: &mut App) -> Task<()> {
        let kinds = self.kinds.borrow().clone();
        let format = self.enabled.get();
        if kinds.is_empty() && !format {
            return Task::ready(());
        }
        let editor = editor.clone();
        let workbench = self.workbench.clone();
        cx.spawn(async move |cx| {
            // The code actions get 750 ms in all: none is started after,
            // what's pending is dropped, and formatting goes on.
            let executor = cx.background_executor().clone();
            let deadline = executor.now() + CODE_ACTIONS_LIMIT;
            let phase = async {
                for kind in &kinds {
                    if let Err(error) =
                        Workbench::run_kind_on_save(&workbench, &editor, kind, deadline, cx).await
                    {
                        log::debug!("code actions on save: {kind}: {error}");
                    }
                }
            };
            if phase
                .with_timeout(CODE_ACTIONS_LIMIT, &executor)
                .await
                .is_err()
            {
                log::debug!("code actions on save: out of time");
            }
            if !format {
                return;
            }
            let task = workbench.update(cx, |workbench, cx| workbench.format_buffer(&editor, cx));
            let Ok(task) = task else {
                return;
            };
            match task.await {
                Ok(FormatOutcome::Formatted | FormatOutcome::Unchanged) => {}
                Ok(FormatOutcome::NoFormatter) => log::debug!("format on save: no formatter"),
                Err(error) => log::debug!("format on save: {error}"),
            }
        })
    }
}

/// `edits`, sorted and apart, with inserts at one place joined in their
/// order: a [`ProposedChange`] takes no two at one place.
fn merge_inserts(edits: Vec<(Range<usize>, String)>) -> Vec<(Range<usize>, String)> {
    let mut merged: Vec<(Range<usize>, String)> = Vec::with_capacity(edits.len());
    for (range, text) in edits {
        match merged.last_mut() {
            Some((last, last_text)) if last.start == last.end && range == *last => {
                last_text.push_str(&text);
            }
            _ => merged.push((range, text)),
        }
    }
    merged
}

/// `range` of `model`'s text widened to whole rows, line breaks included.
/// A selection ending at the start of a row doesn't take that row.
fn rows_of(model: &fe_editor::EditorModel, range: Range<usize>) -> Range<usize> {
    let end = match range.end > range.start {
        true => range.end - 1,
        false => range.end,
    };
    model.line_range_at(range.start).start..model.line_range_at(end).end
}

#[cfg(test)]
#[path = "lsp_format_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "lsp_format_save_tests.rs"]
mod save_tests;
