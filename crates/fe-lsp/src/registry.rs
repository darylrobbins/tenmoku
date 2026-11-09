//! Which server a document gets (docs/decisions/lsp-client.md §3.2,
//! §3.3): the built-in specs, the specs extensions contribute, the user's
//! `[lsp]` settings over both, and specs the host registers.
//!
//! The settings reaching here are the effective ones, whose project layer
//! fe-config has already narrowed to turning servers off.

use std::path::{Component, Path, PathBuf};

use fe_config::settings::{LspReadiness, LspServerSettings, LspSettings, TypescriptServer};
use serde_json::Value;

use crate::launcher::{PullDiagnostics, ReadinessMode, ServerSpec, UnversionedDiagnostics};

/// The built-in servers, in the order a language's server is chosen.
pub(crate) fn builtin_specs() -> Vec<ServerSpec> {
    let ts_markers = ["tsconfig.json", "jsconfig.json", "package.json"];
    let typescript = ["javascript", "typescript", "tsx"];
    let markers = |markers: &[&str]| markers.iter().map(|&m| m.into()).collect::<Vec<String>>();
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
            root_markers: markers(&["Cargo.lock", "Cargo.toml"]),
            ..spec("rust-analyzer", "rust-analyzer", &[], &["rust"])
        },
        ServerSpec {
            root_markers: markers(&[
                "pyproject.toml",
                "setup.py",
                "setup.cfg",
                "pyrightconfig.json",
                "requirements.txt",
            ]),
            ..spec("pyright", "pyright-langserver", &["--stdio"], &["python"])
        },
        // Its pushes carry no version, so they're dropped (§4.1); its
        // diagnostics are pulled through tsserver instead.
        ServerSpec {
            unversioned_diagnostics: UnversionedDiagnostics::Drop,
            pull_diagnostics: PullDiagnostics::TsserverRequest,
            root_markers: markers(&ts_markers),
            ..spec(
                "typescript-language-server",
                "typescript-language-server",
                &["--stdio"],
                &typescript,
            )
        },
        ServerSpec {
            root_markers: markers(&ts_markers),
            ..spec("typescript-go", "tsc", &["--lsp", "--stdio"], &typescript)
        },
        // Speaks LSP on stdio by default. A go.work workspace beats a
        // member module's go.mod.
        ServerSpec {
            root_markers: markers(&["go.work", "go.mod"]),
            ..spec("gopls", "gopls", &[], &["go"])
        },
        // "less" has no grammar yet; listing it gives one the server. The
        // server offers formatting only when provideFormatter is true, and
        // asks workspace/configuration per languageId (css, scss, less): the
        // empty settings answer null, so its defaults apply. package.json
        // roots a stylesheet where node_modules `~` imports resolve.
        ServerSpec {
            root_markers: markers(&["package.json"]),
            initialization_options: Some(serde_json::json!({ "provideFormatter": true })),
            ..spec(
                "vscode-css-language-server",
                "vscode-css-language-server",
                &["--stdio"],
                &["css", "scss", "less"],
            )
        },
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

/// The folder a server runs in for `path`: `window_root` when the file is
/// under it (one server per window folder); otherwise the nearest ancestor
/// holding the first of `markers` found anywhere up the tree, so
/// `Cargo.lock`'s workspace beats a member's `Cargo.toml`; with none, the
/// file's own folder. Only checks that entries exist.
pub fn server_root(path: &Path, window_root: Option<&Path>, markers: &[String]) -> PathBuf {
    let path = lexically_normal(path);
    if let Some(root) = window_root
        && root.is_absolute()
        && path.starts_with(root)
    {
        return root.to_path_buf();
    }
    let folder = path.parent().unwrap_or(&path);
    for marker in markers {
        if let Some(found) = folder.ancestors().find(|dir| dir.join(marker).exists()) {
            return found.to_path_buf();
        }
    }
    folder.to_path_buf()
}

/// `path` with its `.` and `..` components resolved by name, so a path
/// through `..` isn't taken for one under the window root.
fn lexically_normal(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            // Only a normal component can be popped; a `..` with nothing
            // to cancel (or after another) stays, and one at the root is
            // dropped.
            Component::ParentDir => match out.components().next_back() {
                Some(Component::Normal(_)) => {
                    out.pop();
                }
                Some(Component::RootDir | Component::Prefix(_)) => {}
                _ => out.push(".."),
            },
            other => out.push(other),
        }
    }
    out
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
    if let Some(markers) = &settings.root_markers {
        spec.root_markers.clone_from(markers);
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
