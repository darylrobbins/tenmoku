//! [`EditorView`] as kiln-shell's [`PaneItem`]: what a pane shows in an
//! editor's tab (its file's name, a dirty dot), the window's title while
//! it's active, its status bar items ([`status`]), and its file, saving,
//! splitting and saved state, from its
//! [`EditorModel`](fe_editor::EditorModel).

use std::{
    cell::{Cell, RefCell},
    collections::{HashMap, HashSet},
    fmt,
    hash::{DefaultHasher, Hash as _, Hasher as _},
    io,
    path::{Path, PathBuf},
    rc::Rc,
    sync::{
        Arc, Mutex, PoisonError,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, SystemTime},
};

use fe_buffer::{
    Backup, BackupId, BackupMeta, BackupStore, Bias, Buffer, BufferId, FileEncoding, Version,
};
use fe_editor::{EditorModel, IndentStyle, SaveError, SaveJob, Saved, auto_save::SaveReason};

use fe_command::{Invocation, Value};
use fe_text::Text;
use fe_vim::VimState;
use futures::{FutureExt as _, channel::oneshot, future::Shared};
use gpui::{
    AnyView, AnyWindowHandle, App, AppContext as _, AsyncApp, Context, Entity, EntityId, Global,
    Hsla, PromptButton, PromptLevel, SharedString, Task, WeakEntity, Window, WindowId,
};
use kiln_shell::{CloseAnswer, Icon, PaneItem, StatusItem, StatusSide, file_icon};

use crate::{DiffView, EditorSettings, EditorTheme, EditorView, commands::RunCommand};

/// An editor's status bar items, while it's the active item: the vim mode
/// with vim on, and vim's last message, on the left; the cursor,
/// indentation, encoding, line ending and language on the right. The
/// consts are their ids; a host item with one of them shows in its place.
pub mod status {
    use gpui::SharedString;
    use kiln_shell::StatusFill;

    use crate::EditorTheme;

    /// `NORMAL`, `INSERT`, `VISUAL`, `VISUAL LINE` or `VISUAL BLOCK`, only
    /// with vim on, or `JUMP` and the chars typed (`JUMP ab`) while a jump
    /// is pending: on the left, priority 0.
    pub const VIM_MODE: &str = "vim_mode";
    /// Vim's last message, such as `E486: Pattern not found: zzz`, until
    /// the next key in the editor: on the left, priority 10.
    pub const VIM_MESSAGE: &str = "vim_message";
    /// `Ln 12, Col 5`, then ` (17 selected)` or ` (3 selections)`: priority
    /// 100.
    pub const CURSOR: &str = "cursor";
    /// `Spaces: 2` for a file indented with spaces (the editor's tab size
    /// when it can't tell), or `Tab Size: 4` with tabs: priority 200.
    pub const INDENT: &str = "indent";
    /// The file's encoding, such as `UTF-8`, `UTF-8 with BOM` or
    /// `windows-1252`: priority 300.
    pub const ENCODING: &str = "encoding";
    /// `LF` or `CRLF`: priority 400.
    pub const LINE_ENDING: &str = "line_ending";
    /// The language's name, `Plain Text`, or `…` while it loads; a click
    /// runs `language.select`: priority 500.
    pub const LANGUAGE: &str = "language";

    /// The right side's ids and priorities, left to right.
    pub(super) const RIGHT: [(&str, i32); 5] = [
        (CURSOR, 100),
        (INDENT, 200),
        (ENCODING, 300),
        (LINE_ENDING, 400),
        (LANGUAGE, 500),
    ];

    /// What the language item shows with no language.
    pub const PLAIN_TEXT: &str = "Plain Text";

    /// The label the language item and picker show for a language id.
    pub fn language_label(id: &str) -> SharedString {
        let label = match id {
            "bash" => "Bash",
            "c" => "C",
            "cpp" => "C++",
            "csharp" => "C#",
            "css" => "CSS",
            "dockerfile" => "Dockerfile",
            "go" => "Go",
            "html" => "HTML",
            "java" => "Java",
            "javascript" => "JavaScript",
            "json" => "JSON",
            "kotlin" => "Kotlin",
            "lua" => "Lua",
            "make" => "Makefile",
            "markdown" => "Markdown",
            "php" => "PHP",
            "python" => "Python",
            "ruby" => "Ruby",
            "rust" => "Rust",
            "scala" => "Scala",
            "sql" => "SQL",
            "swift" => "Swift",
            "toml" => "TOML",
            "tsx" => "TSX",
            "typescript" => "TypeScript",
            "xml" => "XML",
            "yaml" => "YAML",
            other => return SharedString::from(other.to_owned()),
        };
        label.into()
    }

    /// The bar's text for a `vim.mode`: an operator waiting is still
    /// normal mode.
    pub(super) fn vim_mode_label(mode: &str) -> SharedString {
        match mode {
            "normal" | "operator_pending" => "NORMAL".into(),
            "insert" => "INSERT".into(),
            "visual" => "VISUAL".into(),
            "visual_line" => "VISUAL LINE".into(),
            "visual_block" => "VISUAL BLOCK".into(),
            other => other.to_uppercase().into(),
        }
    }

    /// The vim mode item's segment for a `vim.mode`, in `theme`'s colours:
    /// an operator waiting is still normal mode.
    pub(super) fn vim_mode_fill(mode: &str, theme: &EditorTheme) -> StatusFill {
        let background = match mode {
            "insert" => theme.vim_mode_insert,
            "visual" | "visual_line" | "visual_block" => theme.vim_mode_visual,
            "replace" => theme.vim_mode_replace,
            _ => theme.vim_mode_normal,
        };
        StatusFill {
            background,
            foreground: theme.vim_mode_foreground,
        }
    }

    /// The bar's text while a jump is pending: `JUMP`, then what's typed
    /// of its query ([`fe_vim::VimState::jump_query`]).
    pub(super) fn jump_label(query: &str) -> SharedString {
        match query {
            "" => "JUMP".into(),
            query => format!("JUMP {query}").into(),
        }
    }
}

/// A tab's label for a buffer at `path`: its file name, or `untitled`.
pub fn file_name(path: Option<&Path>) -> String {
    path.and_then(Path::file_name).map_or_else(
        || "untitled".into(),
        |name| name.to_string_lossy().into_owned(),
    )
}

/// The buttons of the prompt before a dirty editor closes, Cancel the
/// default: Return and Escape keep the editor. GPUI gives Return to the
/// first button and Escape to a cancel button, which would take Return from
/// it, so Cancel is an ok button: Escape reaches it by its title, as AppKit's
/// alerts and kiln-ui's in-window prompt give "Cancel" Escape.
pub const DISCARD_BUTTONS: [PromptButton; 2] = [
    PromptButton::Ok(SharedString::new_static("Cancel")),
    PromptButton::Other(SharedString::new_static("Discard")),
];

/// The answer to [`DISCARD_BUTTONS`] that closes the editor.
const DISCARD: usize = 1;

/// Vim's file and window commands, for a host to run.
impl EditorView {
    /// The [`fe_vim::HostCommand`] (`:w`, `:q` …) the last ex line left,
    /// once; `None` with vim off.
    pub fn take_vim_host_command(&mut self) -> Option<fe_vim::HostCommand> {
        self.vim.as_mut()?.take_host_command()
    }

