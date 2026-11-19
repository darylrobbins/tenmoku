//! Claude Agent's security profile, the inherited Claude Code settings
//! check and the states, against the fake agent in-process over
//! `Channel::duplex()` (decision record § 12).
//!
//! Each test makes a temporary repository with a lane worktree, and a fake
//! home and managed-settings directories, so nothing on the machine running
//! the tests is read.

#[allow(dead_code, unreachable_pub)]
#[path = "../src/bin/fe-acp-fake-agent.rs"]
mod fake;

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use fe_acp::claude::{self, ALWAYS_ASK, CLAUDE_TOOLS, MCP_ASK_RULE, ToolClass};
use fe_acp::claude_settings::{
    self, Environment, Loosened, MANAGED_PREFERENCES_DOMAIN, Source, SourceKind,
};
use fe_acp::schema::{ContentBlock, Meta, SessionId, StopReason};
use fe_acp::{AgentState, Channel, ClaudeAgent, Incoming, Lane, LaneDirs};
use fe_config::agent_settings::{AgentLevel, AgentSettings};
use futures::channel::mpsc;
use futures::{FutureExt as _, StreamExt as _};
use serde_json::{Value, json};
use tempfile::TempDir;

const TIMEOUT: Duration = Duration::from_secs(30);

// ---------------------------------------------------------------------------
// Fixtures

fn git(dir: &Path, args: &[&str]) {
    let output = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_AUTHOR_NAME", "Test")
        .env("GIT_AUTHOR_EMAIL", "test@example.com")
        .env("GIT_COMMITTER_NAME", "Test")
        .env("GIT_COMMITTER_EMAIL", "test@example.com")
        .output()
        .expect("running git");
    assert!(
        output.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

/// A temporary main checkout `repo` with one commit, and a fake home and
/// managed directories beside it.
struct Fixture {
    dir: TempDir,
    /// The main checkout.
    repo: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let dir = tempfile::tempdir().expect("tempdir");
        // Without Windows' verbatim `\\?\` prefix, where git can't add a
        // worktree.
        let base = dunce::canonicalize(dir.path()).unwrap();
        let repo = base.join("repo");
        std::fs::create_dir_all(&repo).unwrap();
        git(&repo, &["init", "-q", "-b", "main"]);
        std::fs::write(repo.join("notes.txt"), "notes\n").unwrap();
        git(&repo, &["add", "notes.txt"]);
        git(&repo, &["commit", "-q", "-m", "first"]);
        for sub in ["home/.claude", "managed", "prefs"] {
            std::fs::create_dir_all(base.join(sub)).unwrap();
        }
        Self { dir, repo }
    }

    fn base(&self) -> PathBuf {
        dunce::canonicalize(self.dir.path()).unwrap()
    }

    /// A linked worktree of the main checkout at `<base>/<name>`.
    fn worktree(&self, name: &str) -> PathBuf {
        let path = self.base().join(name);
        git(
            &self.repo,
            &["worktree", "add", "-q", "-b", name, path.to_str().unwrap()],
        );
        path
    }

    /// A lane: the main checkout as the root, with a lane worktree.
    fn lane(&self) -> Lane {
        Lane {
            root: self.repo.clone(),
            worktree: self.worktree("lane"),
        }
    }

    fn environment(&self) -> Environment {
        let base = self.base();
        Environment {
            managed_dirs: vec![base.join("managed")],
            managed_preferences: vec![base.join("prefs")],
            // Joined by component, as the settings check joins
            // `settings.json`, so its message names the path as
            // `user_settings` writes it on Windows too.
            user_config_dir: Some(base.join("home").join(".claude")),
            uninspectable_platform: None,
        }
    }

    fn record(&self) -> PathBuf {
        self.base().join("record.jsonl")
    }

    fn user_settings(&self) -> PathBuf {
        self.base()
            .join("home")
            .join(".claude")
            .join("settings.json")
    }

    fn drop_in(&self, name: &str) -> PathBuf {
        let dir = self.base().join("managed/managed-settings.d");
        std::fs::create_dir_all(&dir).unwrap();
        dir.join(name)
    }
}

fn write(path: &Path, value: &Value) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, serde_json::to_string_pretty(value).unwrap()).unwrap();
}

fn write_text(path: &Path, text: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

/// What the fake recorded: `(method, params)`, in order.
fn recorded(path: &Path) -> Vec<(String, Value)> {
    let Ok(text) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    text.lines()
        .map(|line| {
            let value: Value = serde_json::from_str(line).unwrap();
            (
                value["method"].as_str().unwrap().to_owned(),
                value["params"].clone(),
            )
        })
        .collect()
}

fn methods(path: &Path) -> Vec<String> {
    recorded(path)
        .into_iter()
        .map(|(method, _)| method)
        .collect()
}

/// The `_meta` of every recorded `method`, as sent: its JSON text.
fn metas(path: &Path, method: &str) -> Vec<String> {
    recorded(path)
        .into_iter()
        .filter(|(m, _)| m == method)
        .map(|(_, params)| serde_json::to_string(&params["_meta"]).unwrap())
        .collect()
}

fn meta_text(meta: &Meta) -> String {
    serde_json::to_string(meta).unwrap()
}

fn text(s: &str) -> Vec<ContentBlock> {
    vec![ContentBlock::from(s)]
}

/// Runs `body` with a [`ClaudeAgent`] connected to the fake playing
/// `script`, both connections driven alongside it.
fn run<F>(
    environment: Environment,
    settings: AgentSettings,
    script: fake::Script,
    body: impl FnOnce(ClaudeAgent, mpsc::UnboundedReceiver<Incoming>) -> F,
) where
    F: Future<Output = ()>,
{
    let (client_end, agent_end) = Channel::duplex();
    let (client, incoming, driver) = fe_acp::connect(client_end);
    let agent = ClaudeAgent::new(client, None, environment, "git", settings);
    let served = fake::serve(agent_end, script);
    let body = {
        let agent = agent.clone();
        async move {
            body(agent.clone(), incoming).await;
            agent.client().shutdown();
        }
    };
    let all = async {
        let ((), driven, served) = futures::join!(body, driver, served);
        driven.expect("the client's connection");
        // A test that ends the connection mid-request leaves the fake's
        // side failing; what it recorded is what's checked.
        if let Err(err) = served {
            eprintln!("the fake agent's connection: {err}");
        }
    };
    async_io::block_on(async {
        futures::select! {
            () = all.fuse() => {}
            _ = futures::FutureExt::fuse(async_io::Timer::after(TIMEOUT)) => panic!("timed out"),
        }
    });
}

fn script(fixture: &Fixture) -> fake::Script {
    fake::Script {
        record: Some(fixture.record()),
        ..fake::Script::default()
    }
}

fn options(meta: &Meta) -> &Value {
    &meta["claudeCode"]["options"]
}

fn strings(value: &Value) -> Vec<String> {
    value
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap().to_owned())
        .collect()
}

fn path_text(path: &Path) -> String {
    path.to_str().unwrap().to_owned()
}

/// A path as the profile writes it: as it's written, except on Windows,
/// where it's `/c/Users/…`, the POSIX form Claude Code matches rules
/// against.
#[cfg(not(windows))]
fn profile_text(path: &Path) -> String {
    path_text(path)
}

/// A path as the profile writes it: as it's written, except on Windows,
/// where it's `/c/Users/…`, the POSIX form Claude Code matches rules
/// against.
#[cfg(windows)]
fn profile_text(path: &Path) -> String {
    let text = path_text(path).replace('\\', "/");
    let (drive, rest) = text.split_once(":/").expect("a path on a drive");
    format!("/{}/{rest}", drive.to_ascii_lowercase())
}

/// An existing directory outside the fixture, as a user would write it:
/// `/var/tmp` (on macOS a link to `/private/var/tmp`), or the temp
/// directory on Windows.
fn outside_dir() -> PathBuf {
    if cfg!(windows) {
        std::env::temp_dir()
    } else {
        PathBuf::from("/var/tmp")
    }
}

// ---------------------------------------------------------------------------
// The profile on new, load and resume

