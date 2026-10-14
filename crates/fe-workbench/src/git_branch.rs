//! Switching to a branch and creating one: `git.switch_branch` and
//! `git.create_branch`, with the branch typed after `:Git switch` in
//! [`GitOp::args`], routed here by `git_commands`. Not supported yet.

use gpui::{Context, Window};

use super::{Workbench, git_commands::GitOp};

/// Runs `op`, a branch command.
pub(super) fn run(
    workbench: &mut Workbench,
    op: GitOp,
    _: &mut Window,
    cx: &mut Context<Workbench>,
) {
    let text = format!("Not supported yet: {}", op.command.title());
    workbench.answer_git_op(&op, text, cx);
}
