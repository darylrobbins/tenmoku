//! fe-lsp against real language servers (docs/decisions/lsp-client.md §6.2,
//! §6.3): rust-analyzer, pyright, typescript-language-server and
//! typescript-go, each over a copy of its project in `tests/fixtures/real/`.
//!
//! Every test is `#[ignore]`d, so the gate runs none of them:
//! `scripts/lsp-real.sh` installs the servers and runs them. Each test finds
//! its server from `FE_LSP_<SERVER>`, then `tools/lsp/node_modules`, then
//! `rustup which`. One that's missing prints
//! `skipped: <bin> not found (run scripts/lsp-real.sh)` and passes, unless
//! `FE_LSP_REQUIRE_SERVERS=1`, which makes it fail.
//!
//! The servers start from the built-in specs, through the `[lsp]` settings
//! a user would write (`command` and `env`), and a launcher that wraps
//! [`LocalLauncher`] to watch the wire ([`Tap`]): what `publishDiagnostics`
//! and the replies to `textDocument/diagnostic` (or to
//! `typescript.tsserverRequest`'s diagnostic commands) carry, how the process
//! went, and, to offer rust-analyzer only utf-16, a rewrite of
//! `initialize`'s `positionEncodings`.

use std::collections::{BTreeMap, HashMap};
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::atomic::AtomicBool;
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::task::{Context, Poll, ready};
use std::time::{Duration, Instant};
use std::{env, fs, io};

use fe_buffer::{Buffer, Origin};
use fe_config::settings::{LspServerSettings, LspSettings, TypescriptServer};
use fe_lsp::{
    ExitInfo, LaunchCancel, LocalLauncher, LspEvent, LspManager, LspNavigation, PositionEncoding,
    ServerId, ServerIo, ServerLauncher, ServerProcess, ServerSpec, ServerStatus, from_lsp, to_lsp,
};
use fe_nav::{FileReferences, NavError, NavRequest, NavigationProvider};
use futures::{AsyncRead, AsyncWrite};
use lsp_types::request::GotoDefinition;
use lsp_types::{
    GotoDefinitionParams, GotoDefinitionResponse, Position, TextDocumentIdentifier,
    TextDocumentPositionParams, Url,
};
use serde_json::Value;
use tempfile::TempDir;

/// The Rust fixture's manifest, written into its copy rather than kept in
/// the repository, where cargo and editors would take it for a package.
const CARGO_TOML: &str = "[package]
name = \"real\"
version = \"0.1.0\"
edition = \"2024\"
publish = false

[workspace]
";

/// How long a server may take from launch to `Ready` (§6.3).
const READY_WITHIN: Duration = Duration::from_secs(60);
/// How long anything else may take: an answer that isn't empty, a
/// `cargo check`, a process exiting.
const WAIT: Duration = Duration::from_secs(120);
/// How long a server may take to report diagnostics for an edit.
const DIAGNOSTICS_WITHIN: Duration = Duration::from_secs(30);

#[test]
#[ignore = "needs language servers: scripts/lsp-real.sh"]
fn real_server_rust_analyzer_utf8() {
    check_server(Server::RustAnalyzer, PositionEncoding::Utf8);
}

#[test]
#[ignore = "needs language servers: scripts/lsp-real.sh"]
fn real_server_rust_analyzer_utf16() {
    let Some(s) = Session::start(Server::RustAnalyzer, Some(&["utf-16"])) else {
        return;
    };
    s.wait_ready();
    s.check_columns(PositionEncoding::Utf16);
    s.stop_cleanly();
}

#[test]
#[ignore = "needs language servers: scripts/lsp-real.sh"]
fn real_server_rust_analyzer_diagnostics_follow_saves() {
    let Some(s) = Session::start(Server::RustAnalyzer, None) else {
        return;
    };
    s.wait_ready();
    let rel = Server::RustAnalyzer.greet();
    let path = s.root.join(rel);
    let mut greet = Buffer::new(&fs::read_to_string(&path).unwrap());
    s.open(&mut greet, rel);
    let mismatch = |items: &[fe_lsp::Diagnostic]| {
        items.iter().any(|item| {
            item.source.as_deref() == Some("rustc") && item.code.as_deref() == Some("E0308")
        })
    };

    // `cargo check`'s E0308 arrives after saving a type error...
    let literal = range_of(&greet, "\"hello\"");
    greet.edit([(literal, "1")], Origin::User);
    s.save(&mut greet, &path);
    // It's shown with what rust-analyzer's pull finds: printed, to see
    // whether the two lists repeat each other (§10).
    let items = s.wait_diagnostics(&greet, "cargo check's E0308 after the save", |items| {
        mismatch(items)
    });
    for item in &items {
        println!(
            "rust-analyzer: after the save: {:?} {:?}: {}",
            item.source,
            item.code,
            item.message.lines().next().unwrap_or_default(),
        );
    }

    // ... and leaves after saving the fix.
    let literal = range_of(&greet, "1");
    greet.edit([(literal, "\"hello\"")], Origin::User);
    s.save(&mut greet, &path);
    s.wait_diagnostics(&greet, "no E0308 after saving the fix", |items| {
        !mismatch(items)
    });
    s.stop_cleanly();
}

