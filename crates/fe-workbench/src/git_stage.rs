//! Staging, unstaging and reverting a file or a change:
//! `git.stage_file`, `git.unstage_file`, `git.discard_file`,
//! `git.reset_buffer`, `git.stage_hunk`, `git.unstage_hunk` and
//! `git.revert_hunk`, routed here by `git_commands`. Not supported yet.

use gpui::{Context, Window};

use super::{Workbench, git_commands::GitOp};

/// Runs `op`, a staging command.
pub(super) fn run(
    workbench: &mut Workbench,
    op: GitOp,
    _: &mut Window,
    cx: &mut Context<Workbench>,
) {
    let text = format!("Not supported yet: {}", op.command.title());
    workbench.answer_git_op(&op, text, cx);
}