    /// Shows `message` as vim's ([`status::VIM_MESSAGE`]) until the next
    /// key in the editor: the host's answer to a host command. Nothing
    /// with vim off.
    pub fn set_vim_message(&mut self, message: impl Into<String>, cx: &mut Context<Self>) {
        if let Some(vim) = &mut self.vim {
            vim.set_host_message(message);
            cx.notify();
        }
    }
}

/// The host's command that asks where to save the active editor and saves
/// it there ([`EditorView::save_as`]): what `editor.save` runs on an
/// untitled buffer.
pub const SAVE_AS: &str = "workbench.save_as";

/// The buttons of the prompt before a save overwrites a file that changed
/// on disk.
const OVERWRITE_BUTTONS: [&str; 2] = ["Overwrite", "Cancel"];

/// The answer to [`OVERWRITE_BUTTONS`] that saves over the file.
const OVERWRITE: usize = 0;

/// A save the user wouldn't let overwrite a file that changed on disk:
/// `not saved: main.rs changed on disk`.
#[derive(Debug)]
struct NotSaved(String);

impl fmt::Display for NotSaved {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "not saved: {} changed on disk", self.0)
    }
}

impl std::error::Error for NotSaved {}

/// Whether `error` is a save the person declined, by answering Cancel to the
/// prompt before overwriting a file that changed on disk: nothing to report.
pub fn save_declined(error: &(dyn std::error::Error + 'static)) -> bool {
    error.downcast_ref::<NotSaved>().is_some()
}

thread_local! {
    /// The last save asked for of each buffer being saved, which the next
    /// one waits for. GPUI runs on one thread.
    static SAVES: RefCell<HashMap<BufferId, RunningSave>> = RefCell::default();
    static NEXT_SAVE: Cell<u64> = const { Cell::new(0) };
}

struct RunningSave {
    generation: u64,
    /// Resolves, cancelled, once the save is over.
    over: Shared<oneshot::Receiver<()>>,
}

/// Ends a save in [`SAVES`] when it's over or dropped, unless a later save
/// of the buffer has taken its place.
struct SaveGuard {
    buffer: BufferId,
    generation: u64,
    _over: oneshot::Sender<()>,
}

impl Drop for SaveGuard {
    fn drop(&mut self) {
        let (buffer, generation) = (self.buffer, self.generation);
        SAVES
            .try_with(|saves| {
                let mut saves = saves.borrow_mut();
                if saves
                    .get(&buffer)
                    .is_some_and(|save| save.generation == generation)
                {
                    saves.remove(&buffer);
                }
            })
            .ok();
    }
}

thread_local! {
    /// The reason of the save whose hook runs, per editor.
    static HOOK_REASONS: RefCell<HashMap<EntityId, SaveReason>> = RefCell::default();
    /// What the next [`EditorView::write_copy`] waits for before it writes.
    static COPY_GATE: RefCell<Option<oneshot::Receiver<()>>> = const { RefCell::new(None) };
}

/// Ends an editor's entry in [`HOOK_REASONS`] when its hook is over or the
/// save is dropped.
struct HookReason(EntityId);

impl Drop for HookReason {
    fn drop(&mut self) {
        HOOK_REASONS
            .try_with(|reasons| reasons.borrow_mut().remove(&self.0))
            .ok();
    }
}

impl EditorView {
    /// Whether `reason` lets a save edit the text now: see
    /// [`SaveReason::edits_allowed`], with this editor's vim mode as it is.
    fn edits_allowed(&self, reason: SaveReason) -> bool {
        reason.edits_allowed(self.typing_mode().is_some())
    }

    /// For a [`SaveHook`] to ask before it applies an edit it asked for
    /// earlier: false if the save it runs for doesn't allow edits now, such
    /// as with vim in insert mode since. True with no save running a hook.
    pub fn save_edits_allowed(&self, cx: &Context<Self>) -> bool {
        let reason = HOOK_REASONS.with_borrow(|reasons| reasons.get(&cx.entity_id()).copied());
        reason.is_none_or(|reason| self.edits_allowed(reason))
    }
}

/// For tests: the next copy ([`EditorView::write_copy`]) waits until `gate`
/// is sent to or dropped before it writes, so a test can look at the world
/// while one is under way.
#[doc(hidden)]
pub fn pause_next_copy(gate: oneshot::Receiver<()>) {
    COPY_GATE.with_borrow_mut(|slot| *slot = Some(gate));
}

/// How long a save waits for its [`SaveHook`] before saving anyway.
const SAVE_HOOK_LIMIT: Duration = Duration::from_secs(1);

/// Something to do to an editor's text before it's saved, such as formatting
/// it ([`EditorView::set_save_hook`]): fe-gpui knows nothing of language
/// servers.
pub trait SaveHook {
    /// Edits `editor` as it should be saved; the save goes on when the task
    /// finishes, or after a second.
    fn before_save(&self, editor: &Entity<EditorView>, cx: &mut App) -> Task<()>;
}

impl EditorView {
    /// Sets what runs before each save, or nothing.
    pub fn set_save_hook(&mut self, hook: Option<Rc<dyn SaveHook>>) {
        self.save_hook = hook;
    }

    /// What runs before each save, if anything.
    pub fn save_hook(&self) -> Option<&Rc<dyn SaveHook>> {
        self.save_hook.as_ref()
    }
}

/// What saving fixes in an editor's text, as an `.editorconfig` asks
/// ([`EditorView::set_save_fixes`]).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SaveFixes {
    /// Remove the spaces and tabs at the end of every row.
    pub trim_trailing_whitespace: bool,
    /// End a non-empty text with a line ending.
    pub insert_final_newline: bool,
}

impl EditorView {
    /// Sets what each save fixes in the text, after the save hook runs.
    pub fn set_save_fixes(&mut self, fixes: SaveFixes) {
        self.save_fixes = fixes;
    }

    /// What each save fixes in the text.
    pub fn save_fixes(&self) -> SaveFixes {
        self.save_fixes
    }

    /// Applies the [`SaveFixes`] as one edit of the user, so one undo takes
    /// them back; none if the text needs nothing.
    fn apply_save_fixes(&mut self, fixes: SaveFixes, cx: &mut Context<Self>) {
        let SaveFixes {
            trim_trailing_whitespace: trim,
            insert_final_newline: newline,
        } = fixes;
        if !trim && !newline {
            return;
        }
        let model = &mut self.model;
        // Catch up with edits made through another view of the buffer.
        model.sync_display();
        let mut edits: Vec<(std::ops::Range<usize>, &str)> = Vec::new();
        {
            let text = model.text();
            if trim {
                for range in fe_editor::trailing_whitespace(text, &[]) {
                    edits.push((range, ""));
                }
            }
            let last = text.row_count() - 1;
            let last_start = text.row_start(last);
            let last_end = last_start + text.row_len(last) as usize;
            let trimmed = edits.last().map_or(0, |(range, _)| {
                if range.end == last_end {
                    range.len()
                } else {
                    0
                }
            });
            let last_kept = last_end - last_start - trimmed;
            if newline && last_kept > 0 {
                let end = text.len();
                match edits.last_mut() {
                    Some((range, new)) if range.end == end => *new = "\n",
                    _ => edits.push((end..end, "\n")),
                }
            }
        }
        if edits.is_empty() {
            return;
        }
        // Not a continuation of the typing before it.
        let selections = model.selections();
        model.set_selections(selections);
        // A transaction of its own, even over an open one of the user's
        // (vim's insert session).
        model.buffer_mut().with_transaction_aside(|buffer| {
            buffer.edit(edits, fe_buffer::Origin::User);
        });
        cx.notify();
    }
}

