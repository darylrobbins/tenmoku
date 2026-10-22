//! The prompt for an extension's language server waiting on the user's
//! consent ([`crate::lsp_consent`]; extensions record §1): one native
//! prompt at a time, in the active window, showing what the server would
//! run: its resolved executable, every argument, its environment, a warning
//! for an interpreter and what changed since the last grant.
//!
//! Windows don't ask twice: a server one window is asking about isn't
//! offered to another. A prompt closed without a choice (its window closed)
//! leaves the server pending, and the next window to be active asks again. A
//! server the user declined isn't asked about again until its invocation
//! changes.

use std::{collections::HashMap, path::PathBuf};

use gpui::{App, Context, Global, PromptLevel, Subscription, Window, WindowId};

use crate::{
    ExtensionConsent, Invocation, PendingConsent, Workbench, decline_extension_server,
    grant_extension_server, pending_extension_consents,
};

/// The prompt's buttons, and the indexes of the answers.
const BUTTONS: [&str; 2] = ["Allow", "Don't Allow"];
const ALLOW: usize = 0;
const DONT_ALLOW: usize = 1;

/// The servers some window is asking about, by extension directory and
/// server name, and the window.
#[derive(Default)]
struct Asking(HashMap<(PathBuf, String), WindowId>);

impl Global for Asking {}

/// A workbench's follow of the pending consents.
pub(crate) struct ConsentPrompt {
    /// Whether a prompt is up in this window: it shows one at a time.
    pub(crate) asking: bool,
    _subscriptions: [Subscription; 2],
}

impl ConsentPrompt {
    /// Asks when `window` becomes active and when the consents change, and
    /// once the workbench exists.
    pub(crate) fn new(window: &mut Window, cx: &mut Context<Workbench>) -> Self {
        let activation = cx.observe_window_activation(window, |workbench, window, cx| {
            workbench.ask_consent(window, cx)
        });
        let changed = cx.observe_global_in::<ExtensionConsent>(window, |workbench, window, cx| {
            workbench.ask_consent(window, cx)
        });
        cx.defer_in(window, |workbench, window, cx| {
            workbench.ask_consent(window, cx)
        });
        Self {
            asking: false,
            _subscriptions: [activation, changed],
        }
    }
}

impl Workbench {
    /// Asks about the first server waiting for a decision that no window is
    /// asking about, if this window is active and not asking already.
    fn ask_consent(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.consent_prompt.asking || !window.is_window_active() {
            return;
        }
        let open: Vec<WindowId> = cx
            .windows()
            .iter()
            .map(|handle| handle.window_id())
            .collect();
        let me = window.window_handle().window_id();
        let asking = cx.default_global::<Asking>();
        // A window that closed mid-prompt asks no more.
        asking.0.retain(|_, window| open.contains(window));
        let Some(pending) = pending_extension_consents(cx)
            .into_iter()
            .find(|pending| !pending.declined && !is_asked(pending, cx))
        else {
            return;
        };
        let key = (pending.extension_dir.clone(), pending.server.clone());
        cx.global_mut::<Asking>().0.insert(key.clone(), me);
        self.consent_prompt.asking = true;
        let (question, detail) = consent_prompt_text(&pending);
        let answer = window.prompt(PromptLevel::Warning, &question, Some(&detail), &BUTTONS, cx);
        cx.spawn_in(window, async move |workbench, cx| {
            let answer = answer.await.ok();
            // Outside any update of the workbench: a decision hands the
            // servers to every window's managers.
            let decided = cx.update(|window, cx| {
                cx.global_mut::<Asking>().0.remove(&key);
                match answer {
                    Some(ALLOW) => grant_extension_server(cx, &pending).ok(),
                    Some(DONT_ALLOW) => decline_extension_server(cx, &pending).ok(),
                    _ => None,
                };
                window.window_handle()
            });
            workbench
                .update_in(cx, |workbench, window, cx| {
                    workbench.consent_prompt.asking = false;
                    if decided.is_ok() && answer.is_some() {
                        workbench.ask_consent(window, cx);
                    }
                })
                .ok();
        })
        .detach();
    }
}

/// Whether some window is asking about `pending`'s server.
fn is_asked(pending: &PendingConsent, cx: &App) -> bool {
    cx.try_global::<Asking>().is_some_and(|asking| {
        asking
            .0
            .contains_key(&(pending.extension_dir.clone(), pending.server.clone()))
    })
}

/// The question and the detail the prompt shows for `pending`.
pub(crate) fn consent_prompt_text(pending: &PendingConsent) -> (String, String) {
    let question = format!("Allow {} to run a language server?", pending.extension_name);
    let mut detail = format!(
        "Extension: {} {}, licensed {}\nFrom: {}\nLanguage server: {} ({})\n",
        pending.extension_id,
        pending.extension_version,
        pending.extension_license,
        pending.extension_dir.display(),
        pending.server,
        pending.languages.join(", "),
    );
    if let Some(previous) = &pending.previous {
        detail.push_str("\nThis extension changed what it runs.\nBefore:\n");
        detail.push_str(&invocation_text(previous));
        detail.push_str("\nNow:\n");
    } else {
        detail.push('\n');
    }
    detail.push_str(&invocation_text(&pending.invocation));
    let invocation = &pending.invocation;
    if invocation.runs_arbitrary_code() {
        let program = invocation.command.file_name().map_or_else(
            || invocation.command.to_string_lossy(),
            |name| name.to_string_lossy(),
        );
        detail.push_str(&format!("\n{program} can run any code in its arguments."));
        for hashed in invocation.script_hashes.values() {
            // "<sha256> <canonical path>"
            if let Some((_, path)) = hashed.split_once(' ') {
                detail.push_str(&format!("\nScript: {path}"));
            }
        }
    }
    detail.push_str(
        "\n\nIt runs with your permissions, outside any sandbox. Don't Allow keeps it from \
         running and isn't asked again until the command changes.",
    );
    (question, detail)
}

/// `invocation`'s executable, arguments and environment, a line each.
fn invocation_text(invocation: &Invocation) -> String {
    let mut text = format!("  Runs: {}\n", invocation.command.display());
    for arg in &invocation.args {
        text.push_str(&format!("  Argument: {arg}\n"));
    }
    for (name, value) in &invocation.env {
        text.push_str(&format!("  Environment: {name}={value}\n"));
    }
    text
}

#[cfg(test)]
#[path = "lsp_consent_prompt_tests.rs"]
mod lsp_consent_prompt_tests;
