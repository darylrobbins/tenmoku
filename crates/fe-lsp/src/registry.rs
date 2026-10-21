//! Which server a document gets (docs/decisions/lsp-client.md §3.2,
//! §3.3): the built-in specs, the specs extensions contribute, the user's
//! `[lsp]` settings over both, and specs the host registers.
//!
//! The settings reaching here are the effective ones, whose project layer
//! fe-config has already narrowed to turning servers off.

use fe_config::settings::{LspReadiness, LspServerSettings, LspSettings, TypescriptServer};
use serde_json::Value;

use crate::launcher::{PullDiagnostics, ReadinessMode, ServerSpec, UnversionedDiagnostics};

/// The built-in servers, in the order a language's server is chosen.
pub(crate) fn builtin_specs() -> Vec<ServerSpec> {
    let typescript = ["javascript", "typescript", "tsx"];
    let spec = |name: &str, command: &str, args: &[&str], languages: &[&str]| ServerSpec {
        args: args.iter().map(|&arg| arg.into()).collect(),
        languages: languages.iter().map(|&language| language.into()).collect(),
        settings: Value::Object(serde_json::Map::new()),
        ..ServerSpec::new(name, command)
    };
    vec![
        ServerSpec {
            readiness: ReadinessMode::ServerStatus,
            ready_ignores_progress: vec!["rust-analyzer/flycheck/".into()],
            ..spec("rust-analyzer", "rust-analyzer", &[], &["rust"])
        },
        spec("pyright", "pyright-langserver", &["--stdio"], &["python"]),
        // Its pushes carry no version, so they're dropped (§4.1); its
        // diagnostics are pulled through tsserver instead.
        ServerSpec {
            unversioned_diagnostics: UnversionedDiagnostics::Drop,
            pull_diagnostics: PullDiagnostics::TsserverRequest,
            ..spec(
                "typescript-language-server",
                "typescript-language-server",
                &["--stdio"],
                &typescript,
            )
        },
        spec("typescript-go", "tsc", &["--lsp", "--stdio"], &typescript),
    ]
}

/// The built-in TypeScript server `lsp.typescript_server` picks; the other
/// never runs.
fn typescript_server(choice: TypescriptServer) -> (&'static str, &'static str) {
    const TSLS: &str = "typescript-language-server";
    const TSGO: &str = "typescript-go";
    match choice {
        TypescriptServer::TypescriptLanguageServer => (TSLS, TSGO),
        TypescriptServer::TypescriptGo => (TSGO, TSLS),
    }
}

/// The specs servers start from.
#[derive(Default)]
pub(crate) struct Registry {
    settings: LspSettings,
    /// Contributed by extensions, in order, each name once; one named like
    /// a built-in or a host spec is ignored.
    extension: Vec<ServerSpec>,
    /// Registered by the host, in order, each name once.
    host: Vec<ServerSpec>,
}

impl Registry {
    pub(crate) fn set_settings(&mut self, settings: LspSettings) {
        for (name, server) in &settings.servers {
            let known = builtin_specs().iter().any(|spec| spec.name == *name)
                || self.extension.iter().any(|spec| spec.name == *name)
                || self.host.iter().any(|spec| spec.name == *name);
            if server.enabled && server.command.is_none() && !known {
                log::warn!("language server {name} has no command, so it never starts");
            }
        }
        self.settings = settings;
    }

    pub(crate) fn settings(&self) -> &LspSettings {
        &self.settings
    }

    /// Replaces the extensions' specs with `specs`, a tier between the
    /// built-ins and the settings: the settings apply over each as over a
    /// built-in. One named like a built-in, a host spec or an earlier one
    /// is ignored, with a warning. Whether they changed.
    pub(crate) fn set_extension_specs(&mut self, specs: Vec<ServerSpec>) -> bool {
        if self.extension == specs {
            return false;
        }
        let builtins = builtin_specs();
        for (i, spec) in specs.iter().enumerate() {
            let name = &spec.name;
            if builtins.iter().any(|builtin| builtin.name == *name) {
                log::warn!("an extension's language server {name} is ignored: it's built in");
            } else if self.host.iter().any(|host| host.name == *name) {
                log::warn!("an extension's language server {name} is ignored: the host has one");
            } else if specs[..i].iter().any(|earlier| earlier.name == *name) {
                log::warn!("an extension's language server {name} is ignored: another has it");
            }
        }
        self.extension = specs;
        true
    }

    /// Adds `spec`, replacing a host spec of the same name. It wins over
    /// a built-in, extension or settings spec of that name, but settings
    /// can still turn it off.
    pub(crate) fn register(&mut self, spec: ServerSpec) {
        if self.extension.iter().any(|ext| ext.name == spec.name) {
            let name = &spec.name;
            log::warn!("an extension's language server {name} is ignored: the host has one");
        }
        match self.host.iter_mut().find(|host| host.name == spec.name) {
            Some(host) => *host = spec,
            None => self.host.push(spec),
        }
    }

