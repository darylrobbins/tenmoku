//! The built-in rust-analyzer's memory-conscious defaults
//! (docs/decisions/ztdenw59dh-rust-analyzer-memory-defaults.md), as a
//! routed instance launches with them: seen by a launcher that records
//! each spec it's given and starts nothing.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex, PoisonError};
use std::time::Duration;

use fe_buffer::Buffer;
use fe_config::settings::{LspServerSettings, LspSettings};
use fe_lsp::{LaunchCancel, LspManager, ServerIo, ServerLauncher, ServerSpec};
use serde_json::{Value, json};
use tempfile::TempDir;

/// Records each launch and refuses it.
#[derive(Default)]
struct Recorder {
    launches: Mutex<Vec<(ServerSpec, PathBuf)>>,
    launched: Condvar,
}

impl ServerLauncher for Recorder {
    fn launch(
        &self,
        spec: &ServerSpec,
        root: &Path,
        _cancel: &LaunchCancel,
    ) -> io::Result<ServerIo> {
        let mut launches = self.launches.lock().unwrap_or_else(PoisonError::into_inner);
        launches.push((spec.clone(), root.to_owned()));
        self.launched.notify_all();
        Err(io::Error::other("recorded, not started"))
    }
}

impl Recorder {
    /// The first spec launched, waiting for it.
    fn first(&self) -> (ServerSpec, PathBuf) {
        let launches = self.launches.lock().unwrap_or_else(PoisonError::into_inner);
        let (launches, _) = self
            .launched
            .wait_timeout_while(launches, Duration::from_secs(10), |l| l.is_empty())
            .unwrap_or_else(PoisonError::into_inner);
        launches.first().cloned().expect("rust-analyzer launched")
    }
}

/// The `initializationOptions` the built-in rust-analyzer launches with
/// for a Rust file at the window root `root`, under `settings`.
fn launched_options(root: &Path, settings: &LspSettings) -> Value {
    launched(root, settings, None)
        .initialization_options
        .expect("rust-analyzer gets initializationOptions")
}

/// The rust-analyzer spec launched for a Rust file at the window root
/// `root`, under `settings`, with `host` registered if given.
fn launched(root: &Path, settings: &LspSettings, host: Option<ServerSpec>) -> ServerSpec {
    let recorder = Arc::new(Recorder::default());
    let mut manager = LspManager::new(recorder.clone(), |_| {});
    manager.update_settings(settings);
    if let Some(host) = host {
        manager.register_spec(host);
    }
    let path = root.join("src/main.rs");
    let mut buffer = Buffer::new("fn main() {}\n");
    let subscription = buffer.subscribe();
    let _ = manager.open(
        Some(root),
        buffer.id(),
        buffer.text().clone(),
        buffer.version(),
        subscription,
        &path,
        "rust",
    );
    let (spec, launched_root) = recorder.first();
    assert_eq!(spec.name, "rust-analyzer");
    assert_eq!(launched_root, root);
    spec
}

/// A root with `target/`, `src/` and the given nested worktrees (each a
/// folder with a `.git` file, as `git worktree add` makes them).
fn workspace(worktrees: &[&str]) -> TempDir {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    fs::write(root.join("Cargo.toml"), "[workspace]\n").unwrap();
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(root.join("src/main.rs"), "fn main() {}\n").unwrap();
    fs::create_dir_all(root.join("target/debug")).unwrap();
    // A plain checkout's `.git` is a folder: never excluded.
    fs::create_dir_all(root.join(".git")).unwrap();
    for worktree in worktrees {
        let at = root.join(worktree);
        fs::create_dir_all(at.join("src")).unwrap();
        fs::write(at.join(".git"), "gitdir: /elsewhere/.git/worktrees/x\n").unwrap();
    }
    dir
}

fn excludes(options: &Value) -> Vec<&str> {
    options["files"]["exclude"]
        .as_array()
        .expect("files.exclude is a list")
        .iter()
        .map(|path| path.as_str().expect("an exclude is a string"))
        .collect()
}

fn rust_analyzer_settings(options: Value) -> LspSettings {
    let mut settings = LspSettings::default();
    settings.servers.insert(
        "rust-analyzer".into(),
        LspServerSettings {
            initialization_options: Some(options),
            ..LspServerSettings::default()
        },
    );
    settings
}

#[test]
fn defaults_exclude_nested_worktrees_and_target() {
    let dir = workspace(&[".worktrees/a"]);
    // A folder holding plain checkouts (`.git` folders) and files.
    let other = dir.path().join("vendor/checkout");
    fs::create_dir_all(other.join(".git")).unwrap();
    fs::write(dir.path().join("vendor/README"), "").unwrap();
    let options = launched_options(dir.path(), &LspSettings::default());
    let listed = excludes(&options);
    assert_eq!(listed, ["target", ".worktrees/a"]);
    assert!(!listed.contains(&"src"));
    // Cache priming and proc macros keep rust-analyzer's own defaults.
    assert_eq!(options["cachePriming"], Value::Null);
    assert_eq!(options["procMacro"], Value::Null);

    // A worktree among the root's children counts too, and one nested
    // deeper than a child's child isn't looked for.
    let dir = workspace(&["lane", ".worktrees/.verify/snapshot"]);
    let options = launched_options(dir.path(), &LspSettings::default());
    assert_eq!(excludes(&options), ["target", "lane"]);

    let dir = workspace(&[]);
    let options = launched_options(dir.path(), &LspSettings::default());
    assert_eq!(excludes(&options), ["target"]);
}

#[test]
fn user_options_merge_over_defaults() {
    let dir = workspace(&[".worktrees/a"]);
    let settings = rust_analyzer_settings(json!({
        "checkOnSave": false,
        "cachePriming": { "enable": true },
    }));
    let options = launched_options(dir.path(), &settings);
    assert_eq!(excludes(&options), ["target", ".worktrees/a"]);
    assert_eq!(options["checkOnSave"], json!(false));
    assert_eq!(options["cachePriming"], json!({ "enable": true }));

    // A list replaces the default one; a sibling key in the same table
    // merges beside the defaults' keys.
    let settings = rust_analyzer_settings(json!({
        "files": { "exclude": ["generated"], "watcher": "server" },
    }));
    let options = launched_options(dir.path(), &settings);
    assert_eq!(excludes(&options), ["generated"]);
    assert_eq!(options["files"]["watcher"], json!("server"));
    assert_eq!(options.as_object().unwrap().len(), 1);

    // Options that aren't a table are the user's alone.
    let options = launched_options(dir.path(), &rust_analyzer_settings(json!("as given")));
    assert_eq!(options, json!("as given"));

    // A host's spec of that name is the host's alone.
    let host = ServerSpec {
        languages: vec!["rust".into()],
        ..ServerSpec::new("rust-analyzer", "/opt/ra")
    };
    let spec = launched(dir.path(), &LspSettings::default(), Some(host));
    assert_eq!(spec.initialization_options, None);
}