/// rust-analyzer counts its unresolved-import diagnostic as experimental,
/// off unless the user turns experimental diagnostics on (§10). With them
/// on, an unresolved import in an unsaved buffer comes by pull, and reaches
/// the sink at the edited version, never saved.
#[test]
#[ignore = "needs language servers: scripts/lsp-real.sh"]
fn real_server_rust_analyzer_unresolved_import_unsaved() {
    let experimental = serde_json::json!({ "diagnostics": { "experimental": { "enable": true } } });
    let Some(s) = Session::start_with(Server::RustAnalyzer, None, Some(experimental)) else {
        return;
    };
    s.wait_ready();
    let rel = Server::RustAnalyzer.greet();
    let path = s.root.join(rel);
    let mut greet = Buffer::new(&fs::read_to_string(&path).unwrap());
    s.open(&mut greet, rel);
    let pulled_from = s.seen.lock().pulled.len();
    let end = greet.len();
    greet.edit([(end..end, "\nuse crate::missing;\n")], Origin::User);
    let edited = Instant::now();
    s.manager().changed(greet.id());
    let items = s.wait_until_diagnostics(
        &greet,
        "the unresolved import, unsaved",
        edited + DIAGNOSTICS_WITHIN,
        unresolved_import,
    );
    println!(
        "rust-analyzer: the unresolved import reached the sink {:.2?} after the edit",
        edited.elapsed(),
    );
    for item in &items {
        println!(
            "rust-analyzer:   {:?} {:?} {:?}: {}",
            item.severity,
            item.source,
            item.code,
            item.message.lines().next().unwrap_or_default(),
        );
    }
    let uri = Url::from_file_path(&path).unwrap();
    assert!(
        (s.seen.lock().pulled[pulled_from..].iter())
            .any(|pulled| pulled.uri == uri && pulled.items > 0),
        "the unresolved import came by pull",
    );
    assert!(
        fs::read_to_string(&path).unwrap().ends_with("}\n"),
        "never saved"
    );
    s.stop_cleanly();
}

/// rust-analyzer's own unresolved-import diagnostic among `items`.
fn unresolved_import(items: &[fe_lsp::Diagnostic]) -> bool {
    items.iter().any(|item| {
        item.source.as_deref() == Some("rust-analyzer")
            && item.message.contains("unresolved import")
    })
}

#[test]
#[ignore = "needs language servers: scripts/lsp-real.sh"]
fn real_server_pyright() {
    check_server(Server::Pyright, PositionEncoding::Utf16);
}

#[test]
#[ignore = "needs language servers: scripts/lsp-real.sh"]
fn real_server_typescript_language_server() {
    check_server(Server::TypescriptLanguageServer, PositionEncoding::Utf16);
}

#[test]
#[ignore = "needs language servers: scripts/lsp-real.sh"]
fn real_server_typescript_go() {
    check_server(Server::TypescriptGo, PositionEncoding::Utf8);
}

/// Everything one server is checked for, in one session: launch to
/// `Ready`, the §4.4 columns under `encoding`, definition and references
/// through [`LspNavigation`], whether its diagnostics carry a version, and
/// a clean stop.
fn check_server(server: Server, encoding: PositionEncoding) {
    let Some(s) = Session::start(server, None) else {
        return;
    };
    s.wait_ready();
    s.check_columns(encoding);
    s.check_navigation();
    s.check_diagnostics_version();
    s.stop_cleanly();
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(clippy::enum_variant_names)] // The servers' own names.
enum Server {
    RustAnalyzer,
    Pyright,
    TypescriptLanguageServer,
    TypescriptGo,
}

