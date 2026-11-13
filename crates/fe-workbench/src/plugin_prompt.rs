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
use gpui::{App, PromptLevel, SharedString, Task, Window};
use kiln_l10n::tr;
use kiln_plugin::{
    grants::{ProgramIdentity, Version, needs_wildcard_acceptance},
    warnings::{Warning, combination_warnings, describe},
};

/// The first prompt's buttons, and the indexes of the answers: an answer
/// is matched by its button's index, never its title.
fn buttons() -> [SharedString; 2] {
    [
        tr!("workbench-extensions-allow"),
        tr!("workbench-extensions-dont-allow"),
    ]
}
const ALLOW: usize = 0;
const DONT_ALLOW: usize = 1;

/// The full-access prompt's buttons, and the indexes of the answers.
fn full_access_buttons() -> [SharedString; 2] {
    [
        tr!("workbench-extensions-plugin-allow-full-access"),
        tr!("workbench-extensions-dont-allow"),
    ]
}
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
    /// What each `process.spawn:<program>` it asks for resolves to now,
    /// for the warning about one that runs through an interpreter.
    pub programs: Vec<ProgramIdentity>,
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
    let buttons = buttons();
    let answer = window.prompt(
        PromptLevel::Warning,
        &question,
        Some(&detail),
        &buttons.each_ref().map(|button| button.as_ref()),
        cx,
    );
    let full_access = pending
        .needs_full_access()
        .then(|| full_access_prompt_text(pending));
    window.spawn(cx, async move |cx| {
        let answer = answer.await.ok();
        // An answer that comes after the window closed decides nothing.
        let Ok(second) = cx.update(|window, cx| match (answer, &full_access) {
            (Some(ALLOW), Some((question, detail))) => {
                let buttons = full_access_buttons();
                Some(window.prompt(
                    PromptLevel::Critical,
                    question,
                    Some(detail),
                    &buttons.each_ref().map(|button| button.as_ref()),
                    cx,
                ))
            }
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
    // Each paragraph is a whole message; a blank line parts them.
    let mut detail = format!(
        "{}\n\n",
        tr!(
            "workbench-extensions-plugin-prompt-header",
            id = pending.id.as_str(),
            version = version_text(pending.version),
            license = pending.license.as_str(),
            dir = pending.dir.display().to_string()
        )
    );
    let question = match &pending.ask {
        PluginAsk::Install => {
            push_line(&mut detail, tr!("workbench-extensions-plugin-asks-to"));
            detail.push_str(&capabilities_text(&pending.requested));
            tr!(
                "workbench-extensions-plugin-load-question",
                name = pending.name.as_str()
            )
        }
        PluginAsk::Escalation { added, .. } => {
            push_line(&mut detail, tr!("workbench-extensions-plugin-adds"));
            detail.push_str(&capabilities_text(added));
            let kept: Grants = pending
                .requested
                .iter()
                .filter(|capability| !added.iter().any(|added| added == *capability))
                .cloned()
                .collect();
            if !kept.is_empty() {
                push_line(&mut detail, tr!("workbench-extensions-plugin-keeps"));
                detail.push_str(&capabilities_text(&kept));
            }
            tr!(
                "workbench-extensions-plugin-more-access-question",
                name = pending.name.as_str(),
                version = version_text(pending.version)
            )
        }
    };
    // The warnings are for everything it would hold, since what was
    // granted before can combine with what's added.
    let warnings = combination_warnings(&pending.requested);
    let batch = interpreter_warnings(&pending.programs);
    if !warnings.is_empty() || !batch.is_empty() {
        detail.push('\n');
        push_line(&mut detail, tr!("workbench-extensions-plugin-warnings"));
        detail.push_str(&warnings_text(&warnings));
        detail.push_str(&batch);
    }
    if pending.needs_full_access() {
        detail.push('\n');
        push_line(
            &mut detail,
            tr!("workbench-extensions-plugin-full-access-note"),
        );
    }
    detail.push('\n');
    detail.push_str(&match &pending.ask {
        PluginAsk::Install => tr!("workbench-extensions-plugin-install-footer"),
        PluginAsk::Escalation {
            keep_running: None, ..
        } => tr!("workbench-extensions-plugin-escalation-footer"),
        PluginAsk::Escalation {
            keep_running: Some(version),
            ..
        } => tr!(
            "workbench-extensions-plugin-escalation-footer-version",
            version = version_text(*version)
        ),
    });
    (question.into(), detail)
}

/// The question and the detail the full-access prompt shows for `pending`,
/// which asks for `net:*` or `process.spawn:*`.
fn full_access_prompt_text(pending: &PendingPluginGrant) -> (String, String) {
    let question = tr!(
        "workbench-extensions-plugin-full-access-question",
        name = pending.name.as_str()
    );
    let extreme: Vec<Warning> = combination_warnings(pending.asked())
        .into_iter()
        .filter(Warning::is_extreme)
        .collect();
    let asks_for = tr!(
        "workbench-extensions-plugin-full-access-asks-for",
        id = pending.id.as_str(),
        version = version_text(pending.version)
    );
    let detail = format!(
        "{}\n{}\n{}\n{}",
        asks_for,
        wildcards_text(pending.asked()),
        warnings_text(&extreme),
        tr!("workbench-extensions-plugin-full-access-footer"),
    );
    (question.into(), detail)
}

/// Adds `line` to `text`, then a line break.
fn push_line(text: &mut String, line: SharedString) {
    text.push_str(&line);
    text.push('\n');
}

/// `grants`, a line each with what it lets the plugin do.
fn capabilities_text(grants: &Grants) -> String {
    grants
        .iter()
        .map(|capability| {
            let line = tr!(
                "workbench-extensions-plugin-capability",
                capability = capability.to_string(),
                description = prompt_description(capability)
            );
            format!("  {}\n", line)
        })
        .collect()
}

/// The language servers fastedit ships a spec for, which `lsp:<server>`
/// may name besides the plugin's own (`fe_lsp`'s built-in registry).
const BUILT_IN_SERVERS: [&str; 9] = [
    "rust-analyzer",
    "pyright",
    "typescript-language-server",
    "typescript-go",
    "vscode-css-language-server",
    "gopls",
    "jdtls",
    "kotlin-lsp",
    "csharp-ls",
];

/// What `capability` lets the plugin do; `lsp:<server>` also says whether
/// the server is built in.
fn prompt_description(capability: &Capability) -> String {
    match capability {
        Capability::Lsp(server) if BUILT_IN_SERVERS.contains(&server.as_str()) => tr!(
            "workbench-extensions-plugin-lsp-built-in",
            server = server.as_str()
        )
        .to_string(),
        Capability::Lsp(server) => tr!(
            "workbench-extensions-plugin-lsp-not-built-in",
            server = server.as_str()
        )
        .to_string(),
        _ => describe(capability),
    }
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

/// A line for each of `programs` that runs through an interpreter
/// (`cmd.exe` for a batch file), which reads its arguments again.
fn interpreter_warnings(programs: &[ProgramIdentity]) -> String {
    programs
        .iter()
        .filter_map(|program| {
            let interpreter = program.interpreter.as_deref()?;
            let line = tr!(
                "workbench-extensions-plugin-warning-interpreter",
                program = program.name.as_str(),
                interpreter = interpreter
            );
            Some(format!("  {}\n", line))
        })
        .collect()
}

/// `warnings`, a line each.
fn warnings_text(warnings: &[Warning]) -> String {
    warnings
        .iter()
        .map(|warning| format!("  {}\n", warning))
        .collect()
}

fn version_text((major, minor, patch): Version) -> String {
    format!("{major}.{minor}.{patch}")
}

#[cfg(test)]
#[path = "plugin_prompt_tests.rs"]
mod plugin_prompt_tests;
