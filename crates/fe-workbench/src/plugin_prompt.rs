//! The prompt for a plugin waiting on the user's grant (extensions record
//! §7): a native prompt naming the plugin, where it's from, every
//! capability it asks for in plain words, and the warnings their
//! combination calls for ([`kiln_plugin::warnings`]). An update that asks
//! for more says what it adds, and that the installed version keeps
//! running meanwhile.
//!
//! `net:*` and `process.spawn:*` are extreme: Allow on a request that adds
//! one only leads to a second prompt, and only its separate "Allow Full
//! Access" grants. The answer comes back as a [`PluginAnswer`]; recording it
//! in the [`kiln_plugin::grants::GrantStore`] is the caller's.

use std::path::PathBuf;

use fe_command::capability::{Capability, Grants, Target};
use gpui::{App, PromptLevel, Task, Window};
use kiln_plugin::{
    grants::{Version, needs_wildcard_acceptance},
    warnings::{Warning, combination_warnings, describe},
};

/// The first prompt's buttons, and the indexes of the answers.
const BUTTONS: [&str; 2] = ["Allow", "Don't Allow"];
const ALLOW: usize = 0;
const DONT_ALLOW: usize = 1;

/// The full-access prompt's buttons, and the indexes of the answers.
const FULL_ACCESS_BUTTONS: [&str; 2] = ["Allow Full Access", "Don't Allow"];
const ALLOW_FULL_ACCESS: usize = 0;
const DONT_ALLOW_FULL_ACCESS: usize = 1;

/// A plugin in the user's extensions directory waiting for the user to
/// grant what it asks for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PendingPluginGrant {
    pub id: String,
    /// The name people see.
    pub name: String,
    pub version: Version,
    pub license: String,
    /// The extension's directory.
    pub dir: PathBuf,
    /// Everything its manifest asks for.
    pub requested: Grants,
    pub ask: PluginAsk,
}

/// Why a [`PendingPluginGrant`] is asked about, as
/// [`kiln_plugin::grants::evaluate`] decided.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PluginAsk {
    /// Nothing was ever granted to it.
    Install,
    /// It asks for `added` beyond what was granted, while `keep_running`,
    /// the installed version, runs on with its own grants.
    Escalation {
        added: Grants,
        keep_running: Option<Version>,
    },
}

impl PendingPluginGrant {
    /// What granting adds: the whole request for an install.
    fn asked(&self) -> &Grants {
        match &self.ask {
            PluginAsk::Install => &self.requested,
            PluginAsk::Escalation { added, .. } => added,
        }
    }

    /// Whether granting needs the full-access prompt's own acceptance.
    fn needs_full_access(&self) -> bool {
        needs_wildcard_acceptance(self.asked())
    }
}

/// What the user answered.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PluginAnswer {
    /// Grant exactly what was requested. `accept_wildcards` is true only
    /// when the request adds `net:*` or `process.spawn:*` and the user chose
    /// Allow Full Access for it, as `GrantStore::grant` takes it.
    Allowed { accept_wildcards: bool },
    /// Don't Allow, at either prompt: record the request as declined.
    Declined,
    /// No answer, as when the prompt's window closed: ask again later.
    Pending,
}

/// Asks in `window` about `pending`, and after Allow on a request that adds
/// a wildcard, asks again for full access. The task gives the answer; it
/// stops asking if dropped.
pub fn show_plugin_prompt(
    window: &mut Window,
    pending: &PendingPluginGrant,
    cx: &mut App,
) -> Task<PluginAnswer> {
    let (question, detail) = plugin_prompt_text(pending);
    let answer = window.prompt(PromptLevel::Warning, &question, Some(&detail), &BUTTONS, cx);
    let full_access = pending
        .needs_full_access()
        .then(|| full_access_prompt_text(pending));
    window.spawn(cx, async move |cx| {
        let answer = answer.await.ok();
        // An answer that comes after the window closed decides nothing.
        let Ok(second) = cx.update(|window, cx| match (answer, &full_access) {
            (Some(ALLOW), Some((question, detail))) => Some(window.prompt(
                PromptLevel::Critical,
                question,
                Some(detail),
                &FULL_ACCESS_BUTTONS,
                cx,
            )),
            _ => None,
        }) else {
            return PluginAnswer::Pending;
        };
        match (answer, second) {
            (Some(ALLOW), None) => PluginAnswer::Allowed {
                accept_wildcards: false,
            },
            (Some(DONT_ALLOW), _) => PluginAnswer::Declined,
            (Some(ALLOW), Some(second)) => {
                let second = second.await.ok();
                if cx.update(|_, _| ()).is_err() {
                    return PluginAnswer::Pending;
                }
                match second {
                    Some(ALLOW_FULL_ACCESS) => PluginAnswer::Allowed {
                        accept_wildcards: true,
                    },
                    Some(DONT_ALLOW_FULL_ACCESS) => PluginAnswer::Declined,
                    _ => PluginAnswer::Pending,
                }
            }
            _ => PluginAnswer::Pending,
        }
    })
}