impl Server {
    /// Its built-in spec's name.
    fn name(self) -> &'static str {
        match self {
            Self::RustAnalyzer => "rust-analyzer",
            Self::Pyright => "pyright",
            Self::TypescriptLanguageServer => "typescript-language-server",
            Self::TypescriptGo => "typescript-go",
        }
    }

    /// The binary it runs.
    fn bin(self) -> &'static str {
        match self {
            Self::RustAnalyzer => "rust-analyzer",
            Self::Pyright => "pyright-langserver",
            Self::TypescriptLanguageServer => "typescript-language-server",
            Self::TypescriptGo => "tsc",
        }
    }

    /// The variable naming its binary.
    fn var(self) -> &'static str {
        match self {
            Self::RustAnalyzer => "FE_LSP_RUST_ANALYZER",
            Self::Pyright => "FE_LSP_PYRIGHT",
            Self::TypescriptLanguageServer => "FE_LSP_TYPESCRIPT_LANGUAGE_SERVER",
            Self::TypescriptGo => "FE_LSP_TYPESCRIPT_GO",
        }
    }

    /// Where `npm ci --prefix tools/lsp` puts it, under `node_modules`.
    /// typescript-go is found by its package's path, since `typescript`
    /// 6.0.3 has a `tsc` bin too.
    fn in_node_modules(self) -> Option<&'static str> {
        match self {
            Self::RustAnalyzer => None,
            Self::Pyright => Some(".bin/pyright-langserver"),
            Self::TypescriptLanguageServer => Some(".bin/typescript-language-server"),
            Self::TypescriptGo => Some("typescript-go/bin/tsc"),
        }
    }

    /// Its project under `tests/fixtures/real`.
    fn project(self) -> &'static str {
        match self {
            Self::RustAnalyzer => "rust",
            Self::Pyright => "python",
            Self::TypescriptLanguageServer | Self::TypescriptGo => "typescript",
        }
    }

    /// fe-syntax's name for its files.
    fn language(self) -> &'static str {
        match self {
            Self::RustAnalyzer => "rust",
            Self::Pyright => "python",
            Self::TypescriptLanguageServer | Self::TypescriptGo => "typescript",
        }
    }

    /// The file holding the §4.4 cases and a call of `hello`.
    fn main(self) -> &'static str {
        match self {
            Self::RustAnalyzer => "src/main.rs",
            Self::Pyright => "main.py",
            Self::TypescriptLanguageServer | Self::TypescriptGo => "main.ts",
        }
    }

    /// The file defining `hello`.
    fn greet(self) -> &'static str {
        match self {
            Self::RustAnalyzer => "src/greet.rs",
            Self::Pyright => "greet.py",
            Self::TypescriptLanguageServer | Self::TypescriptGo => "greet.ts",
        }
    }

    /// The column of the name declared after §4.4's case 1, 3 and 7 text,
    /// as the probe measured it (§4.2), in `encoding`.
    fn columns(self, encoding: PositionEncoding) -> [u32; 3] {
        match (self, encoding) {
            (Self::Pyright, _) => [11, 17, 11],
            (_, PositionEncoding::Utf8) => [25, 39, 24],
            (_, PositionEncoding::Utf16) => [23, 29, 23],
        }
    }

    /// An error, for the end of the greet file, left unsaved. For
    /// rust-analyzer, an unresolved import and a type mismatch. Its own
    /// diagnostics, offered only by pull, report the mismatch (E0308); the
    /// import (E0432) is an experimental one, off by default, so under the
    /// built-in spec only `cargo check` reports it, pushed after a save
    /// (§10, and `real_server_rust_analyzer_unresolved_import_unsaved`).
    fn error(self) -> &'static str {
        match self {
            Self::RustAnalyzer => "\nuse crate::missing;\npub fn broken() -> i32 { \"x\" }\n",
            Self::Pyright => "\nbroken: int = \"x\"\n",
            Self::TypescriptLanguageServer | Self::TypescriptGo => {
                "\nexport const broken: number = \"x\";\n"
            }
        }
    }

    /// Whether its `publishDiagnostics` holding the unsaved error carry a
    /// version, as the runs showed (§10); `None` if it pushes none for it
    /// (rust-analyzer's own diagnostics and typescript-go's are pull-only).
    fn versions_diagnostics(self) -> Option<bool> {
        match self {
            Self::Pyright => Some(true),
            Self::TypescriptLanguageServer => Some(false),
            Self::RustAnalyzer | Self::TypescriptGo => None,
        }
    }

    /// Whether it advertises `diagnosticProvider`, for fe-lsp's
    /// `textDocument/diagnostic`.
    fn advertises_pull(self) -> bool {
        match self {
            Self::RustAnalyzer | Self::TypescriptGo => true,
            Self::Pyright | Self::TypescriptLanguageServer => false,
        }
    }

    /// Whether a reply to fe-lsp's pull holds the unsaved error, as the
    /// runs showed (§10): to `textDocument/diagnostic`, or for
    /// typescript-language-server, whose pushes carry no version, to one
    /// of the three `typescript.tsserverRequest`s
    /// (`PullDiagnostics::TsserverRequest`).
    fn pulls_diagnostics(self) -> bool {
        match self {
            Self::RustAnalyzer | Self::TypescriptGo | Self::TypescriptLanguageServer => true,
            Self::Pyright => false,
        }
    }

    /// Whether the unsaved error reaches the sink: decoded from a
    /// versioned push or a pull.
    fn shows_unsaved_error(self) -> bool {
        self.pulls_diagnostics() || self.versions_diagnostics() == Some(true)
    }

    /// Whether `items`, at the edited version, hold the unsaved error.
    fn holds_error(self, items: &[fe_lsp::Diagnostic]) -> bool {
        items.iter().any(|item| match self {
            Self::RustAnalyzer => {
                item.source.as_deref() == Some("rust-analyzer")
                    && item.code.as_deref() == Some("E0308")
            }
            Self::Pyright => item.severity == Some(fe_lsp::Severity::Error),
            Self::TypescriptLanguageServer | Self::TypescriptGo => {
                item.code.as_deref() == Some("2322")
            }
        })
    }

    /// Its binary: `FE_LSP_<SERVER>`, then `tools/lsp/node_modules`, then
    /// `rustup which`.
    fn find(self) -> Option<PathBuf> {
        if let Some(path) = env::var_os(self.var()).filter(|path| !path.is_empty()) {
            return Some(path.into());
        }
        if let Some(rel) = self.in_node_modules() {
            let path = manifest_dir()
                .join("../../tools/lsp/node_modules")
                .join(rel);
            if path.is_file() {
                return Some(path);
            }
        }
        rustup_which(self.bin())
    }

    /// Its binary, or `None` once it has said the test is skipped. Panics
    /// instead when `FE_LSP_REQUIRE_SERVERS=1`.
    fn command(self) -> Option<PathBuf> {
        if let Some(path) = self.find() {
            return Some(path);
        }
        let missing = format!("{} not found (run scripts/lsp-real.sh)", self.bin());
        assert!(
            env::var_os("FE_LSP_REQUIRE_SERVERS").is_none_or(|require| require != "1"),
            "{missing}, and FE_LSP_REQUIRE_SERVERS=1",
        );
        println!("skipped: {missing}");
        None
    }
}

fn manifest_dir() -> PathBuf {
    // Read at run time: a test binary built in another checkout keeps that
    // checkout's compile-time path.
    env::var_os("CARGO_MANIFEST_DIR").map_or_else(|| env!("CARGO_MANIFEST_DIR").into(), Into::into)
}

/// `rustup which <bin>`, run here so it's the pinned toolchain's.
fn rustup_which(bin: &str) -> Option<PathBuf> {
    let cargo_home = env::var_os("CARGO_HOME")
        .map(PathBuf::from)
        .or_else(|| env::var_os("HOME").map(|home| Path::new(&home).join(".cargo")));
    let rustups = [
        Some(PathBuf::from("rustup")),
        cargo_home.map(|home| home.join("bin/rustup")),
    ];
    rustups.into_iter().flatten().find_map(|rustup| {
        let out = std::process::Command::new(rustup)
            .args(["which", bin])
            .current_dir(manifest_dir())
            .output()
            .ok()?;
        let path = PathBuf::from(String::from_utf8(out.stdout).ok()?.trim());
        (out.status.success() && path.is_file()).then_some(path)
    })
}