    /// Every server that may run now, in the order a language's server is
    /// chosen: the built-ins, then servers only the settings define (by
    /// name), then those extensions contribute (in the order given), then
    /// servers only the host does (in the order registered).
    /// None while `lsp.enabled` is false; none that its settings turn off,
    /// and only the TypeScript server `lsp.typescript_server` picks.
    pub(crate) fn specs(&self) -> Vec<ServerSpec> {
        if !self.settings.enabled {
            return Vec::new();
        }
        let builtins = builtin_specs();
        let builtin = |name: &str| builtins.iter().any(|spec| spec.name == name);
        let extension = self.extensions(&builtins);
        let contributed = |name: &str| extension.iter().any(|spec| spec.name == name);
        let names = builtins
            .iter()
            .map(|spec| spec.name.clone())
            .chain(
                self.settings
                    .servers
                    .keys()
                    .filter(|name| !builtin(name) && !contributed(name))
                    .cloned(),
            )
            .chain(extension.iter().map(|spec| spec.name.clone()))
            .chain(
                self.host
                    .iter()
                    .map(|spec| spec.name.clone())
                    .filter(|name| !builtin(name) && !self.settings.servers.contains_key(name)),
            );
        let (_, not_chosen) = typescript_server(self.settings.typescript_server);
        names
            .filter(|name| name != not_chosen)
            .filter_map(|name| self.spec(&name, &builtins, &extension))
            .collect()
    }

    /// The extensions' specs that aren't ignored: none named like a
    /// built-in, a host spec or an earlier one.
    fn extensions(&self, builtins: &[ServerSpec]) -> Vec<&ServerSpec> {
        let mut kept: Vec<&ServerSpec> = Vec::new();
        for spec in &self.extension {
            let taken = builtins.iter().any(|builtin| builtin.name == spec.name)
                || self.host.iter().any(|host| host.name == spec.name)
                || kept.iter().any(|earlier| earlier.name == spec.name);
            if !taken {
                kept.push(spec);
            }
        }
        kept
    }

    /// The effective spec named `name`, if it's enabled and has a command.
    fn spec(
        &self,
        name: &str,
        builtins: &[ServerSpec],
        extension: &[&ServerSpec],
    ) -> Option<ServerSpec> {
        let settings = self.settings.servers.get(name);
        if settings.is_some_and(|server| !server.enabled) {
            return None;
        }
        if let Some(host) = self.host.iter().find(|spec| spec.name == name) {
            return Some(host.clone());
        }
        let base = builtins
            .iter()
            .chain(extension.iter().copied())
            .find(|spec| spec.name == name)
            .cloned();
        match (base, settings) {
            (Some(base), Some(settings)) => Some(apply(base, settings)),
            (Some(base), None) => Some(base),
            (None, Some(settings)) => extension_spec(name, settings),
            (None, None) => None,
        }
    }
}

/// The spec of a server `settings` define on their own, as an extension's
/// `[contributes.lsp.<name>]` does: `None` without a `command`, or turned
/// off.
pub fn extension_spec(name: &str, settings: &LspServerSettings) -> Option<ServerSpec> {
    if !settings.enabled {
        return None;
    }
    let command = settings.command.as_deref()?;
    let base = ServerSpec {
        settings: Value::Object(serde_json::Map::new()),
        ..ServerSpec::new(name, command)
    };
    Some(apply(base, settings))
}

/// The spec of the server a document in fe-syntax's `language` gets: the
/// first of `specs` ([`Registry::specs`]) listing it.
pub(crate) fn server_for<'a>(specs: &'a [ServerSpec], language: &str) -> Option<&'a ServerSpec> {
    specs
        .iter()
        .find(|spec| spec.languages.iter().any(|l| l == language))
}

/// `spec` with what `settings` sets.
fn apply(mut spec: ServerSpec, settings: &LspServerSettings) -> ServerSpec {
    if let Some(command) = &settings.command {
        spec.command = command.into();
    }
    if let Some(args) = &settings.args {
        spec.args.clone_from(args);
    }
    if let Some(env) = &settings.env {
        spec.env = env.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
    }
    if let Some(languages) = &settings.languages {
        spec.languages.clone_from(languages);
    }
    if let Some(readiness) = settings.readiness {
        spec.readiness = match readiness {
            LspReadiness::ServerStatus => ReadinessMode::ServerStatus,
            LspReadiness::Progress => ReadinessMode::Progress,
        };
    }
    if let Some(prefixes) = &settings.ready_ignores_progress {
        spec.ready_ignores_progress.clone_from(prefixes);
    }
    if let Some(options) = &settings.initialization_options {
        spec.initialization_options = Some(options.clone());
    }
    if let Some(settings) = &settings.settings {
        spec.settings = Value::Object(settings.clone());
    }
    spec
}

/// Whether a running server's spec changing from `old` to `new` takes a
/// restart: anything but its `settings`, which `didChangeConfiguration`
/// carries.
pub(crate) fn needs_restart(old: &ServerSpec, new: &ServerSpec) -> bool {
    ServerSpec {
        settings: Value::Null,
        ..old.clone()
    } != ServerSpec {
        settings: Value::Null,
        ..new.clone()
    }
}