/// Saving off the main thread.
impl EditorView {
    /// Saves the buffer: prepares the save here, writes the file on the
    /// background executor, then records it here, so the buffer is clean
    /// unless edited meanwhile. One save of a buffer runs at a time: one
    /// asked for while another runs waits for it, then saves the text as it
    /// is then.
    ///
    /// A file that changed on disk since it was read or saved asks whether
    /// to overwrite it: Overwrite saves over it, Cancel fails the save with
    /// `not saved: <name> changed on disk`. Other failures are the I/O
    /// error. The save goes on if the task is dropped. Its prompts show in
    /// `window`.
    pub fn save(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Task<gpui::Result<()>> {
        self.save_with(SaveReason::Explicit, window, cx)
    }

    /// [`EditorView::save`] for `reason`. Where [`SaveReason::edits_allowed`]
    /// says no (an auto-save after a delay, or during a vim insert), the
    /// save hook and the [`SaveFixes`] don't run: the file is saved as it
    /// is. An auto reason never prompts: a file changed on disk fails the
    /// task with an error that downcasts to
    /// [`SaveError::ChangedOnDisk`](fe_editor::SaveError), and any other
    /// failure is the I/O error.
    pub fn save_with(
        &mut self,
        reason: SaveReason,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Task<gpui::Result<()>> {
        self.save_in(reason, None, window, cx)
    }

    /// [`EditorView::save`], writing the file in `encoding`, which becomes
    /// the buffer's once the save is recorded, before any save queued
    /// behind it starts. Until then the buffer keeps its own encoding, and
    /// a save that fails leaves it so: one queued behind this one writes in
    /// whichever the buffer has when it starts. A char `encoding` can't
    /// represent fails it, as any save in an encoding fails on one, and so
    /// does a save that isn't recorded ([`EditorModel::finish_save`]): the
    /// file changed on disk after it was written.
    pub fn save_in_encoding(
        &mut self,
        encoding: FileEncoding,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Task<gpui::Result<()>> {
        self.save_in(SaveReason::Explicit, Some(encoding), window, cx)
    }

    /// [`EditorView::save_with`], in `encoding` if one is given (see
    /// [`EditorView::save_in_encoding`]).
    fn save_in(
        &mut self,
        reason: SaveReason,
        encoding: Option<FileEncoding>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Task<gpui::Result<()>> {
        let (previous, guard) = self.queue_save();
        let edits = self.edits_allowed(reason);
        let ask = reason.asks();
        // With none running, the text as it is now, or as the hook leaves
        // it; typing from here on is unsaved.
        let hook = self.save_hook.clone().filter(|_| edits);
        let job = (previous.is_none() && hook.is_none()).then(|| {
            if edits {
                self.apply_save_fixes(self.save_fixes, cx);
            }
            prepare_save_in(&mut self.model, encoding)
        });
        let editor = cx.entity();
        let window = window.window_handle();
        Self::spawn_save(cx, async move |this, cx| {
            if let Some(previous) = previous {
                previous.over.await.ok();
            }
            // Asked again: a save that waited, or a hook that took its time,
            // may have met a vim insert.
            let hook = match hook {
                Some(hook) if this.update(cx, |view, _| view.edits_allowed(reason))? => Some(hook),
                _ => None,
            };
            if let Some(hook) = hook {
                let id = editor.entity_id();
                HOOK_REASONS.with_borrow_mut(|reasons| reasons.insert(id, reason));
                let _over = HookReason(id);
                let task = cx.update(|cx| hook.before_save(&editor, cx));
                let limit = cx.background_executor().timer(SAVE_HOOK_LIMIT);
                if let futures::future::Either::Right(_) =
                    futures::future::select(task, limit).await
                {
                    log::debug!("the save hook took over {SAVE_HOOK_LIMIT:?}; saving anyway");
                }
            }
            if job.is_none() {
                this.update(cx, |view, cx| {
                    if view.edits_allowed(reason) {
                        view.apply_save_fixes(view.save_fixes, cx);
                    }
                })?;
            }
            let saved = save_asking(this, job, encoding, ask.then_some(window), cx).await;
            drop(guard);
            saved
        })
    }

    /// Saves the buffer to `path`, as [`EditorView::save`] does, over the
    /// file there if there is one (the save panel asked about replacing
    /// it), then has it save there from now on: the tab, title and
    /// breadcrumbs follow, in every editor over the buffer, and the language
    /// is detected again from the new name. The old file is left as it
    /// was. A failed save changes nothing, nor one whose file another
    /// writer replaced before it was recorded ([`Saved::is_current`]).
    pub fn save_as(
        &mut self,
        path: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Task<gpui::Result<()>> {
        let fixes = self.save_fixes;
        self.save_as_with(path, None, fixes, true, |_| {}, window, cx)
    }

    /// [`EditorView::save_as`], written with `line_ending` if given (the
    /// new file's setting), which the buffer takes only once the save
    /// commits ([`EditorModel::prepare_save_as`]). `before_commit` is called
    /// once the file is written, just before the buffer takes `path` and is
    /// marked saved, in the same turn: for the host to let go of what it
    /// keeps of the buffer under its old path, such as a language server's
    /// document, which would otherwise hear of the save. Saves of the buffer
    /// asked for earlier are over by then, so the path changes in the order
    /// asked. `fixes` are the destination's: applied to the text before it is
    /// written, as a save applies the file's own. Unless `overwrite`, a file at `path` is refused with an error
    /// whose inner error downcasts to [`SaveError::ChangedOnDisk`].
    #[allow(clippy::too_many_arguments)]
    pub fn save_as_with(
        &mut self,
        path: PathBuf,
        line_ending: Option<fe_buffer::LineEnding>,
        fixes: SaveFixes,
        overwrite: bool,
        before_commit: impl FnOnce(&mut App) + 'static,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) -> Task<gpui::Result<()>> {
        let (previous, guard) = self.queue_save();
        // With none running, the text as it is now; typing from here on is
        // unsaved.
        let job = previous.is_none().then(|| {
            self.apply_save_fixes(fixes, cx);
            self.model
                .prepare_save_as(path.clone(), line_ending, overwrite)
        });
        Self::spawn_save(cx, async move |this, cx| {
            if let Some(previous) = previous {
                previous.over.await.ok();
            }
            let saved = async {
                let job = match job {
                    Some(job) => job,
                    None => this.update(cx, |view, cx| {
                        view.apply_save_fixes(fixes, cx);
                        view.model
                            .prepare_save_as(path.clone(), line_ending, overwrite)
                    })?,
                };
                let saved = run(job, cx).await.map_err(io::Error::from)?;
                // Written, then replaced by another writer: the buffer isn't
                // what's there, so it stays where it was.
                let replaced = || {
                    let name = file_name(Some(&path));
                    io::Error::other(format!("{name} was replaced while it was being saved"))
                };
                if !saved.is_current() {
                    return Err(replaced().into());
                }
                cx.update(before_commit);
                this.update(cx, |view, cx| {
                    if !view.model.finish_save_as(saved) {
                        return Err(replaced());
                    }
                    view.detect_language(cx);
                    Ok(())
                })??;
                Ok(())
            }
            .await;
            drop(guard);
            saved
        })
    }

    /// Writes the buffer as it is now to `path` off the UI thread, as a
    /// copy: the buffer keeps its own path and its dirty state
    /// ([`EditorModel::prepare_save_to`], never finished). Unless
    /// `overwrite`, a file already there is refused with an error whose
    /// inner error downcasts to [`SaveError::ChangedOnDisk`]. It waits its
    /// turn in the buffer's queue of saves, so writes to one file land in
    /// the order asked.
    pub fn write_copy(
        &mut self,
        path: PathBuf,
        overwrite: bool,
        cx: &mut Context<Self>,
    ) -> Task<io::Result<()>> {
        let (previous, guard) = self.queue_save();
        let job = self.model.prepare_save_to(path, overwrite);
        let gate = COPY_GATE.with_borrow_mut(Option::take);
        cx.spawn(async move |_, cx| {
            if let Some(previous) = previous {
                previous.over.await.ok();
            }
            if let Some(gate) = gate {
                gate.await.ok();
            }
            let written = run(job, cx).await.map(drop).map_err(io::Error::from);
            drop(guard);
            written
        })
    }

    /// Highlights the buffer in the language its path and first lines say,
    /// unless it already is.
    pub fn detect_language(&mut self, cx: &mut Context<Self>) {
        let text = self.model.text();
        let start =
            text.slice_to_string(0..text.clip_offset(crate::view::DETECT_BYTES, Bias::Left));
        let language = fe_syntax::detect_language_name(self.model.path(), &start);
        let current = match self.syntax() {
            Some(syntax) => Some(syntax.language().name()),
            None if self.is_loading_language() => None,
            None => Some(""),
        };
        if current.is_none() || current != Some(language.unwrap_or("")) {
            self.set_language(language, cx);
        }
        self.language_chosen = false;
        cx.notify();
    }

    /// Queues a save of the buffer: the save running or waiting before it,
    /// if any, which it waits for, and what ends it in [`SAVES`].
    fn queue_save(&self) -> (Option<RunningSave>, SaveGuard) {
        let buffer = self.model.buffer().id();
        let generation = NEXT_SAVE.replace(NEXT_SAVE.get() + 1);
        let (over, done) = oneshot::channel();
        let running = RunningSave {
            generation,
            over: done.shared(),
        };
        let previous = SAVES.with_borrow_mut(|saves| saves.insert(buffer, running));
        let guard = SaveGuard {
            buffer,
            generation,
            _over: over,
        };
        (previous, guard)
    }

    /// Runs `save`, then redraws the view, which shows it saved; the task
    /// resolves to its outcome. The save goes on if the task is dropped.
    fn spawn_save(
        cx: &mut Context<Self>,
        save: impl AsyncFnOnce(&WeakEntity<Self>, &mut AsyncApp) -> gpui::Result<()> + 'static,
    ) -> Task<gpui::Result<()>> {
        let (send, result) = oneshot::channel();
        cx.spawn(async move |this, cx| {
            let saved = save(&this, cx).await;
            this.update(cx, |_, cx| cx.notify()).ok();
            send.send(saved).ok();
        })
        .detach();
        cx.foreground_executor().spawn(async move {
            result
                .await
                .unwrap_or_else(|_| Err(io::Error::other("the save was dropped").into()))
        })
    }

    /// Whether a save of the buffer is running or waiting, in this editor
    /// or another over the same buffer.
    pub fn is_saving(&self) -> bool {
        let buffer = self.model.buffer().id();
        SAVES.with_borrow(|saves| saves.contains_key(&buffer))
    }

    /// `editor.save` from a key or menu: [`EditorView::save`], showing why
    /// it failed, unless the user cancelled it. An untitled buffer runs
    /// [`SAVE_AS`] instead, in the window's context, for the host to ask
    /// where.
    /// A host that shows the view's file another way, such as a preview in
    /// its place, runs it for `editor.save` there.
    pub fn save_from_command(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.model.path().is_none() {
            let stack = crate::keymap::context_stack(&window.context_stack());
            let invocation = Invocation::of(SAVE_AS)
                .expect("a valid command ID")
                .context(stack);
            // Deferred: the view, which handles it first, is out until then.
            return window.dispatch_action(Box::new(RunCommand(invocation)), cx);
        }
        let saved = self.save(window, cx);
        let name = file_name(self.model.path());
        let window = window.window_handle();
        cx.spawn(async move |_, cx| {
            let Err(error) = saved.await else {
                return;
            };
            if save_declined(&*error) {
                return;
            }
            let detail = format!("{error:#}");
            let answer = cx.update_window(window, |_, window, cx| {
                window.prompt(
                    PromptLevel::Critical,
                    &format!("“{name}” couldn’t be saved."),
                    Some(&detail),
                    &["OK"],
                    cx,
                )
            });
            if let Ok(answer) = answer {
                answer.await.ok();
            }
        })
        .detach();
    }
}

/// Prepares a save of `model`'s buffer, in `encoding` if one is given,
/// without changing the buffer's own.
fn prepare_save_in(model: &mut EditorModel, encoding: Option<FileEncoding>) -> io::Result<SaveJob> {
    let Some(encoding) = encoding else {
        return model.prepare_save(false);
    };
    let own = model.buffer().encoding();
    model.buffer_mut().set_encoding(encoding);
    let job = model.prepare_save(false);
    model.buffer_mut().set_encoding(own);
    job
}

/// A save of `this`'s buffer, from `job` if it was prepared, else of the
/// text as it is now, in `encoding` if one is given, which becomes the
/// buffer's once the save is recorded. If the file changed on disk and the
/// user, asked in `window`, says to overwrite it, the same text is saved
/// over it: what was typed since stays unsaved. With no `window` it doesn't
/// ask: the conflict is the error.
async fn save_asking(
    this: &WeakEntity<EditorView>,
    job: Option<io::Result<SaveJob>>,
    encoding: Option<FileEncoding>,
    window: Option<AnyWindowHandle>,
    cx: &mut AsyncApp,
) -> gpui::Result<()> {
    let job = match job {
        Some(job) => job,
        None => this.update(cx, |view, _| prepare_save_in(&mut view.model, encoding))?,
    }?;
    let name = file_name(Some(job.path()));
    let saved = match run(job.clone(), cx).await {
        Err(SaveError::ChangedOnDisk) if let Some(window) = window => {
            let answer = cx.update_window(window, |_, window, cx| {
                window.prompt(
                    PromptLevel::Warning,
                    &format!("“{name}” changed on disk since it was opened."),
                    Some("Overwrite it with your version?"),
                    &OVERWRITE_BUTTONS,
                    cx,
                )
            })?;
            if answer.await != Ok(OVERWRITE) {
                return Err(NotSaved(name).into());
            }
            run(job.overwriting(), cx).await
        }
        saved => saved,
    };
    let saved = saved.map_err(io::Error::from)?;
    let recorded = this.update(cx, |view, _| {
        let recorded = view.model.finish_save(saved);
        if let Some(encoding) = encoding
            && recorded
        {
            view.model.buffer_mut().set_encoding(encoding);
        }
        recorded
    })?;
    // In an encoding, the buffer keeps its own unless the save is its.
    if let Some(encoding) = encoding
        && !recorded
    {
        return Err(io::Error::other(format!(
            "the file changed on disk after it was written in {}; \
             the buffer keeps its encoding",
            encoding.label()
        ))
        .into());
    }
    Ok(())
}

/// Runs `job` on the background executor.
async fn run(job: SaveJob, cx: &mut AsyncApp) -> Result<Saved, SaveError> {
    cx.background_spawn(async move { job.run() }).await
}

impl PaneItem for EditorView {
    const KIND: &'static str = "fastedit.editor";

    fn tab_title(&self, _: &App) -> SharedString {
        file_name(self.model.path()).into()
    }

    /// Its file's icon in the project tree; an untitled buffer's is a
    /// plain file's.
    fn tab_icon(&self, cx: &App) -> Option<Icon> {
        Some(file_icon(&self.tab_title(cx)))
    }

    fn tab_title_color(&self, _: &App) -> Option<Hsla> {
        self.tab_tint
    }

    /// `main.rs — fastedit`, `main.rs ● — fastedit` with unsaved changes.
    fn window_title(&self, cx: &App) -> SharedString {
        let dirty = if self.is_dirty(cx) { " ●" } else { "" };
        format!("{}{dirty} — fastedit", self.tab_title(cx)).into()
    }

    fn is_dirty(&self, _: &App) -> bool {
        self.model.is_dirty()
    }

    /// Another editor over its buffer ([`EditorModel::split`](fe_editor::EditorModel::split)),
    /// or a diff view whose right side is.
    fn shares_document(&self, other: &AnyView, cx: &App) -> bool {
        if let Ok(diff) = other.clone().downcast::<DiffView>() {
            return diff
                .read(cx)
                .right_model(cx)
                .shares_buffer_with(&self.model);
        }
        other
            .clone()
            .downcast::<EditorView>()
            .is_ok_and(|other| other.read(cx).model.shares_buffer_with(&self.model))
    }

    /// A dirty editor asks whether to discard its changes: Discard closes
    /// it, Cancel keeps it and is the default, with Return and Escape (see
    /// [`DISCARD_BUTTONS`]). A clean one closes. The shell doesn't ask while
    /// another editor shows its buffer.
    ///
    /// While the app quits ([`kiln_shell::layout::quitting`]), a dirty
    /// editor whose buffer is backed up ([`back_up_to_close`]) closes
    /// without asking: the next launch restores it. A host's quit can ask
    /// that of its own close transaction alone, through
    /// [`back_up_to_close`].
    fn may_close(&mut self, window: &mut Window, cx: &mut Context<Self>) -> CloseAnswer {
        if !self.is_dirty(cx) {
            return CloseAnswer::Close;
        }
        if kiln_shell::layout::quitting(cx) && back_up_to_close(self, cx) {
            return CloseAnswer::Close;
        }
        let question = format!(
            "Discard unsaved changes to {}?",
            file_name(self.model.path())
        );
        let answer = window.prompt(
            PromptLevel::Warning,
            &question,
            Some("Your changes will be lost if you close it without saving."),
            &DISCARD_BUTTONS,
            cx,
        );
        CloseAnswer::Ask(cx.spawn(async move |_, _| answer.await == Ok(DISCARD)))
    }

    /// Changes with its dirtiness and with every edit to its buffer.
    fn close_revision(&self, _: &App) -> u64 {
        let buffer = self.model.buffer();
        let mut hasher = DefaultHasher::new();
        (self.model.is_dirty(), buffer.id(), buffer.version()).hash(&mut hasher);
        hasher.finish()
    }

    /// [`EditorView::save`]: off the main thread, asking before it
    /// overwrites a file changed on disk.
    fn save(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<Task<gpui::Result<()>>> {
        Some(EditorView::save(self, window, cx))
    }

    fn path(&self, _: &App) -> Option<&Path> {
        self.model.path()
    }

    // `breadcrumbs` keeps the default: the symbols enclosing the caret
    // come from the outline the workbench keeps, which pushes them into
    // the pane.

    /// See [`status`].
    fn status_items(&self, _: &App) -> Vec<StatusItem> {
        status_items(self)
    }

    /// An editor over the same buffer, with its own view of it, in this
    /// one's theme.
    fn split(&mut self, _: &mut Window, cx: &mut Context<Self>) -> Option<Entity<Self>> {
        let model = self.model.split();
        let theme = self.theme.clone();
        Some(cx.new(|cx| EditorView::new(model, theme, cx)))
    }

    /// Its file's path, or for an untitled buffer its title, and its
    /// caret, the primary selection's head; and with unsaved changes the id
    /// of the buffer's backup ([`set_buffer_backup`]), as `backup`. An
    /// untitled buffer without one, or a file at a path that isn't UTF-8,
    /// isn't saved.
    fn serialize(&self, cx: &App) -> Option<Value> {
        let backup = (self.model.is_dirty())
            .then(|| buffer_backup(self.model.buffer().id(), cx))
            .flatten();
        let mut state = Vec::new();
        match self.model.path() {
            Some(path) => state.push(("path", Value::from(path.to_str()?))),
            None => {
                backup.as_ref()?;
                state.push(("title", Value::from(self.tab_title(cx).as_ref())));
            }
        }
        state.push(("caret", Value::from(self.model.primary_selection().head())));
        if let Some(backup) = backup {
            state.push(("backup", Value::from(backup.id().to_string())));
        }
        Some(Value::from_iter(state))
    }

    /// Reads `state`'s backup, if it names one in the window's store
    /// ([`set_window_backups`]) that no live editor owns and that reads,
    /// restored over its file ([`restore_backup`]), else `state`'s file, off
    /// the main thread, and opens it, in the theme and settings of
    /// [`set_restore_style`], its caret placed, clamped to the text. A
    /// restored backup stays the buffer's, owned ([`BufferBackup::claim`]).
    /// A file that's gone, or can't be read, without a backup, isn't
    /// restored.
    fn restore(state: Value, window: &mut Window, cx: &mut App) -> Task<Option<Entity<Self>>> {
        let path = state.get("path").and_then(Value::as_str).map(PathBuf::from);
        let backup = (state.get("backup").and_then(Value::as_str))
            .and_then(|id| id.parse::<BackupId>().ok());
        if path.is_none() && backup.is_none() {
            return Task::ready(None);
        }
        let window = window.window_handle().window_id();
        cx.spawn(async move |cx| {
            // Looked up once the window's host is built: it gives the
            // window its store after restoring its layout.
            let store = backup.and_then(|_| cx.update(|cx| window_backups(window, cx)));
            let read = cx.background_spawn({
                let path = path.clone();
                async move {
                    if let (Some(store), Some(id)) = (store, backup) {
                        let read = BufferBackup::claim(store, id).and_then(|backup| {
                            let backup = backup.ok_or_else(|| io::Error::other("it's in use"))?;
                            let (buffer, path) = restore_backup(&backup.read()?)?;
                            Ok((buffer, path, Some(backup)))
                        });
                        match read {
                            Ok(read) => return Ok(read),
                            Err(error) => log::info!("not restoring backup {id}: {error}"),
                        }
                    }
                    let path = path.ok_or_else(|| io::Error::other("no file and no backup"))?;
                    let file = std::fs::File::open(&path)?;
                    // Stamped, so a save refuses a file changed since.
                    Ok::<_, io::Error>((Buffer::from_file(file)?, Some(path), None))
                }
            });
            let (buffer, path, backup) = match read.await {
                Ok(read) => read,
                Err(error) => {
                    let name = path.as_deref().unwrap_or(Path::new("untitled"));
                    log::info!("not restoring {}: {error}", name.display());
                    return None;
                }
            };
            cx.update(|cx| {
                let (theme, settings) = restore_style(cx);
                let mut model = EditorModel::new(buffer, path);
                model.set_tab_size(settings.tab_size);
                model.set_soft_wrap(settings.soft_wrap);
                model.set_line_numbers(crate::line_numbers(settings.line_numbers));
                model.set_scroll_off(settings.cursor_surrounding_lines);
                model.set_cursor_line(settings.cursor_line);
                model.set_auto_close(settings.auto_close_pairs);
                let view = cx.new(|cx| EditorView::new(model, theme, cx));
                view.update(cx, |view, cx| place_caret(view, &state, cx));
                if let Some(backup) = backup {
                    adopt_backup(&view, backup, cx);
                }
                Some(view)
            })
        })
    }

    /// Places the caret `state` saved, clamped to the text.
    fn apply_state(&mut self, state: &Value, _: &mut Window, cx: &mut Context<Self>) {
        place_caret(self, state, cx);
    }
}

/// Puts `editor`'s caret where `state` says, clamped to its text, and
/// scrolls to it.
fn place_caret(editor: &mut EditorView, state: &Value, cx: &mut Context<EditorView>) {
    let Some(caret) = state.get("caret").and_then(Value::as_u64) else {
        return;
    };
    let text = editor.model.text();
    let caret = text.clip_offset((caret as usize).min(text.len()), Bias::Left);
    editor.select_and_reveal(caret..caret, cx);
}

/// What restored editors open with: the host's theme and editor
/// settings.
struct RestoreStyle(EditorTheme, EditorSettings);

impl Global for RestoreStyle {}

/// Has editors restored from a saved layout open in `theme`, with
/// `settings`' tab size and soft wrap, as the host's own do. Until it's
/// called, they open in the default dark theme with default settings.
pub fn set_restore_style(cx: &mut App, theme: EditorTheme, settings: EditorSettings) {
    cx.set_global(RestoreStyle(theme, settings));
}

pub(crate) fn restore_style(cx: &App) -> (EditorTheme, EditorSettings) {
    match cx.try_global::<RestoreStyle>() {
        Some(RestoreStyle(theme, settings)) => (theme.clone(), settings.clone()),
        None => (EditorTheme::default_dark(cx), EditorSettings::default()),
    }
}

/// Hot exit's part in the editor: each window's backup store, which its
/// restored editors read their backups from ([`set_window_backups`]), and
/// each buffer's backup ([`set_buffer_backup`]), which a dirty editor's
/// saved state names and which it writes to close as the app quits
/// ([`back_up_to_close`]). When to write and remove backups is the host's.
#[derive(Default)]
struct Backups {
    windows: HashMap<WindowId, BackupStore>,
    buffers: HashMap<BufferId, BufferBackup>,
}

impl Global for Backups {}

/// Has the editors restored in `window` read their backups from `store`,
/// or from none.
pub fn set_window_backups(window: WindowId, store: Option<BackupStore>, cx: &mut App) {
    let windows = &mut cx.default_global::<Backups>().windows;
    match store {
        Some(store) => windows.insert(window, store),
        None => windows.remove(&window),
    };
}

/// Where `window`'s backups are kept, if it has a store.
pub fn window_backups(window: WindowId, cx: &App) -> Option<BackupStore> {
    cx.try_global::<Backups>()?.windows.get(&window).cloned()
}

/// Gives `buffer` its backup, or takes it away.
pub fn set_buffer_backup(buffer: BufferId, backup: Option<BufferBackup>, cx: &mut App) {
    let buffers = &mut cx.default_global::<Backups>().buffers;
    match backup {
        Some(backup) => buffers.insert(buffer, backup),
        None => buffers.remove(&buffer),
    };
}

/// `buffer`'s backup, if it has one.
pub fn buffer_backup(buffer: BufferId, cx: &App) -> Option<BufferBackup> {
    cx.try_global::<Backups>()?.buffers.get(&buffer).cloned()
}

/// Gives `editor`'s buffer `backup`, read into it just now
/// ([`BufferBackup::read`]): it holds the buffer as it is.
pub fn adopt_backup(editor: &Entity<EditorView>, backup: BufferBackup, cx: &mut App) {
    let editor = editor.read(cx);
    backup.landed().holds = Some(BackupRevision::of(editor));
    let buffer = editor.model.buffer().id();
    set_buffer_backup(buffer, Some(backup), cx);
}

/// Whether `editor`'s unsaved changes are in its buffer's backup, written
/// now if it's behind them: so it may close, as the app quits, without
/// asking. `false` without a backup, or if it can't be written.
pub fn back_up_to_close(editor: &EditorView, cx: &App) -> bool {
    let Some(backup) = buffer_backup(editor.model.buffer().id(), cx) else {
        return false;
    };
    if backup.holds(&BackupRevision::of(editor)) {
        return true;
    }
    match backup.write_job(editor).run() {
        Ok(()) => true,
        Err(error) => {
            let name = file_name(editor.model.path());
            log::error!("can't back up {name}, so asking: {error}");
            false
        }
    }
}

/// What a backup of a buffer restores: its text's version, and what it
/// saves as and where, and the file as the buffer last read or saved it. A
/// backup behind any of it is written again.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BackupRevision {
    version: Version,
    path: Option<PathBuf>,
    line_ending: fe_buffer::LineEnding,
    encoding: fe_text::FileEncoding,
    disk_stamp: Option<fe_buffer::DiskStamp>,
}

impl BackupRevision {
    /// `editor`'s buffer's, as it is now.
    pub fn of(editor: &EditorView) -> Self {
        let buffer = editor.model.buffer();
        Self {
            version: buffer.version(),
            path: editor.model.path().map(Path::to_owned),
            line_ending: buffer.line_ending(),
            encoding: buffer.encoding(),
            disk_stamp: buffer.disk_stamp(),
        }
    }
}

/// The top bit of a backup id's process id, which no process id has: set,
/// it names a buffer's second backup slot ([`twin`]).
const TWIN_BIT: u32 = 1 << 31;

/// The id of the second slot of the backup `id` (see [`BufferBackup`]):
/// `id` with the top bit of its process id set.
pub fn twin(id: BackupId) -> BackupId {
    with_pid(id, |pid| pid | TWIN_BIT)
}

/// The backup a slot's id belongs to: the id itself, or the one it's the
/// [`twin`] of.
pub fn primary(id: BackupId) -> BackupId {
    with_pid(id, |pid| pid & !TWIN_BIT)
}

/// `id` with its process id, the middle of its name, mapped by `map`.
fn with_pid(id: BackupId, map: impl Fn(u32) -> u32) -> BackupId {
    let name = id.to_string();
    let mut parts = name.splitn(3, '-');
    let (Some(millis), Some(pid), Some(counter)) = (parts.next(), parts.next(), parts.next())
    else {
        return id;
    };
    let Ok(pid) = pid.parse::<u32>() else {
        return id;
    };
    format!("{millis}-{}-{counter}", map(pid))
        .parse()
        .unwrap_or(id)
}

/// A buffer's backup: its store and id, and what was last written or
/// removed of it. Writes and removals ([`BackupJob`]) each take a turn as
/// they're made, and run under a lock, skipping any made before one that
/// has run: so one runs at a time, and they land in the order they were
/// made, whatever thread runs them.
///
/// It's kept in two slots, the id and its [`twin`], and each write goes to
/// the slot that doesn't hold the newest backup: as a store's write first
/// uncommits what it replaces, a crash part-way through one leaves the
/// other, a write older. A read takes the newer of the two that read
/// ([`read_newest`]).
///
/// While its backup is on disk, the editor that keeps it owns it: it holds
/// an exclusive lock on `<id>.owner` beside it, which the OS drops if the
/// process dies. A launch reopens only backups no one owns
/// ([`BufferBackup::claim`]). Owner files are made, locked and removed only
/// under the store's `owners.lock`, so one is never removed between being
/// opened and locked, nor locked once removed.
#[derive(Clone, Debug)]
pub struct BufferBackup {
    store: BackupStore,
    id: BackupId,
    /// Held while a job runs, with the owner lock, if taken.
    running: Arc<Mutex<Option<std::fs::File>>>,
    /// Held only to read or note what landed, so the main thread never
    /// waits on a write.
    landed: Arc<Mutex<Landed>>,
}

#[derive(Debug, Default)]
struct Landed {
    turn: u64,
    /// What the backup holds; `None` while it's removed, or being written.
    holds: Option<BackupRevision>,
    /// The slot holding the newest backup, if either does, and when its
    /// meta says it was written.
    newest: Option<(BackupId, SystemTime)>,
}

static NEXT_TURN: AtomicU64 = AtomicU64::new(1);

impl BufferBackup {
    /// A backup `id` in `store`, not yet written, nor owned until it is.
    pub fn new(store: BackupStore, id: BackupId) -> Self {
        Self {
            store,
            id: primary(id),
            running: Arc::default(),
            landed: Arc::default(),
        }
    }

    /// The backup `id` in `store`, owned: `None` if a live editor, in this
    /// process or another, owns it.
    pub fn claim(store: BackupStore, id: BackupId) -> io::Result<Option<Self>> {
        let backup = Self::new(store, id);
        let Some(owner) = backup.try_own()? else {
            return Ok(None);
        };
        *backup.running() = Some(owner);
        Ok(Some(backup))
    }

    /// The id it's named by; its slots are this and its [`twin`].
    pub fn id(&self) -> BackupId {
        self.id
    }

    pub fn store(&self) -> &BackupStore {
        &self.store
    }

    /// Reads the newer of its slots that read ([`read_newest`]), noting
    /// it as the newest.
    pub fn read(&self) -> io::Result<Backup> {
        let backup = read_newest(&self.store, self.id)?;
        self.landed().newest = Some((backup.id, backup.meta.written));
        Ok(backup)
    }

    /// Whether the backup holds `revision` of its buffer.
    pub fn holds(&self, revision: &BackupRevision) -> bool {
        self.landed().holds.as_ref() == Some(revision)
    }

    /// A write of `editor`'s buffer as it is now.
    pub fn write_job(&self, editor: &EditorView) -> BackupJob {
        let buffer = editor.model.buffer();
        let path = editor.model.path().map(Path::to_owned);
        let title = file_name(path.as_deref());
        let meta = BackupMeta::for_buffer(&buffer, path, title);
        let text = buffer.text().clone();
        drop(buffer);
        self.job(Some((meta, text, BackupRevision::of(editor))))
    }

    /// A removal of both slots, and of the owner lock.
    pub fn remove_job(&self) -> BackupJob {
        self.job(None)
    }

    fn job(&self, write: Option<(BackupMeta, Text, BackupRevision)>) -> BackupJob {
        BackupJob {
            backup: self.clone(),
            turn: NEXT_TURN.fetch_add(1, Ordering::Relaxed),
            write,
        }
    }

    fn landed(&self) -> std::sync::MutexGuard<'_, Landed> {
        self.landed.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn running(&self) -> std::sync::MutexGuard<'_, Option<std::fs::File>> {
        self.running.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The owner lock, taken: `None` if someone holds it.
    fn try_own(&self) -> io::Result<Option<std::fs::File>> {
        let owner = owner_file(&self.store, self.id);
        with_owners(&self.store, || try_lock(&owner))
    }
}

/// The newer of the two slots of the backup `id` in `store` (see
/// [`BufferBackup`]) that read: the later `written`, which each write keeps
/// past the slot before it, whatever the clock says. If neither reads, the
/// error is [`io::ErrorKind::InvalidData`] only if each slot is corrupt or
/// missing, and not both missing: a slot that failed otherwise (it can't be
/// opened, say) may yet read, so the backup isn't given up as corrupt.
pub fn read_newest(store: &BackupStore, id: BackupId) -> io::Result<Backup> {
    let mut newest: Option<Backup> = None;
    let mut failed: Vec<io::Error> = Vec::new();
    for slot in [primary(id), twin(id)] {
        match store.read(slot) {
            Ok(backup) => {
                if (newest.as_ref()).is_none_or(|newest| backup.meta.written > newest.meta.written)
                {
                    newest = Some(backup);
                }
            }
            Err(error) => failed.push(error),
        }
    }
    if let Some(backup) = newest {
        return Ok(backup);
    }
    let kind = |kind| move |error: &io::Error| error.kind() == kind;
    let recoverable = failed.iter().position(|error| {
        !kind(io::ErrorKind::InvalidData)(error) && !kind(io::ErrorKind::NotFound)(error)
    });
    let pick = recoverable
        .or_else(|| failed.iter().position(kind(io::ErrorKind::InvalidData)))
        .unwrap_or(0);
    Err(failed.swap_remove(pick))
}

/// `<id>.owner` in `store`: see [`BufferBackup`].
fn owner_file(store: &BackupStore, id: BackupId) -> PathBuf {
    store.dir().join(format!("{id}.owner"))
}

/// Runs `f` holding `store`'s `owners.lock` exclusively, making the store's
/// directory, owner-only, if need be.
fn with_owners<T>(store: &BackupStore, f: impl FnOnce() -> io::Result<T>) -> io::Result<T> {
    let mut dir = std::fs::DirBuilder::new();
    dir.recursive(true);
    #[cfg(unix)]
    std::os::unix::fs::DirBuilderExt::mode(&mut dir, 0o700);
    dir.create(store.dir())?;
    let lock = private_file(&store.dir().join("owners.lock"))?;
    lock.lock()?;
    f()
}

/// The file at `path`, opened to read and write, created owner-only if
/// need be.
fn private_file(path: &Path) -> io::Result<std::fs::File> {
    let mut options = std::fs::OpenOptions::new();
    options.read(true).write(true).create(true).truncate(false);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    options.open(path)
}

/// The file at `path` locked exclusively: `None` if someone holds the lock.
fn try_lock(path: &Path) -> io::Result<Option<std::fs::File>> {
    let file = private_file(path)?;
    match file.try_lock() {
        Ok(()) => Ok(Some(file)),
        Err(std::fs::TryLockError::WouldBlock) => Ok(None),
        Err(std::fs::TryLockError::Error(error)) => Err(error),
    }
}

/// Removes `store`'s owner files whose backup isn't among `ids` (slots of
/// either kind) and that no one holds.
pub fn prune_owners(store: &BackupStore, ids: &[BackupId]) -> io::Result<()> {
    let live: HashSet<BackupId> = ids.iter().copied().map(primary).collect();
    with_owners(store, || {
        for entry in std::fs::read_dir(store.dir())?.flatten() {
            let name = entry.file_name();
            let Some(id) = (name.to_str())
                .and_then(|name| name.strip_suffix(".owner"))
                .and_then(|id| id.parse::<BackupId>().ok())
            else {
                continue;
            };
            if !live.contains(&id)
                && let Some(_held) = try_lock(&entry.path())?
            {
                std::fs::remove_file(entry.path())?;
            }
        }
        Ok(())
    })
}

/// A write or removal of a [`BufferBackup`], made on the main thread, to
/// run on any.
pub struct BackupJob {
    backup: BufferBackup,
    turn: u64,
    /// What to write, or `None` to remove the backup.
    write: Option<(BackupMeta, Text, BackupRevision)>,
}

impl BackupJob {
    /// Writes or removes the backup, unless a job made after this one has
    /// run. A write owns the backup first, then writes the slot that
    /// doesn't hold the newest; a removal removes both, then lets the
    /// backup go.
    pub fn run(self) -> io::Result<()> {
        let backup = &self.backup;
        let mut owner = backup.running();
        let newest = {
            let mut landed = backup.landed();
            if landed.turn > self.turn {
                return Ok(());
            }
            landed.turn = self.turn;
            landed.holds = None;
            landed.newest
        };
        match self.write {
            Some((mut meta, text, revision)) => {
                if owner.is_none() {
                    *owner = backup.try_own()?;
                }
                if owner.is_none() {
                    return Err(io::Error::other(format!(
                        "backup {} is another editor's",
                        backup.id
                    )));
                }
                let slot = match newest {
                    Some((newest, _)) if newest == backup.id => twin(backup.id),
                    _ => backup.id,
                };
                // Past the newest slot's, so a read takes this one even if
                // the clock has gone back.
                if let Some((_, written)) = newest {
                    meta.written = meta.written.max(written + Duration::from_nanos(1));
                }
                backup.store.write(slot, &meta, &text)?;
                let mut landed = backup.landed();
                landed.holds = Some(revision);
                landed.newest = Some((slot, meta.written));
            }
            None => {
                for slot in [backup.id, twin(backup.id)] {
                    backup.store.remove(slot)?;
                }
                backup.landed().newest = None;
                if owner.is_some() {
                    let path = owner_file(&backup.store, backup.id);
                    with_owners(&backup.store, || {
                        let removed = std::fs::remove_file(&path);
                        // Let go only once it's gone.
                        drop(owner.take());
                        removed
                    })?;
                }
            }
        }
        Ok(())
    }
}

/// Restores `backup` over its file as the file is now, or over nothing
/// for an untitled buffer or a file that's gone ([`Buffer::from_backup`]):
/// the buffer, dirty, one undo of the user's from the file's text and save
/// format, and the backup's path.
pub fn restore_backup(backup: &Backup) -> io::Result<(Buffer, Option<PathBuf>)> {
    let path = backup.meta.path.clone();
    let disk = match path.as_deref().map(std::fs::File::open) {
        Some(Ok(file)) => Some(file),
        Some(Err(error)) if error.kind() == io::ErrorKind::NotFound => None,
        Some(Err(error)) => return Err(error),
        None => None,
    };
    Ok((Buffer::from_backup(disk, backup)?, path))
}

/// `editor`'s items: see [`status`].
pub(crate) fn status_items(editor: &EditorView) -> Vec<StatusItem> {
    let model = &editor.model;
    let text = model.text();
    let selections = model.selections();
    let primary = model.primary_selection();
    let head = if primary.reversed {
        primary.range.start
    } else {
        primary.range.end
    };
    let row = text.offset_to_point(head).row;
    let column = text.offset_to_char(head) - text.offset_to_char(text.row_start(row)) + 1;
    let mut cursor = format!("Ln {}, Col {column}", row + 1);
    if selections.len() > 1 {
        cursor += &format!(" ({} selections)", selections.len());
    } else if !primary.range.is_empty() {
        let chars =
            text.offset_to_char(primary.range.end) - text.offset_to_char(primary.range.start);
        cursor += &format!(" ({chars} selected)");
    }
    let encoding = model.buffer().encoding().label();
    let line_ending = match model.buffer().line_ending() {
        fe_buffer::LineEnding::Lf => "LF",
        fe_buffer::LineEnding::CrLf => "CRLF",
    };
    let language = match editor.syntax() {
        Some(syntax) => status::language_label(syntax.language().name()),
        None if editor.is_loading_language() => "…".into(),
        None => status::PLAIN_TEXT.into(),
    };
    let vim_mode = editor.vim_mode().map(|mode| {
        // A pending jump is normal mode's colour, whatever mode it left.
        let (text, fill_mode) = match editor.vim_state().and_then(VimState::jump_query) {
            Some(query) => (status::jump_label(query), "normal"),
            None => (status::vim_mode_label(mode), mode),
        };
        StatusItem::new(status::VIM_MODE, StatusSide::Left, 0, text)
            .fill(status::vim_mode_fill(fill_mode, editor.theme()))
    });
    let vim_message = editor
        .vim_state()
        .and_then(VimState::message)
        .map(|message| {
            StatusItem::new(
                status::VIM_MESSAGE,
                StatusSide::Left,
                10,
                message.to_owned(),
            )
        });
    let indent = match model.indent_style() {
        Some(IndentStyle::Tabs) => format!("Tab Size: {}", model.tab_size()),
        Some(IndentStyle::Spaces(n)) => format!("Spaces: {n}"),
        None => format!("Spaces: {}", model.tab_size()),
    };
    let language_command = Invocation::of("language.select").expect("a valid command ID");
    let go_to_line_command = Invocation::of("go_to_line.toggle").expect("a valid command ID");
    let texts: [(SharedString, Option<(Invocation, &str)>); 5] = [
        (
            cursor.into(),
            Some((go_to_line_command, "Go to Line/Column")),
        ),
        (indent.into(), None),
        (encoding.into(), None),
        (line_ending.into(), None),
        (language, Some((language_command, "Change Language Mode"))),
    ];
    let right = status::RIGHT
        .into_iter()
        .zip(texts)
        .map(|((id, priority), (text, command))| {
            let item = StatusItem::new(id, StatusSide::Right, priority, text);
            match command {
                Some((command, tooltip)) => item.command(command).tooltip(tooltip),
                None => item,
            }
        });
    vim_mode
        .into_iter()
        .chain(vim_message)
        .chain(right)
        .collect()
}

#[cfg(test)]
#[path = "pane_item_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "pane_item_save_tests.rs"]
mod save_tests;

#[cfg(test)]
#[path = "save_fixes_tests.rs"]
mod save_fixes_tests;

#[cfg(test)]
#[path = "save_hook_tests.rs"]
mod save_hook_tests;

#[cfg(test)]
#[path = "save_reason_tests.rs"]
mod save_reason_tests;

#[cfg(test)]
#[path = "indent_status_tests.rs"]
mod indent_status_tests;

#[cfg(test)]
#[path = "vim_mode_status_tests.rs"]
mod vim_mode_status_tests;