/// The `[lsp]` settings that run `server`'s built-in spec from `command`,
/// on this process's `PATH` (the one `scripts/lsp-real.sh` gives it, with
/// tools/lsp's Node) rather than the login shell's, with `options` as its
/// `initializationOptions` if given.
fn settings(server: Server, command: &Path, options: Option<Value>) -> LspSettings {
    let mut settings = LspSettings::default();
    if server == Server::TypescriptLanguageServer {
        settings.typescript_server = TypescriptServer::TypescriptLanguageServer;
    }
    let path = env::var("PATH").unwrap_or_default();
    settings.servers.insert(
        server.name().into(),
        LspServerSettings {
            command: Some(command.to_str().unwrap().into()),
            env: Some(BTreeMap::from([("PATH".into(), path)])),
            initialization_options: options,
            ..LspServerSettings::default()
        },
    );
    settings
}

fn copy_dir(from: &Path, to: &Path) {
    fs::create_dir_all(to).unwrap();
    for entry in fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let target = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_dir(&entry.path(), &target);
        } else {
            fs::copy(entry.path(), target).unwrap();
        }
    }
}

/// The bytes of the first `needle` in `buffer`.
fn range_of(buffer: &Buffer, needle: &str) -> Range<usize> {
    let text = buffer.text().to_string();
    let start = text
        .find(needle)
        .unwrap_or_else(|| panic!("no {needle:?} in {text:?}"));
    start..start + needle.len()
}

/// A server running over a copy of its project, with its main file open.
struct Session {
    server: Server,
    manager: Arc<Mutex<LspManager>>,
    /// Each event, with when the sink got it.
    events: Receiver<(Instant, LspEvent)>,
    id: ServerId,
    root: PathBuf,
    seen: Seen,
    main: Buffer,
    launched: Instant,
    _dir: TempDir,
}

impl Session {
    /// Starts `server` by opening its main file, offering `offer` as
    /// `positionEncodings` instead of fe-lsp's own if given. `None` if the
    /// server isn't installed.
    fn start(server: Server, offer: Option<&[&str]>) -> Option<Self> {
        Self::start_with(server, offer, None)
    }

    /// As [`start`](Self::start), with `options` as the server's
    /// `initializationOptions`.
    fn start_with(server: Server, offer: Option<&[&str]>, options: Option<Value>) -> Option<Self> {
        let command = server.command()?;
        let dir = tempfile::tempdir().unwrap();
        let root = fs::canonicalize(dir.path()).unwrap();
        let fixture = manifest_dir().join("tests/fixtures/real");
        copy_dir(&fixture.join(server.project()), &root);
        if server == Server::RustAnalyzer {
            fs::write(root.join("Cargo.toml"), CARGO_TOML).unwrap();
        }
        let seen = Seen::default();
        let launcher = Tap {
            offer: offer.map(|offer| offer.iter().map(|&kind| kind.to_owned()).collect()),
            seen: seen.clone(),
        };
        let (sender, events) = std::sync::mpsc::channel();
        let mut manager = LspManager::new(Arc::new(launcher), move |event| {
            let _ = sender.send((Instant::now(), event));
        });
        manager.update_settings(&settings(server, &command, options));
        let mut main = Buffer::new(&fs::read_to_string(root.join(server.main())).unwrap());
        let launched = Instant::now();
        let id = open(&mut manager, &root, &mut main, server, server.main())
            .unwrap_or_else(|| panic!("no server for {}", server.main()));
        Some(Self {
            server,
            manager: Arc::new(Mutex::new(manager)),
            events,
            id,
            root,
            seen,
            main,
            launched,
            _dir: dir,
        })
    }

    fn manager(&self) -> MutexGuard<'_, LspManager> {
        self.manager.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Opens `buffer`, the file at `rel`, on the session's server.
    fn open(&self, buffer: &mut Buffer, rel: &str) {
        let manager = self.manager.clone();
        let mut manager = manager.lock().unwrap_or_else(PoisonError::into_inner);
        let server = open(&mut manager, &self.root, buffer, self.server, rel);
        assert_eq!(server, Some(self.id), "{rel} went to another server");
    }

    /// Waits for the next event `pick` takes that the sink got by
    /// `deadline`. Panics at the deadline, or on an event the sink got after
    /// it.
    fn until<T>(
        &self,
        what: &str,
        deadline: Instant,
        mut pick: impl FnMut(LspEvent) -> Option<T>,
    ) -> T {
        let late = || -> ! {
            panic!(
                "no {what} from {} in time; its log:\n{}",
                self.server.name(),
                self.log(),
            )
        };
        loop {
            let Some(left) = deadline.checked_duration_since(Instant::now()) else {
                late();
            };
            match self.events.recv_timeout(left) {
                Ok((at, _)) if at > deadline => late(),
                Ok((_, event)) => {
                    let stopped = match &event {
                        LspEvent::Status {
                            server,
                            status: ServerStatus::Stopped(why),
                            ..
                        } if *server == self.id => Some(why.clone()),
                        _ => None,
                    };
                    if let Some(picked) = pick(event) {
                        return picked;
                    }
                    if let Some(why) = stopped {
                        panic!(
                            "{} stopped ({why:?}) waiting for {what}; its log:\n{}",
                            self.server.name(),
                            self.log(),
                        );
                    }
                }
                Err(RecvTimeoutError::Timeout) => late(),
                Err(RecvTimeoutError::Disconnected) => panic!("the manager has gone"),
            }
        }
    }

