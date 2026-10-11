//! Config problems: the diagnostics of the user's settings, keymap and
//! theme files and the launch root's project settings, which kiln's config
//! service publishes as [`ConfigDiagnostics`], shown on the status bar and
//! listed by `workbench.show_config_problems`.
//!
//! Each [`Workbench`] observes the global and keeps a [`ITEM`] status item
//! on the left, `1 config problem` or `N config problems`, while there are
//! any; a click runs [`SHOW`]. That command shows a [`Picker`] over a
//! [`ConfigProblemsPicker`]: errors first, then warnings, each in file
//! order. Confirming a row opens its file in the active pane with the caret
//! at the problem. The command's handler only says it ran: the workbench
//! shows the list ([`Workbench::show_config_problems`]).

use std::{
    fs::OpenOptions,
    io::{self, Read as _},
    path::Path,
    rc::Rc,
};

use fe_buffer::Buffer;

use fe_command::{
    Actor, CommandTarget, ContextPredicate, Dispatched, Effect, Invocation, Registry,
};
use fe_config::{Diagnostic, Severity};
use fe_gpui::{EditorView, commands::RunCommand};
use fe_ui::UiTheme;
use gpui::{
    AnyElement, App, AppContext as _, Context, FocusHandle, Focusable as _, IntoElement as _,
    ParentElement as _, PromptButton, PromptLevel, SharedString, Styled as _, Subscription,
    WeakEntity, Window, div, px,
};
use kiln_platform::config::{self, ConfigDiagnostics};

use crate::{Pane, Picker, PickerDelegate, StatusItem, StatusSide, Workbench, workbench::Palette};

/// The status item's id.
pub(crate) const ITEM: &str = "config.problems";

/// The command that lists the problems.
pub(crate) const SHOW: &str = "workbench.show_config_problems";

/// The item's priority: on the left, after vim's mode and message.
const PRIORITY: i32 = 50;

/// The [`CommandTarget`] of [`SHOW`]: whether it ran.
pub(crate) struct ConfigProblemsCommands;

impl CommandTarget for ConfigProblemsCommands {
    type Tx = ();
    type Target<'a> = bool;
}

struct Commands(Rc<Registry<ConfigProblemsCommands>>);

impl gpui::Global for Commands {}

/// Registers [`SHOW`].
pub(crate) fn init(cx: &mut App) {
    cx.set_global(Commands(Rc::new(registry())));
}

pub(crate) fn registry() -> Registry<ConfigProblemsCommands> {
    let mut registry = Registry::new();
    registry
        .register(
            SHOW,
            "Show Config Problems",
            |shown: &mut bool, _: &Actor| {
                *shown = true;
                Ok(Dispatched::new(Effect::Notify))
            },
        )
        .expect("the config problems command ID is valid")
        .context(ContextPredicate::identifier(crate::KEY_CONTEXT));
    registry
}

/// Whether `run` is [`SHOW`], applying in its context.
pub(crate) fn shows(run: &RunCommand, cx: &App) -> bool {
    let Some(registry) = cx
        .try_global::<Commands>()
        .map(|commands| commands.0.clone())
    else {
        return false;
    };
    if registry.get(&run.0.command).is_none() {
        return false;
    }
    let mut shown = false;
    if let Err(error) = registry.dispatch(&mut shown, &run.0) {
        log::warn!("{error}");
    }
    shown
}

/// The status item for `count` problems: none for none.
fn status_item(count: usize) -> Option<StatusItem> {
    let text = match count {
        0 => return None,
        1 => "1 config problem".to_owned(),
        count => format!("{count} config problems"),
    };
    Some(StatusItem {
        id: ITEM.into(),
        side: StatusSide::Left,
        priority: PRIORITY,
        text: text.into(),
        command: Some(Invocation::of(SHOW).expect("a valid command ID")),
    })
}

