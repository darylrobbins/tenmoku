//! The TODO comments' commands: Search TODO Comments (`todo.list`, the
//! [`TodoPicker`](super::todo_picker::TodoPicker)), and Next and Previous
//! TODO Comment (`todo.next`, `todo.previous`), LazyVim's `<leader>st`
//! (and `<leader>xt`), `]t` and `[t` in the `lazyvim` preset.
//!
//! Next and Previous move the active editor's caret to the start of the
//! next keyword after it, or the last before it, of those
//! [`todo_comments::scan`] finds in the comments of the whole buffer (not
//! just the rows in view, as the badges are), selecting nothing. They wrap
//! nowhere: with no keyword that way, a toast says "No more TODO comments".
//! The comments are read [`CHUNK_ROWS`] rows at a time, out from the
//! caret's, until a keyword turns up: a highlight query has a time budget
//! sized for about a view's rows. A buffer whose parse is behind its text,
//! or a chunk whose highlighting doesn't finish in a few tries, has none
//! known yet, and the toast says that instead; one too big for the badges
//! has its own toast.
//!
//! `Workbench`'s one `#[kiln::command]` block is `workbench_commands.rs`'s,
//! so the commands are declared on [`TodoCommands`], which no element
//! attaches, and the workbench's root element handles their actions
//! ([`Workbench::attach_todo_commands`]).

use std::ops::Range;

use fe_gpui::EditorView;
use fe_syntax::Syntax;
use fe_text::Text;
use gpui::{Context, Div, InteractiveElement as _, SharedString};
use kiln::Cx;
use kiln_l10n::tr;

use crate::{
    Workbench,
    todo_comments::{self, Keywords},
    workbench::toast::{self, Toast},
};

/// What the toast says at the last keyword, or the first.
pub(crate) fn no_more() -> SharedString {
    tr!("workbench-workbench-todo-no-more")
}

/// The commands' declaration: see the module's docs. They're handled by
/// the workbench's root element, not here.
#[allow(dead_code)]
struct TodoCommands;

#[kiln::command(context = "workbench")]
#[allow(dead_code)]
impl TodoCommands {
    /// Search TODO Comments
    #[command(id = "todo.list")]
    fn search_todo_comments(&mut self, _: &mut Cx<Self>) {}

    /// Next TODO Comment
    #[command(id = "todo.next")]
    fn next_todo_comment(&mut self, _: &mut Cx<Self>) {}

    /// Previous TODO Comment
    #[command(id = "todo.previous")]
    fn previous_todo_comment(&mut self, _: &mut Cx<Self>) {}
}

/// The rows of the text one highlight query reads: about what a view
/// shows, which the query's time budget is for.
pub(super) const CHUNK_ROWS: u32 = 64;

/// How many times a chunk is queried before its comments count as not
/// known: each try gets twice the time the last did, up to the query's cap.
const TRIES: u32 = 4;

/// The starts of the keywords in the comments of `rows` of `text`, as
/// `syntax` highlights them, in order: None if highlighting didn't finish
/// in [`TRIES`].
pub(super) fn keyword_starts_in(
    syntax: Option<&Syntax>,
    text: &Text,
    rows: Range<u32>,
) -> Option<Vec<usize>> {
    let ranges =
        (0..TRIES).find_map(|_| todo_comments::comment_ranges(syntax, text, rows.clone()))?;
    let mut starts: Vec<usize> = todo_comments::scan(text, &ranges, &Keywords::default())
        .into_iter()
        .map(|hit| hit.keyword_range.start)
        .collect();
    // A comment that runs into the rows from before them may be read whole.
    starts.dedup();
    Some(starts)
}

/// The chunk of [`CHUNK_ROWS`] rows `row` is in, of `count` rows.
pub(super) fn chunk_of(row: u32, count: u32) -> Range<u32> {
    let start = row - row % CHUNK_ROWS;
    start..start.saturating_add(CHUNK_ROWS).min(count)
}

/// What looking for the next keyword came to.
enum Found {
    At(usize),
    None,
    NotYet,
    /// Over [`MAX_SPELLABLE_BYTES`](crate::spell_ranges::MAX_SPELLABLE_BYTES):
    /// its comments aren't read, as for the badges.
    TooBig,
}

/// The first keyword after `head` in `view`'s buffer, or with `forward`
/// false the last before it, reading its comments a chunk at a time out
/// from the caret's.
fn next_keyword(view: &EditorView, head: usize, forward: bool) -> Found {
    let model = view.model();
    let snapshot = model.snapshot();
    let text = snapshot.text();
    let syntax = view.syntax();
    let behind = model.buffer().version() != snapshot.version()
        || syntax.is_some_and(|it| it.version() != snapshot.version() || it.needs_parse());
    if view.is_loading_language() || behind {
        return Found::NotYet;
    }
    if text.len() > crate::spell_ranges::MAX_SPELLABLE_BYTES {
        return Found::TooBig;
    }
    let count = text.row_count();
    let mut chunk = chunk_of(text.offset_to_point(head).row, count);
    loop {
        let Some(starts) = keyword_starts_in(syntax, text, chunk.clone()) else {
            return Found::NotYet;
        };
        let found = match forward {
            true => starts.into_iter().find(|&start| start > head),
            false => starts.into_iter().rev().find(|&start| start < head),
        };
        if let Some(at) = found {
            return Found::At(at);
        }
        chunk = match forward {
            true if chunk.end < count => chunk_of(chunk.end, count),
            false if chunk.start > 0 => chunk_of(chunk.start - 1, count),
            _ => return Found::None,
        };
    }
}

impl Workbench {
    /// Moves the active editor's caret to the next keyword after it, or
    /// with `forward` false the last before it: see the module docs.
    fn go_to_todo_comment(&mut self, forward: bool, cx: &mut Context<Self>) {
        let Some(editor) = self.active_editor(cx).cloned() else {
            return;
        };
        let view = editor.read(cx);
        let head = view.model().primary_selection().head();
        match next_keyword(view, head, forward) {
            Found::At(at) => editor.update(cx, |editor, cx| editor.select_and_reveal(at..at, cx)),
            Found::None => toast::show(cx, Toast::info(no_more())),
            // While the buffer's comments aren't known.
            Found::NotYet => toast::show(cx, Toast::info(tr!("workbench-workbench-todo-not-yet"))),
            // A buffer too big to read the comments of.
            Found::TooBig => toast::show(cx, Toast::info(tr!("workbench-workbench-todo-too-big"))),
        }
    }

    /// `root` handling the TODO comments' commands, in the workbench's key
    /// context.
    pub(super) fn attach_todo_commands(root: Div, cx: &mut Context<Self>) -> Div {
        root.on_action(
            cx.listener(|workbench, _: &SearchTodoComments, window, cx| {
                workbench.show_todos(window, cx)
            }),
        )
        .on_action(cx.listener(|workbench, _: &NextTodoComment, _, cx| {
            workbench.go_to_todo_comment(true, cx)
        }))
        .on_action(cx.listener(|workbench, _: &PreviousTodoComment, _, cx| {
            workbench.go_to_todo_comment(false, cx)
        }))
    }
}

#[cfg(test)]
#[path = "todo_commands_tests.rs"]
mod todo_commands_tests;
