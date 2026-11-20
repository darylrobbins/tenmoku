//! The `fastedit:editor/lsp` imports, called as a plugin's store calls them: which sessions a
//! plugin's custom requests reach, and what is refused before one is sent.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use fe_plugin::HostState;
use fe_plugin::fastedit::editor::lsp::{Host as _, Target};
use fe_plugin::lsp::{
    LspRequests, LspSessions, MAX_PENDING_PER_PLUGIN, SessionCall, SessionTarget, State,
};
use kiln_command::capability::{Capability, Grants};
use kiln_plugin::bindings::Error;

const PLUGIN: &str = "acme";

/// What a fake server saw: (server, root, method, params).
type Seen = Arc<Mutex<Vec<(String, String, String, String)>>>;

/// Fake sessions: one per (server, root). Buffer 1 is at root `/a`, buffer 2 at `/b`.
struct Sessions {
    seen: Seen,
    /// Roots with a session of each server.
    started: BTreeMap<String, Vec<String>>,
    /// Calls to run while a request is "waiting".
    during: Option<Box<dyn FnMut() + Send>>,
}

impl Sessions {
    fn new(seen: &Seen) -> Self {
        let both = vec!["/a".to_owned(), "/b".to_owned()];
        Self {
            seen: seen.clone(),
            started: [
                ("tls".to_owned(), both.clone()),
                ("rust-analyzer".to_owned(), both),
            ]
            .into(),
            during: None,
        }
    }

    fn root(&self, server: &str, call: &SessionCall<'_>) -> Result<String, Error> {
        let root = match call.target {
            SessionTarget::Buffer(1) => "/a",
            SessionTarget::Buffer(2) => "/b",
            SessionTarget::Buffer(id) => {
                return Err(Error::NotFound(format!("no open buffer has id {id}")));
            }
            SessionTarget::Root(root) => root.as_str(),
        };
        let started = self
            .started
            .get(server)
            .is_some_and(|roots| roots.iter().any(|r| r == root));
        if started {
            Ok(root.to_owned())
        } else {
            Err(Error::NotFound(format!("no {server} session at {root}")))
        }
    }

    fn record(&self, call: &SessionCall<'_>, root: String) {
        self.seen.lock().unwrap().push((
            call.server.into(),
            root,
            call.method.into(),
            call.params.into(),
        ));
    }
}

impl LspSessions for Sessions {
    fn is_builtin(&self, server: &str) -> bool {
        server == "rust-analyzer"
    }

    fn request(&mut self, call: SessionCall<'_>) -> Result<String, Error> {
        if let Some(during) = &mut self.during {
            during();
        }
        let root = self.root(call.server, &call)?;
        self.record(&call, root.clone());
        Ok(format!("{{\"root\":\"{root}\"}}"))
    }

    fn notify(&mut self, call: SessionCall<'_>) -> Result<(), Error> {
        let root = self.root(call.server, &call)?;
        self.record(&call, root);
        Ok(())
    }
}

fn grants(granted: &[&str]) -> Grants {
    granted
        .iter()
        .map(|capability| Capability::parse(capability).unwrap())
        .collect()
}

fn plugin(granted: &[&str], own: &[&str], requests: &LspRequests, sessions: Sessions) -> HostState {
    let own = own.iter().map(|server| (*server).to_owned()).collect();
    HostState {
        lsp: State::new(PLUGIN, &grants(granted), own, requests, Box::new(sessions)),
        ..HostState::default()
    }
}

fn denied<T: std::fmt::Debug>(result: Result<T, Error>) {
    assert!(matches!(result, Err(Error::Denied(_))), "{result:?}");
}