#[test]
fn the_profile_is_byte_identical_on_new_load_and_resume() {
    let fixture = Fixture::new();
    let lane = fixture.lane();
    run(
        fixture.environment(),
        AgentSettings::default(),
        script(&fixture),
        async |agent: ClaudeAgent, _incoming| {
            agent.initialize().await.expect("initialize");
            let id = agent.new_session(&lane).await.expect("new").session_id;
            agent.load_session(id.clone(), &lane).await.expect("load");
            agent.resume_session(id, &lane).await.expect("resume");
        },
    );
    let new = metas(&fixture.record(), "session/new");
    let load = metas(&fixture.record(), "session/load");
    let resume = metas(&fixture.record(), "session/resume");
    assert_eq!(new.len(), 1);
    assert_eq!(new, load, "load sends the same bytes as new");
    assert_eq!(new, resume, "resume sends the same bytes as new");

    // And they are the profile's, for this lane.
    let dirs = LaneDirs {
        root: lane.root.clone(),
        worktree: lane.worktree.clone(),
        common_dir: fixture.repo.join(".git"),
        worktree_admin_dir: fixture.repo.join(".git/worktrees/lane"),
        root_admin_dir: fixture.repo.join(".git"),
    };
    let profile = claude::profile(&AgentSettings::default(), &dirs);
    assert_eq!(new[0], meta_text(&profile));

    // The cwd is the worktree, never the root.
    for (method, params) in recorded(&fixture.record()) {
        if method.starts_with("session/") && params.get("cwd").is_some() {
            assert_eq!(params["cwd"], json!(path_text(&lane.worktree)), "{method}");
        }
    }
}

#[test]
fn the_profile_holds_the_mode_tools_asks_deny_rules_and_sandbox() {
    let fixture = Fixture::new();
    let lane = fixture.lane();
    let mut settings = AgentSettings::default();
    settings.claude_code.sandbox_allow_write = vec![
        path_text(&outside_dir().join("agent-cache")),
        // Inside the root, relative, and inside .git: never passed.
        path_text(&fixture.repo.join("src")),
        "relative/path".into(),
        path_text(&fixture.repo.join(".git/hooks")),
    ];
    let dirs = async_io::block_on(LaneDirs::resolve(Path::new("git"), &lane)).expect("dirs");
    let meta = claude::profile(&settings, &dirs);
    let options = options(&meta);
    assert_eq!(options["permissionMode"], json!("default"));
    assert_eq!(options["allowDangerouslySkipPermissions"], json!(false));
    assert_eq!(options["strictMcpConfig"], json!(true));
    // Defaults: read, search and propose_edit allow; run_command, fetch and
    // other ask.
    assert_eq!(
        strings(&options["tools"]),
        CLAUDE_TOOLS
            .iter()
            .map(|(n, _)| (*n).to_owned())
            .collect::<Vec<_>>()
    );
    assert_eq!(
        strings(&options["settings"]["permissions"]["ask"]),
        [
            "Edit",
            "Write",
            "NotebookEdit",
            "Bash",
            "WebFetch",
            "WebSearch"
        ]
    );
    let deny = strings(&options["settings"]["permissions"]["deny"]);
    for anchor in [&lane.root, &lane.worktree] {
        for rule in ["Read", "Edit"] {
            for pattern in ["**/.env*", "**/id_*", "**/*.pem", "**/*.key", "**/.git/**"] {
                let expected = format!("{rule}(/{}/{pattern})", profile_text(anchor));
                assert!(expected.starts_with(&format!("{rule}(//")));
                assert!(deny.contains(&expected), "{expected} in {deny:?}");
            }
        }
    }
    let sandbox = &options["sandbox"];
    assert_eq!(sandbox["enabled"], json!(true));
    assert_eq!(sandbox["failIfUnavailable"], json!(true));
    assert_eq!(sandbox["autoAllowBashIfSandboxed"], json!(false));
    assert_eq!(sandbox["allowUnsandboxedCommands"], json!(false));
    // Passed as its real path, as the overlap check resolves it: on macOS
    // /var is a link to /private/var.
    let real = dunce::canonicalize(outside_dir())
        .unwrap()
        .join("agent-cache");
    assert_eq!(
        strings(&sandbox["filesystem"]["allowWrite"]),
        [profile_text(&real)]
    );
}

#[test]
fn a_changed_setting_reloads_before_the_next_prompt() {
    let fixture = Fixture::new();
    let lane = fixture.lane();
    let record = fixture.record();
    run(
        fixture.environment(),
        AgentSettings::default(),
        script(&fixture),
        async |agent: ClaudeAgent, _incoming| {
            agent.initialize().await.expect("initialize");
            let id = agent.new_session(&lane).await.expect("new").session_id;
            agent.prompt(id.clone(), text("one")).await.expect("one");
            assert_eq!(
                methods(&record)
                    .iter()
                    .filter(|m| *m == "session/load")
                    .count(),
                0,
                "an unchanged profile isn't re-sent"
            );

            let mut settings = AgentSettings::default();
            settings.permissions.fetch = AgentLevel::Deny;
            agent.set_settings(settings);
            agent.prompt(id.clone(), text("two")).await.expect("two");
            agent.prompt(id, text("three")).await.expect("three");
        },
    );
    let methods = methods(&record);
    let sends: Vec<&str> = methods
        .iter()
        .map(String::as_str)
        .filter(|m| matches!(*m, "session/load" | "session/prompt"))
        .collect();
    assert_eq!(
        sends,
        [
            "session/prompt",
            "session/load",
            "session/prompt",
            "session/prompt"
        ],
        "one re-load, before the first prompt after the change"
    );
    let new = metas(&record, "session/new");
    let load = metas(&record, "session/load");
    assert_ne!(new, load, "the load carries the new profile");
    let load: Value = serde_json::from_str(&load[0]).unwrap();
    let tools = strings(&load["claudeCode"]["options"]["tools"]);
    assert!(!tools.contains(&"WebFetch".to_owned()), "{tools:?}");
}

// ---------------------------------------------------------------------------
// Class levels

fn profile_with(edit: impl FnOnce(&mut AgentSettings)) -> Meta {
    let mut settings = AgentSettings::default();
    edit(&mut settings);
    let dirs = LaneDirs {
        root: "/work/root".into(),
        worktree: "/work/lane".into(),
        common_dir: "/work/root/.git".into(),
        worktree_admin_dir: "/work/root/.git/worktrees/lane".into(),
        root_admin_dir: "/work/root/.git".into(),
    };
    claude::profile(&settings, &dirs)
}

#[test]
fn class_levels_map_to_tools_and_ask_rules() {
    type Set = fn(&mut AgentSettings, AgentLevel);
    let classes: [(ToolClass, Set); 5] = [
        (ToolClass::Read, |s, l| s.permissions.read = l),
        (ToolClass::Search, |s, l| s.permissions.search = l),
        (ToolClass::ProposeEdit, |s, l| {
            s.permissions.propose_edit = l
        }),
        (ToolClass::RunCommand, |s, l| s.permissions.run_command = l),
        (ToolClass::Fetch, |s, l| s.permissions.fetch = l),
    ];
    for (class, set) in classes {
        let names: Vec<String> = CLAUDE_TOOLS
            .iter()
            .filter(|(_, c)| *c == class)
            .map(|(n, _)| (*n).to_owned())
            .collect();
        for level in [AgentLevel::Allow, AgentLevel::Ask, AgentLevel::Deny] {
            let meta = profile_with(|s| {
                // Everything else allowed, so only this class shows.
                for (_, other) in &classes {
                    other(s, AgentLevel::Allow);
                }
                set(s, level);
            });
            let tools = strings(&options(&meta)["tools"]);
            let ask = strings(&options(&meta)["settings"]["permissions"]["ask"]);
            for name in &names {
                let always = ALWAYS_ASK.contains(&name.as_str());
                match level {
                    AgentLevel::Deny => {
                        assert!(!tools.contains(name), "{class:?} deny: {name} in {tools:?}");
                    }
                    AgentLevel::Ask => {
                        assert!(tools.contains(name), "{class:?} ask: {name} missing");
                        assert!(ask.contains(name), "{class:?} ask: {name} not asked");
                    }
                    AgentLevel::Allow => {
                        assert!(tools.contains(name), "{class:?} allow: {name} missing");
                        assert_eq!(ask.contains(name), always, "{class:?} allow: {name}");
                    }
                }
            }
            // Edits and commands are always asked about, whatever the
            // levels; tools of no class are always there.
            for name in ALWAYS_ASK {
                assert!(ask.contains(&name.to_owned()), "{name} always asks");
            }
            for (name, c) in CLAUDE_TOOLS {
                if c == ToolClass::None {
                    assert!(tools.contains(&name.to_owned()), "{name} has no class");
                }
            }
        }
    }
}

