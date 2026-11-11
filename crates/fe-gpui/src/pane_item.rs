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
    Backup, BackupId, BackupMeta, BackupStore, Bias, Buffer, BufferId, FileEncoding, LocalHistory,
    UndoRecord, UndoStore, Version, local_history::Reason,
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
use kiln_l10n::tr;
use kiln_shell::{CloseAnswer, Icon, IconRef, PaneItem, StatusItem, StatusSide, file_icon_ref};

use crate::{DiffView, EditorSettings, EditorTheme, EditorView, commands::RunCommand};

/// An editor's status bar items, while it's the active item: the vim mode
/// with vim on, and vim's last message, on the left; the cursor,
/// indentation, encoding, line ending and language on the right. The
/// consts are their ids; a host item with one of them shows in its place.
pub mod status {
    use gpui::SharedString;
    use kiln_l10n::{tr, verbatim};
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

    /// What the language item shows with no language: `Plain Text`.
    pub fn plain_text() -> SharedString {
        tr!("gpui-status-plain-text")
    }

    /// What a cursor item says of the selection, beside the caret's line
    /// and column ([`cursor_label`]).
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub enum Selected {
        /// One caret, nothing selected.
        Nothing,
        /// One selection of this many characters.
        Chars(usize),
        /// This many carets or selections, more than one.
        Selections(usize),
    }

    /// The cursor item's text: `Ln 12, Col 5`, `Ln 12, Col 5 (17 selected)`
    /// or `Ln 12, Col 5 (3 selections)`, each one whole message, its count
    /// a plural selector. `line` and `column` count from 1.
    pub fn cursor_label(line: u32, column: usize, selected: Selected) -> SharedString {
        match selected {
            Selected::Nothing => tr!("gpui-status-cursor", line = line, column = column),
            Selected::Chars(count) => tr!(
                "gpui-status-cursor-selected",
                line = line,
                column = column,
                count = count
            ),
            Selected::Selections(count) => tr!(
                "gpui-status-cursor-selections",
                line = line,
                column = column,
                count = count
            ),
        }
    }

    /// The label the language item and picker show for a language id: its
    /// name, which stays as written in every locale.
    pub fn language_label(id: &str) -> SharedString {
        let label = match id {
            "bash" => verbatim!("Bash"),
            "c" => verbatim!("C"),
            "cpp" => verbatim!("C++"),
            "csharp" => verbatim!("C#"),
            "css" => verbatim!("CSS"),
            "csv" => verbatim!("CSV"),
            "diff" => verbatim!("Diff"),
            "dockerfile" => verbatim!("Dockerfile"),
            "dotenv" => verbatim!("Dotenv"),
            "gitattributes" => verbatim!("Git Attributes"),
            "gitcommit" => verbatim!("Git Commit Message"),
            "gitconfig" => verbatim!("Git Config"),
            "gitignore" => verbatim!("Ignore"),
            "gitrebase" => verbatim!("Git Rebase Todo"),
            "go" => verbatim!("Go"),
            "html" => verbatim!("HTML"),
            "ini" => verbatim!("INI"),
            "java" => verbatim!("Java"),
            "javascript" => verbatim!("JavaScript"),
            "json" => verbatim!("JSON"),
            "json5" => verbatim!("JSON5"),
            "just" => verbatim!("Just"),
            "kotlin" => verbatim!("Kotlin"),
            "lua" => verbatim!("Lua"),
            "make" => verbatim!("Makefile"),
            "markdown" => verbatim!("Markdown"),
            "php" => verbatim!("PHP"),
            "python" => verbatim!("Python"),
            "ruby" => verbatim!("Ruby"),
            "rust" => verbatim!("Rust"),
            "scala" => verbatim!("Scala"),
            "sql" => verbatim!("SQL"),
            "swift" => verbatim!("Swift"),
            "toml" => verbatim!("TOML"),
            "tsv" => verbatim!("TSV"),
            "tsx" => verbatim!("TSX"),
            "typescript" => verbatim!("TypeScript"),
            "xml" => verbatim!("XML"),
            "yaml" => verbatim!("YAML"),
            other => return SharedString::from(other.to_owned()),
        };
        label.into()
    }

    /// The bar's text for a `vim.mode`: an operator waiting is still
    /// normal mode.
    pub(super) fn vim_mode_label(mode: &str) -> SharedString {
        match mode {
            "normal" | "operator_pending" => tr!("gpui-vim-mode-normal"),
            "insert" => tr!("gpui-vim-mode-insert"),
            "replace" => tr!("gpui-vim-mode-replace"),
            "visual" => tr!("gpui-vim-mode-visual"),
            "visual_line" => tr!("gpui-vim-mode-visual-line"),
            "visual_block" => tr!("gpui-vim-mode-visual-block"),
            "cmdline" => tr!("gpui-vim-mode-cmdline"),
            other => other.to_uppercase().into(),
        }
    }

    /// The vim mode item's pill for a `vim.mode`, in `theme`'s colours:
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
            "" => tr!("gpui-vim-mode-jump"),
            query => tr!("gpui-vim-mode-jump-query", query = query),
        }
    }
}

/// Why an unsavable editor's save failed ([`EditorView::set_unsavable`]):
/// `this is a copy to review, not saved`.
fn unsavable() -> io::Error {
    io::Error::other(tr!("gpui-save-unsavable").to_string())
}

/// A tab's label for a buffer at `path`: its file name, or `untitled`.
pub fn file_name(path: Option<&Path>) -> String {
    path.and_then(Path::file_name).map_or_else(
        || tr!("gpui-untitled").to_string(),
        |name| name.to_string_lossy().into_owned(),
    )
}

/// The buttons of the prompt before a dirty editor closes, Cancel the
/// default: Return and Escape keep the editor. GPUI gives Return to the
/// first button and Escape to a cancel button, which would take Return from
/// it, so Cancel is an ok button: Escape reaches it by its title, as AppKit's
/// alerts and kiln-ui's in-window prompt give "Cancel" Escape.
pub fn discard_buttons() -> [PromptButton; 2] {
    [
        PromptButton::Ok(tr!("gpui-close-cancel")),
        PromptButton::Other(tr!("gpui-close-discard")),
    ]
}

