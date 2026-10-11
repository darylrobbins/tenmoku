//! The status bar along the window's bottom edge: the active editor's vim
//! mode, with vim on, on the left; its cursor, selections, indentation,
//! encoding, line endings and language on the right; and whatever items a
//! host sets.
//!
//! Items are data ([`StatusItem`], fastedit's name for kiln-shell's
//! [`kiln_shell::StatusItem`], with the built-in items' ids), kept and
//! drawn by kiln-shell's [`kiln_shell::status`]: the host's over the
//! built-in ones, sorted. The built-in ones are computed from the
//! active editor each time the bar renders, so nothing new is tracked: the
//! bar observes the editor the workbench says is active
//! ([`StatusBar::set_editor`]), and re-renders when it notifies, as it does
//! on every caret move and edit. The workbench embeds the bar cached, so
//! its other redraws reuse the bar's last frame.
//!
//! While that editor has vim's command line open, the line shows in the
//! bar's place, even with the bar hidden: see [`vim_line`].
//!
//! Its commands, `view.toggle_status_bar` and `language.select`, are those
//! of [`init`]'s registry, in the workbench's [`crate::KEY_CONTEXT`], with
//! `vim_line.cancel` in the line's. Their handlers only name an op: the
//! workbench does it, or the line.

use std::rc::Rc;

use fe_command::{
    Actor, CommandTarget, ContextPredicate, Dispatched, Effect, Invocation, Registry,
};
use fe_gpui::{EditorView, commands::RunCommand};
use fe_ui::UiTheme;
use fe_vim::VimState;
use gpui::{
    App, Context, Entity, EntityId, Global, IntoElement, Render, SharedString, Subscription,
    WeakEntity, Window,
};
use kiln_shell::status::{self, StatusItems};

pub use kiln_shell::StatusSide;

/// One item on the bar: kiln-shell's [`kiln_shell::StatusItem`] by
/// fastedit's name, which holds the built-in items' ids
/// ([`StatusItem::CURSOR`] …). Items sort by side, then by `priority`,
/// lower first, so left to right; then in the order they were set, the
/// built-in ones first.
#[derive(Clone, Debug, PartialEq)]
pub struct StatusItem {
    /// Distinct among the bar's items. The built-in ones' are
    /// [`StatusItem::CURSOR`] and the other consts on `StatusItem`.
    pub id: SharedString,
    pub side: StatusSide,
    pub priority: i32,
    pub text: SharedString,
    /// Run when the item is clicked, as a key would run it: dispatched to
    /// the focused element, and up to the workbench. An invocation with no
    /// context gets the focused element's then.
    pub command: Option<Invocation>,
}

impl From<StatusItem> for kiln_shell::StatusItem {
    fn from(item: StatusItem) -> Self {
        let StatusItem {
            id,
            side,
            priority,
            text,
            command,
        } = item;
        Self {
            id,
            side,
            priority,
            text,
            command,
        }
    }
}

/// The built-in items' ids: the vim mode on the left, the others on the
/// right, in this order. A host item set with one of them shows in that
/// item's place.
impl StatusItem {
    /// `NORMAL`, `INSERT`, `VISUAL`, `VISUAL LINE` or `VISUAL BLOCK`, only
    /// with vim on: on the left, priority 0.
    pub const VIM_MODE: &str = "vim_mode";
    /// Vim's last message, such as `E486: Pattern not found: zzz`, until
    /// the next key in the editor: on the left, priority 10.
    pub const VIM_MESSAGE: &str = "vim_message";
    /// `Ln 12, Col 5`, then ` (17 selected)` or ` (3 selections)`: priority
    /// 100.
    pub const CURSOR: &str = "cursor";
    /// `Spaces: 4`: priority 200.
    pub const INDENT: &str = "indent";
    /// The file's encoding, such as `UTF-8`, `UTF-8 with BOM` or `windows-1252`:
    /// priority 300.
    pub const ENCODING: &str = "encoding";
    /// `LF` or `CRLF`: priority 400.
    pub const LINE_ENDING: &str = "line_ending";
    /// The language's name, `Plain Text`, or `…` while it loads; a click
    /// runs `language.select`: priority 500.
    pub const LANGUAGE: &str = "language";
}