impl Workbench {
    /// Follows [`ConfigDiagnostics`]: call once, from `new`, then
    /// [`Workbench::sync_config_problems`] for those there are already.
    pub(crate) fn follow_config_problems(window: &Window, cx: &mut Context<Self>) -> Subscription {
        cx.observe_global_in::<ConfigDiagnostics>(window, Self::sync_config_problems)
    }

    /// Sets the status item for the problems now, and gives an open list
    /// them; with none, the list goes.
    pub(crate) fn sync_config_problems(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let problems = config::diagnostics(cx).to_vec();
        let item = status_item(problems.len());
        self.status_bar().clone().update(cx, |bar, cx| match item {
            Some(item) => bar.set_item(item, cx),
            None => bar.remove_item(ITEM, cx),
        });
        if self.palette != Some(Palette::ConfigProblems) {
            return;
        }
        if problems.is_empty() {
            return self.dismiss_palette(window, cx);
        }
        let picker = self.config_problems.clone();
        picker.update(cx, |picker, cx| {
            let query = picker.input().read(cx).text().to_owned();
            picker.delegate.set_problems(problems);
            picker.delegate.update_matches(query, cx);
        });
    }

    /// The config problems list, shown by [`SHOW`].
    #[cfg(test)]
    pub(crate) fn config_problems(&self) -> &gpui::Entity<Picker<ConfigProblemsPicker>> {
        &self.config_problems
    }

    /// Whether the config problems list shows.
    #[cfg(test)]
    pub(crate) fn config_problems_open(&self) -> bool {
        self.palette == Some(Palette::ConfigProblems)
    }

    /// Shows the config problems list, focused, in place of any other
    /// palette, or afresh if it shows. Nothing while there are none.
    pub(crate) fn show_config_problems(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let problems = config::diagnostics(cx).to_vec();
        if problems.is_empty() {
            return;
        }
        // Dismissing it would hide it again once its dismissal lands.
        if self.palette != Some(Palette::ConfigProblems) {
            self.dismiss_palette(window, cx);
        }
        let previous = window.focused(cx);
        let picker = self.config_problems.clone();
        picker.update(cx, |picker, cx| {
            picker.delegate.opened(problems, previous);
            picker.show(window, cx);
        });
        self.show_palette(Palette::ConfigProblems, picker, window, cx);
    }
}

/// The config problems, errors first, which the input filters by message
/// and file name.
pub(crate) struct ConfigProblemsPicker {
    workbench: WeakEntity<Workbench>,
    /// The errors, then the warnings, each in file order.
    rows: Vec<Diagnostic>,
    /// Bumped by each change to `rows`, so a row's identity is its list's:
    /// a click pressed on a row isn't taken by the one that replaced it.
    version: u64,
    /// The rows matching the query, as indices into `rows`.
    matches: Vec<usize>,
    /// Focused before the list opened, and again once it's dismissed.
    previous_focus: Option<FocusHandle>,
    /// Bumped by each pick, so only the last one's open goes ahead.
    picks: u64,
    ui: UiTheme,
}

impl ConfigProblemsPicker {
    pub(crate) fn new(workbench: WeakEntity<Workbench>, ui: UiTheme) -> Self {
        Self {
            workbench,
            rows: Vec::new(),
            version: 0,
            matches: Vec::new(),
            previous_focus: None,
            picks: 0,
            ui,
        }
    }

    /// The list is opening over `problems`, from `previous`, the focus.
    fn opened(&mut self, problems: Vec<Diagnostic>, previous: Option<FocusHandle>) {
        self.set_problems(problems);
        self.previous_focus = previous;
    }

    /// Lists `problems`, in kiln's order, which is each file's in order:
    /// the errors first, then the warnings.
    fn set_problems(&mut self, mut problems: Vec<Diagnostic>) {
        // Stable, so each file keeps its order.
        problems.sort_by_key(|problem| !problem.is_error());
        self.matches = (0..problems.len()).collect();
        self.rows = problems;
        self.version += 1;
    }

