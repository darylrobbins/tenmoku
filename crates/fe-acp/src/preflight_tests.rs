use std::cell::RefCell;
use std::ffi::{OsStr, OsString};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use super::*;

/// Real files in a temp dir, canned `--version` answers.
struct Fake {
    answer: Option<String>,
    ran: RefCell<Vec<(PathBuf, PathBuf)>>,
}

impl Fake {
    fn new(answer: Option<&str>) -> Self {
        Self {
            answer: answer.map(str::to_owned),
            ran: RefCell::new(Vec::new()),
        }
    }
}

impl Probe for Fake {
    fn is_runnable(&self, path: &Path) -> bool {
        path.is_file()
    }
    fn canonicalize(&self, path: &Path) -> Option<PathBuf> {
        fs::canonicalize(path).ok()
    }
    fn read_small(&self, path: &Path) -> Option<String> {
        fs::read_to_string(path).ok()
    }
    fn run_version(
        &self,
        program: &Path,
        cwd: &Path,
        _path_env: Option<&OsStr>,
        _timeout: Duration,
    ) -> Option<String> {
        self.ran
            .borrow_mut()
            .push((program.to_path_buf(), cwd.to_path_buf()));
        self.answer.clone()
    }
}

fn touch(path: &Path) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, "").unwrap();
}

fn env(home: &Path, login: Option<&Path>) -> PreflightEnv {
    let mut env = PreflightEnv::new(home.to_path_buf(), login.map(OsString::from));
    env.homebrew_prefix = Some(home.join("no-brew"));
    env
}

fn found(report: &Report) -> (&Path, Source, Option<Version>) {
    match report {
        Report::Found {
            path,
            source,
            version,
        } => (path, *source, *version),
        other => panic!("not found: {other:?}"),
    }
}

#[test]
fn configured_path_wins_over_path() {
    let dir = tempfile::tempdir().unwrap();
    let bin = dir.path().join("bin");
    touch(&bin.join("claude"));
    touch(&dir.path().join("mine/claude"));
    let mut env = env(dir.path(), Some(&bin));
    env.configured_claude = "~/mine/claude".into();
    let report = check_claude(&env, &Fake::new(Some("2.0.1 (Claude Code)")));
    let (path, source, version) = found(&report);
    assert_eq!(path, dir.path().join("mine/claude"));
    assert_eq!(source, Source::Configured);
    assert_eq!(version, Some((2, 0, 1)));
}

#[test]
fn missing_configured_path_does_not_fall_back() {
    let dir = tempfile::tempdir().unwrap();
    let bin = dir.path().join("bin");
    touch(&bin.join("claude"));
    let mut env = env(dir.path(), Some(&bin));
    env.configured_claude = "/nonexistent/claude".into();
    assert_eq!(
        check_claude(&env, &Fake::new(None)),
        Report::Missing { configured: true }
    );
}

#[test]
fn found_on_login_path() {
    let dir = tempfile::tempdir().unwrap();
    let bin = dir.path().join("bin");
    touch(&bin.join("claude"));
    let env = env(dir.path(), Some(&bin));
    let fake = Fake::new(Some("1.2.3"));
    let report = check_claude(&env, &fake);
    let (path, source, _) = found(&report);
    assert_eq!(path, bin.join("claude"));
    assert_eq!(source, Source::LoginPath);
    // The home directory is the cwd, never the workspace.
    assert_eq!(fake.ran.borrow()[0].1, dir.path());
    assert_eq!(
        check_claude(&self::env(dir.path(), None), &Fake::new(None)),
        Report::Missing { configured: false }
    );
}

#[test]
fn relative_path_entries_are_skipped() {
    let dir = tempfile::tempdir().unwrap();
    let cwd = std::env::current_dir().unwrap();
    // A relative entry that would resolve to a real file from the cwd.
    let rel = pathdiff(dir.path(), &cwd);
    touch(&dir.path().join("claude"));
    let joined = std::env::join_paths([PathBuf::from(""), PathBuf::from("."), rel]).unwrap();
    let mut env = env(dir.path(), None);
    env.login_path = Some(joined);
    assert_eq!(
        check_claude(&env, &Fake::new(None)),
        Report::Missing { configured: false }
    );
}

