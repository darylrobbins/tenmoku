//! Launching the adapter, against the fake agent binary
//! (`CARGO_BIN_EXE_fe-acp-fake-agent`; decision record § 12): the
//! environment of every spawn and respawn, the programs' states, a crash
//! mid-turn and the load after it, stderr, and shutdown killing the process
//! group.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use fe_acp::claude_settings::Environment;
use fe_acp::launch::{
    self, CLAUDE_CODE_EXECUTABLE, DISABLE_BACKGROUND_TASKS, HIDE_CLAUDE_AUTH, LaunchSpec, Launched,
    STDERR_LINES,
};
use fe_acp::schema::ContentBlock;
use fe_acp::{AgentState, ClaudeAgent, Lane};
use fe_config::agent_settings::AgentSettings;
use futures::FutureExt as _;
use serde_json::{Value, json};
use tempfile::TempDir;

const FAKE: &str = env!("CARGO_BIN_EXE_fe-acp-fake-agent");
const TIMEOUT: Duration = Duration::from_secs(30);

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

/// A repository with a lane worktree, a fake home and managed directories,
/// and the fake's record file.
struct Fixture {
    _dir: TempDir,
    base: PathBuf,
    lane: Lane,
}

impl Fixture {
    fn new() -> Self {
        let dir = tempfile::tempdir().expect("tempdir");
        let base = std::fs::canonicalize(dir.path()).unwrap();
        let repo = base.join("repo");
        std::fs::create_dir_all(&repo).unwrap();
        git(&repo, &["init", "-q", "-b", "main"]);
        std::fs::write(repo.join("notes.txt"), "notes\n").unwrap();
        git(&repo, &["add", "notes.txt"]);
        git(&repo, &["commit", "-q", "-m", "first"]);
        let worktree = base.join("lane");
        git(
            &repo,
            &[
                "worktree",
                "add",
                "-q",
                "-b",
                "lane",
                worktree.to_str().unwrap(),
            ],
        );
        for sub in ["home/.claude", "managed", "prefs"] {
            std::fs::create_dir_all(base.join(sub)).unwrap();
        }
        Self {
            _dir: dir,
            base,
            lane: Lane {
                root: repo,
                worktree,
            },
        }
    }

    fn environment(&self) -> Environment {
        Environment {
            managed_dirs: vec![self.base.join("managed")],
            managed_preferences: vec![self.base.join("prefs")],
            user_config_dir: Some(self.base.join("home/.claude")),
            uninspectable_platform: None,
        }
    }

    fn record(&self) -> PathBuf {
        self.base.join("record.jsonl")
    }

    /// The spec that runs the fake with `script` (its `record` set to the
    /// fixture's), and the fake itself standing in for `claude`.
    fn spec(&self, mut script: Value) -> LaunchSpec {
        script["record"] = json!(self.record());
        let mut spec = LaunchSpec::new(Some(FAKE.into()), Some(FAKE.into()), &self.lane.root);
        spec.args = vec![script.to_string().into()];
        spec
    }

