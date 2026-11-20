//! Language servers' custom methods, which plugins send through the host's sessions.
//!
//! A plugin with the `lsp:<server>` capability sends requests and notifications to the session
//! of `server` that a buffer or a root names. The session is chosen by the server and the root,
//! never by name alone; the host's [`LspSessions`] finds it, and starts nothing. The slice checks
//! what it can alone: the grant, that the server is the plugin's own extension's or a built-in
//! one, that the method is not one the editor routes itself, the size of params and result, and
//! how many requests wait. [`LspRequests`] keeps the waiting ones, and unloading a plugin
//! cancels them.

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use kiln_command::capability::{Capability, FsScope, Grants};
use kiln_plugin::bindings::{Error, unimplemented};
use kiln_plugin::paths::{self, Rules};

use crate::HostState;
use crate::fastedit::editor::lsp::{Host, Target};

/// How many requests one plugin may have waiting at once.
pub const MAX_PENDING_PER_PLUGIN: usize = 4;

/// The most bytes of params, and of a result, in one call.
pub const MAX_BYTES: usize = 16 * 1024 * 1024;

/// Method prefixes the editor routes itself, which a plugin may not send.
const ROUTED_PREFIXES: [&str; 16] = [
    "textDocument/",
    "workspace/",
    "completionItem/",
    "codeAction/",
    "codeLens/",
    "inlayHint/",
    "documentLink/",
    "callHierarchy/",
    "typeHierarchy/",
    "notebookDocument/",
    "workspaceSymbol/",
    "window/",
    "client/",
    "$/",
    "capability/",
    "telemetry/",
];

/// Methods that are the editor's alone to send.
const ROUTED_METHODS: [&str; 4] = ["initialize", "initialized", "shutdown", "exit"];

/// Whether the editor routes `method` itself.
pub fn is_routed(method: &str) -> bool {
    ROUTED_METHODS.contains(&method)
        || ROUTED_PREFIXES
            .iter()
            .any(|prefix| method.starts_with(prefix))
}

/// The session a call names, as the host resolves it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SessionTarget {
    /// The session the open buffer with this id is placed on.
    Buffer(u64),
    /// The session started at this root, a host path.
    Root(String),
}

/// One call to a session.
pub struct SessionCall<'a> {
    pub server: &'a str,
    pub target: &'a SessionTarget,
    pub method: &'a str,
    /// JSON.
    pub params: &'a str,
    /// Set when the plugin unloads: the host drops the request.
    pub cancelled: &'a Arc<AtomicBool>,
}

/// The app's language-server sessions, as one plugin reaches them.
pub trait LspSessions: Send {
    /// Whether `server` is one the editor runs without any extension.
    fn is_builtin(&self, server: &str) -> bool;