    fn log(&self) -> String {
        self.manager().log(self.server.name(), &self.root)
    }

    /// Waits for `Ready`, which must come within [`READY_WITHIN`] of the
    /// launch.
    fn wait_ready(&self) {
        let id = self.id;
        self.until("Ready", self.launched + READY_WITHIN, |event| match event {
            LspEvent::Status {
                server,
                status: ServerStatus::Ready,
                ..
            } if server == id => Some(()),
            _ => None,
        });
        println!(
            "{}: Ready {:.2} s after launch",
            self.server.name(),
            self.launched.elapsed().as_secs_f64(),
        );
    }

    /// §4.4's cases 1, 3 and 7: a definition asked at a use, mid-row,
    /// lands on the declaration at the probe's column in `encoding`, which
    /// the server must have negotiated, and decodes to its byte offset.
    fn check_columns(&self, encoding: PositionEncoding) {
        let negotiated = self.manager().position_encoding(self.id);
        assert_eq!(
            negotiated,
            Some(encoding),
            "{}'s encoding",
            self.server.name()
        );
        let main = self.main.text().to_string();
        let uri = Url::from_file_path(self.root.join(self.server.main())).unwrap();
        let columns = self.server.columns(encoding);
        for (case, column) in [1, 3, 7].into_iter().zip(columns) {
            let declared = main.find(&format!("e{case} = ")).unwrap();
            let used = main
                .find(&format!("(e{case}, "))
                .or_else(|| main.find(&format!("[e{case}, ")));
            let used = used.unwrap() + 1;
            let expected = to_lsp(self.main.text(), declared, encoding);
            assert_eq!(
                expected.character, column,
                "case {case}: the fixture's column"
            );
            let found = self.retry(&format!("case {case}'s definition"), || {
                let reply = self.manager().document_request::<GotoDefinition>(
                    self.main.id(),
                    self.main.version(),
                    move |at| GotoDefinitionParams {
                        text_document_position_params: TextDocumentPositionParams {
                            text_document: TextDocumentIdentifier {
                                uri: at.document.uri.clone(),
                            },
                            position: to_lsp(&at.document.sent.text, used, at.encoding),
                        },
                        work_done_progress_params: Default::default(),
                        partial_result_params: Default::default(),
                    },
                );
                let (reply, _) = async_io::block_on(reply).map_err(|err| err.to_string())?;
                Ok(targets(reply.result))
            });
            assert_eq!(
                found,
                vec![(uri.clone(), expected)],
                "case {case}: {} under {encoding:?}",
                self.server.name(),
            );
            assert_eq!(from_lsp(self.main.text(), expected, encoding), declared);
        }
    }

    /// `f`'s answer once it's neither an error nor empty: a server may
    /// answer before it has loaded the project. Only an answer that came
    /// within [`WAIT`] counts.
    fn retry<T>(&self, what: &str, mut f: impl FnMut() -> Result<Vec<T>, String>) -> Vec<T> {
        let deadline = Instant::now() + WAIT;
        loop {
            let answer = f();
            let why = match answer {
                Ok(found) if !found.is_empty() && Instant::now() <= deadline => return found,
                Ok(found) if !found.is_empty() => "answered too late".to_owned(),
                Ok(_) => "empty".to_owned(),
                Err(why) => why,
            };
            assert!(
                Instant::now() < deadline,
                "{what} from {}: {why}; its log:\n{}",
                self.server.name(),
                self.log(),
            );
            std::thread::sleep(Duration::from_millis(250));
        }
    }

    /// Go-to-definition and references on the call of `hello` in the main
    /// file, through [`LspNavigation`]: the definition is `hello`'s name in
    /// the greet file, which isn't open, and the references are every call
    /// and the declaration, in both files.
    fn check_navigation(&self) {
        let navigation = LspNavigation::new(Arc::downgrade(&self.manager), &self.root);
        let main = self.main.text().to_string();
        let greet_rel = self.server.greet();
        let greet = fs::read_to_string(self.root.join(greet_rel)).unwrap();
        let req = NavRequest {
            root: self.root.clone(),
            rel: self.server.main().into(),
            language: Some(self.server.language()),
            text: self.main.text().clone(),
            offset: main.rfind("hello()").unwrap() + 2,
            buffer: Some((self.main.id(), self.main.version())),
        };
        let cancel = AtomicBool::new(false);

        let declared = greet.find("hello").unwrap();
        let definitions = self.retry("definitions", || {
            let found = navigation.definitions(&req, &cancel).map_err(nav)?;
            Ok(found
                .items
                .into_iter()
                .map(|item| (item.location.rel, item.location.range))
                .collect())
        });
        assert!(
            definitions.contains(&(PathBuf::from(greet_rel), declared..declared + 5)),
            "{}'s definitions: {definitions:?}",
            self.server.name(),
        );

        let mut found = self.retry("references", || {
            let mut found = Vec::new();
            let mut sink = |files: Vec<FileReferences>| {
                for file in files {
                    found.extend(
                        file.refs
                            .into_iter()
                            .map(|r| (r.location.rel, r.location.range)),
                    );
                }
            };
            navigation
                .references(&req, &cancel, &mut sink)
                .map_err(nav)?;
            Ok(found)
        });
        found.sort_by_key(|(rel, range)| (rel.clone(), range.start));
        let calls = |rel: &str, text: &str| {
            text.match_indices("hello(")
                .map(|(at, _)| (PathBuf::from(rel), at..at + 5))
                .collect::<Vec<_>>()
        };
        for expected in calls(self.server.main(), &main)
            .into_iter()
            .chain(calls(greet_rel, &greet))
        {
            assert!(
                found.contains(&expected),
                "{}'s references lack {expected:?}: {found:?}",
                self.server.name(),
            );
        }
    }

