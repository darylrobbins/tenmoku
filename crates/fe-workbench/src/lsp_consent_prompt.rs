//! The prompt for an extension's language server waiting on the user's
//! consent ([`crate::lsp_consent`]; extensions record §1): one native
//! prompt at a time, in the active window, showing what the server would
//! run: its resolved executable, every argument, its environment, a warning
//! for an interpreter and what changed since the last grant.
//!
//! Windows don't ask twice: a server one window is asking about isn't
//! offered to another, and a window asks about one at a time, whether by
//! itself or from the extensions list ([`ask_in`]). A prompt closed without
//! a choice (its window closed) leaves the server pending, and the next
//! window to be active asks again. A server the user declined isn't asked
//! about again until its invocation changes, unless the extensions list
//! asks.

use std::{
    collections::{HashMap, HashSet, VecDeque},
    path::PathBuf,
};

use gpui::{App, Context, Global, PromptLevel, SharedString, Subscription, Window, WindowId};
use kiln_l10n::tr;

use crate::{
    ExtensionConsent, Invocation, PendingConsent, Workbench, decline_extension_server,
    grant_extension_server, pending_extension_consents,
};

/// The prompt's buttons, and the indexes of the answers: an answer is
/// matched by its button's index, never its title.
fn buttons() -> [SharedString; 2] {
    [
        tr!("workbench-extensions-allow"),
        tr!("workbench-extensions-dont-allow"),
    ]
}
const ALLOW: usize = 0;
const DONT_ALLOW: usize = 1;

/// A server by its extension's canonical directory and its name.
pub(crate) type ServerKey = (PathBuf, String);

/// `pending`'s server.
pub(crate) fn server_key(pending: &PendingConsent) -> ServerKey {
    (pending.extension_dir.clone(), pending.server.clone())
}

/// The servers some window is asking about, and the window.
#[derive(Default)]
struct Asking(HashMap<ServerKey, WindowId>);

impl Global for Asking {}

/// A workbench's follow of the pending consents.
pub(crate) struct ConsentPrompt {
    _subscriptions: [Subscription; 2],
}

impl ConsentPrompt {
    /// Asks when `window` becomes active and when the consents change, and
    /// once the workbench exists.
    pub(crate) fn new(window: &mut Window, cx: &mut Context<Workbench>) -> Self {
        let activation = cx.observe_window_activation(window, |_, window, cx| {
            ask_in(window, cx, Ask::Undecided);
        });
        let changed = cx.observe_global_in::<ExtensionConsent>(window, |_, window, cx| {
            ask_in(window, cx, Ask::Undecided);
        });
        cx.defer_in(window, |_, window, cx| {
            ask_in(window, cx, Ask::Undecided);
        });
        Self {
            _subscriptions: [activation, changed],
        }
    }
}

/// Which servers a window asks about, one after another.
pub(crate) enum Ask {
    /// Each waiting for a first decision, while the window is active.
    Undecided,
    /// These, in order, each while it waits, undecided or declined: the
    /// extensions list's, for one extension.
    These(VecDeque<ServerKey>),
}

/// Asks in `window` about the next server of `ask` that waits for a decision
/// now and that no window is asking about, unless `window` is asking about
/// one already; once that's answered, about the next. Returns whether it
/// asked. Not to be called while a workbench is being updated by anything
/// but this window's own view.
pub(crate) fn ask_in(window: &mut Window, cx: &mut App, mut ask: Ask) -> bool {
    let Some(pending) = next_to_ask(window, cx, &mut ask) else {
        return false;
    };
    ask_about(window, cx, pending, ask);
    true
}

