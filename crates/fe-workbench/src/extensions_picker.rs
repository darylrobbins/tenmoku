//! Extensions: Show Installed (`workbench.show_extensions`): a [`Picker`]
//! over the extensions kiln has loaded ([`kiln::extensions::loaded`]), each
//! with its kinds, licence, source and state, and its language servers'
//! effective invocations (extensions record §1, §6). Enter on one waiting
//! on the user's consent, or that they declined, shows its consent prompt.
//! An extension that failed to load isn't listed: its diagnostics are in
//! the config problems list.

use gpui::{
    AnyElement, App, Context, FocusHandle, IntoElement as _, ParentElement as _, PromptLevel,
    SharedString, Styled as _, Window, div, px,
};
use kiln::core::{LicenseClass, Manifest};

use crate::{
    PendingConsent, decline_extension_server, extension_servers, grant_extension_server,
    lsp_consent_prompt::consent_prompt_text,
    pending_extension_consents,
    picker::{Picker, PickerDelegate},
};

/// Where an extension is installed from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(dead_code, reason = "workspace extensions come from their own card")]
pub(crate) enum ExtensionSource {
    User,
    Workspace,
}

/// What an extension is doing.
#[derive(Clone, Debug, PartialEq, Eq)]
#[allow(
    dead_code,
    reason = "disabling and untrusted workspaces come from their own cards"
)]
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
    Disabled,
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
    pub(crate) version: String,
    /// "1 theme, 2 lsp servers": what it contributes.
    pub(crate) kinds: String,
    pub(crate) license: String,
    pub(crate) class: LicenseClass,
    pub(crate) source: ExtensionSource,
    pub(crate) state: ExtensionState,
    /// Each language server, its effective invocation and its status.
    pub(crate) servers: Vec<String>,
    /// The servers waiting or declined, for the prompt.
    pending: Vec<PendingConsent>,
}

impl Row {
    fn of(manifest: &Manifest, pending: &[PendingConsent], granted: &[fe_lsp::ServerSpec]) -> Self {
        let (major, minor, patch) = manifest.version;
        let own: Vec<PendingConsent> = (pending.iter())
            .filter(|pending| pending.extension_id == manifest.id)
            .cloned()
            .collect();
        let names = |declined: bool| -> Vec<String> {
            (own.iter())
                .filter(|pending| pending.declined == declined)
                .map(|pending| pending.server.clone())
                .collect()
        };
        let state = if !names(false).is_empty() {
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
                    invocation_line(pending)
                )
            })
            .collect();
        let listed = manifest.app.get("lsp").and_then(toml::Value::as_table);
        for name in listed.into_iter().flat_map(|table| table.keys()) {
            if own.iter().any(|pending| pending.server == *name) {
                continue;
            }
            if let Some(spec) = granted.iter().find(|spec| spec.name == *name) {
                let args = spec.args.join(" ");
                servers.push(format!(
                    "{name}: {} {args} (granted)",
                    spec.command.display()
                ));
            }
        }
        Self {
            name: manifest.name.clone(),
            id: manifest.id.clone(),
            version: format!("{major}.{minor}.{patch}"),
            kinds: kinds(manifest),
            license: manifest.license.expr.clone(),
            class: manifest.license.class,
            source: ExtensionSource::User,
            state,
            servers,
            pending: own,
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

    /// The secondary line: kinds, licence, source and servers.
    pub(crate) fn detail(&self) -> String {
        let source = match self.source {
            ExtensionSource::User => "User",
            ExtensionSource::Workspace => "Workspace",
        };
        let mut parts = vec![self.kinds.clone(), self.license_text(), source.to_owned()];
        parts.extend(self.servers.iter().cloned());
        parts.join(" · ")
    }
}

/// `pending`'s server's executable and arguments.
fn invocation_line(pending: &PendingConsent) -> String {
    let mut line = pending.invocation.command.display().to_string();
    for arg in &pending.invocation.args {
        line.push(' ');
        line.push_str(arg);
    }
    line
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
    /// What was focused before it opened, focused again once it closes.
    previous_focus: Option<FocusHandle>,
}

impl ExtensionsPicker {
    pub(crate) fn new() -> Self {
        Self {
            rows: Vec::new(),
            matches: Vec::new(),
            previous_focus: None,
        }
    }

    /// The picker is opening, with `previous_focus` focused until now, or
    /// opening afresh, keeping what was focused before, if that's `None`.
    pub(crate) fn opened(&mut self, previous_focus: Option<FocusHandle>, cx: &App) {
        self.previous_focus = previous_focus.or(self.previous_focus.take());
        let pending = pending_extension_consents(cx);
        let granted = extension_servers(cx);
        self.rows = (kiln::extensions::loaded(cx).iter())
            .map(|manifest| Row::of(manifest, &pending, &granted))
            .collect();
        self.rows.sort_by_key(|row| row.name.to_lowercase());
        self.matches = (0..self.rows.len()).collect();
    }

    fn row(&self, ix: usize) -> &Row {
        &self.rows[self.matches[ix]]
    }

    fn restore_focus(&mut self, window: &mut Window, cx: &mut App) {
        if let Some(focus) = self.previous_focus.take() {
            window.focus(&focus, cx);
        }
    }
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

/// Asks about `pending`'s server as the consent prompt does, and records
/// the answer.
fn ask(pending: PendingConsent, window: &mut Window, cx: &mut Context<Picker<ExtensionsPicker>>) {
    let (question, detail) = consent_prompt_text(&pending);
    let answer = window.prompt(
        PromptLevel::Warning,
        &question,
        Some(&detail),
        &["Allow", "Don't Allow"],
        cx,
    );
    cx.spawn_in(window, async move |_, cx| {
        let answer = answer.await.ok();
        // Outside any update: a decision hands the servers to every
        // window's managers.
        cx.update(|_, cx| match answer {
            Some(0) => grant_extension_server(cx, &pending).ok(),
            Some(1) => decline_extension_server(cx, &pending).ok(),
            _ => None,
        })
        .ok();
    })
    .detach();
}

impl PickerDelegate for ExtensionsPicker {
    /// Rows whose name or ID holds `query`, ignoring case.
    fn update_matches(&mut self, query: String, cx: &mut Context<Picker<Self>>) {
        let query = query.to_lowercase();
        self.matches = (self.rows.iter().enumerate())
            .filter(|(_, row)| {
                row.name.to_lowercase().contains(&query) || row.id.to_lowercase().contains(&query)
            })
            .map(|(ix, _)| ix)
            .collect();
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
        self.row(ix).id.clone().into()
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

    /// Shows the consent prompt of a row waiting or declined; any other
    /// row is left alone.
    fn confirm(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Picker<Self>>) -> bool {
        let row = self.row(ix);
        let waiting = matches!(
            row.state,
            ExtensionState::NeedsConsent { .. } | ExtensionState::Declined { .. }
        );
        let Some(pending) = row.pending.first().cloned().filter(|_| waiting) else {
            return false;
        };
        self.restore_focus(window, cx);
        ask(pending, window, cx);
        true
    }

    fn dismissed(&mut self, window: &mut Window, cx: &mut Context<Picker<Self>>) {
        self.restore_focus(window, cx);
    }
}

#[cfg(test)]
#[path = "extensions_picker_tests.rs"]
mod extensions_picker_tests;