    /// Sends the request to the session `call` names and returns its JSON result. No session
    /// there, or no open buffer or trusted root as named, is [`Error::NotFound`]; nothing is
    /// started.
    fn request(&mut self, call: SessionCall<'_>) -> Result<String, Error>;

    /// Sends the notification, as for [`LspSessions::request`].
    fn notify(&mut self, call: SessionCall<'_>) -> Result<(), Error>;
}

/// Every plugin's waiting requests, which the app shares with every plugin's slice. Clones
/// share it.
#[derive(Clone, Default)]
pub struct LspRequests {
    shared: Arc<Mutex<Shared>>,
}

#[derive(Default)]
struct Shared {
    /// Each loaded plugin's attachment: the number of its load.
    attached: BTreeMap<String, u64>,
    loads: u64,
    /// Each plugin's waiting requests' cancel flags.
    pending: BTreeMap<String, Vec<Arc<AtomicBool>>>,
}

impl LspRequests {
    pub fn new() -> Self {
        Self::default()
    }

    /// How many requests plugin `plugin` has waiting.
    pub fn pending(&self, plugin: &str) -> usize {
        self.lock().pending.get(plugin).map_or(0, Vec::len)
    }

    /// Holds one of plugin `plugin`'s places for waiting requests until the result is dropped:
    /// what another thread's request would, for tests of the cap.
    pub fn hold(&self, plugin: &str) -> Result<impl Drop + use<>, Error> {
        let load = self.lock().attached.get(plugin).copied().unwrap_or(0);
        self.begin(plugin, load)
    }

    /// Forgets plugin `plugin`: its waiting requests are cancelled, and it sends no more.
    pub fn unload(&self, plugin: &str) {
        let mut shared = self.lock();
        shared.attached.remove(plugin);
        for flag in shared.pending.remove(plugin).into_iter().flatten() {
            flag.store(true, Ordering::Release);
        }
    }

    /// Cancels plugin `plugin`'s waiting requests and its sends, if `load` is still its load: the
    /// comparison and the unload are one step, so a newer load is never touched.
    fn unload_load(&self, plugin: &str, load: u64) {
        let mut shared = self.lock();
        if shared.attached.get(plugin) == Some(&load) {
            shared.attached.remove(plugin);
            for flag in shared.pending.remove(plugin).into_iter().flatten() {
                flag.store(true, Ordering::Release);
            }
        }
    }

    /// Attaches a new load of `plugin`; an earlier load's waiting requests are cancelled.
    fn attach(&self, plugin: &str) -> u64 {
        let mut shared = self.lock();
        shared.loads += 1;
        let load = shared.loads;
        shared.attached.insert(plugin.to_owned(), load);
        for flag in shared.pending.remove(plugin).into_iter().flatten() {
            flag.store(true, Ordering::Release);
        }
        load
    }

    /// Starts a request of `plugin`'s, if `load` is still its and it has room.
    fn begin(&self, plugin: &str, load: u64) -> Result<Waiting, Error> {
        let mut shared = self.lock();
        if shared.attached.get(plugin) != Some(&load) {
            return Err(Error::Denied(format!("plugin {plugin} was unloaded")));
        }
        let waiting = shared.pending.entry(plugin.to_owned()).or_default();
        if waiting.len() >= MAX_PENDING_PER_PLUGIN {
            return Err(Error::Failed(format!(
                "plugin {plugin} already has {MAX_PENDING_PER_PLUGIN} language-server requests \
                 waiting, the most it may"
            )));
        }
        let cancelled = Arc::new(AtomicBool::new(false));
        waiting.push(cancelled.clone());
        Ok(Waiting {
            requests: self.clone(),
            plugin: plugin.to_owned(),
            cancelled,
        })
    }

    fn lock(&self) -> MutexGuard<'_, Shared> {
        self.shared.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// A request that is waiting; ending it frees its place.
struct Waiting {
    requests: LspRequests,
    plugin: String,
    cancelled: Arc<AtomicBool>,
}

impl Drop for Waiting {
    fn drop(&mut self) {
        let mut shared = self.requests.lock();
        if let Some(waiting) = shared.pending.get_mut(&self.plugin) {
            waiting.retain(|flag| !Arc::ptr_eq(flag, &self.cancelled));
            if waiting.is_empty() {
                shared.pending.remove(&self.plugin);
            }
        }
    }
}

/// Cancels one load's waiting requests: [`State::unloader`].
#[derive(Clone)]
pub struct Unloader {
    requests: LspRequests,
    plugin: String,
    load: u64,
}

impl Unloader {
    /// Drops the load's waiting requests, and it sends no more.
    pub fn unload(&self) {
        self.requests.unload_load(&self.plugin, self.load);
    }
}

/// The lsp slice of [`HostState`]. The default has no sessions and answers every import with
/// [`unimplemented`].
#[derive(Default)]
pub struct State {
    attached: Option<Attached>,
}

struct Attached {
    plugin: String,
    grants: Grants,
    /// The servers the plugin's own extension contributes.
    own: Vec<String>,
    requests: LspRequests,
    load: u64,
    rules: Rules,
    sessions: Box<dyn LspSessions>,
}

impl State {
    /// Plugin `plugin`'s slice over `sessions`, as far as `grants` allow, for `own`, the servers
    /// its extension contributes. It is a new load: requests an earlier load left waiting are
    /// cancelled.
    pub fn new(
        plugin: &str,
        grants: &Grants,
        own: Vec<String>,
        requests: &LspRequests,
        sessions: Box<dyn LspSessions>,
    ) -> Self {
        Self {
            attached: Some(Attached {
                plugin: plugin.to_owned(),
                grants: grants.clone(),
                own,
                load: requests.attach(plugin),
                requests: requests.clone(),
                rules: Rules::HOST,
                sessions,
            }),
        }
    }

    /// What cancels this load's waiting requests when the plugin unloads, without touching a
    /// later load's. None for a slice with no sessions.
    pub fn unloader(&self) -> Option<Unloader> {
        let attached = self.attached.as_ref()?;
        Some(Unloader {
            requests: attached.requests.clone(),
            plugin: attached.plugin.clone(),
            load: attached.load,
        })
    }

    /// The slice, reading root paths as `rules` spell them: for tests of another OS's paths.
    pub fn with_path_rules(mut self, rules: Rules) -> Self {
        if let Some(attached) = &mut self.attached {
            attached.rules = rules;
        }
        self
    }
}

impl Attached {
    /// Checks a call to `import`, then runs `send` on the session it names, as one waiting
    /// request.
    fn call<T>(
        &mut self,
        import: &str,
        server: &str,
        target: Target,
        method: &str,
        params: &str,
        send: impl FnOnce(&mut dyn LspSessions, SessionCall<'_>) -> Result<T, Error>,
    ) -> Result<T, Error> {
        let capability = Capability::Lsp(server.to_owned());
        if !self.grants.allows(&capability) {
            return Err(Error::Denied(format!(
                "{import} needs the {capability} capability"
            )));
        }
        if !self.own.iter().any(|own| own == server) && !self.sessions.is_builtin(server) {
            return Err(Error::Denied(format!(
                "{server} is neither a server of plugin {}'s extension nor a built-in one",
                self.plugin
            )));
        }
        if is_routed(method) {
            return Err(Error::Denied(format!(
                "{method} is a method the editor routes itself"
            )));
        }
        if params.len() > MAX_BYTES {
            return Err(Error::InvalidArgument(format!(
                "{} bytes of params are over the {MAX_BYTES}-byte limit",
                params.len()
            )));
        }
        let target = match target {
            Target::Buffer(id) => {
                if !self.grants.allows(&Capability::BufferRead) {
                    return Err(Error::Denied(format!(
                        "{import} on a buffer needs the {} capability",
                        Capability::BufferRead
                    )));
                }
                SessionTarget::Buffer(id)
            }
            Target::Root(root) => {
                let reaches = [FsScope::Workspace].into_iter().any(|scope| {
                    self.grants.allows(&Capability::FsRead(scope))
                        || self.grants.allows(&Capability::FsWrite(scope))
                });
                if !reaches {
                    return Err(Error::Denied(format!(
                        "{import} on a root needs an fs capability that reaches it"
                    )));
                }
                SessionTarget::Root(paths::to_host(&root, self.rules)?)
            }
        };
        let waiting = self.requests.begin(&self.plugin, self.load)?;
        let call = SessionCall {
            server,
            target: &target,
            method,
            params,
            cancelled: &waiting.cancelled,
        };
        let result = send(self.sessions.as_mut(), call);
        if waiting.cancelled.load(Ordering::Acquire) {
            return Err(Error::Denied(format!(
                "plugin {} was unloaded",
                self.plugin
            )));
        }
        result
    }
}

impl Host for HostState {
    fn request(
        &mut self,
        server: String,
        target: Target,
        method: String,
        params: String,
    ) -> Result<String, Error> {
        let import = "fastedit:editor/lsp.request";
        let Some(attached) = &mut self.lsp.attached else {
            return unimplemented(import);
        };
        let result = attached.call(import, &server, target, &method, &params, |s, call| {
            s.request(call)
        })?;
        if result.len() > MAX_BYTES {
            return Err(Error::Failed(format!(
                "{server}'s answer to {method} is {} bytes, over the {MAX_BYTES}-byte limit",
                result.len()
            )));
        }
        Ok(result)
    }

    fn notify(
        &mut self,
        server: String,
        target: Target,
        method: String,
        params: String,
    ) -> Result<(), Error> {
        let import = "fastedit:editor/lsp.notify";
        let Some(attached) = &mut self.lsp.attached else {
            return unimplemented(import);
        };
        attached.call(import, &server, target, &method, &params, |s, call| {
            s.notify(call)
        })
    }
}