#[test]
fn lsp_custom_method_reaches_fake_server() {
    let seen = Seen::default();
    let requests = LspRequests::new();
    let mut state = plugin(
        &["lsp:tls", "buffer.read"],
        &["tls"],
        &requests,
        Sessions::new(&seen),
    );
    let answer = state
        .request(
            "tls".into(),
            Target::Buffer(1),
            "tls/custom".into(),
            "{\"x\":1}".into(),
        )
        .unwrap();
    assert_eq!(answer, "{\"root\":\"/a\"}");
    state
        .notify(
            "tls".into(),
            Target::Buffer(1),
            "tls/ping".into(),
            "null".into(),
        )
        .unwrap();
    assert_eq!(
        *seen.lock().unwrap(),
        [
            (
                "tls".into(),
                "/a".into(),
                "tls/custom".into(),
                "{\"x\":1}".into()
            ),
            ("tls".into(), "/a".into(), "tls/ping".into(), "null".into()),
        ]
    );
    assert_eq!(requests.pending(PLUGIN), 0);

    // No session there: not found, and nothing started.
    let none = state.request(
        "tls".into(),
        Target::Buffer(9),
        "tls/custom".into(),
        "{}".into(),
    );
    assert!(matches!(none, Err(Error::NotFound(_))), "{none:?}");
    // Without buffer.read a buffer is out of reach.
    let mut blind = plugin(&["lsp:tls"], &["tls"], &requests, Sessions::new(&seen));
    denied(blind.request(
        "tls".into(),
        Target::Buffer(1),
        "tls/custom".into(),
        "{}".into(),
    ));
    // Without lsp:<server>, nothing goes.
    let mut ungranted = plugin(&["buffer.read"], &["tls"], &requests, Sessions::new(&seen));
    denied(ungranted.request(
        "tls".into(),
        Target::Buffer(1),
        "tls/custom".into(),
        "{}".into(),
    ));
}

#[test]
fn lsp_two_roots_same_server_reach_the_right_session() {
    let seen = Seen::default();
    let requests = LspRequests::new();
    let mut state = plugin(
        &["lsp:tls", "buffer.read", "fs.read:workspace"],
        &["tls"],
        &requests,
        Sessions::new(&seen),
    );
    let by_buffer = |state: &mut HostState, id| {
        state.request(
            "tls".into(),
            Target::Buffer(id),
            "tls/where".into(),
            "{}".into(),
        )
    };
    assert_eq!(by_buffer(&mut state, 1).unwrap(), "{\"root\":\"/a\"}");
    assert_eq!(by_buffer(&mut state, 2).unwrap(), "{\"root\":\"/b\"}");
    let by_root = state
        .request(
            "tls".into(),
            Target::Root("/b".into()),
            "tls/where".into(),
            "{}".into(),
        )
        .unwrap();
    assert_eq!(by_root, "{\"root\":\"/b\"}");
    let missing = state.request(
        "tls".into(),
        Target::Root("/c".into()),
        "tls/where".into(),
        "{}".into(),
    );
    assert!(matches!(missing, Err(Error::NotFound(_))), "{missing:?}");
    // A root needs an fs capability.
    let mut no_fs = plugin(&["lsp:tls"], &["tls"], &requests, Sessions::new(&seen));
    denied(no_fs.request(
        "tls".into(),
        Target::Root("/a".into()),
        "tls/where".into(),
        "{}".into(),
    ));
    // And a guest path, which is absolute.
    let relative = state.request(
        "tls".into(),
        Target::Root("a".into()),
        "tls/where".into(),
        "{}".into(),
    );
    assert!(
        matches!(relative, Err(Error::InvalidArgument(_))),
        "{relative:?}"
    );
}

#[test]
fn lsp_routed_text_document_method_is_denied() {
    let seen = Seen::default();
    let requests = LspRequests::new();
    let mut state = plugin(
        &["lsp:tls", "buffer.read"],
        &["tls"],
        &requests,
        Sessions::new(&seen),
    );
    for method in [
        "textDocument/hover",
        "workspace/executeCommand",
        "completionItem/resolve",
        "$/cancelRequest",
        "initialize",
        "shutdown",
    ] {
        denied(state.request("tls".into(), Target::Buffer(1), method.into(), "{}".into()));
        denied(state.notify("tls".into(), Target::Buffer(1), method.into(), "{}".into()));
    }
    assert!(seen.lock().unwrap().is_empty());
}

