//! The notification inbox, opened from the status bar's bell
//! (`notifications.show`): Needs you, an agent's row on the agent's soft
//! colour and a task's on the `selected` surface, each with its action;
//! Earlier, with Clear earlier; the popup on the raised surface above the
//! bell, the keyboard's row ringed in the focus colour.

use fe_command::CommandId;
use fe_workbench::Workbench;

use crate::{
    audit_scenes::{DEFAULT, Step, Surface, idle, run},
    token_probe::{Probe, probe, probe_over},
};

pub(crate) const SURFACE: Surface = Surface {
    name: "inbox",
    steps: |_| vec![notifications(), idle(), run("notifications.show"), idle()],
    probes: PROBES,
    ..DEFAULT
};

const PROBES: &[Probe] = &[
    // The popup's edge, left of the rows.
    probe(614., 470., "surface.raised"),
    // The agent's row, left of its icon: the agent's tint.
    probe_over(621., 310., "agent.soft", "surface.raised"),
    // The task's row, left of its icon: the selected surface.
    probe_over(621., 403., "state.selected", "surface.raised"),
    // The keyboard's row, the agent's: its focus ring, the popup's one
    // accent. The bell's dot is `accent.base` too, but the popup's float
    // shadow falls over the whole status bar under it.
    probe(617.5, 310., "state.focus_ring"),
];

/// Two notifications that need you, an agent's and a task's, and two
/// earlier ones, as their toasts raise them.
fn notifications() -> Step {
    Box::new(|_, _, cx| {
        let command = |id: &str| CommandId::parse(id).expect("a command ID");
        Workbench::notify_from(
            cx,
            log::Level::Info,
            "git",
            "Pushed main to origin".into(),
            Some("2 commits".into()),
            None,
        );
        Workbench::notify_from(
            cx,
            log::Level::Warn,
            "lsp",
            "rust-analyzer restarted".into(),
            Some("It stopped responding".into()),
            None,
        );
        Workbench::notify_from(
            cx,
            log::Level::Error,
            "task",
            "cargo test failed".into(),
            Some("2 tests failed in src/main.rs".into()),
            Some(("Rerun".into(), command("tasks.rerun_last"))),
        );
        Workbench::notify_from(
            cx,
            log::Level::Info,
            "agent",
            "Lane 2 finished".into(),
            Some("Counts words per line · 3 files changed".into()),
            Some(("Review changes".into(), command("ai.show_changes"))),
        );
    })
}