    fn recorded(&self) -> Vec<(String, Value)> {
        let Ok(text) = std::fs::read_to_string(self.record()) else {
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

    fn all(&self, method: &str) -> Vec<Value> {
        self.recorded()
            .into_iter()
            .filter(|(m, _)| m == method)
            .map(|(_, params)| params)
            .collect()
    }
}

/// Launches `spec`, drives its connection on a thread of its own, and
/// returns Claude Agent over it.
fn start(fixture: &Fixture, spec: &LaunchSpec) -> (ClaudeAgent, launch::AgentProcess) {
    let Launched {
        client,
        incoming,
        connection,
        process,
    } = launch::launch(spec).expect("launch");
    std::thread::spawn(move || {
        let _incoming = incoming;
        let _ = async_io::block_on(connection);
    });
    let agent = ClaudeAgent::new(
        client,
        Some(process.clone()),
        fixture.environment(),
        "git",
        AgentSettings::default(),
    );
    (agent, process)
}

fn block_on<T>(future: impl Future<Output = T>) -> T {
    async_io::block_on(async {
        futures::select! {
            value = future.fuse() => value,
            _ = futures::FutureExt::fuse(async_io::Timer::after(TIMEOUT)) => panic!("timed out"),
        }
    })
}

/// Whether process `pid` is gone: no longer there, or a zombie.
#[cfg(target_os = "linux")]
fn gone(pid: u64) -> bool {
    match std::fs::read_to_string(format!("/proc/{pid}/stat")) {
        Err(_) => true,
        Ok(stat) => stat
            .rsplit(')')
            .next()
            .is_some_and(|rest| rest.trim_start().starts_with('Z')),
    }
}

fn wait_until(what: &str, mut done: impl FnMut() -> bool) {
    let deadline = std::time::Instant::now() + TIMEOUT;
    while !done() {
        assert!(
            std::time::Instant::now() < deadline,
            "timed out waiting for {what}"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
}

// ---------------------------------------------------------------------------
// The environment

#[test]
fn every_spawn_and_respawn_disables_background_tasks() {
    let fixture = Fixture::new();
    let mut spec = fixture.spec(json!({}));
    spec.hide_claude_auth = true;
    for _ in 0..2 {
        let (agent, process) = start(&fixture, &spec);
        block_on(agent.initialize()).expect("initialize");
        block_on(agent.shutdown());
        assert!(process.try_exited().is_some());
    }
    let started = fixture.all("started");
    assert_eq!(started.len(), 2, "a spawn and a respawn");
    for started in started {
        assert_eq!(started["env"][DISABLE_BACKGROUND_TASKS], json!("1"));
        assert_eq!(started["env"][CLAUDE_CODE_EXECUTABLE], json!(FAKE));
        assert_eq!(started["cwd"], json!(fixture.lane.root));
        assert_eq!(started["args"][0], json!(HIDE_CLAUDE_AUTH));
    }
}

#[test]
fn hide_claude_auth_is_a_parameter() {
    let fixture = Fixture::new();
    let spec = fixture.spec(json!({}));
    assert!(!spec.hide_claude_auth);
    let (agent, _process) = start(&fixture, &spec);
    block_on(agent.initialize()).expect("initialize");
    block_on(agent.shutdown());
    let started = fixture.all("started");
    let args = started[0]["args"].as_array().unwrap();
    assert!(!args.contains(&json!(HIDE_CLAUDE_AUTH)), "{args:?}");
}

// ---------------------------------------------------------------------------
// The programs' states

#[test]
fn state_adapter_not_installed() {
    let fixture = Fixture::new();
    for adapter in [
        None,
        Some(fixture.base.join("no-such-adapter")),
        Some(fixture.base.clone()),
    ] {
        let mut spec = fixture.spec(json!({}));
        spec.adapter = adapter;
        assert_eq!(
            launch::launch(&spec).unwrap_err(),
            AgentState::AdapterNotInstalled
        );
    }
}

#[test]
fn state_claude_not_installed() {
    let fixture = Fixture::new();
    let not_executable = fixture.base.join("claude");
    std::fs::write(&not_executable, "#!/bin/sh\n").unwrap();
    for claude in [
        None,
        Some(fixture.base.join("no-such-claude")),
        Some(not_executable.clone()),
    ] {
        let mut spec = fixture.spec(json!({}));
        spec.claude = claude;
        assert_eq!(
            launch::launch(&spec).unwrap_err(),
            AgentState::ClaudeNotInstalled
        );
    }
    assert!(fixture.all("started").is_empty(), "nothing ran");
}

// ---------------------------------------------------------------------------
// A crash mid-turn, and the load after it

#[test]
fn a_crash_mid_turn_disconnects_with_stderr_and_the_respawn_loads_the_same_profile() {
    let fixture = Fixture::new();
    let crashing = fixture.spec(json!({
        "stderr": ["adapter: starting up"],
        "turns": [{"updates": ["half an answer"], "exit": 3}],
    }));
    let (agent, process) = start(&fixture, &crashing);
    block_on(agent.initialize()).expect("initialize");
    let id = block_on(agent.new_session(&fixture.lane))
        .expect("new")
        .session_id;
    let state = block_on(agent.prompt(id.clone(), vec![ContentBlock::from("go")])).unwrap_err();
    let AgentState::Disconnected { stderr } = state else {
        panic!("{state:?}");
    };
    assert_eq!(
        stderr,
        [
            "adapter: starting up",
            "fake agent: exiting mid-turn with 3"
        ]
    );
    assert_eq!(process.try_exited().and_then(|s| s.code()), Some(3));
    drop(agent);

    // The next send respawns and loads with the whole current profile.
    let (agent, _process) = start(&fixture, &fixture.spec(json!({})));
    block_on(agent.initialize()).expect("initialize");
    block_on(agent.load_session(id.clone(), &fixture.lane)).expect("load");
    let response = block_on(agent.prompt(id, vec![ContentBlock::from("again")])).expect("prompt");
    assert_eq!(response.stop_reason, fe_acp::schema::StopReason::EndTurn);
    block_on(agent.shutdown());

    let new = fixture.all("session/new");
    let load = fixture.all("session/load");
    assert_eq!(new.len(), 1);
    assert_eq!(load.len(), 1);
    assert_eq!(
        serde_json::to_string(&new[0]["_meta"]).unwrap(),
        serde_json::to_string(&load[0]["_meta"]).unwrap(),
        "the load after a crash sends the same bytes"
    );
    assert_eq!(fixture.all("started").len(), 2);
}

#[test]
fn stderr_keeps_the_last_lines() {
    let fixture = Fixture::new();
    let lines: Vec<String> = (0..STDERR_LINES + 50)
        .map(|i| format!("line {i}"))
        .collect();
    let (agent, process) = start(&fixture, &fixture.spec(json!({ "stderr": lines })));
    block_on(agent.initialize()).expect("initialize");
    let last = lines.last().unwrap().clone();
    wait_until("stderr", || process.stderr_tail().last() == Some(&last));
    let tail = process.stderr_tail();
    assert_eq!(tail.len(), STDERR_LINES);
    assert_eq!(tail, lines[50..]);
    block_on(agent.shutdown());
}

#[test]
fn stderr_bounds_unterminated_lines_and_survives_invalid_utf8() {
    let fixture = Fixture::new();
    // A megabyte with no newline, then bytes that aren't UTF-8, then a
    // diagnostic that must still arrive.
    let (agent, process) = start(
        &fixture,
        &fixture.spec(json!({
            "stderr_fill": [b'x', 1_000_000],
            "stderr_bytes": [b'\n', 0xff, 0xfe, b'\n'],
            "stderr": ["valid after"],
        })),
    );
    block_on(agent.initialize()).expect("initialize");
    wait_until("stderr", || {
        process.stderr_tail().last().map(String::as_str) == Some("valid after")
    });
    let tail = process.stderr_tail();
    assert_eq!(
        tail.len(),
        3,
        "{:?}",
        tail.iter().map(String::len).collect::<Vec<_>>()
    );
    assert!(tail[0].bytes().all(|b| b == b'x'));
    assert_eq!(tail[0].len(), 4096);
    assert_eq!(tail[1], "\u{fffd}\u{fffd}");
    assert_eq!(tail[2], "valid after");
    block_on(agent.shutdown());
}

// ---------------------------------------------------------------------------
// Shutdown

#[cfg(target_os = "linux")]
#[test]
fn shutdown_closes_sessions_and_kills_the_group() {
    let fixture = Fixture::new();
    // The fake starts a child in its group and stays alive after its stdin
    // closes, as a stuck adapter would.
    let (agent, process) = start(
        &fixture,
        &fixture.spec(json!({ "spawn_child": true, "linger": true })),
    );
    block_on(agent.initialize()).expect("initialize");
    let id = block_on(agent.new_session(&fixture.lane))
        .expect("new")
        .session_id;
    let child = fixture.all("child")[0]["pid"].as_u64().unwrap();
    let adapter = u64::from(process.id());
    assert!(!gone(child) && !gone(adapter));

    let started = std::time::Instant::now();
    block_on(agent.shutdown());
    let took = started.elapsed();
    assert!(
        took >= launch::SHUTDOWN_GRACE,
        "it waited the grace: {took:?}"
    );

    let closed = fixture.all("session/close");
    assert_eq!(closed.len(), 1);
    assert_eq!(closed[0]["sessionId"], json!(id.0.as_ref()));
    assert!(process.try_exited().is_some(), "the adapter is gone");
    wait_until("the child to be killed", || gone(child));
    assert!(gone(adapter));
}

#[cfg(target_os = "linux")]
#[test]
fn dropping_the_process_kills_the_group() {
    let fixture = Fixture::new();
    let (agent, process) = start(
        &fixture,
        &fixture.spec(json!({ "spawn_child": true, "linger": true })),
    );
    block_on(agent.initialize()).expect("initialize");
    wait_until("the child", || !fixture.all("child").is_empty());
    let child = fixture.all("child")[0]["pid"].as_u64().unwrap();
    assert!(!gone(child));
    agent.client().shutdown();
    drop(agent);
    drop(process);
    wait_until("the child to be killed", || gone(child));
}

/// A re-load that's never answered doesn't keep the agent alive: with every
/// handle dropped mid-load, and no shutdown, the process group is still
/// killed and reaped (review round 3, F7).
#[cfg(target_os = "linux")]
#[test]
fn dropping_every_handle_during_an_unanswered_reload_kills_the_group() {
    let fixture = Fixture::new();
    // The gate is never opened: the load is never answered.
    let gate = fixture.base.join("never");
    let (agent, process) = start(
        &fixture,
        &fixture.spec(json!({ "spawn_child": true, "linger": true, "load_gate": gate })),
    );
    block_on(agent.initialize()).expect("initialize");
    let id = block_on(agent.new_session(&fixture.lane))
        .expect("new")
        .session_id;
    let child = fixture.all("child")[0]["pid"].as_u64().unwrap();
    let adapter = u64::from(process.id());

    // A changed setting makes the next prompt re-load first.
    let mut settings = AgentSettings::default();
    settings.permissions.fetch = fe_config::agent_settings::AgentLevel::Deny;
    agent.set_settings(settings);
    block_on(async {
        let prompt = agent.prompt(id, vec![ContentBlock::from("go")]);
        futures::pin_mut!(prompt);
        loop {
            futures::select! {
                _ = prompt.as_mut().fuse() => panic!("the prompt ended"),
                _ = futures::FutureExt::fuse(async_io::Timer::after(Duration::from_millis(20))) => {}
            }
            if !fixture.all("session/load").is_empty() {
                break;
            }
        }
        // The prompt is dropped here, its load still unanswered.
    });
    assert!(!gone(child) && !gone(adapter));

    drop(agent);
    drop(process);
    wait_until("the child to be killed", || gone(child));
    wait_until("the adapter to be reaped", || gone(adapter));
}

/// The transport fails while the adapter stays alive: the turn, and every
/// request after the driver has ended, are "Disconnected" with its stderr
/// (review round 1 of rev 4, F11).
#[test]
fn a_transport_failure_with_the_adapter_alive_is_disconnected() {
    let fixture = Fixture::new();
    let (agent, process) = start(
        &fixture,
        &fixture.spec(json!({
            "stderr": ["adapter: up"],
            "turns": [{"updates": ["partial"], "hang_up": true}],
            "linger": true,
        })),
    );
    block_on(agent.initialize()).expect("initialize");
    let id = block_on(agent.new_session(&fixture.lane))
        .expect("new")
        .session_id;
    let expected = AgentState::Disconnected {
        stderr: vec!["adapter: up".into(), "fake agent: hanging up".into()],
    };
    let state = block_on(agent.prompt(id.clone(), vec![ContentBlock::from("go")])).unwrap_err();
    assert_eq!(state, expected);
    assert!(
        process.try_exited().is_none(),
        "the adapter is still running"
    );
    // The driver has ended: later requests are disconnects too, at once.
    let state = block_on(agent.prompt(id.clone(), vec![ContentBlock::from("again")])).unwrap_err();
    assert_eq!(state, expected);
    let state = block_on(agent.load_session(id, &fixture.lane)).unwrap_err();
    assert_eq!(state, expected);
    block_on(agent.shutdown());
}