    fn row(&self, ix: usize) -> &Diagnostic {
        &self.rows[self.matches[ix]]
    }

    /// Opens `problem`'s file in `pane`, now it's `read`, unless pick
    /// `pick` was superseded: by another pick, by the pane closing, or by
    /// another open in it (`opens`, its open serial then). The pane is
    /// handed the buffer read, as a host hands it a file it read
    /// ([`Pane::opening`], [`Pane::file_read`]), so it never reads the path
    /// itself; if the file is open already, its tab shows instead. A file
    /// that can't be read says why instead.
    #[expect(clippy::too_many_arguments, reason = "one pick's state, from its task")]
    fn open(
        &mut self,
        pick: u64,
        problem: &Diagnostic,
        read: io::Result<Buffer>,
        pane: &WeakEntity<Pane>,
        opens: u64,
        window: &mut Window,
        cx: &mut Context<Picker<Self>>,
    ) {
        let Some(workbench) = self.workbench.upgrade() else {
            return;
        };
        let pane = pane.upgrade().filter(|pane| {
            let panes = workbench.read(cx).pane_group().panes();
            panes.contains(&pane)
        });
        let Some(pane) = pane else {
            return;
        };
        if pick != self.picks || pane.read(cx).open_serial() != opens {
            return;
        }
        let buffer = match read {
            Ok(buffer) => buffer,
            Err(error) => {
                let message = format!("Can't open {}", file_name(problem));
                // Answered by its one button; nothing waits on it.
                drop(window.prompt(
                    PromptLevel::Warning,
                    &message,
                    Some(&error.to_string()),
                    &[PromptButton::ok("OK")],
                    cx,
                ));
                return;
            }
        };
        let path = problem.file.clone();
        pane.update(cx, |pane, cx| {
            pane.opening(path.clone(), cx);
            pane.file_read(path.clone(), Ok(Some(buffer)), window, cx);
        });
        let editor = pane.read(cx).active_editor().cloned();
        let editor = editor.filter(|editor| editor.read(cx).model().path() == Some(&path));
        if let Some(editor) = editor {
            let (line, column) = (problem.line, problem.column);
            editor.update(cx, |editor, cx| place_caret(editor, line, column, cx));
        }
    }
}

/// Puts `editor`'s caret at 1-based `line` and `column`, in chars, of its
/// text as it is now, edits since the file was saved and all: at the
/// line's end if it's shorter, on the last line if there are fewer.
fn place_caret(editor: &mut EditorView, line: usize, column: usize, cx: &mut Context<EditorView>) {
    let text = editor.model().text();
    let row = u32::try_from(line.saturating_sub(1)).unwrap_or(u32::MAX);
    let row = row.min(text.row_count().saturating_sub(1));
    let start = text.row_start(row);
    let line = text.slice_to_string(start..start + text.row_len(row) as usize);
    let line = line.trim_end_matches('\r');
    let chars = column.saturating_sub(1);
    let offset = start + (line.char_indices().nth(chars)).map_or(line.len(), |(ix, _)| ix);
    editor.select_and_reveal(offset..offset, cx);
}

#[cfg(test)]
impl ConfigProblemsPicker {
    /// The matches' two lines: the message, then the place and severity.
    pub(crate) fn labels(&self) -> Vec<(String, String)> {
        (0..self.matches.len())
            .map(|ix| (self.row(ix).message.clone(), detail(self.row(ix))))
            .collect()
    }
}

/// The file's name, without its folder.
fn file_name(problem: &Diagnostic) -> String {
    let name = problem.file.file_name().unwrap_or(problem.file.as_os_str());
    name.to_string_lossy().into_owned()
}

/// A row's second line: `name:line:column · severity`.
fn detail(problem: &Diagnostic) -> String {
    let severity = match problem.severity {
        Severity::Error => "error",
        Severity::Warning => "warning",
    };
    let name = file_name(problem);
    format!("{name}:{}:{} · {severity}", problem.line, problem.column)
}

