//! Committing the staged changes: `git.commit` and `git.commit_amend`, and
//! the host commands typed in the commit box (`:w`, `:wq`), routed here by
//! `git_commands`. Not supported yet.

use fe_gpui::EditorView;
use fe_vim::HostCommand;
use gpui::{Context, Entity, Window};

use super::{Workbench, git_commands::GitOp};

/// What a host command typed in the commit box answers.
const HOST_COMMAND: &str = "Not supported yet: this command in the commit box";

/// Runs `op`, a commit command.
pub(super) fn run(
    workbench: &mut Workbench,
    op: GitOp,
    _: &mut Window,
    cx: &mut Context<Workbench>,
) {
    let text = format!("Not supported yet: {}", op.command.title());
    workbench.answer_git_op(&op, text, cx);
}

/// Runs `command`, typed in `editor`, the commit box.
pub(super) fn host_command(
    _: &mut Workbench,
    editor: Entity<EditorView>,
    _: HostCommand,
    _: &mut Window,
    cx: &mut Context<Workbench>,
) {
    editor.update(cx, |editor, cx| editor.set_vim_message(HOST_COMMAND, cx));
}
