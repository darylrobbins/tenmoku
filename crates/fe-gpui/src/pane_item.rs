//! [`EditorView`] as kiln-shell's [`PaneItem`]: what a pane shows in an
//! editor's tab (its file's name, a dirty dot), the window's title while
//! it's active, its status bar items ([`status`]), and its file, saving,
//! splitting and saved state, from its
//! [`EditorModel`](fe_editor::EditorModel).

use std::path::Path;

use fe_command::{Invocation, Value};
use fe_vim::VimState;
use gpui::{AnyView, App, AppContext as _, Context, Entity, SharedString, Task, Window};
use kiln_shell::{PaneItem, StatusItem, StatusSide};

use crate::EditorView;

/// An editor's status bar items, while it's the active item: the vim mode
/// with vim on, and vim's last message, on the left; the cursor,
/// indentation, encoding, line ending and language on the right. The
/// consts are their ids; a host item with one of them shows in its place.
pub mod status {
    use gpui::SharedString;

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
}

/// A tab's label for a buffer at `path`: its file name, or `untitled`.
pub fn file_name(path: Option<&Path>) -> String {
    path.and_then(Path::file_name).map_or_else(
        || "untitled".into(),
        |name| name.to_string_lossy().into_owned(),
    )
}

impl PaneItem for EditorView {
    const KIND: &'static str = "fastedit.editor";

    fn tab_title(&self, _: &App) -> SharedString {
        file_name(self.model.path()).into()
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

    // `may_close` keeps the default: a dirty editor stays open.

    /// What `editor.save` does: [`EditorModel::save`](fe_editor::EditorModel::save),
    /// which is synchronous, so the task is ready at once.
    fn save(&mut self, _: &mut Window, cx: &mut Context<Self>) -> Option<Task<gpui::Result<()>>> {
        let saved = self.model.save().map_err(Into::into);
        cx.notify();
        Some(Task::ready(saved))
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
}

/// `editor`'s items: see [`status`].
fn status_items(editor: &EditorView) -> Vec<StatusItem> {
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
        StatusItem::new(
            status::VIM_MODE,
            StatusSide::Left,
            0,
            status::vim_mode_label(mode),
        )
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
    let language_command = Invocation::of("language.select").expect("a valid command ID");
    let texts: [(SharedString, Option<Invocation>); 5] = [
        (cursor.into(), None),
        (format!("Spaces: {}", model.tab_size()).into(), None),
        (encoding.into(), None),
        (line_ending.into(), None),
        (language, Some(language_command)),
    ];
    let right = status::RIGHT
        .into_iter()
        .zip(texts)
        .map(|((id, priority), (text, command))| StatusItem {
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

#[cfg(test)]
#[path = "pane_item_tests.rs"]
mod tests;
