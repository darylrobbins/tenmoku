//! A line's blame and its commit: `git.toggle_line_blame`,
//! `git.blame_line` and `git.open_line_commit`, routed here by `git_commands`. Not supported yet.

use gpui::{Context, Window};

use super::{Workbench, git_commands::GitOp};

/// Runs `op`, a blame command.
pub(super) fn run(
    workbench: &mut Workbench,
    op: GitOp,
    _: &mut Window,
    cx: &mut Context<Workbench>,
) {
    let text = format!("Not supported yet: {}", op.command.title());
    workbench.answer_git_op(&op, text, cx);
}