/// The next server of `ask` that `window` may ask about now, as
/// [`ask_in`] chooses, taken from `ask`.
pub(crate) fn next_to_ask(window: &Window, cx: &mut App, ask: &mut Ask) -> Option<PendingConsent> {
    if matches!(ask, Ask::Undecided) && !window.is_window_active() {
        return None;
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
    if asking.0.values().any(|window| *window == me) {
        return None;
    }
    let asked: HashSet<ServerKey> = asking.0.keys().cloned().collect();
    let waiting = pending_extension_consents(cx);
    let free = |pending: &&PendingConsent| !asked.contains(&server_key(pending));
    let next = match ask {
        Ask::Undecided => waiting
            .iter()
            .filter(free)
            .find(|pending| !pending.declined),
        Ask::These(keys) => std::iter::from_fn(|| keys.pop_front())
            .find_map(|key| waiting.iter().filter(free).find(|p| server_key(p) == key)),
    };
    next.cloned()
}

/// Asks in `window` about `pending`, which [`next_to_ask`] gave, then about
/// the rest of `ask` once it's answered.
pub(crate) fn ask_about(window: &mut Window, cx: &mut App, pending: PendingConsent, ask: Ask) {
    let me = window.window_handle().window_id();
    let key = server_key(&pending);
    cx.default_global::<Asking>().0.insert(key.clone(), me);
    let (question, detail) = consent_prompt_text(&pending);
    let buttons = buttons();
    let answer = window.prompt(
        PromptLevel::Warning,
        &question,
        Some(&detail),
        &buttons.each_ref().map(|button| button.as_ref()),
        cx,
    );
    window
        .spawn(cx, async move |cx| {
            let answer = answer.await.ok();
            // Outside any update of a workbench: a decision hands the
            // servers to every window's managers.
            cx.update(|window, cx| {
                cx.global_mut::<Asking>().0.remove(&key);
                match answer {
                    Some(ALLOW) => grant_extension_server(cx, &pending).ok(),
                    Some(DONT_ALLOW) => decline_extension_server(cx, &pending).ok(),
                    _ => return,
                };
                ask_in(window, cx, ask);
            })
            .ok();
        })
        .detach();
}

/// The question and the detail the prompt shows for `pending`. Each
/// paragraph and each line is a whole message.
pub(crate) fn consent_prompt_text(pending: &PendingConsent) -> (String, String) {
    let question = tr!(
        "workbench-extensions-lsp-consent-question",
        name = pending.extension_name.as_str()
    );
    let mut detail = tr!(
        "workbench-extensions-lsp-consent-header",
        id = pending.extension_id.as_str(),
        version = pending.extension_version.as_str(),
        license = pending.extension_license.as_str(),
        dir = pending.extension_dir.display().to_string(),
        server = pending.server.as_str(),
        languages = pending.languages.join(", ")
    )
    .to_string();
    detail.push_str("\n\n");
    if let Some(previous) = &pending.previous {
        detail.push_str(&tr!("workbench-extensions-lsp-consent-changed"));
        detail.push('\n');
        detail.push_str(&invocation_text(previous));
        detail.push('\n');
        detail.push_str(&tr!("workbench-extensions-lsp-consent-now"));
        detail.push('\n');
    }
    detail.push_str(&invocation_text(&pending.invocation));
    let invocation = &pending.invocation;
    if invocation.runs_arbitrary_code() {
        let program = invocation.command.file_name().map_or_else(
            || invocation.command.to_string_lossy(),
            |name| name.to_string_lossy(),
        );
        detail.push('\n');
        detail.push_str(&tr!(
            "workbench-extensions-lsp-consent-interpreter",
            program = program
        ));
        for hashed in invocation.script_hashes.values() {
            // "<sha256> <canonical path>"
            if let Some((_, path)) = hashed.split_once(' ') {
                detail.push('\n');
                detail.push_str(&tr!("workbench-extensions-lsp-consent-script", path = path));
            }
        }
    }
    detail.push_str("\n\n");
    detail.push_str(&tr!("workbench-extensions-lsp-consent-footer"));
    (question.into(), detail)
}

/// `invocation`'s executable, arguments and environment, a line each.
fn invocation_text(invocation: &Invocation) -> String {
    let mut lines = vec![tr!(
        "workbench-extensions-lsp-consent-runs",
        command = invocation.command.display().to_string()
    )];
    for arg in &invocation.args {
        lines.push(tr!("workbench-extensions-lsp-consent-argument", arg = arg));
    }
    for (name, value) in &invocation.env {
        lines.push(tr!(
            "workbench-extensions-lsp-consent-environment",
            name = name,
            value = value
        ));
    }
    lines.iter().map(|line| format!("  {}\n", line)).collect()
}

#[cfg(test)]
#[path = "lsp_consent_prompt_tests.rs"]
mod lsp_consent_prompt_tests;
