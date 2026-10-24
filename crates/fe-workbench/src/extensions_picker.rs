//! Extensions: Show Installed (`workbench.show_extensions`): a [`Picker`]
//! over the extensions kiln has loaded ([`kiln::extensions::loaded_from`]),
//! the user's and a trusted workspace folder's, each with its kinds,
//! licence, source and state, and its language servers' effective
//! invocations (extensions record §1, §3, §6), then those of a workspace
//! folder that isn't trusted, skipped ([`kiln::extensions::skipped`]).
//! One the settings disable ([`kiln::extensions::disabled`]) is listed as
//! such. The rows follow the extensions and the consents while it shows.
//! Enter on one waiting on the user's consent, or that they declined, asks
//! about each of its servers in turn, those waiting first, through the
//! consent prompt ([`crate::lsp_consent_prompt::ask_in`]); on any other, it shows all the row has to
//! say. An extension that failed to load isn't listed: its diagnostics are
//! in the config problems list.

use std::{collections::VecDeque, path::PathBuf};

use gpui::{
    AnyElement, App, Context, FocusHandle, IntoElement as _, ParentElement as _, PromptLevel,
    SharedString, Styled as _, Subscription, Window, div, px,
};
use kiln::{
    core::{LicenseClass, Manifest, extension::ExtensionSource as Installed},
    extensions::Skipped,
};

use crate::{
    ExtensionConsent, Invocation, PendingConsent,
    lsp_consent::{GrantedServer, granted_extension_servers},
    lsp_consent_prompt::{Ask, ask_about, next_to_ask, server_key},
    pending_extension_consents,
    picker::{Picker, PickerDelegate},
};

/// Where an extension is installed from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ExtensionSource {
    User,
    Workspace,
}

impl From<Installed> for ExtensionSource {
    fn from(installed: Installed) -> Self {
        match installed {
            Installed::User => Self::User,
            Installed::Workspace => Self::Workspace,
        }
    }
}

/// What an extension is doing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum ExtensionState {
    Active,
    /// These servers wait for the user's decision.
    NeedsConsent {
        servers: Vec<String>,
    },
    /// The user declined these servers' invocations.
    Declined {
        servers: Vec<String>,
    },
    /// The settings' `extensions.disabled` has it: it contributes nothing.
    Disabled,
    /// A workspace folder's, which isn't trusted: it isn't loaded.
    SkippedUntrusted,
}