#[test]
fn mcp_servers_follow_the_opt_in_and_the_other_class() {
    let cases = [
        (false, AgentLevel::Ask, true, false),
        (true, AgentLevel::Ask, false, true),
        (true, AgentLevel::Allow, false, false),
        (true, AgentLevel::Deny, true, false),
        (false, AgentLevel::Allow, true, false),
    ];
    for (mcp_servers, other, strict, asked) in cases {
        let meta = profile_with(|s| {
            s.claude_code.mcp_servers = mcp_servers;
            s.permissions.other = other;
        });
        let options = options(&meta);
        assert_eq!(
            options["strictMcpConfig"],
            json!(strict),
            "mcp_servers = {mcp_servers}, other = {other:?}"
        );
        let ask = strings(&options["settings"]["permissions"]["ask"]);
        assert_eq!(
            ask.contains(&MCP_ASK_RULE.to_owned()),
            asked,
            "mcp_servers = {mcp_servers}, other = {other:?}: {ask:?}"
        );
        assert!(options.get("mcpServers").is_none());
    }
}

// ---------------------------------------------------------------------------
// The git directories

#[test]
fn a_main_checkout_root_denies_the_common_and_lane_admin_dirs() {
    let fixture = Fixture::new();
    let lane = fixture.lane();
    let dirs = async_io::block_on(LaneDirs::resolve(Path::new("git"), &lane)).expect("dirs");
    assert_eq!(
        dirs.deny_write(),
        [
            fixture.repo.join(".git"),
            fixture.repo.join(".git/worktrees/lane")
        ]
    );
    assert_eq!(dirs.main_checkout(), fixture.repo);
    let meta = claude::profile(&AgentSettings::default(), &dirs);
    assert_eq!(
        strings(&options(&meta)["sandbox"]["filesystem"]["denyWrite"]),
        [
            profile_text(&fixture.repo.join(".git")),
            profile_text(&fixture.repo.join(".git/worktrees/lane"))
        ]
    );
}

#[test]
fn a_linked_worktree_root_also_denies_its_own_admin_dir() {
    let fixture = Fixture::new();
    let root = fixture.worktree("opened");
    let lane = Lane {
        root: root.clone(),
        worktree: fixture.worktree("lane"),
    };
    let dirs = async_io::block_on(LaneDirs::resolve(Path::new("git"), &lane)).expect("dirs");
    assert_eq!(
        dirs.deny_write(),
        [
            fixture.repo.join(".git"),
            fixture.repo.join(".git/worktrees/lane"),
            fixture.repo.join(".git/worktrees/opened"),
        ]
    );
    assert_eq!(dirs.main_checkout(), fixture.repo, "from the common dir");
}

#[test]
fn a_rev_parse_failure_refuses_the_session() {
    let fixture = Fixture::new();
    let plain = fixture.base().join("plain");
    std::fs::create_dir_all(&plain).unwrap();
    // The worktree isn't in a repository.
    let lane = Lane {
        root: fixture.repo.clone(),
        worktree: plain,
    };
    let refused = async_io::block_on(LaneDirs::resolve(Path::new("git"), &lane));
    assert!(
        matches!(
            refused,
            Err(AgentState::SandboxLoosened(Loosened::Git { .. }))
        ),
        "{refused:?}"
    );
    // No git at all.
    let lane = fixture.lane();
    let refused = async_io::block_on(LaneDirs::resolve(
        &fixture.base().join("no-such-git"),
        &lane,
    ));
    assert!(
        matches!(
            refused,
            Err(AgentState::SandboxLoosened(Loosened::Git { .. }))
        ),
        "{refused:?}"
    );
    // And the session doesn't start: nothing reaches the agent.
    let record = fixture.record();
    run(
        fixture.environment(),
        AgentSettings::default(),
        script(&fixture),
        async |agent: ClaudeAgent, _incoming| {
            let broken = Lane {
                root: fixture.repo.clone(),
                worktree: fixture.base().join("missing"),
            };
            let refused = agent.new_session(&broken).await;
            assert!(
                matches!(
                    refused,
                    Err(AgentState::SandboxLoosened(Loosened::Git { .. }))
                ),
                "{refused:?}"
            );
        },
    );
    assert!(!methods(&record).contains(&"session/new".to_owned()));
}

// ---------------------------------------------------------------------------
// The inherited-settings check

/// Every source of the table a test can write, for a lane whose root is
/// the main checkout.
fn writable_sources(fixture: &Fixture, lane: &Lane) -> Vec<Source> {
    let source = |kind, path: PathBuf| Source { kind, path };
    let base = fixture.base();
    vec![
        source(
            SourceKind::ManagedFile,
            base.join("managed/managed-settings.json"),
        ),
        source(SourceKind::ManagedDropIn, fixture.drop_in("50-policy.json")),
        source(SourceKind::User, fixture.user_settings()),
        source(
            SourceKind::WorktreeProject,
            lane.worktree.join(".claude/settings.json"),
        ),
        source(
            SourceKind::WorktreeLocal,
            lane.worktree.join(".claude/settings.local.json"),
        ),
        source(
            SourceKind::MainCheckoutLocal,
            fixture.repo.join(".claude/settings.local.json"),
        ),
        source(
            SourceKind::RootProject,
            fixture.repo.join(".claude/settings.json"),
        ),
    ]
}

fn check(fixture: &Fixture, lane: &Lane) -> Result<(), Loosened> {
    claude_settings::SettingsCheck::new(
        fixture.environment(),
        &lane.worktree,
        &lane.root,
        &fixture.repo,
    )
    .check()
}

/// The widening cases of § 12, each with the key it must be refused for.
fn widening_cases() -> Vec<(Value, &'static str)> {
    vec![
        (
            json!({"sandbox": {"excludedCommands": ["touch *"]}}),
            "sandbox.excludedCommands",
        ),
        (
            json!({"sandbox": {"filesystem": {"allowWrite": ["/tmp"]}}}),
            "sandbox.filesystem.allowWrite",
        ),
        (
            json!({"sandbox": {"filesystem": {"disabled": true}}}),
            "sandbox.filesystem.disabled",
        ),
        (json!({"sandbox": {"enabled": false}}), "sandbox.enabled"),
        (
            json!({"sandbox": {"allowUnsandboxedCommands": true}}),
            "sandbox.allowUnsandboxedCommands",
        ),
        (
            json!({"sandbox": {"network": {"allowUnixSockets": ["/var/run/docker.sock"]}}}),
            "sandbox.network.allowUnixSockets",
        ),
        (
            json!({"sandbox": {"network": {"allowAllUnixSockets": true}}}),
            "sandbox.network.allowAllUnixSockets",
        ),
        (
            json!({"sandbox": {"network": {"allowLocalBinding": true}}}),
            "sandbox.network.allowLocalBinding",
        ),
        (
            json!({"sandbox": {"network": {"allowedDomains": ["docs.rs", "localhost"]}}}),
            "sandbox.network.allowedDomains",
        ),
        (
            json!({"sandbox": {"network": {"allowedDomains": ["127.0.0.1"]}}}),
            "sandbox.network.allowedDomains",
        ),
        (
            json!({"sandbox": {"somethingNew": 1}}),
            "sandbox.somethingNew",
        ),
        (
            json!({"permissions": {"additionalDirectories": ["/tmp"]}}),
            "permissions.additionalDirectories",
        ),
        (
            json!({"permissions": {"allow": ["Edit"]}}),
            "permissions.allow: Edit",
        ),
        (
            json!({"permissions": {"allow": ["Edit(//etc/**)"]}}),
            "permissions.allow: Edit(//etc/**)",
        ),
        (
            json!({"permissions": {"allow": ["Edit(~/x)"]}}),
            "permissions.allow: Edit(~/x)",
        ),
        (
            json!({"permissions": {"allow": ["Write(../x)"]}}),
            "permissions.allow: Write(../x)",
        ),
        (
            json!({"permissions": {"allow": ["WebFetch(domain:localhost)"]}}),
            "permissions.allow: WebFetch(domain:localhost)",
        ),
        (
            json!({"permissions": {"allow": ["WebFetch(domain:127.0.0.1)"]}}),
            "permissions.allow: WebFetch(domain:127.0.0.1)",
        ),
        (
            json!({"permissions": {"allow": ["WebFetch(domain:*)"]}}),
            "permissions.allow: WebFetch(domain:*)",
        ),
        (
            json!({"permissions": {"allow": ["WebFetch"]}}),
            "permissions.allow: WebFetch",
        ),
    ]
}

#[test]
fn each_widening_case_refuses_in_each_source_naming_file_and_key() {
    let fixture = Fixture::new();
    let lane = fixture.lane();
    assert_eq!(check(&fixture, &lane), Ok(()), "nothing planted");
    for source in writable_sources(&fixture, &lane) {
        for (value, key) in widening_cases() {
            write(&source.path, &value);
            assert_eq!(
                check(&fixture, &lane),
                Err(Loosened::Widening {
                    file: source.path.clone(),
                    key: key.to_owned()
                }),
                "{value} in {:?}",
                source.kind
            );
            std::fs::remove_file(&source.path).unwrap();
        }
    }
    assert_eq!(check(&fixture, &lane), Ok(()));
}