    /// How the server reports an error in an unsaved buffer (§10),
    /// checked against what the runs showed. It opens the greet file and
    /// adds an error without saving it, then watches the wire for
    /// [`DIAGNOSTICS_WITHIN`]: whether a `publishDiagnostics` holding a
    /// diagnostic for the file comes, and with a version
    /// ([`Server::versions_diagnostics`]), and whether a reply to a pull
    /// for it holds one ([`Server::pulls_diagnostics`]). The error must also reach the sink
    /// as [`LspEvent::Diagnostics`] at the edited version
    /// ([`Server::shows_unsaved_error`]).
    fn check_diagnostics_version(&self) {
        let rel = self.server.greet();
        let path = self.root.join(rel);
        let advertised = (self.manager().capabilities(self.id))
            .is_some_and(|capabilities| capabilities.diagnostic_provider.is_some());
        println!(
            "{}: diagnosticProvider {}",
            self.server.name(),
            if advertised {
                "advertised"
            } else {
                "not advertised"
            },
        );
        assert_eq!(
            advertised,
            self.server.advertises_pull(),
            "whether {} advertises diagnosticProvider",
            self.server.name(),
        );
        let mut greet = Buffer::new(&fs::read_to_string(&path).unwrap());
        self.open(&mut greet, rel);
        let (pushed_from, pulled_from) = {
            let seen = self.seen.lock();
            (seen.diagnostics.len(), seen.pulled.len())
        };
        let end = greet.len();
        greet.edit([(end..end, self.server.error())], Origin::User);
        let edited = Instant::now();
        self.manager().changed(greet.id());
        let uri = Url::from_file_path(&path).unwrap();
        let deadline = edited + DIAGNOSTICS_WITHIN;
        // Everything over the whole window: the diagnostics the sink got
        // for the edited version, and what came on the wire.
        let (id, current) = (greet.id(), greet.version());
        let mut shown: Option<(Duration, Vec<fe_lsp::Diagnostic>)> = None;
        while let Some(left) = deadline.checked_duration_since(Instant::now()) {
            if let Ok((
                at,
                LspEvent::Diagnostics {
                    buffer,
                    version,
                    items,
                    ..
                },
            )) = self.events.recv_timeout(left)
                && buffer == id
                && version == current
                && at <= deadline
                && shown.is_none()
                && self.server.holds_error(&items)
            {
                shown = Some((at - edited, items));
            }
        }
        let seen = self.seen.lock();
        let pushes: Vec<&Published> = (seen.diagnostics[pushed_from..].iter())
            .filter(|published| published.uri == uri)
            .collect();
        let replies: Vec<&PulledReply> = (seen.pulled[pulled_from..].iter())
            .filter(|pulled| pulled.uri == uri)
            .collect();
        let name = self.server.name();
        for push in &pushes {
            println!(
                "{name}: pushed for {rel}: version {:?}, {} items",
                push.version, push.items
            );
        }
        for reply in &replies {
            println!("{name}: pulled for {rel}: {}", reply.what);
        }
        match &shown {
            Some((after, items)) => {
                println!("{name}: the unsaved error reached the sink {after:.2?} after the edit");
                for item in items {
                    println!(
                        "{name}:   {:?} {:?} {:?}: {}",
                        item.severity,
                        item.source,
                        item.code,
                        item.message.lines().next().unwrap_or_default(),
                    );
                }
            }
            None => println!("{name}: the unsaved error never reached the sink"),
        }
        let pushed = (pushes.iter())
            .find(|published| published.items > 0)
            .map(|published| published.version.is_some());
        let pulled = replies.iter().any(|pulled| pulled.items > 0);
        drop(seen);
        assert_eq!(
            (pushed, pulled),
            (
                self.server.versions_diagnostics(),
                self.server.pulls_diagnostics()
            ),
            "how {name} reports an unsaved error (pushed with a version, pulled); its log:\n{}",
            self.log(),
        );
        if self.server == Server::RustAnalyzer
            && let Some((_, items)) = &shown
        {
            assert!(
                !unresolved_import(items),
                "rust-analyzer's unresolved import is an experimental diagnostic, off by default",
            );
        }
        assert_eq!(
            shown.is_some(),
            self.server.shows_unsaved_error(),
            "whether {name}'s unsaved error reached the sink at the edited version",
        );
    }

    /// Waits until `deadline` for diagnostics on `buffer`'s current
    /// version that `want` accepts, and returns them.
    fn wait_until_diagnostics(
        &self,
        buffer: &Buffer,
        what: &str,
        deadline: Instant,
        want: impl Fn(&[fe_lsp::Diagnostic]) -> bool,
    ) -> Vec<fe_lsp::Diagnostic> {
        let (id, current) = (buffer.id(), buffer.version());
        self.until(what, deadline, |event| match event {
            LspEvent::Diagnostics {
                buffer,
                version,
                items,
                ..
            } if buffer == id && version == current && want(&items) => Some(items),
            _ => None,
        })
    }

    /// Writes `buffer` to `path`, marks it saved, and nudges its server.
    fn save(&self, buffer: &mut Buffer, path: &Path) {
        buffer.write_to(fs::File::create(path).unwrap()).unwrap();
        buffer.did_save();
        self.manager().changed(buffer.id());
    }

    /// Waits for diagnostics on `buffer`'s current version that `want`
    /// accepts, and returns them.
    fn wait_diagnostics(
        &self,
        buffer: &Buffer,
        what: &str,
        want: impl Fn(&[fe_lsp::Diagnostic]) -> bool,
    ) -> Vec<fe_lsp::Diagnostic> {
        self.wait_until_diagnostics(buffer, what, Instant::now() + WAIT, want)
    }