/// The built-in items' ids and priorities, left to right.
const BUILT_IN_PRIORITIES: [(&str, i32); 5] = [
    (StatusItem::CURSOR, 100),
    (StatusItem::INDENT, 200),
    (StatusItem::ENCODING, 300),
    (StatusItem::LINE_ENDING, 400),
    (StatusItem::LANGUAGE, 500),
];

/// The label the language picker and the bar show for a language id.
pub(crate) fn language_label(id: &str) -> SharedString {
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

pub(crate) const PLAIN_TEXT: &str = "Plain Text";

pub struct StatusBar {
    editor: Option<WeakEntity<EditorView>>,
    /// The host's items.
    items: StatusItems,
    ui: UiTheme,
    line: vim_line::VimLine,
    _observe: Option<Subscription>,
    /// Each item drawn by the last render, in order: its side, id and text.
    #[cfg(test)]
    drawn: Vec<(StatusSide, SharedString, SharedString)>,
    #[cfg(test)]
    renders: usize,
}

impl StatusBar {
    pub fn new(ui: UiTheme) -> Self {
        Self {
            editor: None,
            items: StatusItems::default(),
            ui,
            line: vim_line::VimLine::default(),
            _observe: None,
            #[cfg(test)]
            drawn: Vec::new(),
            #[cfg(test)]
            renders: 0,
        }
    }

    /// Adds `item`, or replaces the item with its id where it was. An item
    /// with a built-in item's id shows in its place.
    pub fn set_item(&mut self, item: impl Into<kiln_shell::StatusItem>, cx: &mut Context<Self>) {
        self.items.set(item.into());
        cx.notify();
    }

    /// Removes the host's item `id`; a built-in item it replaced comes back.
    pub fn remove_item(&mut self, id: &str, cx: &mut Context<Self>) {
        self.items.remove(id);
        cx.notify();
    }

    /// Follows `editor`, the active pane's active tab's, or none. The
    /// workbench calls it as it renders, as it does
    /// [`crate::OutlinePanel::set_editor`].
    pub fn set_editor(
        &mut self,
        editor: Option<&Entity<EditorView>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if editor.map(Entity::entity_id) == self.editor_id() {
            return;
        }
        // Now, while the last frame still draws the editor left.
        self.leave_line(editor.map(Entity::entity_id), window, cx);
        self.editor = editor.map(Entity::downgrade);
        self._observe = editor.map(|editor| {
            cx.observe_in(editor, window, |bar, _, window, cx| {
                bar.sync_line(window, cx);
                cx.notify();
            })
        });
        // Not while the workbench draws.
        cx.defer_in(window, Self::sync_line);
        cx.notify();
    }

    /// The editor followed.
    pub(crate) fn editor_id(&self) -> Option<EntityId> {
        self.editor.as_ref().map(|editor| editor.entity_id())
    }

    /// Every item to draw, sorted.
    fn items(&self, cx: &App) -> Vec<kiln_shell::StatusItem> {
        let editor = self.editor.as_ref().and_then(WeakEntity::upgrade);
        let built_in = editor.map(|editor| built_in_items(editor.read(cx)));
        self.items.merged(built_in.into_iter().flatten())
    }
}

/// The active editor's items.
fn built_in_items(editor: &EditorView) -> Vec<kiln_shell::StatusItem> {
    let model = editor.model();
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
        Some(syntax) => language_label(syntax.language().name()),
        None if editor.is_loading_language() => "…".into(),
        None => PLAIN_TEXT.into(),
    };
    let vim_mode = editor.vim_mode().map(|mode| {
        kiln_shell::StatusItem::new(
            StatusItem::VIM_MODE,
            StatusSide::Left,
            0,
            vim_mode_label(mode),
        )
    });
    let vim_message = editor
        .vim_state()
        .and_then(VimState::message)
        .map(|message| {
            kiln_shell::StatusItem::new(
                StatusItem::VIM_MESSAGE,
                StatusSide::Left,
                10,
                message.to_owned(),
            )
        });
    let language_command = Invocation::of("language.select").expect("a valid command ID");
    let texts: [(SharedString, Option<Invocation>); 5] = [
        (cursor.into(), None),
        (format!("Spaces: {}", model.tab_size()).into(), None),
        (encoding.into(), None),
        (line_ending.into(), None),
        (language, Some(language_command)),
    ];
    let right =
        BUILT_IN_PRIORITIES
            .into_iter()
            .zip(texts)
            .map(|((id, priority), (text, command))| kiln_shell::StatusItem {
                id: id.into(),
                side: StatusSide::Right,
                priority,
                text,
                command,
            });
    vim_mode
        .into_iter()
        .chain(vim_message)
        .chain(right)
        .collect()
}

