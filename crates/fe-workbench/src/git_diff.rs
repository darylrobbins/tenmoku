//! Comparing a file with the index, with HEAD or with another file, and
//! moving between its changes: `git.diff_index`, `git.diff_head`,
//! `diff.compare_file`, `git.next_change` and `git.previous_change`, and
//! the host commands typed in a diff view's sides, routed here by
//! `git_commands`. Not supported yet.

use fe_gpui::{DiffView, EditorView};
use fe_vim::HostCommand;
use gpui::{Context, Entity, WeakEntity, Window};

use super::{Workbench, git_commands::GitOp};

/// What a host command typed in a diff view's side answers.
const HOST_COMMAND: &str = "Not supported yet: this command in a diff view";

/// Runs `op`, a diff command.
pub(super) fn run(
    workbench: &mut Workbench,
    op: GitOp,
    _: &mut Window,
    cx: &mut Context<Workbench>,
) {
    let text = format!("Not supported yet: {}", op.command.title());
    workbench.answer_git_op(&op, text, cx);
}

/// Runs `command`, typed in `editor`, a side of `diff`: `:w`, `do`, `dp`.
pub(super) fn host_command(
    _: &mut Workbench,
    _diff: WeakEntity<DiffView>,
    editor: Entity<EditorView>,
    _: HostCommand,
    _: &mut Window,
    cx: &mut Context<Workbench>,
) {
    editor.update(cx, |editor, cx| editor.set_vim_message(HOST_COMMAND, cx));
}