    /// Stops the server: it answers `shutdown`, exits after `exit` without
    /// being killed, and reports `Stopped(None)`, a normal stop, whatever
    /// its exit code (typescript-go's is 1, §3.4).
    fn stop_cleanly(self) {
        let id = self.id;
        self.manager().stop(id);
        let stopped = self.until("Stopped", Instant::now() + WAIT, |event| match event {
            LspEvent::Status {
                server,
                status: ServerStatus::Stopped(why),
                ..
            } if server == id => Some(why),
            _ => None,
        });
        assert_eq!(stopped, None, "{}'s stop", self.server.name());
        let seen = self.seen.lock();
        assert_eq!(seen.kills, 0, "{} was killed", self.server.name());
        let code = match self.server {
            Server::TypescriptGo => 1,
            _ => 0,
        };
        assert_eq!(
            seen.exit,
            Some(ExitInfo {
                code: Some(code),
                signal: None
            }),
            "{}'s exit",
            self.server.name(),
        );
    }
}

/// Opens `buffer`, the file at `rel` under `root`, in `server`'s language.
fn open(
    manager: &mut LspManager,
    root: &Path,
    buffer: &mut Buffer,
    server: Server,
    rel: &str,
) -> Option<ServerId> {
    let subscription = buffer.subscribe();
    manager
        .open(
            Some(root),
            buffer.id(),
            buffer.text().clone(),
            buffer.version(),
            subscription,
            &root.join(rel),
            server.language(),
        )
        .unwrap()
}

fn nav(err: NavError) -> String {
    err.to_string()
}

/// Where a definition reply points: each target's URI and the start of
/// the name it selects.
fn targets(reply: Option<GotoDefinitionResponse>) -> Vec<(Url, Position)> {
    match reply {
        None => Vec::new(),
        Some(GotoDefinitionResponse::Scalar(location)) => {
            vec![(location.uri, location.range.start)]
        }
        Some(GotoDefinitionResponse::Array(locations)) => locations
            .into_iter()
            .map(|location| (location.uri, location.range.start))
            .collect(),
        Some(GotoDefinitionResponse::Link(links)) => links
            .into_iter()
            .map(|link| (link.target_uri, link.target_selection_range.start))
            .collect(),
    }
}

/// What a [`Tap`] has seen of its server, across sessions.
#[derive(Clone, Default)]
struct Seen(Arc<Mutex<Wire>>);

#[derive(Default)]
struct Wire {
    /// Each `textDocument/publishDiagnostics`, in order.
    diagnostics: Vec<Published>,
    /// The document of each pull sent, by id, and its tsserver command if
    /// it's a `typescript.tsserverRequest`.
    pulls: HashMap<i64, (Url, Option<String>)>,
    /// Each reply to one, in order.
    pulled: Vec<PulledReply>,
    /// How the process went, once fe-lsp has seen it go.
    exit: Option<ExitInfo>,
    /// How often fe-lsp killed it.
    kills: usize,
}

/// A reply to a pull as it was read.
struct PulledReply {
    uri: Url,
    /// How many diagnostics it held: none for an unchanged report or an
    /// error.
    items: usize,
    /// What it was, for the output.
    what: String,
}

/// A `textDocument/publishDiagnostics` as it was read.
struct Published {
    uri: Url,
    version: Option<i64>,
    /// How many diagnostics it held.
    items: usize,
}

impl Seen {
    fn lock(&self) -> MutexGuard<'_, Wire> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// [`LocalLauncher`], watching the wire and the process, and offering
/// `offer` as `initialize`'s `positionEncodings` if set.
struct Tap {
    offer: Option<Vec<String>>,
    seen: Seen,
}

impl ServerLauncher for Tap {
    fn launch(
        &self,
        spec: &ServerSpec,
        root: &Path,
        cancel: &LaunchCancel,
    ) -> io::Result<ServerIo> {
        let io = LocalLauncher.launch(spec, root, cancel)?;
        Ok(ServerIo {
            input: Box::pin(Offer {
                inner: io.input,
                held: self.offer.as_ref().map(|_| Vec::new()),
                offer: self.offer.clone(),
                owed: Vec::new(),
                written: Vec::new(),
                seen: self.seen.clone(),
            }),
            output: Box::pin(Listen {
                inner: io.output,
                read: Vec::new(),
                seen: self.seen.clone(),
            }),
            stderr: io.stderr,
            process: Box::new(Watch {
                inner: io.process,
                seen: self.seen.clone(),
            }),
        })
    }
}

/// The first whole message in `bytes`: the range of its body.
fn frame(bytes: &[u8]) -> Option<Range<usize>> {
    let end = bytes.windows(4).position(|w| w == b"\r\n\r\n")?;
    let header = std::str::from_utf8(&bytes[..end]).ok()?;
    let length: usize = header.lines().find_map(|line| {
        let (name, value) = line.split_once(':')?;
        name.eq_ignore_ascii_case("content-length")
            .then(|| value.trim().parse().ok())?
    })?;
    let body = end + 4..end + 4 + length;
    (bytes.len() >= body.end).then_some(body)
}

/// The client's side: holds back the first message, `initialize`, until
/// it's whole, then sends it with `positionEncodings` set to `offer`.
/// Notes the document of each pull it passes on.
struct Offer {
    inner: Pin<Box<dyn AsyncWrite + Send>>,
    /// The first message so far, while it's held back.
    held: Option<Vec<u8>>,
    offer: Option<Vec<String>>,
    /// Bytes for `inner` not yet written.
    owed: Vec<u8>,
    /// What's been passed on since the last whole message.
    written: Vec<u8>,
    seen: Seen,
}

impl Offer {
    fn pay(&mut self, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        while !self.owed.is_empty() {
            let n = ready!(self.inner.as_mut().poll_write(cx, &self.owed))?;
            if n == 0 {
                return Poll::Ready(Err(io::ErrorKind::WriteZero.into()));
            }
            self.owed.drain(..n);
        }
        Poll::Ready(Ok(()))
    }
}

impl AsyncWrite for Offer {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        let this = self.get_mut();
        ready!(this.pay(cx))?;
        let Some(held) = &mut this.held else {
            let n = ready!(this.inner.as_mut().poll_write(cx, buf))?;
            this.written.extend_from_slice(&buf[..n]);
            while let Some(body) = frame(&this.written) {
                if let Ok(message) = serde_json::from_slice::<Value>(&this.written[body.clone()])
                    && let Some(id) = message["id"].as_i64()
                    && let Some(pull) = pull_of(&message)
                {
                    this.seen.lock().pulls.insert(id, pull);
                }
                this.written.drain(..body.end);
            }
            return Poll::Ready(Ok(n));
        };
        held.extend_from_slice(buf);
        if let Some(body) = frame(held) {
            let mut message: Value = serde_json::from_slice(&held[body.clone()]).unwrap();
            assert_eq!(message["method"], "initialize", "the first message");
            message["params"]["capabilities"]["general"]["positionEncodings"] =
                this.offer.clone().into();
            let body = serde_json::to_vec(&message).unwrap();
            this.owed = format!("Content-Length: {}\r\n\r\n", body.len()).into_bytes();
            this.owed.extend(body);
            this.held = None;
            // Sent now, in case nothing writes or flushes again until it's
            // answered; a pipe takes it whole.
            if let Poll::Ready(Err(err)) = this.pay(cx) {
                return Poll::Ready(Err(err));
            }
        }
        Poll::Ready(Ok(buf.len()))
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        let this = self.get_mut();
        ready!(this.pay(cx))?;
        this.inner.as_mut().poll_flush(cx)
    }