impl ExtensionState {
    fn label(&self) -> &'static str {
        match self {
            Self::Active => "Active",
            Self::NeedsConsent { .. } => "Needs consent",
            Self::Declined { .. } => "Declined",
            Self::Disabled => "Disabled",
            Self::SkippedUntrusted => "Skipped (untrusted)",
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct Row {
    pub(crate) name: String,
    pub(crate) id: String,
    /// Its directory: the row's identity, as the user's and a workspace's
    /// extension may have one ID.
    pub(crate) dir: PathBuf,
    pub(crate) version: String,
    /// "1 theme, 2 lsp servers": what it contributes.
    pub(crate) kinds: String,
    pub(crate) license: String,
    pub(crate) class: LicenseClass,
    pub(crate) source: ExtensionSource,
    pub(crate) state: ExtensionState,
    /// Each language server, its effective invocation and its status.
    pub(crate) servers: Vec<String>,
    /// Why it isn't loaded, if it isn't.
    pub(crate) reason: Option<String>,
    /// The servers waiting or declined, for the prompt.
    pending: Vec<PendingConsent>,
}

impl Row {
    /// `manifest`'s row: disabled if `disabled`, else as its servers'
    /// consents are, matched by its directory and their names.
    fn of(
        manifest: &Manifest,
        source: ExtensionSource,
        disabled: bool,
        pending: &[PendingConsent],
        granted: &[GrantedServer],
    ) -> Self {
        let (major, minor, patch) = manifest.version;
        // The consents' directories are canonical.
        let dir = (manifest.dir.canonicalize()).unwrap_or_else(|_| manifest.dir.clone());
        let own: Vec<PendingConsent> = (pending.iter())
            .filter(|pending| pending.extension_dir == dir)
            .cloned()
            .collect();
        let names = |declined: bool| -> Vec<String> {
            (own.iter())
                .filter(|pending| pending.declined == declined)
                .map(|pending| pending.server.clone())
                .collect()
        };
        let state = if disabled {
            ExtensionState::Disabled
        } else if !names(false).is_empty() {
            ExtensionState::NeedsConsent {
                servers: names(false),
            }
        } else if !names(true).is_empty() {
            ExtensionState::Declined {
                servers: names(true),
            }
        } else {
            ExtensionState::Active
        };
        let mut servers: Vec<String> = (own.iter())
            .map(|pending| {
                let status = if pending.declined {
                    "declined"
                } else {
                    "pending"
                };
                format!(
                    "{}: {} ({status})",
                    pending.server,
                    invocation_line(&pending.invocation)
                )
            })
            .collect();
        let listed = manifest.app.get("lsp").and_then(toml::Value::as_table);
        for name in listed.into_iter().flat_map(|table| table.keys()) {
            if own.iter().any(|pending| pending.server == *name) {
                continue;
            }
            let key = (dir.clone(), name.clone());
            if let Some(granted) = granted.iter().find(|granted| granted.key == key) {
                let status = if granted.running {
                    "granted"
                } else {
                    "granted; not running: an earlier extension's server of this name runs"
                };
                servers.push(format!(
                    "{name}: {} ({status})",
                    invocation_line(&granted.invocation)
                ));
            }
        }
        Self {
            name: manifest.name.clone(),
            id: manifest.id.clone(),
            dir: manifest.dir.clone(),
            version: format!("{major}.{minor}.{patch}"),
            kinds: kinds(manifest),
            license: manifest.license.expr.clone(),
            class: manifest.license.class,
            source,
            state,
            servers,
            reason: None,
            pending: own,
        }
    }

    /// A workspace extension kiln skipped, as its folder isn't trusted:
    /// none of its servers are asked about.
    fn skipped(skipped: &Skipped) -> Self {
        Self {
            state: ExtensionState::SkippedUntrusted,
            reason: Some(skipped.reason.clone()),
            ..Self::of(
                &skipped.manifest,
                ExtensionSource::Workspace,
                false,
                &[],
                &[],
            )
        }
    }

    /// The licence expression and its class.
    pub(crate) fn license_text(&self) -> String {
        let class = match self.class {
            LicenseClass::Permissive => "permissive",
            LicenseClass::Copyleft => "copyleft",
            LicenseClass::Unknown => "unknown",
        };
        format!("{} ({class})", self.license)
    }

    fn source_text(&self) -> &'static str {
        match self.source {
            ExtensionSource::User => "User",
            ExtensionSource::Workspace => "Workspace",
        }
    }

    /// The secondary line: kinds, licence, source, servers, and why it
    /// isn't loaded.
    pub(crate) fn detail(&self) -> String {
        let mut parts = vec![
            self.kinds.clone(),
            self.license_text(),
            self.source_text().to_owned(),
        ];
        parts.extend(self.servers.iter().cloned());
        parts.extend(self.reason.clone());
        parts.join(" · ")
    }

    /// Everything the row has to say, a line each, for the prompt Enter
    /// shows: the row's line may not have room for it.
    pub(crate) fn full_detail(&self) -> String {
        let mut lines = vec![
            format!("ID: {}", self.id),
            format!("Version: {}", self.version),
            format!("State: {}", self.state.label()),
            format!("Contributes: {}", self.kinds),
            format!("Licence: {}", self.license_text()),
            format!("Source: {}", self.source_text()),
            format!("Directory: {}", self.dir.display()),
        ];
        if !self.servers.is_empty() {
            lines.push("Language servers:".to_owned());
            lines.extend(self.servers.iter().map(|server| format!("  {server}")));
        }
        lines.extend(self.reason.clone());
        lines.join("\n")
    }

    /// The servers Enter asks about: each waiting for a first decision,
    /// then each declined, as the consents are now.
    fn to_ask(&self, cx: &App) -> VecDeque<(PathBuf, String)> {
        let dir = (self.pending.first()).map(|pending| pending.extension_dir.clone());
        let Some(dir) = dir else {
            return VecDeque::new();
        };
        let mut now: Vec<PendingConsent> = (pending_extension_consents(cx).into_iter())
            .filter(|pending| pending.extension_dir == dir)
            .collect();
        // Stable: undecided first, each in its order.
        now.sort_by_key(|pending| pending.declined);
        now.iter().map(server_key).collect()
    }
}

/// What `invocation` runs, as a shell would read it: its environment, its
/// executable and each argument, quoted where it must be.
fn invocation_line(invocation: &Invocation) -> String {
    let env = (invocation.env.iter()).map(|(name, value)| format!("{name}={}", quoted(value)));
    let command = std::iter::once(quoted(&invocation.command.to_string_lossy()));
    let args = invocation.args.iter().map(|arg| quoted(arg));
    env.chain(command).chain(args).collect::<Vec<_>>().join(" ")
}

/// `word` as a shell word: as it is if nothing in it is special, else
/// single-quoted.
fn quoted(word: &str) -> String {
    let plain = |c: char| c.is_ascii_alphanumeric() || "@%+=:,./-_".contains(c);
    if !word.is_empty() && word.chars().all(plain) {
        return word.to_owned();
    }
    format!("'{}'", word.replace('\'', "'\\''"))
}

/// What `manifest` contributes, with counts: "1 theme, 1 lsp server".
fn kinds(manifest: &Manifest) -> String {
    let count = |kind: &str| match manifest.app.get(kind) {
        Some(toml::Value::Array(items)) => items.len(),
        Some(toml::Value::Table(table)) => table.len(),
        _ => 0,
    };
    let counts = [
        ("theme", "themes", manifest.themes.len()),
        ("keymap", "keymaps", manifest.keymaps.len()),
        ("menu", "menus", manifest.menus.len()),
        ("lsp server", "lsp servers", count("lsp")),
        ("grammar", "grammars", count("grammars")),
        ("snippet file", "snippet files", count("snippets")),
    ];
    let parts: Vec<String> = (counts.into_iter())
        .filter(|(.., n)| *n > 0)
        .map(|(one, many, n)| format!("{n} {}", if n == 1 { one } else { many }))
        .collect();
    if parts.is_empty() {
        "no contributions".to_owned()
    } else {
        parts.join(", ")
    }
}

pub(crate) struct ExtensionsPicker {
    /// By name, A–Z.
    rows: Vec<Row>,
    /// The rows matching the query, as indices into `rows`.
    matches: Vec<usize>,
    /// The query, lowercased.
    query: String,
    /// What was focused before it opened, focused again once it closes.
    previous_focus: Option<FocusHandle>,
    /// The follow of the extensions and the consents while it shows.
    following: Option<[Subscription; 2]>,
}

impl ExtensionsPicker {
    pub(crate) fn new() -> Self {
        Self {
            rows: Vec::new(),
            matches: Vec::new(),
            query: String::new(),
            previous_focus: None,
            following: None,
        }
    }

