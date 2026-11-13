//! Add-on servers (docs/decisions/q5j03ax2af-lsp-add-on-servers.md §4,
//! §5, §7): a document placed on its primary and its add-ons at once, each
//! apart, with one holder per (URI, lead), and its features routed across
//! them.

mod crash;
mod first_answer;
mod holders;
mod lead_kept;
mod lead_unavailable;
mod owners;
mod provider;
mod reconcile;
mod sync;

use std::path::{Path, PathBuf};
use std::sync::Arc;

use fe_buffer::{Buffer, Origin};
use fe_config::settings::{LspServerSettings, LspSettings};
use lsp_types::{PositionEncodingKind, ServerCapabilities, Url};
use serde_json::{Value, json};

use super::{Events, ROOT, answer, manager};
use crate::fake::{
    Echo, FakeLaunchers, FakeServer, InitializeReply, Record, Replica, Script, initialize_result,
};
use crate::{LspManager, ManualClock, RouteServer, ServerId, ServerLauncher, ServerSpec};

/// The language the fixture's primary serves, which no built-in lists.
const CSS: &str = "x-css";
/// A second language, for aliases of one path in two languages.
const SCSS: &str = "x-scss";

/// A spec named `name` for `languages`, an add-on if `add_on`.
fn spec(name: &str, languages: &[&str], add_on: bool) -> ServerSpec {
    ServerSpec {
        languages: languages.iter().map(|&l| l.to_owned()).collect(),
        add_on,
        ..ServerSpec::new(name, format!("fake-{name}"))
    }
}

/// Settings defining each server (name, languages, add-on, on) on their
/// own, as `[lsp.servers.<name>]` tables do.
fn defined(servers: &[(&str, &[&str], bool, bool)]) -> LspSettings {
    let mut settings = LspSettings::default();
    for &(name, languages, add_on, enabled) in servers {
        let server = LspServerSettings {
            enabled,
            command: Some(format!("fake-{name}")),
            languages: Some(languages.iter().map(|&l| l.to_owned()).collect()),
            add_on: Some(add_on),
            ..LspServerSettings::default()
        };
        settings.servers.insert(name.into(), server);
    }
    settings
}