/// The question and the detail the first prompt shows for `pending`.
pub fn plugin_prompt_text(pending: &PendingPluginGrant) -> (String, String) {
    let mut detail = format!(
        "Plugin: {} {}, licensed {}\nFrom: {}\n\n",
        pending.id,
        version_text(pending.version),
        pending.license,
        pending.dir.display(),
    );
    let question = match &pending.ask {
        PluginAsk::Install => {
            detail.push_str("It asks to:\n");
            detail.push_str(&capabilities_text(&pending.requested));
            format!("Allow the plugin {} to load?", pending.name)
        }
        PluginAsk::Escalation { added, .. } => {
            detail.push_str("This version asks for more than you granted. It adds:\n");
            detail.push_str(&capabilities_text(added));
            let kept: Grants = pending
                .requested
                .iter()
                .filter(|capability| !added.iter().any(|added| added == *capability))
                .cloned()
                .collect();
            if !kept.is_empty() {
                detail.push_str("It keeps:\n");
                detail.push_str(&capabilities_text(&kept));
            }
            format!(
                "Allow {} {} more access?",
                pending.name,
                version_text(pending.version)
            )
        }
    };
    // The warnings are for everything it would hold, since what was
    // granted before can combine with what's added.
    let warnings = combination_warnings(&pending.requested);
    if !warnings.is_empty() {
        detail.push_str("\nWarnings:\n");
        detail.push_str(&warnings_text(&warnings));
    }
    if pending.needs_full_access() {
        detail.push_str(
            "\nIt asks for full access: after Allow, you'll be asked to accept that on its own.\n",
        );
    }
    detail.push_str(&match &pending.ask {
        PluginAsk::Install => "\nIt doesn't load until you allow it. Don't Allow keeps it from \
                               loading, and isn't asked again until it asks for something else."
            .to_owned(),
        PluginAsk::Escalation { keep_running, .. } => {
            let installed = keep_running.map_or_else(
                || "The installed version".to_owned(),
                |version| format!("The installed version, {},", version_text(version)),
            );
            format!(
                "\n{installed} keeps running with what you granted it until you allow \
                 this. Don't Allow keeps it running as it is, and isn't asked again until the \
                 request changes."
            )
        }
    });
    (question, detail)
}

/// The question and the detail the full-access prompt shows for `pending`,
/// which asks for `net:*` or `process.spawn:*`.
fn full_access_prompt_text(pending: &PendingPluginGrant) -> (String, String) {
    let question = format!("Give {} full access?", pending.name);
    let extreme: Vec<Warning> = combination_warnings(pending.asked())
        .into_iter()
        .filter(Warning::is_extreme)
        .collect();
    let detail = format!(
        "{} {} asks for:\n{}\n{}\nOnly Allow Full Access grants it. Don't Allow grants \
         nothing it asks for.",
        pending.id,
        version_text(pending.version),
        wildcards_text(pending.asked()),
        warnings_text(&extreme),
    );
    (question, detail)
}

/// `grants`, a line each with what it lets the plugin do.
fn capabilities_text(grants: &Grants) -> String {
    grants
        .iter()
        .map(|capability| format!("  {capability}: {}\n", describe(capability)))
        .collect()
}

/// The wildcards in `grants`, a line each.
fn wildcards_text(grants: &Grants) -> String {
    let wildcards: Grants = grants
        .iter()
        .filter(|capability| {
            matches!(
                capability,
                Capability::Net(Target::Any) | Capability::ProcessSpawn(Target::Any)
            )
        })
        .cloned()
        .collect();
    capabilities_text(&wildcards)
}

/// `warnings`, a line each.
fn warnings_text(warnings: &[Warning]) -> String {
    warnings
        .iter()
        .map(|warning| format!("  {warning}\n"))
        .collect()
}

fn version_text((major, minor, patch): Version) -> String {
    format!("{major}.{minor}.{patch}")
}

#[cfg(test)]
#[path = "plugin_prompt_tests.rs"]
mod plugin_prompt_tests;