    /// The picker is opening, with `previous_focus` focused until now, or
    /// opening afresh, keeping what was focused before, if that's `None`.
    /// From now until it closes, a change to the extensions or the
    /// consents shows in the rows.
    pub(crate) fn opened(
        &mut self,
        previous_focus: Option<FocusHandle>,
        cx: &mut Context<Picker<Self>>,
    ) {
        self.previous_focus = previous_focus.or(self.previous_focus.take());
        self.query.clear();
        self.load(cx);
        if self.following.is_none() {
            let picker = cx.entity().downgrade();
            let extensions = kiln::extensions::observe(cx, move |cx| {
                picker.update(cx, refresh_rows).ok();
            });
            let consents = cx.observe_global::<ExtensionConsent>(refresh_rows);
            self.following = Some([extensions, consents]);
        }
    }

    /// Reads the rows again, the query's matches among them.
    fn load(&mut self, cx: &App) {
        let pending = pending_extension_consents(cx);
        let granted = granted_extension_servers(cx);
        let disabled = kiln::extensions::disabled(cx);
        self.rows = (kiln::extensions::loaded_from(cx).iter())
            .map(|(source, manifest)| {
                let off = disabled.contains(&manifest.id);
                Row::of(manifest, (*source).into(), off, &pending, &granted)
            })
            .collect();
        self.rows.sort_by_key(|row| row.name.to_lowercase());
        let mut skipped: Vec<Row> = (kiln::extensions::skipped(cx).iter())
            .map(Row::skipped)
            .collect();
        skipped.sort_by_key(|row| row.name.to_lowercase());
        self.rows.extend(skipped);
        self.filter();
    }

    /// The rows whose name or ID holds the query, ignoring case.
    fn filter(&mut self) {
        let query = &self.query;
        self.matches = (self.rows.iter().enumerate())
            .filter(|(_, row)| {
                row.name.to_lowercase().contains(query) || row.id.to_lowercase().contains(query)
            })
            .map(|(ix, _)| ix)
            .collect();
    }