    fn poll_close(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        let this = self.get_mut();
        ready!(this.pay(cx))?;
        this.inner.as_mut().poll_close(cx)
    }
}

/// The document `message` pulls diagnostics for, and its tsserver command
/// if it's a `typescript.tsserverRequest`; `None` if it's no pull.
fn pull_of(message: &Value) -> Option<(Url, Option<String>)> {
    let params = &message["params"];
    let (uri, command) = match message["method"].as_str()? {
        "textDocument/diagnostic" => (&params["textDocument"]["uri"], None),
        "workspace/executeCommand" if params["command"] == "typescript.tsserverRequest" => {
            let command = params["arguments"][0].as_str()?;
            (&params["arguments"][1]["file"], Some(command.to_owned()))
        }
        _ => return None,
    };
    Some((Url::parse(uri.as_str()?).ok()?, command))
}

/// The server's side, passed through as it is, noting each
/// `publishDiagnostics`.
struct Listen {
    inner: Pin<Box<dyn AsyncRead + Send>>,
    /// What's been read since the last whole message.
    read: Vec<u8>,
    seen: Seen,
}

impl AsyncRead for Listen {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut [u8],
    ) -> Poll<io::Result<usize>> {
        let this = self.get_mut();
        let n = ready!(this.inner.as_mut().poll_read(cx, buf))?;
        this.read.extend_from_slice(&buf[..n]);
        while let Some(body) = frame(&this.read) {
            let message = serde_json::from_slice::<Value>(&this.read[body.clone()]);
            if let Ok(message) = &message
                && message["method"] == "textDocument/publishDiagnostics"
                && let Ok(uri) = Url::parse(message["params"]["uri"].as_str().unwrap_or_default())
            {
                let params = &message["params"];
                this.seen.lock().diagnostics.push(Published {
                    uri,
                    version: params["version"].as_i64(),
                    items: params["diagnostics"].as_array().map_or(0, Vec::len),
                });
            }
            // A reply to one of fe-lsp's pulls.
            if let Ok(message) = &message
                && message.get("method").is_none()
                && let Some(id) = message["id"].as_i64()
            {
                let mut seen = this.seen.lock();
                if let Some((uri, command)) = seen.pulls.remove(&id) {
                    let result = &message["result"];
                    let items = match &command {
                        None => result["items"].as_array(),
                        Some(_) => result["body"].as_array(),
                    };
                    let items = items.map_or(0, Vec::len);
                    let what = match (message.get("error"), &command) {
                        (Some(error), _) => {
                            let message = error["message"].as_str().unwrap_or_default();
                            let first = message.lines().next().unwrap_or_default();
                            format!("error {} {first}", error["code"])
                        }
                        (None, None) => format!("{} report, {items} items", result["kind"]),
                        (None, Some(command)) => format!(
                            "{command}: {} {}, {items} items",
                            result["type"], result["success"],
                        ),
                    };
                    seen.pulled.push(PulledReply { uri, items, what });
                }
            }
            this.read.drain(..body.end);
        }
        Poll::Ready(Ok(n))
    }
}

/// The process, noting kills and how it exited.
struct Watch {
    inner: Box<dyn ServerProcess>,
    seen: Seen,
}

impl ServerProcess for Watch {
    fn exited(&mut self) -> Pin<Box<dyn Future<Output = ExitInfo> + Send + '_>> {
        let seen = self.seen.clone();
        let exited = self.inner.exited();
        Box::pin(async move {
            let exit = exited.await;
            seen.lock().exit = Some(exit);
            exit
        })
    }

    fn kill(&mut self) {
        self.seen.lock().kills += 1;
        self.inner.kill();
    }
}