#[test]
fn each_accepted_key_passes_in_each_source() {
    let fixture = Fixture::new();
    let lane = fixture.lane();
    let accepted = json!({
        "env": {"ANTHROPIC_API_KEY": "sk-secret"},
        "apiKeyHelper": "/usr/local/bin/key",
        "hooks": {},
        "model": "opus",
        "sandbox": {
            "enabled": true,
            "failIfUnavailable": true,
            "allowUnsandboxedCommands": false,
            "autoAllowBashIfSandboxed": false,
            "filesystem": {"denyWrite": ["/etc"], "denyRead": ["~/.ssh"]},
            "network": {
                "deniedDomains": ["evil.example"],
                "strictAllowlist": true,
                "allowManagedDomainsOnly": true,
                "allowedDomains": ["docs.rs", "*.crates.io"],
            },
        },
        "permissions": {
            "allow": ["Bash(cargo test)", "Read", "Read(//etc/**)", "WebFetch(domain:docs.rs)", "mcp__github__*"],
            "additionalDirectories": [],
            "deny": ["Bash(rm *)"],
            "ask": ["Bash"],
            "defaultMode": "plan",
        },
    });
    for source in writable_sources(&fixture, &lane) {
        write(&source.path, &accepted);
        assert_eq!(check(&fixture, &lane), Ok(()), "{:?}", source.kind);
        std::fs::remove_file(&source.path).unwrap();
    }
}

#[test]
fn a_relative_edit_rule_is_refused_in_the_users_file_and_accepted_in_a_project_file() {
    let fixture = Fixture::new();
    let lane = fixture.lane();
    let rule = json!({"permissions": {"allow": ["Edit(src/**)"]}});
    for source in writable_sources(&fixture, &lane) {
        write(&source.path, &rule);
        let result = check(&fixture, &lane);
        if source.kind.is_project() {
            assert_eq!(result, Ok(()), "{:?}", source.kind);
        } else {
            assert_eq!(
                result,
                Err(Loosened::Widening {
                    file: source.path.clone(),
                    key: "permissions.allow: Edit(src/**)".into()
                }),
                "{:?}",
                source.kind
            );
        }
        std::fs::remove_file(&source.path).unwrap();
    }
}

#[test]
fn network_grants_refuse_loopback_and_wildcards_and_accept_named_domains() {
    for domain in [
        "localhost",
        "LOCALHOST",
        "localhost:8080",
        "api.localhost",
        "127.0.0.1",
        "127.0.0.1:3000",
        "127.8.9.10",
        "127.1",
        "2130706433",
        "0x7f000001",
        "0.0.0.0",
        "::1",
        "[::1]",
        "[::1]:80",
        "::ffff:127.0.0.1",
        "*",
        "*.localhost",
        "127.*",
        "*.0.0.1",
        "",
    ] {
        assert!(
            !claude_settings::allow_rule_tightens(&format!("WebFetch(domain:{domain})"), true),
            "{domain:?}"
        );
        assert!(claude_settings::could_be_loopback(domain), "{domain:?}");
    }
    for domain in [
        "docs.rs",
        "*.crates.io",
        "example.com:443",
        "10.0.0.1",
        "1.2.3.4",
    ] {
        assert!(
            claude_settings::allow_rule_tightens(&format!("WebFetch(domain:{domain})"), false),
            "{domain:?}"
        );
    }
    assert!(!claude_settings::allow_rule_tightens("WebFetch", true));
    assert!(!claude_settings::allow_rule_tightens(
        "WebFetch(https://x.example)",
        true
    ));
}

#[test]
fn managed_preferences_refuse_as_uninspectable() {
    let fixture = Fixture::new();
    let lane = fixture.lane();
    let plist = format!("{MANAGED_PREFERENCES_DOMAIN}.plist");
    for path in [
        fixture.base().join("prefs").join(&plist),
        fixture.base().join("prefs/someone").join(&plist),
    ] {
        write_text(&path, "<plist/>");
        let refused = check(&fixture, &lane);
        assert_eq!(refused, Err(Loosened::Uninspectable { path: path.clone() }));
        assert!(
            refused.unwrap_err().to_string().contains(
                "managed preferences (a configuration profile) that Tenmoku can't inspect"
            )
        );
        std::fs::remove_file(&path).unwrap();
    }
    assert_eq!(check(&fixture, &lane), Ok(()));
}

#[test]
fn a_file_that_does_not_parse_refuses() {
    let fixture = Fixture::new();
    let lane = fixture.lane();
    for source in writable_sources(&fixture, &lane) {
        for text in ["{ not json", "[1, 2]"] {
            write_text(&source.path, text);
            let refused = check(&fixture, &lane);
            assert!(
                matches!(&refused, Err(Loosened::Unparsable { file, .. }) if *file == source.path),
                "{text} in {:?}: {refused:?}",
                source.kind
            );
            std::fs::remove_file(&source.path).unwrap();
        }
    }
}

#[test]
fn keys_outside_the_three_are_never_kept() {
    let fixture = Fixture::new();
    let path = fixture.user_settings();
    write(
        &path,
        &json!({
            "env": {"ANTHROPIC_API_KEY": "sk-ant-very-secret"},
            "apiKeyHelper": "print-the-secret-key",
            "permissions": {"allow": ["Read"], "deny": ["Bash"], "additionalDirectories": []},
            "sandbox": {"enabled": true},
        }),
    );
    let kept = claude_settings::read(&path).unwrap().unwrap();
    let shown = format!("{kept:?}");
    for secret in [
        "sk-ant-very-secret",
        "print-the-secret-key",
        "ANTHROPIC_API_KEY",
        "\"Bash\"",
    ] {
        assert!(!shown.contains(secret), "{secret} kept: {shown}");
    }
    assert_eq!(kept.allow, Some(json!(["Read"])));
    assert_eq!(kept.additional_directories, Some(json!([])));
    assert_eq!(kept.sandbox, Some(json!({"enabled": true})));
}

#[test]
fn managed_rules_only_refuses_in_each_managed_source_and_names_the_setting() {
    let fixture = Fixture::new();
    let lane = fixture.lane();
    let managed = [
        fixture.base().join("managed/managed-settings.json"),
        fixture.drop_in("10-rules-only.json"),
    ];
    for path in managed {
        for value in [
            json!({"allowManagedPermissionRulesOnly": true}),
            json!({"permissions": {"allowManagedPermissionRulesOnly": true}}),
            json!({
                "allowManagedPermissionRulesOnly": false,
                "permissions": {"allowManagedPermissionRulesOnly": true},
            }),
        ] {
            write(&path, &value);
            let refused = check(&fixture, &lane);
            assert_eq!(
                refused,
                Err(Loosened::ManagedRulesOnly { file: path.clone() }),
                "{value} in {}",
                path.display()
            );
            assert!(
                refused
                    .unwrap_err()
                    .to_string()
                    .contains("allowManagedPermissionRulesOnly")
            );
            std::fs::remove_file(&path).unwrap();
        }
        write(&path, &json!({"allowManagedPermissionRulesOnly": false}));
        assert_eq!(check(&fixture, &lane), Ok(()));
        std::fs::remove_file(&path).unwrap();
    }

    // Claude Agent refuses to start on it.
    write(
        &fixture.drop_in("20-rules-only.json"),
        &json!({"allowManagedPermissionRulesOnly": true}),
    );
    let record = fixture.record();
    run(
        fixture.environment(),
        AgentSettings::default(),
        script(&fixture),
        async |agent: ClaudeAgent, _incoming| {
            agent.initialize().await.expect("initialize");
            let refused = agent.new_session(&lane).await;
            assert_eq!(
                refused.unwrap_err(),
                AgentState::SandboxLoosened(Loosened::ManagedRulesOnly {
                    file: fixture.drop_in("20-rules-only.json")
                })
            );
        },
    );
    assert!(!methods(&record).contains(&"session/new".to_owned()));
}

