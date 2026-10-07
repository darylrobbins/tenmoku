//! Which server a document gets (docs/decisions/lsp-client.md §3.2,
//! §3.3): the built-in specs, the user's `[lsp]` settings over them, and
//! specs the host registers.
//!
//! The settings reaching here are the effective ones, whose project layer
//! fe-config has already narrowed to turning servers off.

use fe_config::settings::{LspReadiness, LspServerSettings, LspSettings, TypescriptServer};
use serde_json::Value;

use crate::launcher::{ReadinessMode, ServerSpec};

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
        spec(
            "typescript-language-server",
            "typescript-language-server",
            &["--stdio"],
            &typescript,
        ),
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
    /// Registered by the host, in order, each name once.
    host: Vec<ServerSpec>,
}

impl Registry {
    pub(crate) fn set_settings(&mut self, settings: LspSettings) {
        for (name, server) in &settings.servers {
            let known = builtin_specs().iter().any(|spec| spec.name == *name)
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

    /// Adds `spec`, replacing a host spec of the same name. It wins over
    /// a built-in or settings spec of that name, but settings can still
    /// turn it off.
    pub(crate) fn register(&mut self, spec: ServerSpec) {
        match self.host.iter_mut().find(|host| host.name == spec.name) {
            Some(host) => *host = spec,
            None => self.host.push(spec),
        }
    }

    /// Every server that may run now, in the order a language's server is
    /// chosen: the built-ins, then servers only the settings define (by
    /// name), then servers only the host does (in the order registered).
    /// None while `lsp.enabled` is false; none that its settings turn off,
    /// and only the TypeScript server `lsp.typescript_server` picks.
    pub(crate) fn specs(&self) -> Vec<ServerSpec> {
        if !self.settings.enabled {
            return Vec::new();
        }
        let builtins = builtin_specs();
        let names = builtins
            .iter()
            .map(|spec| spec.name.clone())
            .chain(
                self.settings
                    .servers
                    .keys()
                    .filter(|name| builtins.iter().all(|spec| spec.name != **name))
                    .cloned(),
            )
            .chain(
                self.host
                    .iter()
                    .map(|spec| spec.name.clone())
                    .filter(|name| {
                        builtins.iter().all(|spec| spec.name != *name)
                            && !self.settings.servers.contains_key(name)
                    }),
            );
        let (_, not_chosen) = typescript_server(self.settings.typescript_server);
        names
            .filter(|name| name != not_chosen)
            .filter_map(|name| self.spec(&name, &builtins))
            .collect()
    }

    /// The effective spec named `name`, if it's enabled and has a command.
    fn spec(&self, name: &str, builtins: &[ServerSpec]) -> Option<ServerSpec> {
        let settings = self.settings.servers.get(name);
        if settings.is_some_and(|server| !server.enabled) {
            return None;
        }
        if let Some(host) = self.host.iter().find(|spec| spec.name == name) {
            return Some(host.clone());
        }
        let builtin = builtins.iter().find(|spec| spec.name == name).cloned();
        match (builtin, settings) {
            (Some(builtin), Some(settings)) => Some(apply(builtin, settings)),
            (Some(builtin), None) => Some(builtin),
            (None, Some(settings)) => {
                let command = settings.command.as_deref()?;
                let base = ServerSpec {
                    settings: Value::Object(serde_json::Map::new()),
                    ..ServerSpec::new(name, command)
                };
                Some(apply(base, settings))
            }
            (None, None) => None,
        }
    }
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