    fn row(&self, ix: usize) -> &Row {
        &self.rows[self.matches[ix]]
    }

    /// Stops following, and focuses what was focused before it opened.
    fn closed(&mut self, window: &mut Window, cx: &mut App) {
        self.following = None;
        if let Some(focus) = self.previous_focus.take() {
            window.focus(&focus, cx);
        }
    }
}

/// Reads `picker`'s rows again, keeping the query and the selected row, if
/// it's still there.
fn refresh_rows(picker: &mut Picker<ExtensionsPicker>, cx: &mut Context<Picker<ExtensionsPicker>>) {
    let count = picker.delegate.match_count();
    let selected = (picker.selected() < count).then(|| picker.delegate.match_id(picker.selected()));
    picker.delegate.load(cx);
    let count = picker.delegate.match_count();
    let ix = selected
        .and_then(|id| (0..count).find(|&ix| picker.delegate.match_id(ix) == id))
        .unwrap_or(picker.selected());
    picker.select(ix, cx);
}

#[cfg(test)]
impl ExtensionsPicker {
    /// The matching rows.
    pub(crate) fn rows(&self) -> Vec<Row> {
        (0..self.matches.len())
            .map(|ix| self.row(ix).clone())
            .collect()
    }
}

impl PickerDelegate for ExtensionsPicker {
    /// Rows whose name or ID holds `query`, ignoring case.
    fn update_matches(&mut self, query: String, cx: &mut Context<Picker<Self>>) {
        self.query = query.to_lowercase();
        self.filter();
        cx.notify();
        let picker = cx.entity();
        // `Picker::matches_updated` borrows the picker, which is being
        // updated now.
        cx.defer(move |cx| picker.update(cx, |picker, cx| picker.matches_updated(true, cx)));
    }

    fn match_count(&self) -> usize {
        self.matches.len()
    }

    fn match_id(&self, ix: usize) -> SharedString {
        self.row(ix).dir.to_string_lossy().into_owned().into()
    }

    fn render_match(&self, ix: usize, _: bool, cx: &App) -> AnyElement {
        let row = self.row(ix);
        // Two lines in the picker's row height.
        div()
            .flex()
            .flex_col()
            .overflow_hidden()
            .child(div().text_size(px(11.)).line_height(px(12.)).child(format!(
                "{}  {} {}  {}",
                row.name,
                row.id,
                row.version,
                row.state.label()
            )))
            .child(
                div()
                    .text_size(px(10.))
                    .line_height(px(11.))
                    .text_color(fe_ui::ui_theme(cx).muted_text)
                    .child(row.detail()),
            )
            .into_any_element()
    }

    /// On a row waiting or declined, asks about each of its servers that
    /// waits now, those undecided first, through the consent prompt; if
    /// one's being asked about already, or none waits now, nothing. On any
    /// other row, shows all it has to say. Focus goes back to where it was
    /// before the list opened first, so the prompt takes it from there and
    /// gives it back there.
    fn confirm(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Picker<Self>>) -> bool {
        let row = self.row(ix);
        let waiting = matches!(
            row.state,
            ExtensionState::NeedsConsent { .. } | ExtensionState::Declined { .. }
        );
        if waiting {
            let mut ask = Ask::These(row.to_ask(cx));
            let Some(pending) = next_to_ask(window, cx, &mut ask) else {
                return false;
            };
            self.closed(window, cx);
            ask_about(window, cx, pending, ask);
        } else {
            let question = format!("{} {}", row.name, row.version);
            let detail = row.full_detail();
            self.closed(window, cx);
            // Answered by closing it: nothing follows.
            drop(window.prompt(PromptLevel::Info, &question, Some(&detail), &["OK"], cx));
        }
        true
    }

    fn dismissed(&mut self, window: &mut Window, cx: &mut Context<Picker<Self>>) {
        self.closed(window, cx);
    }
}

#[cfg(test)]
#[path = "extensions_picker_tests.rs"]
mod extensions_picker_tests;

#[cfg(test)]
#[path = "extensions_picker_workspace_tests.rs"]
mod extensions_picker_workspace_tests;

#[cfg(test)]
#[path = "extensions_picker_disable_tests.rs"]
mod extensions_picker_disable_tests;
