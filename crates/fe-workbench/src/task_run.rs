//! Running a project's task ([`crate::task_defs`]): in a tab of its own on the
//! terminal panel, whose output, rerun and exit report are the panel's
//! (`TerminalPanel::add_task`). Nothing here is a command yet.

use gpui::{App, Context, Window};

use crate::{
    Workbench,
    task_defs::Task,
    workbench::toast::{self, Toast},
};

/// What running a task says in a folder that isn't trusted.
pub(crate) const UNTRUSTED: &str = "Trust this folder to run its tasks";

impl Workbench {
    /// Runs `task` and shows the terminal dock with it, focused there as a
    /// new terminal is, and remembers it as the last task. A task's command
    /// comes from the project, so one in a root that isn't trusted is refused
    /// with a toast offering to trust it.
    pub fn run_task(&mut self, task: Task, window: &mut Window, cx: &mut Context<Self>) {
        if !self.root_trusted() {
            let trust = fe_command::CommandId::parse("workbench.trust_folder")
                .expect("the command's id is valid");
            let toast = Toast::warning(UNTRUSTED).action("Trust Folder", trust);
            return toast::show(cx, toast);
        }
        let terminal = self.terminal().clone();
        let started = terminal.update(cx, |terminal, cx| terminal.add_task(&task, window, cx));
        if started {
            self.show_terminal_dock(window, cx);
        }
    }

    /// The task last run in this root, for a rerun.
    pub fn last_task(&self, cx: &App) -> Option<Task> {
        self.terminal().read(cx).last_task().cloned()
    }
}