#[test]
fn a_linked_roots_main_checkout_local_file_is_checked_and_its_own_is_not() {
    let fixture = Fixture::new();
    let root = fixture.worktree("opened");
    let lane = Lane {
        root: root.clone(),
        worktree: fixture.worktree("lane"),
    };
    let dirs = async_io::block_on(LaneDirs::resolve(Path::new("git"), &lane)).expect("dirs");
    let check = claude_settings::SettingsCheck::new(
        fixture.environment(),
        &dirs.worktree,
        &dirs.root,
        dirs.main_checkout(),
    );
    let widening = json!({"permissions": {"allow": ["Edit(//tmp/**)"]}});

    // In the linked root's own local file: not the main checkout's.
    let own = root.join(".claude/settings.local.json");
    write(&own, &widening);
    assert_eq!(check.check(), Ok(()));
    assert!(!check.watched().contains(&own));
    std::fs::remove_file(&own).unwrap();

    // In the main checkout's: refused, and watched.
    let main = fixture.repo.join(".claude/settings.local.json");
    write(&main, &widening);
    assert_eq!(
        check.check(),
        Err(Loosened::Widening {
            file: main.clone(),
            key: "permissions.allow: Edit(//tmp/**)".into()
        })
    );
    assert!(check.watched().contains(&main));
    // The opened root's settings.json stays the Update-lane source.
    assert!(
        check
            .watched()
            .contains(&root.join(".claude/settings.json"))
    );
}

#[test]
fn the_check_watches_every_source_and_the_drop_in_and_preferences_directories() {
    let fixture = Fixture::new();
    let lane = fixture.lane();
    write_text(&fixture.base().join("prefs/someone/.keep"), "");
    let check = claude_settings::SettingsCheck::new(
        fixture.environment(),
        &lane.worktree,
        &lane.root,
        &fixture.repo,
    );
    let watched = check.watched();
    let base = fixture.base();
    for path in [
        base.join("managed/managed-settings.json"),
        base.join("managed/managed-settings.d"),
        base.join("prefs"),
        base.join("prefs/someone"),
        fixture.user_settings(),
        lane.worktree.join(".claude/settings.json"),
        lane.worktree.join(".claude/settings.local.json"),
        fixture.repo.join(".claude/settings.local.json"),
        fixture.repo.join(".claude/settings.json"),
    ] {
        assert!(watched.contains(&path), "{} in {watched:?}", path.display());
    }
}

// ---------------------------------------------------------------------------
// Settings changed during a turn

fn waiting_turn(fixture: &Fixture) -> fake::Script {
    fake::Script {
        turns: vec![fake::Turn {
            permission: Some("bash-1".into()),
            wait_for_cancel: true,
            ..fake::Turn::default()
        }],
        ..script(fixture)
    }
}

/// Plants `widen` while a turn waits on a permission request, and calls
/// the watcher callback: the turn is cancelled, the request answered
/// `Cancelled`, and the next prompt refused.
fn a_widening_change_mid_turn_cancels(widen: impl FnOnce(&Fixture) -> PathBuf) {
    let fixture = Fixture::new();
    let lane = fixture.lane();
    let record = fixture.record();
    run(
        fixture.environment(),
        AgentSettings::default(),
        waiting_turn(&fixture),
        async |agent: ClaudeAgent, mut incoming| {
            agent.initialize().await.expect("initialize");
            let id = agent.new_session(&lane).await.expect("new").session_id;
            let turn = agent.prompt(id.clone(), text("go"));
            let watcher = async {
                let request = loop {
                    match incoming.next().await.expect("the stream") {
                        Incoming::Permission(request) => break request,
                        Incoming::Update { .. } => {}
                    }
                };
                // An unrelated, accepted change cancels nothing.
                write(
                    &fixture.user_settings(),
                    &json!({"permissions": {"allow": ["Read"]}}),
                );
                assert!(agent.settings_changed().await.is_empty());
                assert!(request.is_pending());

                let file = widen(&fixture);
                let refused = agent.settings_changed().await;
                assert_eq!(refused.len(), 1, "{refused:?}");
                assert_eq!(refused[0].0, id);
                assert!(
                    matches!(&refused[0].1, AgentState::SandboxLoosened(Loosened::Widening { file: f, .. }) if *f == file),
                    "{refused:?}"
                );
                assert!(!request.is_pending(), "answered Cancelled");
                file
            };
            let (response, file) = futures::join!(turn, watcher);
            assert_eq!(
                response.expect("the turn").stop_reason,
                StopReason::Cancelled
            );

            let refused = agent.prompt(id.clone(), text("again")).await;
            assert!(
                matches!(&refused, Err(AgentState::SandboxLoosened(Loosened::Widening { file: f, .. })) if *f == file),
                "{refused:?}"
            );
            assert!(agent.refused(&id).is_some());

            // Once the file no longer widens, prompts go again.
            std::fs::remove_file(&file).unwrap();
            agent
                .prompt(id.clone(), text("fixed"))
                .await
                .expect("fixed");
            assert_eq!(agent.refused(&id), None);
        },
    );
    let recorded = recorded(&record);
    let cancel = recorded
        .iter()
        .position(|(m, _)| m == "session/cancel")
        .expect("a cancel");
    let answer = recorded
        .iter()
        .position(|(m, _)| m == "answer:bash-1")
        .expect("an answer");
    assert!(cancel < answer);
    assert_eq!(recorded[answer].1, json!({"outcome": "cancelled"}));
    let prompts: Vec<_> = recorded
        .iter()
        .filter(|(m, _)| m == "session/prompt")
        .map(|(_, p)| p["prompt"][0]["text"].clone())
        .collect();
    assert_eq!(
        prompts,
        [json!("go"), json!("fixed")],
        "the refused prompt isn't sent"
    );
}

#[test]
fn a_settings_file_changed_to_widen_mid_turn_cancels_the_turn() {
    a_widening_change_mid_turn_cancels(|fixture| {
        let path = fixture.user_settings();
        write(
            &path,
            &json!({"sandbox": {"excludedCommands": ["touch *"]}}),
        );
        path
    });
}

#[test]
fn a_widening_drop_in_added_mid_turn_cancels_the_turn() {
    a_widening_change_mid_turn_cancels(|fixture| {
        let path = fixture.drop_in("90-late.json");
        write(&path, &json!({"permissions": {"allow": ["Edit"]}}));
        path
    });
}

// ---------------------------------------------------------------------------
// States

#[test]
fn state_not_logged_in_on_auth_required_for_new_or_a_prompt() {
    let fixture = Fixture::new();
    let lane = fixture.lane();
    let auth_required = || fake::ScriptError {
        code: -32000,
        message: "Authentication required".into(),
    };
    run(
        fixture.environment(),
        AgentSettings::default(),
        fake::Script {
            new_error: Some(auth_required()),
            ..script(&fixture)
        },
        async |agent: ClaudeAgent, _incoming| {
            agent.initialize().await.expect("initialize");
            assert_eq!(
                agent.new_session(&lane).await.unwrap_err(),
                AgentState::NotLoggedIn
            );
        },
    );
    let fixture = Fixture::new();
    let lane = fixture.lane();
    run(
        fixture.environment(),
        AgentSettings::default(),
        fake::Script {
            turns: vec![fake::Turn {
                error: Some(auth_required()),
                ..fake::Turn::default()
            }],
            ..script(&fixture)
        },
        async |agent: ClaudeAgent, _incoming| {
            agent.initialize().await.expect("initialize");
            let id = agent.new_session(&lane).await.expect("new").session_id;
            assert_eq!(
                agent.prompt(id, text("hi")).await.unwrap_err(),
                AgentState::NotLoggedIn
            );
        },
    );
}

#[test]
fn state_not_logged_in_on_an_auth_status_of_none() {
    let fixture = Fixture::new();
    let lane = fixture.lane();
    run(
        fixture.environment(),
        AgentSettings::default(),
        fake::Script {
            auth_status: Some("none".into()),
            ..script(&fixture)
        },
        async |agent: ClaudeAgent, _incoming| {
            agent.initialize().await.expect("initialize");
            assert_eq!(agent.auth_state(), None);
            agent.new_session(&lane).await.expect("new");
            assert_eq!(agent.auth_state(), Some(AgentState::NotLoggedIn));
            assert_eq!(agent.client().auth_kind(), Some(fe_acp::AuthKind::None));
        },
    );
    let fixture = Fixture::new();
    let lane = fixture.lane();
    run(
        fixture.environment(),
        AgentSettings::default(),
        fake::Script {
            auth_status: Some("account".into()),
            ..script(&fixture)
        },
        async |agent: ClaudeAgent, _incoming| {
            agent.initialize().await.expect("initialize");
            agent.new_session(&lane).await.expect("new");
            assert_eq!(agent.auth_state(), None);
            assert_eq!(agent.client().auth_kind(), Some(fe_acp::AuthKind::Account));
        },
    );
}