/// The answer to [`discard_buttons`] that closes the editor.
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
/// on disk: Overwrite, and Cancel, the cancel button.
fn overwrite_buttons() -> [PromptButton; 2] {
    [
        PromptButton::new(tr!("gpui-save-overwrite")),
        PromptButton::cancel(tr!("gpui-save-cancel")),
    ]
}

/// The answer to [`overwrite_buttons`] that saves over the file.
const OVERWRITE: usize = 0;

/// A save the user wouldn't let overwrite a file that changed on disk:
/// `not saved: main.rs changed on disk`.
#[derive(Debug)]
struct NotSaved(String);

impl fmt::Display for NotSaved {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&tr!("gpui-save-not-saved-changed", name = &self.0))
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
    /// Has the editor never save its buffer, to its path or any other:
    /// [`EditorView::save`], Save As ([`EditorView::save_as_with`]) and
    /// [`EditorView::write_copy`] fail with `this is a copy to review, not saved`, and the save
    /// command does nothing. For a copy of a file to review, not to keep.
    /// Off by default; a split takes it.
    pub fn set_unsavable(&mut self, unsavable: bool) {
        self.unsavable = unsavable;
    }

    /// Whether the editor never saves ([`EditorView::set_unsavable`]).
    pub fn is_unsavable(&self) -> bool {
        self.unsavable
    }

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
        if self.unsavable {
            return Task::ready(Err(unsavable().into()));
        }
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
            let job = prepare_save_in(&mut self.model, encoding);
            let undo = undo_at_save(&self.model, job.as_ref().ok(), cx);
            (job, undo)
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
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Task<gpui::Result<()>> {
        self.save_as_in(
            path,
            line_ending,
            None,
            fixes,
            overwrite,
            before_commit,
            window,
            cx,
        )
    }

    /// [`EditorView::save_as_with`], written in `untitled_encoding` if
    /// given and the buffer has no path when this save's snapshot is
    /// prepared: at once, or, queued behind another save, once that one is
    /// over (an earlier Save As may have given it a path, or failed and left
    /// it untitled). The buffer takes the encoding only once the save
    /// commits, in the same turn, so a save queued behind it starts from
    /// it; until then, and if the save fails, it keeps its own.
    #[allow(clippy::too_many_arguments)]
    pub fn save_as_in(
        &mut self,
        path: PathBuf,
        line_ending: Option<fe_buffer::LineEnding>,
        untitled_encoding: Option<FileEncoding>,
        fixes: SaveFixes,
        overwrite: bool,
        before_commit: impl FnOnce(&mut App) + 'static,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Task<gpui::Result<()>> {
        if self.unsavable {
            return Task::ready(Err(unsavable().into()));
        }
        let window = window.window_handle().window_id();
        let (previous, guard) = self.queue_save();
        // With none running, the text as it is now; typing from here on is
        // unsaved.
        let job = previous.is_none().then(|| {
            self.apply_save_fixes(fixes, cx);
            let (job, in_encoding) = prepare_save_as_in(
                &mut self.model,
                path.clone(),
                line_ending,
                overwrite,
                untitled_encoding,
            );
            let undo = undo_at_save(&self.model, Some(&job), cx);
            (job, in_encoding, undo)
        });
        Self::spawn_save(cx, async move |this, cx| {
            if let Some(previous) = previous {
                previous.over.await.ok();
            }
            let saved = async {
                let (job, in_encoding, undo_at_save) = match job {
                    Some(job) => job,
                    None => this.update(cx, |view, cx| {
                        view.apply_save_fixes(fixes, cx);
                        let (job, in_encoding) = prepare_save_as_in(
                            &mut view.model,
                            path.clone(),
                            line_ending,
                            overwrite,
                            untitled_encoding,
                        );
                        let undo = undo_at_save(&view.model, Some(&job), cx);
                        (job, in_encoding, undo)
                    })?,
                };
                let saved = run(job, cx).await.map_err(io::Error::from)?;
                let written = saved.snapshot().encoding();
                let saved_text = saved.snapshot().text().clone();
                // Written, then replaced by another writer: the buffer isn't
                // what's there, so it stays where it was.
                let replaced = || {
                    let name = file_name(Some(&path));
                    io::Error::other(tr!("gpui-save-replaced", name = name).to_string())
                };
                if !saved.is_current() {
                    return Err(replaced().into());
                }
                cx.update(before_commit);
                this.update(cx, |view, cx| {
                    if !view.model.finish_save_as(saved) {
                        return Err(replaced());
                    }
                    if in_encoding {
                        view.model.buffer_mut().set_encoding(written);
                    }
                    view.detect_language(cx);
                    queue_save_write(view, Some(window), undo_at_save, &saved_text, cx);
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
        if self.unsavable {
            return Task::ready(Err(unsavable()));
        }
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
        let language = fe_syntax::detect_language_name(self.language_path(), &start);
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

    /// Highlights the buffer in the language `source` and its first lines say,
    /// for a model with no path, as a file's text at a revision has. It's
    /// detected, not chosen, so it can be detected again.
    pub fn detect_language_from(&mut self, source: PathBuf, cx: &mut Context<Self>) {
        self.language_source = Some(source);
        self.detect_language(cx);
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
            result.await.unwrap_or_else(|_| {
                Err(io::Error::other(tr!("gpui-save-dropped").to_string()).into())
            })
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
        if self.unsavable {
            return;
        }
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
                    &tr!("gpui-save-failed", name = name),
                    Some(&detail),
                    &[PromptButton::ok(tr!("gpui-save-failed-ok"))],
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
/// A Save As job of `model` to `path` ([`EditorModel::prepare_save_as`]),
/// in `untitled_encoding` if given and the buffer has no path, and whether
/// it was: the buffer keeps its own encoding.
fn prepare_save_as_in(
    model: &mut EditorModel,
    path: PathBuf,
    line_ending: Option<fe_buffer::LineEnding>,
    overwrite: bool,
    untitled_encoding: Option<FileEncoding>,
) -> (SaveJob, bool) {
    let Some(encoding) = untitled_encoding.filter(|_| model.path().is_none()) else {
        return (model.prepare_save_as(path, line_ending, overwrite), false);
    };
    let own = model.buffer().encoding();
    model.buffer_mut().set_encoding(encoding);
    let job = model.prepare_save_as(path, line_ending, overwrite);
    model.buffer_mut().set_encoding(own);
    (job, true)
}

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
    job: Option<(io::Result<SaveJob>, Option<UndoAtSave>)>,
    encoding: Option<FileEncoding>,
    window: Option<AnyWindowHandle>,
    cx: &mut AsyncApp,
) -> gpui::Result<()> {
    let (job, undo_at_save) = match job {
        Some(job) => job,
        None => this.update(cx, |view, cx| {
            let job = prepare_save_in(&mut view.model, encoding);
            let undo = undo_at_save(&view.model, job.as_ref().ok(), cx);
            (job, undo)
        })?,
    };
    let job = job?;
    let name = file_name(Some(job.path()));
    let saved_path = job.path().to_owned();
    let saved = match run(job.clone(), cx).await {
        Err(SaveError::ChangedOnDisk) if let Some(window) = window => {
            let answer = cx.update_window(window, |_, window, cx| {
                window.prompt(
                    PromptLevel::Warning,
                    &tr!("gpui-save-changed-on-disk", name = &name),
                    Some(&tr!("gpui-save-changed-on-disk-detail")),
                    &overwrite_buttons(),
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
    let saved_text = saved.snapshot().text().clone();
    let recorded = this.update(cx, |view, _| {
        let recorded = view.model.finish_save(saved);
        if let Some(encoding) = encoding
            && recorded
        {
            view.model.buffer_mut().set_encoding(encoding);
        }
        recorded
    })?;
    if recorded && let Some(history) = cx.update(|cx| local_history(cx)) {
        record_history(
            history,
            saved_path,
            saved_text.clone(),
            Reason::Saved,
            cx.background_executor(),
        );
    }
    if recorded {
        let window = window.map(|window| window.window_id());
        let saved = &saved_text;
        this.update(cx, |view, cx| {
            queue_save_write(view, window, undo_at_save, saved, cx)
        })?;
    }
    // In an encoding, the buffer keeps its own unless the save is its.
    if let Some(encoding) = encoding
        && !recorded
    {
        let encoding = encoding.label();
        let error = tr!("gpui-save-encoding-kept", encoding = encoding);
        return Err(io::Error::other(error.to_string()).into());
    }
    Ok(())
}

/// Records `text` as `path`'s past text on the background executor. A
/// failure is logged at debug and never reaches the caller.
pub fn record_history(
    history: LocalHistory,
    path: PathBuf,
    text: Text,
    reason: Reason,
    executor: &gpui::BackgroundExecutor,
) {
    executor
        .spawn(async move {
            let bytes = text.chunks().collect::<String>().into_bytes();
            if let Err(error) = history.record(&path, &bytes, reason, SystemTime::now()) {
                log::debug!("local history: recording {}: {error}", path.display());
            }
        })
        .detach();
}

/// The local history, if the app installed one.
struct LocalHistoryGlobal(Option<LocalHistory>);

impl Global for LocalHistoryGlobal {}

/// Has saves (and, in the workbench, changes found on disk) recorded in
/// `history`, or in none. Only the app installs one.
pub fn set_local_history(cx: &mut App, history: Option<LocalHistory>) {
    cx.set_global(LocalHistoryGlobal(history));
}

/// The installed local history, if any.
pub fn local_history(cx: &App) -> Option<LocalHistory> {
    cx.try_global::<LocalHistoryGlobal>()?.0.clone()
}

/// Persistent undo ([`set_undo_store`]): where each file's undo history is
/// kept, each live editor's buffer and window, each window's
/// `persistent_undo`, and the writes not yet on disk.
#[derive(Default)]
struct UndoPersist {
    store: Option<UndoStore>,
    editors: HashMap<EntityId, UndoEditor>,
    /// Windows given to editors not tracked yet ([`set_editor_window`]):
    /// GPUI tells of a new editor once the update that made it is over.
    placed: HashMap<EntityId, WindowId>,
    windows: HashMap<WindowId, bool>,
    /// Windows whose setting isn't known yet
    /// ([`wait_for_window_persistent_undo`]), with the reads their editors
    /// asked for meanwhile: each a buffer and its file.
    waiting: HashMap<WindowId, Vec<(BufferId, PathBuf)>>,
    writes: Arc<UndoWrites>,
}

impl Global for UndoPersist {}

/// A live editor, as persistent undo knows it.
struct UndoEditor {
    buffer: BufferId,
    /// The window it opened in, or last saved from.
    window: Option<WindowId>,
    view: WeakEntity<EditorView>,
}

/// Each file's newest undo history not yet on disk, shared with the
/// writers on the background executor. A writer writes whatever is newest
/// for its file when it runs, so an older capture never lands after a newer
/// one, and a read for a file opening takes a capture still waiting over
/// what's on disk. One that fails to write stays waiting.
#[derive(Default)]
struct UndoWrites {
    /// A record to write, or `None` to remove the file's: with the number
    /// it was captured under, counting up, and the window it was captured
    /// for.
    pending: Mutex<HashMap<PathBuf, UndoCapture>>,
    /// Held by the writer writing, so one file's writes go one at a time.
    writing: Mutex<()>,
    next: AtomicU64,
    /// What windows may write, as last published; `None` before any was.
    policy: Mutex<Option<UndoPolicy>>,
}

/// Which windows keep undo history: the effective setting of each window
/// given one or waiting for one, and the fallback for any other and for
/// captures with no known window.
#[derive(Clone)]
struct UndoPolicy {
    windows: HashMap<WindowId, bool>,
    fallback: bool,
}

impl UndoPolicy {
    fn allows(&self, window: Option<WindowId>) -> bool {
        window
            .and_then(|window| self.windows.get(&window).copied())
            .unwrap_or(self.fallback)
    }
}

/// A file's undo history waiting to be written; see [`UndoWrites`].
#[derive(Clone)]
struct UndoCapture {
    number: u64,
    record: Option<UndoRecord>,
    window: Option<WindowId>,
}

impl UndoWrites {
    fn capture(&self, path: PathBuf, record: Option<UndoRecord>, window: Option<WindowId>) {
        let number = self.next.fetch_add(1, Ordering::Relaxed);
        let mut pending = self.pending.lock().unwrap_or_else(PoisonError::into_inner);
        let capture = UndoCapture {
            number,
            record,
            window,
        };
        pending.insert(path, capture);
    }

    /// `path`'s capture still waiting, if there is one: `Some(None)` for a
    /// removal.
    fn waiting(&self, path: &Path) -> Option<Option<UndoRecord>> {
        let pending = self.pending.lock().unwrap_or_else(PoisonError::into_inner);
        pending.get(path).map(|capture| capture.record.clone())
    }

    /// Forgets the captures waiting for windows `keep` turns down: those
    /// whose window no longer keeps undo history.
    fn forget_unless(&self, keep: impl Fn(Option<WindowId>) -> bool) {
        let mut pending = self.pending.lock().unwrap_or_else(PoisonError::into_inner);
        pending.retain(|_, capture| keep(capture.window));
    }

    /// Publishes which windows may write now, without waiting for a writer,
    /// and drops the captures waiting for a window that may not. A writer
    /// checks it again under the writing lock, just before it writes or
    /// removes, so one queued earlier never lands for a window turned off.
    fn publish(&self, policy: UndoPolicy) {
        // Waits for the write or removal in flight, if any, to finish.
        let mut published = self.policy.lock().unwrap_or_else(PoisonError::into_inner);
        *published = Some(policy.clone());
        let mut pending = self.pending.lock().unwrap_or_else(PoisonError::into_inner);
        pending.retain(|_, capture| policy.allows(capture.window));
        drop(published);
    }

    /// Writes `path`'s newest capture to `store`, if one is still waiting.
    fn flush(&self, store: &UndoStore, path: &Path) {
        let _writing = self.writing.lock().unwrap_or_else(PoisonError::into_inner);
        let Some(UndoCapture {
            number,
            record,
            window,
        }) = self.waiting_numbered(path)
        else {
            return;
        };
        // Held to the end of the write or removal, so a policy published
        // meanwhile waits for it, and none can land once publish returns.
        let policy = self.policy.lock().unwrap_or_else(PoisonError::into_inner);
        // A publish while this writer waited may have dropped the capture
        // (a window turned off, then on): don't bring it back from the copy.
        if !self.is_pending(path, number) {
            return;
        }
        if !policy.as_ref().is_none_or(|policy| policy.allows(window)) {
            self.remove_if_pending(path, number);
            return;
        }
        let written = match &record {
            Some(record) => store.write(path, record),
            None => store.remove(path),
        };
        if let Err(error) = written {
            // Kept waiting, so reads still get it, until a newer capture
            // replaces it or quitting tries again.
            log::debug!("undo store: writing {}: {error}", path.display());
            return;
        }
        self.remove_if_pending(path, number);
    }

    /// Whether `number` is still `path`'s pending capture.
    fn is_pending(&self, path: &Path, number: u64) -> bool {
        let pending = self.pending.lock().unwrap_or_else(PoisonError::into_inner);
        pending
            .get(path)
            .is_some_and(|newest| newest.number == number)
    }

    fn remove_if_pending(&self, path: &Path, number: u64) {
        let mut pending = self.pending.lock().unwrap_or_else(PoisonError::into_inner);
        if pending
            .get(path)
            .is_some_and(|newest| newest.number == number)
        {
            pending.remove(path);
        }
    }

    fn waiting_numbered(&self, path: &Path) -> Option<UndoCapture> {
        let pending = self.pending.lock().unwrap_or_else(PoisonError::into_inner);
        pending.get(path).cloned()
    }

    /// Writes every capture still waiting, here and now.
    fn flush_all(&self, store: &UndoStore) {
        let paths: Vec<PathBuf> = {
            let pending = self.pending.lock().unwrap_or_else(PoisonError::into_inner);
            pending.keys().cloned().collect()
        };
        for path in paths {
            self.flush(store, &path);
        }
    }
}

/// Keeps each file's undo history in `store`, or in none: read into a
/// file's buffer when its first editor opens it, and written after each
/// save (Save As included) and when the last editor of a clean buffer
/// closes, and for every clean buffer as the app quits, while the editor's
/// window has `persistent_undo` on ([`set_window_persistent_undo`]). Only
/// the app installs one.
///
/// The read is off the main thread, and its history is given to the buffer
/// ([`Buffer::import_undo`]) through whichever of its editors is still open,
/// only if the buffer has none of its own by then and its text is the one
/// the history ends at: a file changed outside the editor since starts with
/// none, and a buffer restored from a hot-exit backup, which holds the
/// restore as history, gets none. Every editor of the buffer keeps its
/// selections. A save or close that leaves no history to keep removes the
/// file's record, unless the buffer never had any history at all.
///
/// Writes run on the background executor, newest per file winning; a read
/// takes a write still waiting over the disk. Quitting writes what's
/// waiting before the app exits.
pub fn set_undo_store(cx: &mut App, store: Option<UndoStore>) {
    let observing = cx.has_global::<UndoPersist>();
    cx.default_global::<UndoPersist>().store = store;
    publish_undo_policy(cx);
    if !observing {
        cx.observe_new(editor_opened).detach();
        cx.on_app_quit(|cx| {
            write_all_on_quit(cx);
            async {}
        })
        .detach();
    }
}

/// Has editors in `window` keep their files' undo history if `on`, as its
/// root's `editor.persistent_undo` says, or, with `None`, as the host's
/// editor settings ([`set_restore_style`]) say. Reads its editors asked for
/// while it waited ([`wait_for_window_persistent_undo`]) go ahead.
pub fn set_window_persistent_undo(window: WindowId, on: Option<bool>, cx: &mut App) {
    let persist = cx.default_global::<UndoPersist>();
    match on {
        Some(on) => persist.windows.insert(window, on),
        None => persist.windows.remove(&window),
    };
    let deferred = persist.waiting.remove(&window).unwrap_or_default();
    publish_undo_policy(cx);
    for (buffer, path) in deferred {
        start_undo_read(buffer, path, cx);
    }
}

/// Has editors in `window` read and write no undo history until
/// [`set_window_persistent_undo`] gives it a setting, for a window whose
/// root's settings are still loading; reads they ask for meanwhile wait for
/// it.
pub fn wait_for_window_persistent_undo(window: WindowId, cx: &mut App) {
    let persist = cx.default_global::<UndoPersist>();
    persist.waiting.entry(window).or_default();
    publish_undo_policy(cx);
}

/// Whether `window` waits for [`set_window_persistent_undo`]'s first
/// setting ([`wait_for_window_persistent_undo`]).
pub fn window_waits_for_persistent_undo(window: WindowId, cx: &App) -> bool {
    cx.try_global::<UndoPersist>()
        .is_some_and(|persist| persist.waiting.contains_key(&window))
}

/// Tells the writers which windows keep history now, as [`undo_store_for`]
/// would say; captures of the others are dropped.
fn publish_undo_policy(cx: &App) {
    let persist = cx.global::<UndoPersist>();
    let windows = (persist.windows.keys().chain(persist.waiting.keys()))
        .map(|&window| (window, undo_store_for(Some(window), cx).is_some()))
        .collect();
    let fallback = undo_store_for(None, cx).is_some();
    persist.writes.publish(UndoPolicy { windows, fallback });
}

/// Writes what is waiting, here and now, as a writer that was held back
/// would. For tests.
#[doc(hidden)]
pub fn flush_undo_writes_for_test(cx: &App) {
    if let Some(persist) = cx.try_global::<UndoPersist>()
        && let Some(store) = &persist.store
    {
        persist.writes.flush_all(store);
    }
}

/// What [`set_window_persistent_undo`] gave `window`.
pub fn window_persistent_undo(window: WindowId, cx: &App) -> Option<bool> {
    cx.try_global::<UndoPersist>()?
        .windows
        .get(&window)
        .copied()
}

/// The installed undo store, if any, while `persistent_undo` is on for
/// `window`: its own setting, or the host's editor settings.
fn undo_store_for(window: Option<WindowId>, cx: &App) -> Option<UndoStore> {
    let persist = cx.try_global::<UndoPersist>()?;
    if window.is_some_and(|window| persist.waiting.contains_key(&window)) {
        return None;
    }
    let on = match window.and_then(|window| persist.windows.get(&window)) {
        Some(&on) => on,
        // A window not known (or given none) is on only if every window
        // given a setting is, and none waits for one: never on where some
        // root has it off.
        None => {
            (cx.try_global::<RestoreStyle>())
                .is_none_or(|RestoreStyle(_, settings)| settings.persistent_undo)
                && persist.windows.values().all(|&on| on)
                && persist.waiting.is_empty()
        }
    };
    persist.store.clone().filter(|_| on)
}

/// The window the editor `id` is in: as tracked, or else the one GPUI last
/// drew it in, which is then tracked.
fn editor_window(id: EntityId, cx: &mut App) -> Option<WindowId> {
    if let Some(window) = tracked_editor(id, cx).and_then(|editor| editor.window) {
        return Some(window);
    }
    let window = cx.with_window(id, |window, _| window.window_handle().window_id())?;
    if let Some(editor) = tracked_editor(id, cx) {
        editor.window = Some(window);
    }
    Some(window)
}

/// The window of one of `buffer`'s live editors, if one's is known.
fn buffer_window(buffer: BufferId, cx: &mut App) -> Option<WindowId> {
    let ids: Vec<EntityId> = (cx.try_global::<UndoPersist>()?.editors.iter())
        .filter(|(_, editor)| editor.buffer == buffer)
        .map(|(&id, _)| id)
        .collect();
    ids.into_iter().find_map(|id| editor_window(id, cx))
}

/// Tracks the editor `id` as in `window`: for editors made outside a
/// window's update, such as restored ones.
pub fn set_editor_window(id: EntityId, window: WindowId, cx: &mut App) {
    if !cx.has_global::<UndoPersist>() {
        return;
    }
    let persist = cx.global_mut::<UndoPersist>();
    match persist.editors.get_mut(&id) {
        Some(editor) => editor.window = Some(window),
        None => {
            persist.placed.insert(id, window);
        }
    }
}

/// Queues `record` as `path`'s undo history, or with `None` the removal of
/// its record, for a writer on the background executor, on behalf of
/// `window`.
fn queue_undo_write(
    store: UndoStore,
    path: PathBuf,
    record: Option<UndoRecord>,
    window: Option<WindowId>,
    cx: &App,
) {
    let writes = cx.global::<UndoPersist>().writes.clone();
    writes.capture(path.clone(), record, window);
    cx.background_spawn(async move { writes.flush(&store, &path) })
        .detach();
}

/// Tracks `editor`, and if it's the first of a file's buffer, reads the
/// file's undo history for the buffer.
fn editor_opened(
    editor: &mut EditorView,
    window: Option<&mut Window>,
    cx: &mut Context<EditorView>,
) {
    let id = cx.entity_id();
    let window = window.map(|window| window.window_handle().window_id());
    let buffer = editor.model.buffer().id();
    let view = cx.weak_entity();
    let persist = cx.default_global::<UndoPersist>();
    let window = persist.placed.remove(&id).or(window);
    let first = !persist
        .editors
        .values()
        .any(|editor| editor.buffer == buffer);
    persist.editors.insert(
        id,
        UndoEditor {
            buffer,
            window,
            view,
        },
    );
    cx.on_release(move |editor, cx| editor_closed(id, editor, cx))
        .detach();
    if !first || editor.model.buffer().history_revision() != 0 {
        return;
    }
    let Some(path) = editor.model.path().map(Path::to_owned) else {
        return;
    };
    // Once the update that made the editor is over, which may give it its
    // window (as restoring does).
    cx.spawn(async move |_, cx| cx.update(|cx| start_undo_read(buffer, path, cx)))
        .detach();
}

/// Reads `path`'s undo history for `buffer` off the main thread, then gives
/// it to the buffer ([`import_undo_into`]), if its window keeps history;
/// one whose window waits for its setting reads once it has one.
fn start_undo_read(buffer: BufferId, path: PathBuf, cx: &mut App) {
    let window = buffer_window(buffer, cx);
    if !cx.has_global::<UndoPersist>() {
        return;
    }
    let persist = cx.global_mut::<UndoPersist>();
    if let Some(deferred) = window.and_then(|window| persist.waiting.get_mut(&window)) {
        deferred.push((buffer, path));
        return;
    }
    let writes = persist.writes.clone();
    let Some(store) = undo_store_for(window, cx) else {
        return;
    };
    cx.spawn(async move |cx| {
        let read = cx.background_spawn({
            let path = path.clone();
            async move {
                match writes.waiting(&path) {
                    Some(record) => Ok(record),
                    None => store.read(&path),
                }
            }
        });
        let record = match read.await {
            Ok(Some(record)) => record,
            Ok(None) => return,
            Err(error) => {
                log::debug!("undo store: reading {}: {error}", path.display());
                return;
            }
        };
        cx.update(|cx| import_undo_into(buffer, &path, record, cx));
    })
    .detach();
}

/// Gives `record` to the buffer `buffer` through one of its live editors,
/// if it's still `path`'s and has no history of its own, keeping each of
/// its editors' selections.
fn import_undo_into(buffer: BufferId, path: &Path, record: UndoRecord, cx: &mut App) {
    let Some(persist) = cx.try_global::<UndoPersist>() else {
        return;
    };
    let editors: Vec<Entity<EditorView>> = (persist.editors.values())
        .filter(|editor| editor.buffer == buffer)
        .filter_map(|editor| editor.view.upgrade())
        .collect();
    let Some(first) = editors.first().cloned() else {
        return;
    };
    let fresh = {
        let model = &first.read(cx).model;
        model.buffer().id() == buffer
            && model.buffer().history_revision() == 0
            && model.path() == Some(path)
    };
    if !fresh {
        return;
    }
    let window = buffer_window(buffer, cx);
    // A window that began waiting for its settings after this read started
    // reads again once they are in, with every check repeated.
    let persist = cx.global_mut::<UndoPersist>();
    if let Some(deferred) = window.and_then(|window| persist.waiting.get_mut(&window)) {
        if !deferred.iter().any(|(b, p)| *b == buffer && p == path) {
            deferred.push((buffer, path.to_owned()));
        }
        return;
    }
    if undo_store_for(window, cx).is_none() {
        return;
    }
    let states: Vec<_> = (editors.iter())
        .map(|editor| editor.read(cx).model.selection_state())
        .collect();
    let imported = first.update(cx, |editor, _| {
        editor.model.buffer_mut().import_undo(record)
    });
    if let Err(error) = imported {
        log::debug!("undo store: not restoring {}: {error}", path.display());
        return;
    }
    // No editor made these, so none recorded where their carets go: `u`
    // and `<C-r>` put one at the start of the text each changes.
    first.update(cx, |editor, _| {
        let user = |origin: &fe_buffer::Origin| *origin == fe_buffer::Origin::User;
        let times = editor.model.buffer().history_times(user);
        let made = times.into_iter().map(|time| time.transaction);
        editor.model.put_carets_at_change_start(made);
    });
    // The text is as it was, so each editor's selections, primary and
    // movement goals go back as they were, as though nothing moved them.
    for (editor, state) in editors.iter().zip(states) {
        editor.update(cx, |editor, cx| {
            editor.model.restore_selection_state(state);
            cx.notify();
        });
    }
}

/// Stops tracking the closed editor `id`, and if it was its buffer's last,
/// writes the buffer's undo history ([`queue_close_write`]).
fn editor_closed(id: EntityId, editor: &mut EditorView, cx: &mut App) {
    if !cx.has_global::<UndoPersist>() {
        return;
    }
    let window = editor_window(id, cx);
    let persist = cx.global_mut::<UndoPersist>();
    let Some(closed) = persist.editors.remove(&id) else {
        return;
    };
    if !persist
        .editors
        .values()
        .any(|other| other.buffer == closed.buffer)
    {
        queue_close_write(editor, window, cx);
    }
}

/// Queues `editor`'s undo history as its file's, if its window keeps it, it
/// has a file, its buffer is clean (its text is the file's) and has had
/// history: none left to keep removes the file's record.
fn queue_close_write(editor: &EditorView, window: Option<WindowId>, cx: &App) {
    let Some(store) = undo_store_for(window, cx) else {
        return;
    };
    let Some(path) = editor.model.path().map(Path::to_owned) else {
        return;
    };
    let record = {
        let buffer = editor.model.buffer();
        if buffer.is_dirty() || buffer.history_revision() == 0 {
            return;
        }
        buffer.export_undo()
    };
    queue_undo_write(store, path, record, window, cx);
}

/// As the app quits: queues the undo history of each clean buffer still
/// open, then writes everything waiting, before the app exits.
fn write_all_on_quit(cx: &mut App) {
    let Some(persist) = cx.try_global::<UndoPersist>() else {
        return;
    };
    let mut buffers = HashSet::new();
    let open: Vec<(EntityId, Entity<EditorView>)> = (persist.editors.iter())
        .filter(|(_, editor)| buffers.insert(editor.buffer))
        .filter_map(|(&id, editor)| Some((id, editor.view.upgrade()?)))
        .collect();
    for (id, editor) in open {
        let window = editor_window(id, cx);
        queue_close_write(editor.read(cx), window, cx);
    }
    // What was captured for a window that has since turned it off, or
    // waits for its setting, isn't written.
    let persist = cx.global::<UndoPersist>();
    persist
        .writes
        .forget_unless(|window| undo_store_for(window, cx).is_some());
    if let Some(store) = &persist.store {
        persist.writes.flush_all(store);
    }
}

/// A buffer's undo history as a save's snapshot was prepared, for
/// [`queue_save_write`].
struct UndoAtSave {
    /// [`Buffer::history_revision`] then.
    revision: u64,
    record: Option<UndoRecord>,
}

/// `model`'s undo history as `job` is prepared from it, while there's an
/// undo store; `None` if the buffer had none, or there's no job.
fn undo_at_save(model: &EditorModel, job: Option<&SaveJob>, cx: &App) -> Option<UndoAtSave> {
    let job = job?;
    cx.try_global::<UndoPersist>()?.store.as_ref()?;
    let buffer = model.buffer();
    (buffer.version() == job.snapshot().version() && buffer.history_revision() != 0).then(|| {
        UndoAtSave {
            revision: buffer.history_revision(),
            record: buffer.export_undo(),
        }
    })
}

/// After a save of `saved` (the text written) from `window`, if any,
/// queues the buffer's undo history as its file's: as it is now if the
/// buffer is clean (`at_save`'s, if the history hasn't changed since),
/// else `at_save`'s, if that ends at the text saved. No history left to
/// keep removes the file's record, unless the buffer never had any.
fn queue_save_write(
    view: &EditorView,
    window: Option<WindowId>,
    at_save: Option<UndoAtSave>,
    saved: &Text,
    cx: &mut Context<EditorView>,
) {
    let id = cx.entity_id();
    if let Some(window) = window {
        set_editor_window(id, window, cx);
    }
    let window = window.or_else(|| editor_window(id, cx));
    let Some(store) = undo_store_for(window, cx) else {
        return;
    };
    let Some(path) = view.model.path().map(Path::to_owned) else {
        return;
    };
    let record = {
        let buffer = view.model.buffer();
        if buffer.history_revision() == 0 {
            return;
        }
        match at_save {
            Some(at_save)
                if !buffer.is_dirty() && at_save.revision == buffer.history_revision() =>
            {
                at_save.record
            }
            _ if !buffer.is_dirty() => buffer.export_undo(),
            Some(UndoAtSave {
                record: Some(record),
                ..
            }) if record.text_len == saved.len() as u64
                && record.text_hash == fe_buffer::undo_store::text_hash(saved) =>
            {
                Some(record)
            }
            Some(UndoAtSave { record: None, .. }) => None,
            _ => return,
        }
    };
    queue_undo_write(store, path, record, window, cx);
}

/// The persistent-undo entry of the live editor `id`.
fn tracked_editor(id: EntityId, cx: &mut App) -> Option<&mut UndoEditor> {
    if !cx.has_global::<UndoPersist>() {
        return None;
    }
    cx.global_mut::<UndoPersist>().editors.get_mut(&id)
}

/// Runs `job` on the background executor.
async fn run(job: SaveJob, cx: &mut AsyncApp) -> Result<Saved, SaveError> {
    cx.background_spawn(async move { job.run() }).await
}

impl PaneItem for EditorView {
    const KIND: &'static str = "fastedit.editor";

    fn tab_title(&self, _: &App) -> SharedString {
        match &self.tab_title {
            Some((title, _)) => title.clone(),
            None => file_name(self.model.path()).into(),
        }
    }

    /// Its file's icon in the project tree, if it's a bundled one.
    fn tab_icon(&self, cx: &App) -> Option<Icon> {
        self.tab_icon_ref(cx)?.bundled()
    }

    /// Its file's icon in the project tree, from the installed icon set; an
    /// untitled buffer's is a plain file's.
    fn tab_icon_ref(&self, cx: &App) -> Option<IconRef> {
        match &self.tab_title {
            Some((_, icon_by)) => Some(file_icon_ref(icon_by)),
            None => Some(file_icon_ref(&self.tab_title(cx))),
        }
    }

    fn tab_title_color(&self, _: &App) -> Option<Hsla> {
        self.tab_tint
    }

    /// `main.rs — Tenmoku`, `main.rs ● — Tenmoku` with unsaved changes.
    fn window_title(&self, cx: &App) -> SharedString {
        let (title, app) = (self.tab_title(cx), fe_config::APP_NAME);
        match self.is_dirty(cx) {
            true => tr!("gpui-window-title-dirty", title = title, app = app),
            false => tr!("gpui-window-title", title = title, app = app),
        }
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
    /// [`discard_buttons`]). A clean one closes. The shell doesn't ask while
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
        let name = file_name(self.model.path());
        let question = tr!("gpui-close-discard-question", name = name);
        let answer = window.prompt(
            PromptLevel::Warning,
            &question,
            Some(&tr!("gpui-close-discard-detail")),
            &discard_buttons(),
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
    /// one's theme: read-only if this one is (taking proposals' Accepts if
    /// it does), unsavable if it is, with its tab title and the language
    /// chosen for it, if any.
    fn split(&mut self, _: &mut Window, cx: &mut Context<Self>) -> Option<Entity<Self>> {
        let model = self.model.split();
        let theme = self.theme.clone();
        let (read_only, title) = (self.read_only, self.tab_title.clone());
        let (accepts, unsavable) = (self.accepts_when_read_only, self.unsavable);
        let language = self.language_chosen.then_some(self.chosen_language);
        let source = self.language_source.clone();
        let (wrap_column, wrap_mode) = (self.wrap_column, self.wrap_mode);
        let max_line_length = self.max_line_length;
        Some(cx.new(|cx| {
            let mut view = EditorView::new(model, theme, cx);
            view.wrap_column = wrap_column;
            view.wrap_mode = wrap_mode;
            view.max_line_length = max_line_length;
            view.set_read_only(read_only, cx);
            view.accepts_when_read_only = accepts;
            view.unsavable = unsavable;
            view.tab_title = title;
            if let Some(source) = source {
                view.detect_language_from(source, cx);
            }
            if let Some(language) = language {
                view.set_language(language, cx);
            }
            view
        }))
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
                let view = cx.new(|cx| {
                    let mut view = EditorView::new(model, theme, cx);
                    view.set_wrap_settings(&settings);
                    view
                });
                view.update(cx, |view, cx| place_caret(view, &state, cx));
                // Made outside the window's update: its undo history is
                // kept as the window's setting says.
                set_editor_window(view.entity_id(), window, cx);
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
    if cx.has_global::<UndoPersist>() {
        publish_undo_policy(cx);
    }
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
                // the clock has gone back: by the clock's smallest step,
                // which on Windows is 100 ns (a nanosecond less is lost).
                const STEP: Duration = Duration::from_nanos(if cfg!(windows) { 100 } else { 1 });
                if let Some((_, written)) = newest {
                    meta.written = meta.written.max(written + STEP);
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
    let selected = if selections.len() > 1 {
        status::Selected::Selections(selections.len())
    } else if !primary.range.is_empty() {
        let chars =
            text.offset_to_char(primary.range.end) - text.offset_to_char(primary.range.start);
        status::Selected::Chars(chars)
    } else {
        status::Selected::Nothing
    };
    let cursor = status::cursor_label(row + 1, column, selected);
    let encoding = model.buffer().encoding().label();
    let line_ending = match model.buffer().line_ending() {
        fe_buffer::LineEnding::Lf => "LF",
        fe_buffer::LineEnding::CrLf => "CRLF",
    };
    let language = match editor.syntax() {
        Some(syntax) => status::language_label(syntax.language().name()),
        None if editor.is_loading_language() => "…".into(),
        None => status::plain_text(),
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
        Some(IndentStyle::Tabs) => tr!("gpui-status-tab-size", size = model.tab_size()),
        Some(IndentStyle::Spaces(n)) => tr!("gpui-status-spaces", size = n),
        None => tr!("gpui-status-spaces", size = model.tab_size()),
    };
    let language_command = Invocation::of("language.select").expect("a valid command ID");
    let go_to_line_command = Invocation::of("go_to_line.toggle").expect("a valid command ID");
    let line_ending_command = (!editor.is_read_only()).then(|| {
        let change = Invocation::of("workbench.change_line_ending").expect("a valid command ID");
        (change, tr!("gpui-status-line-ending-tooltip"))
    });
    let texts: [(SharedString, Option<(Invocation, SharedString)>); 5] = [
        (
            cursor,
            Some((go_to_line_command, tr!("gpui-status-cursor-tooltip"))),
        ),
        (indent, None),
        (encoding.into(), None),
        (line_ending.into(), line_ending_command),
        (
            language,
            Some((language_command, tr!("gpui-status-language-tooltip"))),
        ),
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

#[cfg(test)]
mod undo_writes_tests {
    use std::path::Path;

    use fe_buffer::{Buffer, Origin, UndoStore};

    use super::{UndoPolicy, UndoWrites};

    fn record(text: &str) -> fe_buffer::UndoRecord {
        let mut buffer = Buffer::new("");
        buffer.edit([(0..0, text)], Origin::User);
        buffer.export_undo().unwrap()
    }

    /// Two captures, their writers running newest first: the older one,
    /// running last, finds nothing to write, so the newer stays on disk.
    #[test]
    fn an_older_writer_running_last_writes_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let store = UndoStore::new(dir.path().join("undo"));
        let path = dir.path().join("a.txt");
        let writes = UndoWrites::default();
        let new = record("new");
        writes.capture(path.clone(), Some(record("old")), None);
        writes.capture(path.clone(), Some(new.clone()), None);
        assert_eq!(writes.waiting(&path), Some(Some(new.clone())));
        writes.flush(&store, &path);
        writes.flush(&store, &path);
        assert_eq!(writes.waiting(&path), None);
        assert_eq!(store.read(&path).unwrap(), Some(new));
    }

    /// A capture that can't be written stays waiting, for reads and the
    /// next try.
    #[test]
    fn a_failed_write_stays_waiting() {
        let dir = tempfile::tempdir().unwrap();
        let blocked = dir.path().join("blocked");
        std::fs::write(&blocked, "a file, not a directory").unwrap();
        let store = UndoStore::new(blocked.join("undo"));
        let path = Path::new("/a.txt");
        let writes = UndoWrites::default();
        let kept = record("kept");
        writes.capture(path.to_owned(), Some(kept.clone()), None);
        writes.flush(&store, path);
        assert_eq!(writes.waiting(path), Some(Some(kept.clone())));
        let good = UndoStore::new(dir.path().join("undo"));
        writes.flush_all(&good);
        assert_eq!(writes.waiting(path), None);
        assert_eq!(good.read(path).unwrap(), Some(kept));
    }

    fn policy(window: gpui::WindowId, on: bool) -> UndoPolicy {
        UndoPolicy {
            windows: [(window, on)].into(),
            fallback: on,
        }
    }

    /// A policy published while a writer holds the writing lock doesn't
    /// wait for it, and the writer that runs next honours it, for a write
    /// and for a removal.
    #[test]
    fn a_policy_published_during_a_write_holds_back_the_next_writers() {
        let dir = tempfile::tempdir().unwrap();
        let store = UndoStore::new(dir.path().join("undo"));
        let path = dir.path().join("a.txt");
        let other = dir.path().join("b.txt");
        let old = record("old");
        store.write(&path, &old).unwrap();
        store.write(&other, &old).unwrap();
        let window = gpui::WindowId::from(1u64);
        let writes = UndoWrites::default();
        writes.capture(path.clone(), Some(record("new")), Some(window));
        writes.capture(other.clone(), None, Some(window));
        // Mimics the pending captures being taken by the writers already.
        let held = writes.writing.lock().unwrap();
        writes.capture(path.clone(), Some(record("newer")), Some(window));
        {
            // The captures are dropped at publish; put them back as a
            // writer that had read them would find them: through the
            // policy check alone.
            *writes.policy.lock().unwrap() = Some(policy(window, false));
        }
        drop(held);
        writes.flush(&store, &path);
        writes.flush(&store, &other);
        assert_eq!(store.read(&path).unwrap(), Some(old.clone()));
        assert_eq!(store.read(&other).unwrap(), Some(old));
        assert_eq!(writes.waiting(&path), None);
        // Publishing drops the waiting captures as well.
        writes.capture(path.clone(), None, Some(window));
        writes.publish(policy(window, false));
        assert_eq!(writes.waiting(&path), None);
        writes.capture(path.clone(), None, Some(window));
        writes.publish(policy(window, true));
        assert_eq!(writes.waiting(&path), Some(None));
    }

    /// A policy published while a write is in flight waits for it, so
    /// the write can't land after the publish returns.
    #[test]
    fn publishing_waits_for_the_write_in_flight() {
        let writes = std::sync::Arc::new(UndoWrites::default());
        let window = gpui::WindowId::from(1u64);
        let in_flight = writes.policy.lock().unwrap();
        let (done, finished) = std::sync::mpsc::channel();
        let publisher = {
            let writes = writes.clone();
            std::thread::spawn(move || {
                writes.publish(policy(window, false));
                done.send(()).unwrap();
            })
        };
        assert!(
            finished
                .recv_timeout(std::time::Duration::from_millis(200))
                .is_err(),
            "publish returned during a write"
        );
        drop(in_flight);
        finished
            .recv_timeout(std::time::Duration::from_secs(10))
            .unwrap();
        publisher.join().unwrap();
    }
}
