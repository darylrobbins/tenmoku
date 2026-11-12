//! Workspace trust (docs/decisions/lsp-client.md Q7): whether the
//! workbench's root may have language servers, which can run the project's
//! code.
//!
//! The decisions, their file and the root's trust are kiln-shell's
//! ([`kiln_shell::trust`]): each window's [`Workbench`] holds a
//! [`RootTrust`] for its root and emits [`TrustChanged`] when its trust
//! changes. The first time a folder is opened as one (a folder launch, or
//! Open Folder…), the workbench asks whether to trust it, in Tenmoku's
//! words ([`prompt`]). `workbench.trust_folder` and
//! `workbench.untrust_folder` change the root's decision: they're typed,
//! `#[kiln::command]` methods of the workbench (`workbench_commands.rs`)
//! that record it ([`Workbench::set_root_trust`]).

use gpui::{App, Context, Window};
use kiln_l10n::{text, tr};
#[cfg(test)]
pub(crate) use kiln_shell::trust::decide;
use kiln_shell::trust::{RootTrust, TrustHost, TrustPrompt};
pub use kiln_shell::trust::{TrustChanged, set_trust_file};

use crate::Workbench;

/// What the workbench asks the first time a folder is opened as one.
pub(crate) fn prompt() -> TrustPrompt {
    TrustPrompt::new(text!("workbench-extensions-trust-question"), |folder| {
        tr!(
            "workbench-extensions-trust-detail",
            folder = folder.display().to_string()
        )
        .into()
    })
}

/// Registers the session's decisions unless [`set_trust_file`] came
/// first. Quitting waits for a write under way.
pub(crate) fn init(cx: &mut App) {
    kiln_shell::trust::init(cx);
}

impl TrustHost for Workbench {
    fn root_trust(&mut self) -> &mut RootTrust {
        &mut self.trust
    }
}

impl Workbench {
    /// Follows the trust decisions; call once, from `new`.
    pub(crate) fn follow_trust(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        RootTrust::follow(self, window, cx);
    }

    /// Resolves the trust of a new root, `root`, opened as a folder if
    /// `as_folder` (else as a file's parent), off the main thread; it's
    /// untrusted until then. Then asks about it, if that's due.
    pub(crate) fn resolve_trust(
        &mut self,
        root: Option<std::path::PathBuf>,
        as_folder: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        RootTrust::resolve(self, root, as_folder, window, cx);
    }

    /// Whether the root is trusted to start language servers in: true only
    /// once its canonical path is resolved and is `Trusted`, its own
    /// decision or its nearest decided ancestor's.
    pub fn root_trusted(&self) -> bool {
        self.trust.trusted()
    }

    /// Records whether the root's canonical folder is trusted, as Trust
    /// This Folder and Untrust This Folder do, for every window. One
    /// before the root is resolved applies once it is; with no root it
    /// does nothing.
    pub fn set_root_trust(&mut self, trusted: bool, cx: &mut Context<Self>) {
        RootTrust::set_trusted(self, trusted, cx);
    }
}

#[cfg(test)]
#[path = "trust_tests.rs"]
mod tests;