#[test]
fn state_adapter_too_old_below_the_floor() {
    for (version, ok) in [
        (Some("0.105.9"), false),
        (Some("0.9.200"), false),
        (Some("not a version"), false),
        (None, false),
        (Some("0.106.0"), true),
        (Some("0.106.1"), true),
        (Some("0.107.0-beta.1"), true),
        (Some("1.0.0"), true),
    ] {
        let fixture = Fixture::new();
        run(
            fixture.environment(),
            AgentSettings::default(),
            fake::Script {
                version: version.map(str::to_owned),
                ..script(&fixture)
            },
            async |agent: ClaudeAgent, _incoming| {
                let result = agent.initialize().await;
                if ok {
                    result.expect("initialize");
                } else {
                    assert_eq!(
                        result.unwrap_err(),
                        AgentState::AdapterTooOld {
                            version: version.map(str::to_owned)
                        }
                    );
                }
            },
        );
    }
}

#[test]
fn state_sandbox_unavailable_when_the_session_fails_on_it() {
    let fixture = Fixture::new();
    let lane = fixture.lane();
    let message =
        "Sandbox is unavailable: bubblewrap (bwrap) isn't installed, and failIfUnavailable is set";
    run(
        fixture.environment(),
        AgentSettings::default(),
        fake::Script {
            new_error: Some(fake::ScriptError {
                code: -32603,
                message: message.into(),
            }),
            ..script(&fixture)
        },
        async |agent: ClaudeAgent, _incoming| {
            agent.initialize().await.expect("initialize");
            let state = agent.new_session(&lane).await.unwrap_err();
            assert!(
                matches!(&state, AgentState::SandboxUnavailable { details } if details.contains("bubblewrap")),
                "{state:?}"
            );
        },
    );
}

#[test]
fn state_sandbox_loosened_names_the_file_and_key() {
    let fixture = Fixture::new();
    let lane = fixture.lane();
    write(
        &fixture.user_settings(),
        &json!({"sandbox": {"excludedCommands": ["touch *"]}}),
    );
    run(
        fixture.environment(),
        AgentSettings::default(),
        script(&fixture),
        async |agent: ClaudeAgent, _incoming| {
            agent.initialize().await.expect("initialize");
            let state = agent.new_session(&lane).await.unwrap_err();
            assert_eq!(
                state,
                AgentState::SandboxLoosened(Loosened::Widening {
                    file: fixture.user_settings(),
                    key: "sandbox.excludedCommands".into()
                })
            );
            let AgentState::SandboxLoosened(loosened) = state else {
                unreachable!()
            };
            let shown = loosened.to_string();
            assert!(shown.contains("`sandbox.excludedCommands`"), "{shown}");
            assert!(
                shown.contains(&path_text(&fixture.user_settings())),
                "{shown}"
            );
        },
    );
}

#[test]
fn state_not_a_git_repository() {
    let fixture = Fixture::new();
    let plain = fixture.base().join("plain");
    std::fs::create_dir_all(&plain).unwrap();
    let lane = Lane {
        root: plain.clone(),
        worktree: plain,
    };
    run(
        fixture.environment(),
        AgentSettings::default(),
        script(&fixture),
        async |agent: ClaudeAgent, _incoming| {
            agent.initialize().await.expect("initialize");
            assert_eq!(
                agent.new_session(&lane).await.unwrap_err(),
                AgentState::NotAGitRepository
            );
        },
    );
}

#[test]
fn state_disconnected_when_the_connection_ends() {
    let error = fe_acp::Error::internal_error().data(json!({"reason": "fe-acp/disconnected"}));
    assert_eq!(
        AgentState::from_error(&error),
        AgentState::Disconnected { stderr: Vec::new() }
    );
    let fixture = Fixture::new();
    let lane = fixture.lane();
    run(
        fixture.environment(),
        AgentSettings::default(),
        script(&fixture),
        async |agent: ClaudeAgent, _incoming| {
            agent.initialize().await.expect("initialize");
            let id = agent.new_session(&lane).await.expect("new").session_id;
            agent.client().shutdown();
            assert_eq!(
                agent.prompt(id, text("hi")).await.unwrap_err(),
                AgentState::Disconnected { stderr: Vec::new() }
            );
        },
    );
}

#[test]
fn cancelled_answers_after_cancel() {
    let fixture = Fixture::new();
    let lane = fixture.lane();
    let record = fixture.record();
    run(
        fixture.environment(),
        AgentSettings::default(),
        waiting_turn(&fixture),
        async |agent: ClaudeAgent, mut incoming| {
            agent.initialize().await.expect("initialize");
            let id: SessionId = agent.new_session(&lane).await.expect("new").session_id;
            let turn = agent.prompt(id.clone(), text("go"));
            let stop = async {
                let request = loop {
                    if let Incoming::Permission(request) = incoming.next().await.unwrap() {
                        break request;
                    }
                };
                agent.cancel(id.clone()).await.expect("cancel");
                assert!(!request.is_pending());
                // A late allow isn't sent.
                assert!(!request.select("allow-once"));
            };
            let (response, ()) = futures::join!(turn, stop);
            assert_eq!(response.expect("turn").stop_reason, StopReason::Cancelled);
        },
    );
    let answers: Vec<_> = recorded(&record)
        .into_iter()
        .filter(|(m, _)| m == "answer:bash-1")
        .collect();
    assert_eq!(answers.len(), 1);
    assert_eq!(answers[0].1, json!({"outcome": "cancelled"}));
}

// ---------------------------------------------------------------------------
// Review round 1 (F1–F5)

#[cfg(unix)]
#[test]
fn sandbox_allow_write_is_resolved_before_it_is_checked() {
    use std::os::unix::fs::symlink;
    let fixture = Fixture::new();
    let lane = fixture.lane();
    let base = fixture.base();
    std::fs::create_dir_all(fixture.repo.join("src")).unwrap();
    std::fs::create_dir_all(base.join("cache")).unwrap();
    symlink(fixture.repo.join("src"), base.join("into-root")).unwrap();
    symlink(base.join("cache"), base.join("to-cache")).unwrap();
    symlink(fixture.repo.join("not-yet"), base.join("dangling")).unwrap();
    symlink(&fixture.repo, base.join("root-link")).unwrap();
    let text = |path: PathBuf| path_text(&path);
    let mut settings = AgentSettings::default();
    settings.claude_code.sandbox_allow_write = vec![
        // `..` through a folder that doesn't exist, and one that does.
        format!("{}/elsewhere/../repo/src", path_text(&base)),
        format!("{}/cache/../repo/src", path_text(&base)),
        // A symlink into the root, and a dangling one that would be.
        text(base.join("into-root/x")),
        text(base.join("dangling")),
        // Out of a folder that doesn't exist and back, through a link
        // into the root (review round 2, F1).
        format!("{}/missing/../into-root/x", path_text(&base)),
        format!("{}/missing/deeper/../../root-link/src", path_text(&base)),
        // Above the root.
        text(base.clone()),
        // Into the git directories through `..`.
        format!("{}/cache/../repo/.git/hooks", path_text(&base)),
        // Allowed: `..` taken after the link, as the kernel does, and a
        // link to a folder outside; passed resolved.
        text(base.join("root-link/../cache/x")),
        text(base.join("to-cache/sub")),
        format!("{}/missing/../to-cache/y", path_text(&base)),
    ];
    let dirs = async_io::block_on(LaneDirs::resolve(Path::new("git"), &lane)).expect("dirs");
    let meta = claude::profile(&settings, &dirs);
    assert_eq!(
        strings(&options(&meta)["sandbox"]["filesystem"]["allowWrite"]),
        [
            text(base.join("cache/x")),
            text(base.join("cache/sub")),
            text(base.join("cache/y"))
        ]
    );
}

#[test]
fn wildcards_that_could_reach_loopback_are_refused_in_both_network_grants() {
    let fixture = Fixture::new();
    let lane = fixture.lane();
    let refused = [
        "127.2.*",
        "foo*.localhost",
        "*.localhost",
        "*.LocalHost.",
        "*.example.com:*",
        "*.example.com:80",
        "*.0x1",
        "*.1",
        "*.0177",
        "*",
        "*.*",
        "a.*.com",
        "*example.com",
        "localhost.*",
        "*.[::1]",
    ];
    let accepted = [
        "*.example.com",
        "*.crates.io",
        "*.example.com.",
        "*.ex-ample_1.org",
    ];
    let user = fixture.user_settings();
    for domain in refused {
        write(
            &user,
            &json!({"sandbox": {"network": {"allowedDomains": [domain]}}}),
        );
        assert_eq!(
            check(&fixture, &lane),
            Err(Loosened::Widening {
                file: user.clone(),
                key: "sandbox.network.allowedDomains".into()
            }),
            "allowedDomains {domain}"
        );
        let rule = format!("WebFetch(domain:{domain})");
        write(&user, &json!({"permissions": {"allow": [rule]}}));
        assert_eq!(
            check(&fixture, &lane),
            Err(Loosened::Widening {
                file: user.clone(),
                key: format!("permissions.allow: {rule}")
            }),
            "{rule}"
        );
    }
    for domain in accepted {
        write(
            &user,
            &json!({
                "sandbox": {"network": {"allowedDomains": [domain]}},
                "permissions": {"allow": [format!("WebFetch(domain:{domain})")]},
            }),
        );
        assert_eq!(check(&fixture, &lane), Ok(()), "{domain}");
    }
}