/// One manager over a fake per server name, on a manual clock that moves
/// only when a test moves it.
struct Fixture {
    fakes: Vec<(&'static str, FakeServer)>,
    clock: Arc<ManualClock>,
    manager: LspManager,
    events: Events,
}

impl Fixture {
    /// Fakes named `names`, with no specs registered.
    fn new(names: &[&'static str]) -> Self {
        let fakes: Vec<(&'static str, FakeServer)> = (names.iter())
            .map(|&name| (name, FakeServer::new(Script::default())))
            .collect();
        let launchers =
            FakeLaunchers::new(fakes.iter().map(|(name, fake)| (*name, fake.launcher())));
        Self::with_launcher(fakes, Arc::new(launchers))
    }

    fn with_launcher(
        fakes: Vec<(&'static str, FakeServer)>,
        launcher: Arc<dyn ServerLauncher>,
    ) -> Self {
        Self::with_clock(fakes, launcher, Arc::new(ManualClock::new()))
    }

    fn with_clock(
        fakes: Vec<(&'static str, FakeServer)>,
        launcher: Arc<dyn ServerLauncher>,
        clock: Arc<ManualClock>,
    ) -> Self {
        let (manager, events) = manager(launcher, Some(clock.clone()));
        Self {
            fakes,
            clock,
            manager,
            events,
        }
    }

    /// Primary `css` and add-on `vars`, both for [`CSS`].
    fn css_and_vars() -> Self {
        let mut f = Self::new(&["css", "vars"]);
        f.manager.register_spec(spec("css", &[CSS], false));
        f.manager.register_spec(spec("vars", &[CSS], true));
        f
    }

    /// Primary `css` and add-on `vars`, both for [`CSS`], each fake saying
    /// it can do what `css` and `vars` add to its capabilities, its
    /// scripted delays on the manager's clock.
    fn css_and_vars_with(
        css: impl FnOnce(&mut ServerCapabilities),
        vars: impl FnOnce(&mut ServerCapabilities),
    ) -> Self {
        let clock = Arc::new(ManualClock::new());
        let fake = |give: &mut dyn FnMut(&mut ServerCapabilities)| {
            let mut init = initialize_result(Some(PositionEncodingKind::UTF8));
            give(&mut init.capabilities);
            let script = Script {
                initialize: InitializeReply::Result(Box::new(init)),
                ..Script::default()
            };
            FakeServer::with_clock(script, clock.clone())
        };
        let (mut css, mut vars) = (Some(css), Some(vars));
        let fakes = vec![
            ("css", fake(&mut |c| css.take().unwrap()(c))),
            ("vars", fake(&mut |c| vars.take().unwrap()(c))),
        ];
        let launchers =
            FakeLaunchers::new(fakes.iter().map(|(name, fake)| (*name, fake.launcher())));
        let mut f = Self::with_clock(fakes, Arc::new(launchers), clock);
        f.manager.register_spec(spec("css", &[CSS], false));
        f.manager.register_spec(spec("vars", &[CSS], true));
        f
    }

    /// Opens `buffer` as `a.css` in [`CSS`], and waits until both `css`
    /// and `vars` have it. Returns its URI.
    fn open_on_both(&mut self, buffer: &mut Buffer) -> Url {
        let uri = self.open(buffer, "a.css", CSS);
        self.wait_synced("css", &uri, buffer);
        self.wait_synced("vars", &uri, buffer);
        uri
    }

    /// The params of each `method` request fake `name` got, in any launch.
    fn asked(&self, name: &str, method: &str) -> Vec<Value> {
        (self.fake(name).records().into_iter())
            .filter_map(|record| match record {
                Record::Request {
                    method: asked,
                    params,
                    ..
                } if asked == method => Some(params),
                _ => None,
            })
            .collect()
    }

    fn fake(&self, name: &str) -> &FakeServer {
        &(self.fakes.iter())
            .find(|(named, _)| *named == name)
            .unwrap_or_else(|| panic!("no fake named {name}"))
            .1
    }

    /// Opens `buffer` at `path` in `language`, under window root `root`.
    fn open_at(
        &mut self,
        root: Option<&Path>,
        buffer: &mut Buffer,
        path: &Path,
        language: &str,
    ) -> Option<ServerId> {
        self.manager
            .open(
                root,
                buffer.id(),
                buffer.text().clone(),
                buffer.version(),
                buffer.subscribe(),
                path,
                language,
            )
            .unwrap()
    }

    /// Opens `buffer` as `name` under [`ROOT`], the window root, in
    /// `language`. Returns its URI.
    fn open(&mut self, buffer: &mut Buffer, name: &str, language: &str) -> Url {
        let path = PathBuf::from(ROOT).join(name);
        self.open_at(Some(Path::new(ROOT)), buffer, &path, language);
        Url::from_file_path(&path).unwrap()
    }

    /// The instance of server `name` `buffer` is placed on.
    fn placed(&self, buffer: &Buffer, name: &str) -> Option<ServerId> {
        (self.manager.route_servers(buffer.id()).into_iter())
            .find(|server| server.name == name)
            .and_then(|server| server.placed)
    }

    /// Waits until fake `name` has `uri` open with `buffer`'s text.
    fn wait_synced(&self, name: &str, uri: &Url, buffer: &Buffer) -> Replica {
        let text = buffer.text().to_string();
        self.fake(name)
            .wait_for_document(uri, &format!("{name} to have the text"), |r| {
                r.is_some_and(|r| r.open && r.text == text)
            })
            .unwrap()
    }

    /// Waits until fake `name` has closed `uri`.
    fn wait_closed(&self, name: &str, uri: &Url) {
        self.fake(name)
            .wait_for_document(uri, &format!("{name} to close it"), |r| {
                r.is_some_and(|r| !r.open)
            });
    }

    /// Returns once the instance `server` has read everything the actor
    /// sent before now.
    fn barrier(&self, server: ServerId) {
        answer(self.manager.request::<Echo>(server, json!({}))).unwrap();
    }

    /// The document notifications fake `name` got for `uri`, in order,
    /// without `textDocument/`, with the version each carried.
    fn notes(&self, name: &str, uri: &Url) -> Vec<(String, Option<i64>)> {
        (self.fake(name).records().into_iter())
            .filter_map(|record| match record {
                Record::Notification { method, params }
                    if params["textDocument"]["uri"] == json!(uri) =>
                {
                    let version = params["textDocument"]["version"].as_i64();
                    Some((method["textDocument/".len()..].to_owned(), version))
                }
                _ => None,
            })
            .collect()
    }

    /// Edits `buffer` (inserts `text` at its start) and says so.
    fn edit(&self, buffer: &mut Buffer, text: &str) {
        buffer
            .edit([(0..0, text)], Origin::User)
            .expect("the edit changed something");
        self.manager.changed(buffer.id());
    }
}

/// `RouteServer` as a tuple, to compare.
fn route(servers: Vec<RouteServer>) -> Vec<(String, PathBuf, bool, Option<ServerId>)> {
    (servers.into_iter())
        .map(|server| (server.name, server.root, server.add_on, server.placed))
        .collect()
}

/// The params an Echo on `buffer` came back with: it goes to its lead.
fn echo_on(manager: &LspManager, buffer: &Buffer) -> Result<Value, crate::LspError> {
    let reply = manager.document_request::<Echo>(buffer.id(), buffer.version(), |_| json!({}));
    answer(reply).map(|(reply, _)| reply.result["params"].clone())
}

/// Gives the manager's threads a moment of real time to do anything they
/// would.
fn settle() {
    std::thread::sleep(std::time::Duration::from_millis(100));
}