fn pathdiff(target: &Path, base: &Path) -> PathBuf {
    let mut up = PathBuf::new();
    for _ in base.components().skip(1) {
        up.push("..");
    }
    up.join(target.strip_prefix("/").unwrap())
}

#[test]
fn found_in_fallback_locations() {
    for (rel, source, name) in [
        (".claude/local", Source::ClaudeLocal, "claude"),
        (".local/bin", Source::NativeInstaller, "claude"),
        (".local/share/mise/shims", Source::MiseShims, "claude"),
        ("brew/bin", Source::Homebrew, "claude"),
        (".local/bin", Source::NativeInstaller, "claude-agent-acp"),
        ("brew/bin", Source::Homebrew, "claude-agent-acp"),
    ] {
        let dir = tempfile::tempdir().unwrap();
        touch(&dir.path().join(rel).join(name));
        let mut env = env(dir.path(), None);
        env.homebrew_prefix = Some(dir.path().join("brew"));
        let report = if name == "claude" {
            check_claude(&env, &Fake::new(None))
        } else {
            check_adapter(&env, &Fake::new(None))
        };
        assert_eq!(found(&report).1, source, "{rel} {name}");
    }
    // ~/.claude/local is for claude only.
    let dir = tempfile::tempdir().unwrap();
    touch(&dir.path().join(".claude/local/claude-agent-acp"));
    assert_eq!(
        check_adapter(&env(dir.path(), None), &Fake::new(None)),
        Report::Missing { configured: false }
    );
    // MISE_DATA_DIR moves the shims.
    let dir = tempfile::tempdir().unwrap();
    touch(&dir.path().join("md/shims/claude"));
    let mut env = env(dir.path(), None);
    env.mise_data_dir = Some(dir.path().join("md"));
    assert_eq!(
        found(&check_claude(&env, &Fake::new(None))).1,
        Source::MiseShims
    );
}

#[cfg(unix)]
fn script(path: &Path, body: &str) {
    use std::os::unix::fs::PermissionsExt;
    touch(path);
    fs::write(path, format!("#!/bin/sh\n{body}\n")).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
}

#[cfg(unix)]
#[test]
fn claude_version_is_parsed() {
    let dir = tempfile::tempdir().unwrap();
    let bin = dir.path().join("bin");
    script(&bin.join("claude"), "echo \"2.1.7 (Claude Code)\"; pwd >&2");
    let env = env(dir.path(), Some(&bin));
    let report = check_claude(&env, &SystemProbe);
    assert_eq!(found(&report).2, Some((2, 1, 7)));
}

#[cfg(unix)]
#[test]
fn version_timeout_is_unknown() {
    let dir = tempfile::tempdir().unwrap();
    let bin = dir.path().join("bin");
    script(&bin.join("claude"), "while :; do :; done");
    let mut env = env(dir.path(), Some(&bin));
    env.version_timeout = Duration::from_millis(200);
    let started = std::time::Instant::now();
    let report = check_claude(&env, &SystemProbe);
    assert!(started.elapsed() >= Duration::from_millis(200));
    assert!(started.elapsed() < Duration::from_secs(3));
    assert_eq!(found(&report).2, None);
}

#[cfg(unix)]
#[test]
fn a_descendant_holding_stdout_cannot_stall() {
    let dir = tempfile::tempdir().unwrap();
    let bin = dir.path().join("bin");
    // The wrapper exits at once; a busy child keeps stdout open.
    script(
        &bin.join("claude"),
        "echo 2.0.0; (while :; do :; done) &\nexit 0",
    );
    let mut env = env(dir.path(), Some(&bin));
    env.version_timeout = Duration::from_millis(300);
    let started = std::time::Instant::now();
    let report = check_claude(&env, &SystemProbe);
    assert!(started.elapsed() < Duration::from_secs(3));
    assert!(matches!(report, Report::Found { .. }));
}