/// Makes `path` unreadable for the test's duration; `None` when that has
/// no effect (running as root).
#[cfg(unix)]
struct Unreadable(PathBuf);

#[cfg(unix)]
impl Unreadable {
    fn new(path: &Path) -> Option<Self> {
        use std::os::unix::fs::PermissionsExt as _;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o000)).unwrap();
        let guard = Self(path.to_owned());
        std::fs::read_dir(path).is_err().then_some(guard)
    }
}

#[cfg(unix)]
impl Drop for Unreadable {
    fn drop(&mut self) {
        use std::os::unix::fs::PermissionsExt as _;
        let _ = std::fs::set_permissions(&self.0, std::fs::Permissions::from_mode(0o755));
    }
}

#[cfg(unix)]
#[test]
fn directories_that_cannot_be_inspected_refuse() {
    let fixture = Fixture::new();
    let lane = fixture.lane();
    let base = fixture.base();

    // A drop-in directory that can't be listed.
    let drop_ins = fixture.drop_in("x.json").parent().unwrap().to_owned();
    if let Some(_guard) = Unreadable::new(&drop_ins) {
        assert!(
            matches!(check(&fixture, &lane), Err(Loosened::Unreadable { file, .. }) if file == drop_ins)
        );
    } else {
        eprintln!("running as root: permissions don't apply");
        return;
    }

    // The managed-preferences directory.
    let prefs = base.join("prefs");
    {
        let _guard = Unreadable::new(&prefs).unwrap();
        assert!(
            matches!(check(&fixture, &lane), Err(Loosened::Unreadable { file, .. }) if file == prefs)
        );
    }

    // A per-user directory the property list can't be looked up in.
    let user = prefs.join("someone");
    std::fs::create_dir_all(&user).unwrap();
    {
        let _guard = Unreadable::new(&user).unwrap();
        let plist = user.join(format!("{MANAGED_PREFERENCES_DOMAIN}.plist"));
        assert!(
            matches!(check(&fixture, &lane), Err(Loosened::Unreadable { file, .. }) if file == plist),
            "{:?}",
            check(&fixture, &lane)
        );
    }
    assert_eq!(check(&fixture, &lane), Ok(()));
}

/// Waits until the fake has recorded `count` of `method`.
async fn recorded_at_least(record: &Path, method: &str, count: usize) {
    while methods(record).iter().filter(|m| *m == method).count() < count {
        async_io::Timer::after(Duration::from_millis(10)).await;
    }
}

#[test]
fn a_dropped_prompt_still_has_its_turn_cancelled_on_a_widening_change() {
    let fixture = Fixture::new();
    let lane = fixture.lane();
    let record = fixture.record();
    run(
        fixture.environment(),
        AgentSettings::default(),
        waiting_turn(&fixture),
        async |agent: ClaudeAgent, mut incoming| {
            agent.initialize().await.expect("initialize");
            let id = agent.new_session(&lane).await.expect("new").session_id;
            let request = {
                let turn = agent.prompt(id.clone(), text("go"));
                futures::pin_mut!(turn);
                let request = loop {
                    futures::select! {
                        _ = turn.as_mut().fuse() => panic!("the turn ended early"),
                        message = incoming.next().fuse() => {
                            if let Some(Incoming::Permission(request)) = message {
                                break request;
                            }
                        }
                    }
                };
                // The caller gives up on the turn; the agent still runs it.
                request
            };
            assert!(agent.client().turn_running(&id));
            write(
                &fixture.user_settings(),
                &json!({"sandbox": {"enabled": false}}),
            );
            let refused = agent.settings_changed().await;
            assert_eq!(refused.len(), 1);
            assert!(!request.is_pending(), "answered Cancelled");
            recorded_at_least(&record, "answer:bash-1", 1).await;
        },
    );
    let methods = methods(&record);
    assert!(
        methods.contains(&"session/cancel".to_owned()),
        "{methods:?}"
    );
}

#[test]
fn a_widening_change_during_the_reload_refuses_the_prompt() {
    let fixture = Fixture::new();
    let lane = fixture.lane();
    let record = fixture.record();
    let gate = fixture.base().join("gate");
    run(
        fixture.environment(),
        AgentSettings::default(),
        fake::Script {
            load_gate: Some(gate.clone()),
            ..script(&fixture)
        },
        async |agent: ClaudeAgent, _incoming| {
            agent.initialize().await.expect("initialize");
            let id = agent.new_session(&lane).await.expect("new").session_id;
            let mut settings = AgentSettings::default();
            settings.permissions.fetch = AgentLevel::Deny;
            agent.set_settings(settings);
            let prompt = agent.prompt(id.clone(), text("go"));
            let change = async {
                recorded_at_least(&record, "session/load", 1).await;
                write(
                    &fixture.user_settings(),
                    &json!({"permissions": {"allow": ["Edit"]}}),
                );
                // No turn runs yet: nothing to cancel, but it's recorded.
                assert_eq!(agent.settings_changed().await.len(), 1);
                write_text(&gate, "");
            };
            let (refused, ()) = futures::join!(prompt, change);
            assert!(
                matches!(
                    &refused,
                    Err(AgentState::SandboxLoosened(Loosened::Widening { .. }))
                ),
                "{refused:?}"
            );
        },
    );
    assert!(!methods(&record).contains(&"session/prompt".to_owned()));
}

#[test]
fn settings_changed_during_the_reload_reload_again_before_the_prompt() {
    let fixture = Fixture::new();
    let lane = fixture.lane();
    let record = fixture.record();
    let gate = fixture.base().join("gate");
    run(
        fixture.environment(),
        AgentSettings::default(),
        fake::Script {
            load_gate: Some(gate.clone()),
            ..script(&fixture)
        },
        async |agent: ClaudeAgent, _incoming| {
            agent.initialize().await.expect("initialize");
            let id = agent.new_session(&lane).await.expect("new").session_id;
            let mut settings = AgentSettings::default();
            settings.permissions.fetch = AgentLevel::Deny;
            agent.set_settings(settings.clone());
            let prompt = agent.prompt(id.clone(), text("go"));
            let change = async {
                recorded_at_least(&record, "session/load", 1).await;
                settings.permissions.read = AgentLevel::Deny;
                agent.set_settings(settings);
                write_text(&gate, "");
            };
            let (response, ()) = futures::join!(prompt, change);
            response.expect("the prompt");
        },
    );
    let sends: Vec<(String, Value)> = recorded(&record)
        .into_iter()
        .filter(|(m, _)| m == "session/load" || m == "session/prompt")
        .collect();
    let order: Vec<&str> = sends.iter().map(|(m, _)| m.as_str()).collect();
    assert_eq!(order, ["session/load", "session/load", "session/prompt"]);
    let tools = strings(&sends[1].1["_meta"]["claudeCode"]["options"]["tools"]);
    assert!(!tools.contains(&"Read".to_owned()), "{tools:?}");
    assert!(!tools.contains(&"WebFetch".to_owned()), "{tools:?}");
}

/// The loads and prompts the fake received, in order, with each load's
/// `_meta` text.
fn loads_and_prompts(record: &Path) -> Vec<(String, String)> {
    recorded(record)
        .into_iter()
        .filter(|(m, _)| m == "session/load" || m == "session/prompt")
        .map(|(m, p)| {
            let detail = if m == "session/load" {
                serde_json::to_string(&p["_meta"]).unwrap()
            } else {
                p["prompt"][0]["text"].as_str().unwrap().to_owned()
            };
            (m, detail)
        })
        .collect()
}