/// The bar's text for a `vim.mode`: an operator waiting is still normal
/// mode.
fn vim_mode_label(mode: &str) -> SharedString {
    match mode {
        "normal" | "operator_pending" => "NORMAL".into(),
        "insert" => "INSERT".into(),
        "visual" => "VISUAL".into(),
        "visual_line" => "VISUAL LINE".into(),
        "visual_block" => "VISUAL BLOCK".into(),
        other => other.to_uppercase().into(),
    }
}

impl Render for StatusBar {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if let Some(line) = self.render_line(cx) {
            #[cfg(test)]
            {
                self.renders += 1;
                self.drawn.clear();
            }
            return line.into_any_element();
        }
        let items = self.items(cx);
        #[cfg(test)]
        {
            self.renders += 1;
            self.drawn = items
                .iter()
                .map(|item| (item.side, item.id.clone(), item.text.clone()))
                .collect();
        }
        status::status_bar(items, cx).into_any_element()
    }
}

#[cfg(test)]
impl StatusBar {
    /// The texts of the items the last render drew on `side`, left to
    /// right. Whether that frame showed the bar is the window's to say: see
    /// the `status-item:{id}` debug selectors.
    pub(crate) fn drawn(&self, side: StatusSide) -> Vec<(SharedString, SharedString)> {
        self.drawn
            .iter()
            .filter(|(item_side, ..)| *item_side == side)
            .map(|(_, id, text)| (id.clone(), text.clone()))
            .collect()
    }

    /// How many times the bar has rendered.
    pub(crate) fn renders(&self) -> usize {
        self.renders
    }
}

/// What a status bar command asks for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum StatusOp {
    ToggleStatusBar,
    SelectLanguage,
    /// The line runs it: it applies only there.
    CancelVimLine,
}

/// The [`CommandTarget`] of status bar commands: the op to do.
pub(crate) struct StatusCommands;

impl CommandTarget for StatusCommands {
    type Tx = ();
    type Target<'a> = Option<StatusOp>;
}

struct Commands(Rc<Registry<StatusCommands>>);

impl Global for Commands {}

/// Registers the status bar commands.
pub(crate) fn init(cx: &mut App) {
    cx.set_global(Commands(Rc::new(registry())));
}

pub(crate) fn registry() -> Registry<StatusCommands> {
    use StatusOp::*;
    let commands = [
        (
            "view.toggle_status_bar",
            "Toggle Status Bar",
            ToggleStatusBar,
        ),
        ("language.select", "Change Language Mode…", SelectLanguage),
    ];
    let commands = commands
        .map(|(id, title, op)| (id, title, crate::KEY_CONTEXT, op))
        .into_iter()
        .chain([(
            vim_line::CANCEL,
            "Cancel Command Line",
            vim_line::KEY_CONTEXT,
            CancelVimLine,
        )]);
    let mut registry = Registry::new();
    for (id, title, context, op) in commands {
        registry
            .register(
                id,
                title,
                move |target: &mut Option<StatusOp>, _: &Actor| {
                    *target = Some(op);
                    Ok(Dispatched::new(Effect::Notify))
                },
            )
            .expect("status bar command IDs are valid and distinct")
            .context(ContextPredicate::identifier(context));
    }
    registry
}

/// The op of `run`, if it's a status bar command that applies in its
/// context.
pub(crate) fn op(run: &RunCommand, cx: &App) -> Option<StatusOp> {
    let registry = cx.try_global::<Commands>()?.0.clone();
    registry.get(&run.0.command)?;
    let mut op = None;
    if let Err(error) = registry.dispatch(&mut op, &run.0) {
        log::warn!("{error}");
    }
    op
}

#[path = "vim_line.rs"]
mod vim_line;

#[cfg(test)]
#[path = "status_bar_vim_tests.rs"]
mod vim_tests;

#[cfg(test)]
#[path = "status_bar_visual_tests.rs"]
mod visual_tests;