#[test]
fn lsp_another_extensions_server_is_denied() {
    let seen = Seen::default();
    let requests = LspRequests::new();
    // `tls` is granted, but this plugin's extension doesn't contribute it, and it isn't built in.
    let mut state = plugin(
        &["lsp:tls", "buffer.read"],
        &[],
        &requests,
        Sessions::new(&seen),
    );
    denied(state.request(
        "tls".into(),
        Target::Buffer(1),
        "tls/custom".into(),
        "{}".into(),
    ));
    denied(state.notify(
        "tls".into(),
        Target::Buffer(1),
        "tls/custom".into(),
        "{}".into(),
    ));
    assert!(seen.lock().unwrap().is_empty());
}

#[test]
fn lsp_builtin_server_reachable_with_its_grant() {
    let seen = Seen::default();
    let requests = LspRequests::new();
    let mut state = plugin(
        &["lsp:rust-analyzer", "buffer.read"],
        &[],
        &requests,
        Sessions::new(&seen),
    );
    let answer = state
        .request(
            "rust-analyzer".into(),
            Target::Buffer(2),
            "experimental/runnables".into(),
            "{}".into(),
        )
        .unwrap();
    assert_eq!(answer, "{\"root\":\"/b\"}");
    // Without its grant, no.
    let mut ungranted = plugin(&["buffer.read"], &[], &requests, Sessions::new(&seen));
    denied(ungranted.request(
        "rust-analyzer".into(),
        Target::Buffer(2),
        "experimental/runnables".into(),
        "{}".into(),
    ));
}

#[test]
fn lsp_pending_requests_are_capped_and_dropped_on_unload() {
    let seen = Seen::default();
    let requests = LspRequests::new();
    let mut state = plugin(
        &["lsp:tls", "buffer.read"],
        &["tls"],
        &requests,
        Sessions::new(&seen),
    );
    // Waiting requests are held open from inside the session, as other threads' would be: the
    // slice is one store's, so this fills the cap through the registry the app shares.
    let held: Vec<_> = (0..MAX_PENDING_PER_PLUGIN)
        .map(|_| requests.hold(PLUGIN).unwrap())
        .collect();
    assert_eq!(requests.pending(PLUGIN), MAX_PENDING_PER_PLUGIN);
    let over = state.request(
        "tls".into(),
        Target::Buffer(1),
        "tls/custom".into(),
        "{}".into(),
    );
    assert!(matches!(over, Err(Error::Failed(_))), "{over:?}");
    drop(held);
    assert_eq!(requests.pending(PLUGIN), 0);

    // A request waiting when the plugin unloads is cancelled, and its answer isn't returned.
    let unload = requests.clone();
    let mut sessions = Sessions::new(&seen);
    sessions.during = Some(Box::new(move || unload.unload(PLUGIN)));
    let mut state = plugin(&["lsp:tls", "buffer.read"], &["tls"], &requests, sessions);
    denied(state.request(
        "tls".into(),
        Target::Buffer(1),
        "tls/custom".into(),
        "{}".into(),
    ));
    assert_eq!(requests.pending(PLUGIN), 0);
    // Unloaded, it sends nothing more.
    denied(state.request(
        "tls".into(),
        Target::Buffer(1),
        "tls/custom".into(),
        "{}".into(),
    ));
}

#[test]
fn lsp_an_old_loads_unloader_leaves_a_newer_load_alone() {
    let seen = Seen::default();
    let requests = LspRequests::new();
    let old = plugin(&["lsp:tls"], &["tls"], &requests, Sessions::new(&seen));
    let unloader = old.lsp.unloader().unwrap();
    // A new load of the plugin attaches, and holds a request waiting.
    let _new = plugin(&["lsp:tls"], &["tls"], &requests, Sessions::new(&seen));
    let waiting = requests.hold(PLUGIN).unwrap();
    unloader.unload();
    assert_eq!(
        requests.pending(PLUGIN),
        1,
        "the old unloader cancelled the new load"
    );
    requests
        .hold(PLUGIN)
        .expect("the new load is still attached");
    drop(waiting);
}