/// Settings go from A to B, a load with B is sent and its caller dropped,
/// settings go back to A, and a prompt comes: it waits for B's answer, then
/// restores A before it is sent. `start_load` sends the load with B.
fn a_reverted_setting_is_restored_after_a_dropped_reload(
    start_load: impl AsyncFnOnce(&ClaudeAgent, SessionId, &Lane),
) {
    let fixture = Fixture::new();
    let lane = fixture.lane();
    let record = fixture.record();
    let gate = fixture.base().join("gate");
    run(
        fixture.environment(),
        AgentSettings::default(),
        fake::Script {
            load_gate: Some(gate.clone()),
            ..script(&fixture)
        },
        async |agent: ClaudeAgent, _incoming| {
            agent.initialize().await.expect("initialize");
            let id = agent.new_session(&lane).await.expect("new").session_id;
            let mut loosened = AgentSettings::default();
            loosened.permissions.fetch = AgentLevel::Allow;
            agent.set_settings(loosened);
            // B's load is sent, and its caller gives up on it.
            {
                let load = start_load(&agent, id.clone(), &lane);
                futures::pin_mut!(load);
                futures::select! {
                    () = load.fuse() => panic!("the load was answered before its gate"),
                    () = recorded_at_least(&record, "session/load", 1).fuse() => {}
                }
            }
            agent.set_settings(AgentSettings::default());
            // Admitted only once B's load is answered, and after A is back.
            let prompt = agent.prompt(id.clone(), text("go"));
            let open = async {
                async_io::Timer::after(Duration::from_millis(100)).await;
                assert!(
                    !methods(&record).contains(&"session/prompt".to_owned()),
                    "admitted while a load was in flight"
                );
                write_text(&gate, "");
            };
            let (response, ()) = futures::join!(prompt, open);
            response.expect("the prompt");
        },
    );
    let new = metas(&record, "session/new");
    let sent = loads_and_prompts(&record);
    let kinds: Vec<&str> = sent.iter().map(|(m, _)| m.as_str()).collect();
    assert_eq!(kinds, ["session/load", "session/load", "session/prompt"]);
    assert_ne!(sent[0].1, new[0], "the first load carried B");
    assert_eq!(sent[1].1, new[0], "A is restored before the prompt");
    assert_eq!(sent[2].1, "go");
}

#[test]
fn a_reverted_setting_is_restored_after_a_dropped_prompt_reload() {
    a_reverted_setting_is_restored_after_a_dropped_reload(async |agent, id, _lane| {
        // The prompt that loads B; dropped before its load is answered.
        let _ = agent.prompt(id, text("dropped")).await;
    });
}

#[test]
fn a_reverted_setting_is_restored_after_a_dropped_public_load() {
    a_reverted_setting_is_restored_after_a_dropped_reload(async |agent, id, lane| {
        let _ = agent.load_session(id, lane).await;
    });
}

/// A prompt's re-load is held open; a public load or resume (`queued`)
/// waits behind it; a widening file is added while it waits, and the
/// watcher refuses the session. When the re-load is answered, the queued
/// one is refused too, and never sent (review round 3, F8).
fn a_queued_reconfiguration_rechecks_after_its_wait(
    queued: impl AsyncFnOnce(&ClaudeAgent, SessionId, &Lane) -> Result<(), AgentState>,
    method: &str,
) {
    let fixture = Fixture::new();
    let lane = fixture.lane();
    let record = fixture.record();
    let gate = fixture.base().join("gate");
    run(
        fixture.environment(),
        AgentSettings::default(),
        fake::Script {
            load_gate: Some(gate.clone()),
            ..script(&fixture)
        },
        async |agent: ClaudeAgent, _incoming| {
            agent.initialize().await.expect("initialize");
            let id = agent.new_session(&lane).await.expect("new").session_id;
            let mut settings = AgentSettings::default();
            settings.permissions.fetch = AgentLevel::Deny;
            agent.set_settings(settings);
            let reload = agent.prompt(id.clone(), text("go"));
            let waiting = queued(&agent, id.clone(), &lane);
            let change = async {
                recorded_at_least(&record, "session/load", 1).await;
                while agent.queued(&id) == 0 {
                    async_io::Timer::after(Duration::from_millis(10)).await;
                }
                write(
                    &fixture.user_settings(),
                    &json!({"sandbox": {"excludedCommands": ["touch *"]}}),
                );
                assert_eq!(agent.settings_changed().await.len(), 1);
                write_text(&gate, "");
            };
            let (reloaded, refused, ()) = futures::join!(reload, waiting, change);
            for result in [reloaded.map(drop), refused] {
                assert!(
                    matches!(
                        &result,
                        Err(AgentState::SandboxLoosened(Loosened::Widening { file, .. }))
                            if *file == fixture.user_settings()
                    ),
                    "{result:?}"
                );
            }
            assert!(agent.refused(&id).is_some());
        },
    );
    let sent = methods(&record);
    assert_eq!(
        sent.iter().filter(|m| *m == "session/load").count(),
        1,
        "only the prompt's re-load: {sent:?}"
    );
    assert!(!sent.contains(&method.to_owned()) || method == "session/load");
    assert!(!sent.contains(&"session/prompt".to_owned()));
}

#[test]
fn a_queued_load_rechecks_the_settings_after_its_wait() {
    a_queued_reconfiguration_rechecks_after_its_wait(
        async |agent, id, lane| agent.load_session(id, lane).await.map(drop),
        "session/load",
    );
}

#[test]
fn a_queued_resume_rechecks_the_settings_after_its_wait() {
    a_queued_reconfiguration_rechecks_after_its_wait(
        async |agent, id, lane| agent.resume_session(id, lane).await.map(drop),
        "session/resume",
    );
}

#[test]
fn sandbox_allow_write_refuses_glob_patterns() {
    let fixture = Fixture::new();
    let lane = fixture.lane();
    let base = path_text(&fixture.base());
    std::fs::create_dir_all(fixture.base().join("cache")).unwrap();
    let mut settings = AgentSettings::default();
    settings.claude_code.sandbox_allow_write = vec![
        // Patterns that reach the root (review round 1 of rev 4, F9).
        format!("{base}/rep*/**"),
        format!("{base}/re?o/src"),
        format!("{base}/[r]epo"),
        format!("{base}/{{repo,cache}}"),
        format!("{base}/**"),
        // One outside it is refused too: a pattern can't be bounded.
        format!("{base}/cache/*"),
        // A plain path outside passes.
        format!("{base}/cache/plain"),
    ];
    // On Windows `\\` is the path separator, not an escape.
    #[cfg(not(windows))]
    settings
        .claude_code
        .sandbox_allow_write
        .push(format!("{base}/re\\po"));
    let dirs = async_io::block_on(LaneDirs::resolve(Path::new("git"), &lane)).expect("dirs");
    let meta = claude::profile(&settings, &dirs);
    assert_eq!(
        strings(&options(&meta)["sandbox"]["filesystem"]["allowWrite"]),
        [profile_text(&fixture.base().join("cache/plain"))]
    );
}

#[cfg(unix)]
#[test]
fn sandbox_allow_write_refuses_a_link_to_a_folder_named_like_a_glob() {
    let fixture = Fixture::new();
    let lane = fixture.lane();
    let base = fixture.base();
    // Folders literally named `rep*` and `**`, reached by a glob-free link
    // (rev 4 round 2, F9).
    let globbed = base.join("rep*").join("**");
    std::fs::create_dir_all(&globbed).unwrap();
    std::os::unix::fs::symlink(&globbed, base.join("cache-link")).unwrap();
    let mut settings = AgentSettings::default();
    settings.claude_code.sandbox_allow_write = vec![
        path_text(&base.join("cache-link")),
        path_text(&base.join("cache-link/sub")),
    ];
    let dirs = async_io::block_on(LaneDirs::resolve(Path::new("git"), &lane)).expect("dirs");
    let meta = claude::profile(&settings, &dirs);
    assert_eq!(
        strings(&options(&meta)["sandbox"]["filesystem"]["allowWrite"]),
        Vec::<String>::new()
    );
}

#[test]
fn a_lane_whose_paths_have_glob_characters_is_refused() {
    let fixture = Fixture::new();
    let root = fixture.base().join("pro[j]ect");
    std::fs::create_dir_all(&root).unwrap();
    git(&root, &["init", "-q", "-b", "main"]);
    std::fs::write(root.join("notes.txt"), "notes\n").unwrap();
    git(&root, &["add", "notes.txt"]);
    git(&root, &["commit", "-q", "-m", "first"]);
    let worktree = fixture.base().join("glob-lane");
    git(
        &root,
        &[
            "worktree",
            "add",
            "-q",
            "-b",
            "lane",
            worktree.to_str().unwrap(),
        ],
    );
    let lane = Lane { root, worktree };
    let refused = async_io::block_on(LaneDirs::resolve(Path::new("git"), &lane));
    assert!(
        matches!(&refused, Err(AgentState::SandboxLoosened(Loosened::Git { reason })) if reason.contains("glob")),
        "{refused:?}"
    );
}