#[cfg(unix)]
#[test]
fn execute_bits_this_user_cannot_use_are_not_runnable() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("claude");
    script(&file, "echo 1.0.0");
    assert!(SystemProbe.is_runnable(&file));
    // Executable by others only: the owner is denied.
    fs::set_permissions(&file, fs::Permissions::from_mode(0o011)).unwrap();
    if rustix::process::geteuid().is_root() {
        return;
    }
    assert!(!SystemProbe.is_runnable(&file));
}

fn adapter_package(root: &Path, version: &str) -> PathBuf {
    let pkg = root.join("lib/node_modules/@agentclientprotocol/claude-agent-acp");
    touch(&pkg.join("dist/index.js"));
    fs::write(
        pkg.join("package.json"),
        format!(r#"{{"name":"@agentclientprotocol/claude-agent-acp","version":"{version}"}}"#),
    )
    .unwrap();
    pkg.join("dist/index.js")
}

#[cfg(unix)]
#[test]
fn adapter_version_from_package_json() {
    use std::os::unix::fs::{PermissionsExt, symlink};
    let dir = tempfile::tempdir().unwrap();
    let target = adapter_package(dir.path(), "0.107.2");
    fs::set_permissions(&target, fs::Permissions::from_mode(0o755)).unwrap();
    let bin = dir.path().join("bin");
    fs::create_dir_all(&bin).unwrap();
    symlink(&target, bin.join("claude-agent-acp")).unwrap();
    let env = env(dir.path(), Some(&bin));
    let report = check_adapter(&env, &SystemProbe);
    assert_eq!(found(&report).2, Some((0, 107, 2)));
}

#[test]
fn adapter_below_floor_is_too_old() {
    let dir = tempfile::tempdir().unwrap();
    let target = adapter_package(dir.path(), "0.105.9");
    let mut env = env(dir.path(), None);
    env.configured_adapter = target.to_string_lossy().into_owned();
    assert_eq!(
        check_adapter(&env, &Fake::new(None)),
        Report::TooOld {
            path: target,
            version: (0, 105, 9),
            floor: MIN_ADAPTER_VERSION,
        }
    );
    // No package.json: found, version unknown; never run.
    let dir = tempfile::tempdir().unwrap();
    touch(&dir.path().join("a/claude-agent-acp"));
    let mut env = self::env(dir.path(), None);
    env.configured_adapter = dir.path().join("a/claude-agent-acp").display().to_string();
    let fake = Fake::new(Some("9.9.9"));
    assert_eq!(found(&check_adapter(&env, &fake)).2, None);
    assert!(fake.ran.borrow().is_empty());
}

#[test]
fn ready_needs_both() {
    let dir = tempfile::tempdir().unwrap();
    let adapter = adapter_package(dir.path(), "0.110.0");
    touch(&dir.path().join("claude"));
    let mut env = env(dir.path(), None);
    env.configured_claude = dir.path().join("claude").display().to_string();
    let fake = Fake::new(Some("2.0.0"));
    assert!(!check(&env, &fake).ready());
    env.configured_adapter = adapter.display().to_string();
    assert!(check(&env, &fake).ready());
    let old = adapter_package(&dir.path().join("old"), "0.1.0");
    env.configured_adapter = old.display().to_string();
    assert!(!check(&env, &fake).ready());
}

#[test]
fn versions_parse() {
    assert_eq!(
        parse_leading_version("2.0.1 (Claude Code)\n"),
        Some((2, 0, 1))
    );
    assert_eq!(parse_leading_version("v0.107.1-beta.2"), Some((0, 107, 1)));
    assert_eq!(parse_leading_version("claude"), None);
}