impl PickerDelegate for ConfigProblemsPicker {
    /// Rows whose message or file name holds `query`, ignoring case.
    fn update_matches(&mut self, query: String, cx: &mut Context<Picker<Self>>) {
        let query = query.to_lowercase();
        self.matches = (self.rows.iter().enumerate())
            .filter(|(_, problem)| {
                problem.message.to_lowercase().contains(&query)
                    || file_name(problem).to_lowercase().contains(&query)
            })
            .map(|(ix, _)| ix)
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

    /// The list's version, then the row's position in it.
    fn match_id(&self, ix: usize) -> SharedString {
        format!("{}:{}", self.version, self.matches[ix]).into()
    }

    fn render_match(&self, ix: usize, _: bool, _: &App) -> AnyElement {
        let problem = self.row(ix);
        // Two lines in the picker's row height.
        div()
            .flex()
            .flex_col()
            .overflow_hidden()
            .child(
                div()
                    .text_size(px(11.))
                    .line_height(px(12.))
                    .child(problem.message.clone()),
            )
            .child(
                div()
                    .text_size(px(10.))
                    .line_height(px(11.))
                    .text_color(self.ui.muted_text)
                    .child(detail(problem)),
            )
            .into_any_element()
    }

    /// Opens the row's file in the active pane, the caret at the problem,
    /// once it's read off the main thread ([`read`]): see
    /// [`ConfigProblemsPicker::open`].
    fn confirm(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Picker<Self>>) -> bool {
        let Some(workbench) = self.workbench.upgrade() else {
            return false;
        };
        let problem = self.row(ix).clone();
        self.previous_focus = None;
        self.picks += 1;
        let pick = self.picks;
        let pane = workbench.read(cx).pane();
        let (opens, pane) = (pane.read(cx).open_serial(), pane.downgrade());
        // Meanwhile the active editor has the keys, not the closed list.
        window.focus(&workbench.focus_handle(cx), cx);
        let path = problem.file.clone();
        let read = cx.background_spawn(async move { read(&path) });
        cx.spawn_in(window, async move |picker, cx| {
            let read = read.await;
            picker
                .update_in(cx, |picker, window, cx| {
                    let delegate = &mut picker.delegate;
                    delegate.open(pick, &problem, read, &pane, opens, window, cx)
                })
                .ok();
        })
        .detach();
        true
    }

    fn dismissed(&mut self, window: &mut Window, cx: &mut Context<Picker<Self>>) {
        if let Some(focus) = self.previous_focus.take() {
            window.focus(&focus, cx);
        }
    }

    fn status(&self) -> Option<SharedString> {
        Some(match self.matches.len() {
            1 => "1 problem".into(),
            count => format!("{count} problems").into(),
        })
    }
}

/// Config files bigger than this aren't opened: kiln's config service
/// rejects them too.
const MAX_BYTES: u64 = 1 << 20;

/// `path`, a config file, which may be a project's and so untrusted, read
/// as kiln's config service reads it: only a regular file of at most
/// [`MAX_BYTES`], whatever its size claims. It's opened without blocking,
/// as opening a FIFO would block, and checked once open, so nothing can be
/// swapped in between; its encoding is detected as the pane's loader does.
fn read(path: &Path) -> io::Result<Buffer> {
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::custom_flags(&mut options, libc::O_NONBLOCK);
    let file = options.open(path)?;
    if !file.metadata()?.is_file() {
        return Err(io::Error::other("not a regular file"));
    }
    let mut bytes = Vec::new();
    file.take(MAX_BYTES + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_BYTES {
        return Err(io::Error::other(format!(
            "bigger than {} KiB",
            MAX_BYTES / 1024
        )));
    }
    Buffer::from_reader_detecting(bytes.as_slice())
}

#[cfg(test)]
#[path = "config_problems_tests.rs"]
mod tests;
