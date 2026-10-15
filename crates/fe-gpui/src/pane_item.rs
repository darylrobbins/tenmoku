//! [`EditorView`] as kiln-shell's [`PaneItem`]: what a pane shows in an
//! editor's tab (its file's name, a dirty dot), the window's title while
//! it's active, its status bar items ([`status`]), and its file, saving,
//! splitting and saved state, from its
//! [`EditorModel`](fe_editor::EditorModel).

use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
    fmt,
    hash::{DefaultHasher, Hash as _, Hasher as _},
    io,
    path::{Path, PathBuf},
    rc::Rc,
    time::Duration,
};

use fe_buffer::{Bias, Buffer, BufferId};
use fe_editor::{EditorModel, IndentStyle, SaveError, SaveJob, Saved};

use fe_command::{Invocation, Value};
use fe_vim::VimState;
use futures::{FutureExt as _, channel::oneshot, future::Shared};
use gpui::{
    AnyView, AnyWindowHandle, App, AppContext as _, AsyncApp, Context, Entity, Global, Hsla,
    PromptButton, PromptLevel, SharedString, Task, WeakEntity, Window,
};
use kiln_shell::{CloseAnswer, PaneItem, StatusItem, StatusSide};

use crate::{EditorSettings, EditorTheme, EditorView};

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
        // With none running, the text as it is now, or as the hook leaves
        // it; typing from here on is unsaved.
        let hook = self.save_hook.clone();
        let job = (previous.is_none() && hook.is_none()).then(|| self.model.prepare_save(false));
        let editor = cx.entity();
        let window = window.window_handle();
        let (send, result) = oneshot::channel();
        cx.spawn(async move |this, cx| {
            if let Some(previous) = previous {
                previous.over.await.ok();
            }
            if let Some(hook) = hook {
                let task = cx.update(|cx| hook.before_save(&editor, cx));
                let limit = cx.background_executor().timer(SAVE_HOOK_LIMIT);
                if let futures::future::Either::Right(_) =
                    futures::future::select(task, limit).await
                {
                    log::debug!("the save hook took over {SAVE_HOOK_LIMIT:?}; saving anyway");
                }
            }
            let saved = save_asking(&this, job, window, cx).await;
            drop(guard);
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
    /// it failed, unless the user cancelled it.
    pub(crate) fn save_from_command(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let saved = self.save(window, cx);
        let name = file_name(self.model.path());
        let window = window.window_handle();
        cx.spawn(async move |_, cx| {
            let Err(error) = saved.await else {
                return;
            };
            if error.downcast_ref::<NotSaved>().is_some() {
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

/// A save of `this`'s buffer, from `job` if it was prepared, else of the
/// text as it is now. If the file changed on disk and the user, asked in
/// `window`, says to overwrite it, the same text is saved over it: what was
/// typed since stays unsaved.
async fn save_asking(
    this: &WeakEntity<EditorView>,
    job: Option<io::Result<SaveJob>>,
    window: AnyWindowHandle,
    cx: &mut AsyncApp,
) -> gpui::Result<()> {
    let job = match job {
        Some(job) => job,
        None => this.update(cx, |view, _| view.model.prepare_save(false))?,
    }?;
    let name = file_name(Some(job.path()));
    let saved = match run(job.clone(), cx).await {
        Err(SaveError::ChangedOnDisk) => {
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
    this.update(cx, |view, _| view.model.finish_save(saved))?;
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

    /// Another editor over its buffer ([`EditorModel::split`](fe_editor::EditorModel::split)).
    fn shares_document(&self, other: &AnyView, cx: &App) -> bool {
        other
            .clone()
            .downcast::<EditorView>()
            .is_ok_and(|other| other.read(cx).model.shares_buffer_with(&self.model))
    }

    /// A dirty editor asks whether to discard its changes: Discard closes
    /// it, Cancel keeps it and is the default, with Return and Escape (see
    /// [`DISCARD_BUTTONS`]). A clean one closes. The shell doesn't ask while
    /// another editor shows its buffer.
    fn may_close(&mut self, window: &mut Window, cx: &mut Context<Self>) -> CloseAnswer {
        if !self.is_dirty(cx) {
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

    /// Its file's path and its caret, the primary selection's head. An
    /// untitled buffer, or one at a path that isn't UTF-8, isn't saved.
    fn serialize(&self, _: &App) -> Option<Value> {
        let path = self.model.path()?.to_str()?;
        let caret = self.model.primary_selection().head();
        Some(Value::from_iter([
            ("path", Value::from(path)),
            ("caret", Value::from(caret)),
        ]))
    }

    /// Reads `state`'s file off the main thread and opens it, in the
    /// theme and settings of [`set_restore_style`], its caret placed,
    /// clamped to the text. A file that's gone, or can't be read, isn't
    /// restored.
    fn restore(state: Value, _: &mut Window, cx: &mut App) -> Task<Option<Entity<Self>>> {
        let Some(path) = state.get("path").and_then(Value::as_str).map(PathBuf::from) else {
            return Task::ready(None);
        };
        let read = cx.background_spawn({
            let path = path.clone();
            async move {
                let file = std::fs::File::open(&path)?;
                // Stamped, so a save refuses a file changed since.
                Buffer::from_file(file)
            }
        });
        cx.spawn(async move |cx| {
            let buffer = match read.await {
                Ok(buffer) => buffer,
                Err(error) => {
                    log::info!("not restoring {}: {error}", path.display());
                    return None;
                }
            };
            cx.update(|cx| {
                let (theme, settings) = restore_style(cx);
                let mut model = EditorModel::new(buffer, Some(path));
                model.set_tab_size(settings.tab_size);
                model.set_soft_wrap(settings.soft_wrap);
                model.set_auto_close(settings.auto_close_pairs);
                let view = cx.new(|cx| EditorView::new(model, theme, cx));
                view.update(cx, |view, cx| place_caret(view, &state, cx));
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
#[path = "save_hook_tests.rs"]
mod save_hook_tests;

#[cfg(test)]
#[path = "indent_status_tests.rs"]
mod indent_status_tests;

#[cfg(test)]
#[path = "vim_mode_status_tests.rs"]
mod vim_mode_status_tests;
