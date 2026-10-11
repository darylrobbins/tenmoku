//! Workspace trust (docs/decisions/lsp-client.md Q7): whether the
//! workbench's root may have language servers, which can run the project's
//! code.
//!
//! The decisions, their file and the root's trust are kiln-shell's
//! ([`kiln_shell::trust`]): each window's [`Workbench`] holds a
//! [`RootTrust`] for its root and emits [`TrustChanged`] when its trust
//! changes. The first time a folder is opened as one (a folder launch, or
//! Open Folder…), the workbench asks whether to trust it, in fastedit's
//! words ([`prompt`]). `workbench.trust_folder` and
//! `workbench.untrust_folder` change the root's decision; their handlers
//! only name it, and the workbench records it ([`Workbench::set_root_trust`]).

use std::rc::Rc;

use fe_command::{Actor, CommandTarget, ContextPredicate, Dispatched, Effect, Registry};
use fe_gpui::commands::RunCommand;
use gpui::{App, Context, Global, Window};
#[cfg(test)]
pub(crate) use kiln_shell::trust::decide;
use kiln_shell::trust::{RootTrust, TrustHost, TrustPrompt};
pub use kiln_shell::trust::{TrustChanged, set_trust_file};

use crate::Workbench;

/// The prompt's question.
const QUESTION: &str = "Do you trust the authors of the files in this folder?";

/// What the workbench asks the first time a folder is opened as one.
pub(crate) fn prompt() -> TrustPrompt {
    TrustPrompt::new(QUESTION, |folder| {
        format!(
            "{}\n\nTrusting this folder lets fastedit start language servers in it, which \
             can run the project's code: rust-analyzer runs build scripts and proc macros. \
             Its subfolders are trusted too. Change this later with File > Trust This \
             Folder or Untrust This Folder.",
            folder.display()
        )
    })
}

/// Registers the trust commands, and the session's decisions unless
/// [`set_trust_file`] came first. Quitting waits for a write under way.
pub(crate) fn init(cx: &mut App) {
    cx.set_global(Commands(Rc::new(registry())));
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

/// The [`CommandTarget`] of the trust commands: whether to trust the root.
pub(crate) struct TrustCommands;

impl CommandTarget for TrustCommands {
    type Tx = ();
    type Target<'a> = Option<bool>;
}

struct Commands(Rc<Registry<TrustCommands>>);

impl Global for Commands {}

pub(crate) fn registry() -> Registry<TrustCommands> {
    let commands = [
        ("workbench.trust_folder", "Trust This Folder", true),
        ("workbench.untrust_folder", "Untrust This Folder", false),
    ];
    let mut registry = Registry::new();
    for (id, title, trusted) in commands {
        registry
            .register(id, title, move |target: &mut Option<bool>, _: &Actor| {
                *target = Some(trusted);
                Ok(Dispatched::new(Effect::Notify))
            })
            .expect("trust command IDs are valid and distinct")
            .context(ContextPredicate::identifier(crate::KEY_CONTEXT));
    }
    registry
}

/// Whether `run` trusts (`true`) or untrusts the root, if it's a trust
/// command that applies in its context.
pub(crate) fn op(run: &RunCommand, cx: &App) -> Option<bool> {
    let registry = cx.try_global::<Commands>()?.0.clone();
    registry.get(&run.0.command)?;
    let mut trusted = None;
    if let Err(error) = registry.dispatch(&mut trusted, &run.0) {
        log::warn!("{error}");
    }
    trusted
}

#[cfg(test)]
#[path = "trust_tests.rs"]
mod tests;
