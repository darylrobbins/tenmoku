//! `run_command` ([`super`]): the executable-substitution cases of the
//! agents record's § 12, the group killed on a timeout and on cancel, and
//! the output caps.
//!
//! Programs are shell scripts in temp folders, looked up on a `PATH` each
//! test builds, never the login shell's.

#![cfg_attr(windows, allow(unused_imports, dead_code))]

use std::{
    ffi::OsString,
    fs,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

use fe_chat::{
    ChatId, RunCommandInput,
    permissions::{Level, Policy},
};
use kiln::command::{Capability, Grants};

use super::*;
#[cfg(unix)]
use std::os::unix::fs::{PermissionsExt as _, symlink};

/// A folder under the real temp folder.
#[cfg(unix)]
fn folder() -> tempfile::TempDir {
    let temp = dunce::canonicalize(std::env::temp_dir()).unwrap();
    tempfile::tempdir_in(temp).unwrap()
}

/// Writes an executable `sh` script at `path` running `body`.
#[cfg(unix)]
fn script(path: &Path, body: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, format!("#!/bin/sh\n{body}\n")).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
}

/// A `git` at `path` that says which it is, and leaves `marker` behind if
/// given, so a test can tell it ran.
#[cfg(unix)]
fn git(path: &Path, says: &str, marker: Option<&Path>) {
    let touch = marker.map_or(String::new(), |marker| {
        format!("touch '{}'\n", marker.display())
    });
    script(path, &format!("{touch}echo \"{says} $*\""));
}

/// A `PATH` of `entries`, as written: relative and empty ones too.
#[cfg(unix)]
fn path_of(entries: &[&str]) -> OsString {
    OsString::from(entries.join(":"))
}

/// The system's own programs, outside every test folder.
#[cfg(unix)]
fn system_path() -> OsString {
    OsString::from("/usr/bin:/bin")
}

#[cfg(unix)]
fn input(program: &str, args: &[&str], cwd: Option<&str>) -> RunCommandInput {
    RunCommandInput {
        program: program.to_owned(),
        args: args.iter().map(|arg| (*arg).to_owned()).collect(),
        cwd: cwd.map(str::to_owned),
    }
}

#[cfg(unix)]
fn grants(capabilities: &[&str]) -> Grants {
    (capabilities.iter())
        .map(|capability| Capability::parse(capability).unwrap())
        .collect()
}

#[cfg(unix)]
fn any_program() -> Grants {
    grants(&["process.spawn:*"])
}

/// The policy of `commands_allow = ["git status"]`, the rest as the
/// defaults (`run_command = "ask"`).
#[cfg(unix)]
fn git_status_allowed() -> Policy {
    let mut policy = Policy::new();
    policy.allow_commands(&["git status"]);
    policy
}

#[cfg(unix)]
fn real(path: &Path) -> PathBuf {
    dunce::canonicalize(path).unwrap()
}

/// Runs `prepared` with a generous timeout and returns how it ended.
#[cfg(unix)]
fn run_it(prepared: Prepared) -> Ran {
    RunningCommand::start(prepared, Limits::secs(30))
        .unwrap()
        .wait()
}

// ---------------------------------------------------------------------------
// Executable substitution (§ 12)

/// With `commands_allow = ["git status"]`, a `git` in the root, on a `PATH`
/// entry inside it (as written, through a link, or a link to one in it),
/// or on a relative entry, is not what `git` resolves to: the system's is,
/// and that is exactly what runs.
#[cfg(unix)]
#[test]
fn a_git_in_the_root_or_on_a_relative_path_entry_is_not_resolved() {
    let temp = folder();
    let root = temp.path().join("project");
    let marker = temp.path().join("the project's git ran");
    for planted in ["git", "bin/git", "tools/git", "node_modules/.bin/git"] {
        git(&root.join(planted), "project git", Some(&marker));
    }
    // Outside the root: a folder that is a link to one inside it, and a
    // `git` that is a link to the root's.
    let elsewhere = temp.path().join("elsewhere");
    fs::create_dir_all(&elsewhere).unwrap();
    symlink(root.join("bin"), elsewhere.join("linked-bin")).unwrap();
    fs::create_dir_all(elsewhere.join("links")).unwrap();
    symlink(root.join("git"), elsewhere.join("links/git")).unwrap();
    let system = temp.path().join("system/bin");
    git(&system.join("git"), "system git", None);
    // The window opened the root through a link to it, too.
    let alias = temp.path().join("alias");
    symlink(&root, &alias).unwrap();

    let path = path_of(&[
        root.to_str().unwrap(),
        root.join("bin").to_str().unwrap(),
        "",
        ".",
        "bin",
        "tools",
        "node_modules/.bin",
        alias.join("tools").to_str().unwrap(),
        elsewhere.join("linked-bin").to_str().unwrap(),
        elsewhere.join("links").to_str().unwrap(),
        system.to_str().unwrap(),
    ]);
    let system_git = real(&system.join("git"));
    assert_eq!(
        resolve_program("git", &path, std::slice::from_ref(&alias)),
        Some(system_git.clone())
    );

    let prepared = prepare(
        &input("git", &["status"], None),
        &alias,
        std::slice::from_ref(&alias),
        Some(&path),
    )
    .unwrap();
    assert_eq!(prepared.program, Program::Resolved(system_git.clone()));
    assert_eq!(prepared.cwd, real(&root));
    let chat = ChatId::new();
    let decision = decide(&any_program(), &git_status_allowed(), chat, &prepared);
    assert_eq!(decision.level, Level::Allow);
    assert_eq!(decision.by, Decider::Rule("git status".to_owned()));

    let ran = run_it(prepared);
    assert_eq!(ran.result_text(), "system git status\nexit 0");
    assert!(!marker.exists(), "the project's git never runs");
}

/// With only entries inside the root and relative ones, `git` isn't found:
/// the call fails, and nothing runs.
#[cfg(unix)]
#[test]
fn with_only_project_and_relative_entries_git_is_not_found() {
    let temp = folder();
    let root = temp.path().join("project");
    let marker = temp.path().join("the project's git ran");
    git(&root.join("git"), "project git", Some(&marker));
    git(&root.join("bin/git"), "project git", Some(&marker));
    let path = path_of(&[
        root.to_str().unwrap(),
        root.join("bin").to_str().unwrap(),
        "",
        ".",
        "bin",
    ]);
    assert_eq!(
        resolve_program("git", &path, std::slice::from_ref(&root)),
        None
    );
    let error = prepare(&input("git", &["status"], None), &root, &[], Some(&path)).unwrap_err();
    assert!(error.contains("was not found on the login PATH"), "{error}");
    assert!(!marker.exists());
}

/// `./git status` names a program by its path: it matches no allow
/// pattern, isn't remembered, and asks every time, even with
/// `commands_allow = ["git status"]`. It runs exactly that file once
/// allowed. A deny pattern on its file name denies it.
#[cfg(unix)]
#[test]
fn dot_slash_git_status_asks() {
    let temp = folder();
    let root = temp.path().join("project");
    git(&root.join("git"), "project git", None);
    let system = temp.path().join("system/bin");
    git(&system.join("git"), "system git", None);
    let path = path_of(&[system.to_str().unwrap()]);
    let chat = ChatId::new();
    let mut policy = git_status_allowed();

    for program in [
        "./git",
        root.join("git").to_str().unwrap(),
        system.join("git").to_str().unwrap(),
    ] {
        let prepared =
            prepare(&input(program, &["status"], None), &root, &[], Some(&path)).unwrap();
        assert!(matches!(prepared.program, Program::Given(_)), "{program}");
        assert_eq!(prepared.program.resolved(), None, "{program}");
        let decide_now = |policy: &Policy| decide(&any_program(), policy, chat, &prepared).level;
        assert_eq!(decide_now(&policy), Level::Ask, "{program}");
        assert!(!policy.remember(&prepared.request(chat)), "{program}");
        assert_eq!(
            decide_now(&policy),
            Level::Ask,
            "{program} after Allow for this chat"
        );
    }

    let prepared = prepare(&input("./git", &["status"], None), &root, &[], Some(&path)).unwrap();
    assert_eq!(prepared.program, Program::Given(real(&root).join("./git")));
    assert_eq!(
        run_it(prepared.clone()).result_text(),
        "project git status\nexit 0"
    );

    policy.deny_commands(&["git"]);
    assert_eq!(
        decide(&any_program(), &policy, chat, &prepared).level,
        Level::Deny
    );
}

/// A remembered "Allow for this chat" holds while `git` resolves where it
/// did, and asks again once it resolves elsewhere: another entry first on
/// the `PATH`, or the link it is retargeted.
#[cfg(unix)]
#[test]
fn a_remembered_answer_asks_again_once_the_name_resolves_elsewhere() {
    let temp = folder();
    let root = temp.path().join("project");
    fs::create_dir_all(&root).unwrap();
    let first = temp.path().join("first/bin");
    let second = temp.path().join("second/bin");
    git(&first.join("git"), "first git", None);
    git(&second.join("git"), "second git", None);
    let chat = ChatId::new();
    let mut policy = Policy::new();
    let level = |policy: &Policy, path: &OsString| {
        let prepared = prepare(&input("git", &["status"], None), &root, &[], Some(path)).unwrap();
        decide(&any_program(), policy, chat, &prepared).level
    };

    let in_order = path_of(&[first.to_str().unwrap(), second.to_str().unwrap()]);
    assert_eq!(level(&policy, &in_order), Level::Ask);
    let prepared = prepare(
        &input("git", &["status"], None),
        &root,
        &[],
        Some(&in_order),
    )
    .unwrap();
    assert!(policy.remember(&prepared.request(chat)));
    assert_eq!(level(&policy, &in_order), Level::Allow);

    let reversed = path_of(&[second.to_str().unwrap(), first.to_str().unwrap()]);
    assert_eq!(level(&policy, &reversed), Level::Ask, "resolved elsewhere");
    assert_eq!(
        level(&policy, &in_order),
        Level::Allow,
        "where it was remembered"
    );

    // A `git` that is a link: retargeted, it resolves elsewhere.
    let linked = temp.path().join("linked/bin");
    fs::create_dir_all(&linked).unwrap();
    symlink(first.join("git"), linked.join("git")).unwrap();
    let through_link = path_of(&[linked.to_str().unwrap()]);
    assert_eq!(
        level(&policy, &through_link),
        Level::Allow,
        "the same real path"
    );
    fs::remove_file(linked.join("git")).unwrap();
    symlink(second.join("git"), linked.join("git")).unwrap();
    assert_eq!(level(&policy, &through_link), Level::Ask, "retargeted");
}

/// The chat's grants are the ceiling: with no `process.spawn` for the
/// program, the call is denied by a hard rule, whatever the policy says.
#[cfg(unix)]
#[test]
fn a_program_not_granted_is_denied_by_a_hard_rule() {
    let temp = folder();
    let root = temp.path().join("project");
    fs::create_dir_all(&root).unwrap();
    let system = temp.path().join("system/bin");
    git(&system.join("git"), "system git", None);
    let path = path_of(&[system.to_str().unwrap()]);
    let prepared = prepare(&input("git", &["status"], None), &root, &[], Some(&path)).unwrap();
    let mut policy = Policy::new();
    policy.set_level(fe_chat::permissions::Class::RunCommand, Level::Allow);
    let chat = ChatId::new();

    let none = decide(&grants(&["buffer.read"]), &policy, chat, &prepared);
    assert_eq!((none.level, none.by), (Level::Deny, Decider::Hard));
    let other = decide(&grants(&["process.spawn:cargo"]), &policy, chat, &prepared);
    assert_eq!(other.level, Level::Deny);
    let named = decide(&grants(&["process.spawn:git"]), &policy, chat, &prepared);
    assert_eq!((named.level, named.by), (Level::Allow, Decider::Policy));
}

/// `cwd` is a folder inside the root, which the command runs in; one
/// outside it, or not a folder, fails the call.
#[cfg(unix)]
#[test]
fn cwd_is_a_folder_inside_the_root() {
    let temp = folder();
    let root = temp.path().join("project");
    fs::create_dir_all(root.join("sub")).unwrap();
    fs::write(root.join("notes.txt"), "").unwrap();
    symlink(temp.path(), root.join("out")).unwrap();
    let path = system_path();
    let pwd = prepare(&input("pwd", &[], Some("sub")), &root, &[], Some(&path)).unwrap();
    assert_eq!(pwd.cwd, real(&root.join("sub")));
    let shown = run_it(pwd).result_text();
    assert_eq!(
        shown,
        format!("{}\nexit 0", real(&root.join("sub")).display())
    );
    for outside in ["..", "../project", "/tmp", "out", "notes.txt"] {
        let refused = prepare(&input("pwd", &[], Some(outside)), &root, &[], Some(&path));
        assert!(refused.is_err(), "{outside}: {refused:?}");
    }
}

// ---------------------------------------------------------------------------
// Timeout and cancel kill the group

/// Whether process `pid` is alive: there, and not a zombie.
#[cfg(unix)]
fn alive(pid: i32) -> bool {
    if cfg!(target_os = "linux") {
        let Ok(stat) = fs::read_to_string(format!("/proc/{pid}/stat")) else {
            return false;
        };
        let state = stat.rsplit_once(") ").map(|(_, rest)| rest.chars().next());
        return !matches!(state, Some(Some('Z' | 'X')));
    }
    // SAFETY: signal 0 only checks that the process exists.
    unsafe { libc::kill(pid, 0) == 0 }
}

/// Waits up to five seconds for `pid` to be gone.
#[cfg(unix)]
fn gone(pid: i32) -> bool {
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        if !alive(pid) {
            return true;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    false
}

/// The pid a command wrote to `file`, waiting up to five seconds for it.
#[cfg(unix)]
fn pid_in(file: &Path) -> i32 {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if let Ok(text) = fs::read_to_string(file)
            && let Ok(pid) = text.trim().parse()
        {
            return pid;
        }
        assert!(Instant::now() < deadline, "no pid in {}", file.display());
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// `sh -c <script>` in `root`, `sh` from the system's `PATH`.
#[cfg(unix)]
fn sh(root: &Path, script: &str) -> Prepared {
    prepare(
        &input("sh", &["-c", script], None),
        root,
        &[],
        Some(&system_path()),
    )
    .unwrap()
}

/// A shell that starts a `sleep` in the background, writes its pid to
/// `pids`, and waits for it: `prefix` runs first.
#[cfg(unix)]
fn sleeper(prefix: &str, pids: &Path) -> String {
    format!(
        "{prefix}sleep 30 & echo $! > '{}'; echo started; wait",
        pids.display()
    )
}

#[cfg(unix)]
#[test]
fn a_timeout_kills_the_group() {
    let temp = folder();
    let pids = temp.path().join("pid");
    let limits = Limits {
        timeout: Duration::from_secs(1),
        grace: Duration::from_millis(500),
    };
    let started = Instant::now();
    let running = RunningCommand::start(sh(temp.path(), &sleeper("", &pids)), limits).unwrap();
    let child = pid_in(&pids);
    let ran = running.wait();
    assert_eq!(ran.end, End::Killed(KillReason::Timeout));
    assert!(ran.is_error());
    assert_eq!(ran.result_text(), "started\nkilled after 1 s");
    assert!(started.elapsed() >= Duration::from_secs(1));
    assert!(
        gone(child),
        "the background sleep was killed with its group"
    );
}

/// A group that ignores `SIGTERM` gets `SIGKILL` after the grace.
#[cfg(unix)]
#[test]
fn a_group_ignoring_sigterm_is_killed_after_the_grace() {
    let temp = folder();
    let pids = temp.path().join("pid");
    let limits = Limits {
        timeout: Duration::from_millis(300),
        grace: Duration::from_millis(700),
    };
    let started = Instant::now();
    let script = sleeper("trap '' TERM; ", &pids);
    let running = RunningCommand::start(sh(temp.path(), &script), limits).unwrap();
    let child = pid_in(&pids);
    let ran = running.wait();
    assert_eq!(ran.end, End::Killed(KillReason::Timeout));
    assert!(
        started.elapsed() >= Duration::from_millis(1000),
        "{:?}",
        started.elapsed()
    );
    assert!(gone(child));
}

#[cfg(unix)]
#[test]
fn cancel_kills_the_group() {
    let temp = folder();
    let pids = temp.path().join("pid");
    let running =
        RunningCommand::start(sh(temp.path(), &sleeper("", &pids)), Limits::secs(60)).unwrap();
    let child = pid_in(&pids);
    assert!(alive(child));
    let started = Instant::now();
    running.cancel();
    let ran = running.wait();
    assert_eq!(ran.end, End::Killed(KillReason::Cancelled));
    assert!(started.elapsed() < Duration::from_secs(5));
    assert!(gone(child));
}

/// Dropping the command (the turn's task dropped) cancels it.
#[cfg(unix)]
#[test]
fn dropping_a_running_command_kills_the_group() {
    let temp = folder();
    let pids = temp.path().join("pid");
    let running =
        RunningCommand::start(sh(temp.path(), &sleeper("", &pids)), Limits::secs(60)).unwrap();
    let child = pid_in(&pids);
    // The runner records the group just after the spawn, so the script can
    // write its pid first.
    let deadline = Instant::now() + Duration::from_secs(5);
    let group = loop {
        if let Some(group) = running.group() {
            break group;
        }
        assert!(Instant::now() < deadline, "it leads a group");
        std::thread::sleep(Duration::from_millis(10));
    };
    drop(running);
    assert!(gone(child));
    assert!(gone(group));
}

/// What a command leaves running in its group when it ends is killed with
/// it, so its result doesn't wait for the timeout.
#[cfg(unix)]
#[test]
fn what_a_command_leaves_running_is_killed_when_it_ends() {
    let temp = folder();
    let pids = temp.path().join("pid");
    let script = format!("sleep 30 & echo $! > '{}'; echo done", pids.display());
    let started = Instant::now();
    let ran = run_it(sh(temp.path(), &script));
    assert_eq!(ran.result_text(), "done\nexit 0");
    assert!(started.elapsed() < Duration::from_secs(10));
    assert!(gone(pid_in(&pids)));
}

/// Each decision says what made it, as the audit trail records it: the
/// class's level, the pattern, the remembered answer, or a hard rule.
#[cfg(unix)]
#[test]
fn a_decision_says_what_made_it() {
    let temp = folder();
    let root = temp.path().join("project");
    fs::create_dir_all(&root).unwrap();
    let system = temp.path().join("system/bin");
    git(&system.join("git"), "system git", None);
    let path = path_of(&[system.to_str().unwrap()]);
    let chat = ChatId::new();
    let by = |policy: &Policy, program: &str, args: &[&str]| {
        let prepared = prepare(&input(program, args, None), &root, &[], Some(&path)).unwrap();
        let decision = decide(&any_program(), policy, chat, &prepared);
        (decision.level, decision.by)
    };
    let rule = |pattern: &str| Decider::Rule(pattern.to_owned());

    let mut policy = Policy::new();
    policy
        .allow_commands(&["git status", "git log *"])
        .ask_commands(&["git log --all"])
        .deny_commands(&["git push"]);
    assert_eq!(
        by(&policy, "git", &["status"]),
        (Level::Allow, rule("git status"))
    );
    assert_eq!(
        by(&policy, "git", &["push", "-f"]),
        (Level::Deny, rule("git push"))
    );
    assert_eq!(
        by(&policy, "git", &["log", "--all"]),
        (Level::Ask, rule("git log --all"))
    );
    assert_eq!(by(&policy, "git", &["diff"]), (Level::Ask, Decider::Policy));
    assert_eq!(
        by(&policy, "./git", &["status"]),
        (Level::Ask, Decider::Hard)
    );

    let prepared = prepare(&input("git", &["diff"], None), &root, &[], Some(&path)).unwrap();
    assert!(policy.remember(&prepared.request(chat)));
    assert_eq!(
        by(&policy, "git", &["diff"]),
        (Level::Allow, Decider::Session)
    );

    policy.set_level(fe_chat::permissions::Class::RunCommand, Level::Deny);
    assert_eq!(
        by(&policy, "git", &["status"]),
        (Level::Deny, Decider::Policy)
    );
}

/// The resolved real path runs, but with the name asked for as its
/// `argv[0]`: a program that dispatches on its name (rustup's `cargo`, a
/// link to it) runs as that name. Here a link named `tool` to the
/// system's `sh` reports its own command line.
#[cfg(target_os = "linux")]
#[cfg(unix)]
#[test]
fn the_program_runs_as_the_name_asked_for() {
    let temp = folder();
    let root = temp.path().join("project");
    fs::create_dir_all(&root).unwrap();
    let bin = temp.path().join("bin");
    fs::create_dir_all(&bin).unwrap();
    let sh = resolve_program("sh", &system_path(), &[]).unwrap();
    symlink(&sh, bin.join("tool")).unwrap();
    let path = path_of(&[bin.to_str().unwrap(), "/usr/bin", "/bin"]);
    let script = r#"tr '\0' ' ' < /proc/$$/cmdline"#;
    let prepared = prepare(
        &input("tool", &["-c", script], None),
        &root,
        &[],
        Some(&path),
    )
    .unwrap();
    assert_eq!(prepared.program, Program::Resolved(sh));
    let shown = run_it(prepared).result_text();
    assert!(shown.starts_with("tool -c "), "{shown}");
}

/// When `SIGTERM` ends the leader at once, what is left of its group still
/// has the grace to clean up in, and the group is killed once it's gone,
/// not at the grace's end.
#[cfg(unix)]
#[test]
fn the_whole_group_has_the_grace() {
    let temp = folder();
    let ready = temp.path().join("ready");
    let cleaned = temp.path().join("cleaned");
    let script = format!(
        "( trap 'sleep 0.3; echo cleaned > \"{cleaned}\"; exit 0' TERM; \
           : > \"{ready}\"; while :; do sleep 0.05; done ) & wait",
        cleaned = cleaned.display(),
        ready = ready.display()
    );
    let limits = Limits {
        timeout: Duration::from_secs(60),
        grace: Duration::from_secs(4),
    };
    let running = RunningCommand::start(sh(temp.path(), &script), limits).unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    while !ready.exists() {
        assert!(Instant::now() < deadline, "the cleaner never started");
        std::thread::sleep(Duration::from_millis(10));
    }
    let stopped = Instant::now();
    running.cancel();
    let ran = running.wait();
    assert_eq!(ran.end, End::Killed(KillReason::Cancelled));
    assert_eq!(fs::read_to_string(&cleaned).unwrap(), "cleaned\n");
    if cfg!(target_os = "linux") {
        assert!(
            stopped.elapsed() < Duration::from_secs(3),
            "{:?}",
            stopped.elapsed()
        );
    }
}

/// A process that left the group and kept the output pipe doesn't hold
/// the result: the pipe is read for a moment once the group is gone, then
/// closed on it, so its later writes fail.
#[cfg(unix)]
#[test]
fn a_process_that_left_the_group_doesnt_keep_the_pipe() {
    let Some(setsid) = resolve_program("setsid", &system_path(), &[]) else {
        eprintln!("no setsid here: skipped");
        return;
    };
    let temp = folder();
    let ready = temp.path().join("ready");
    let broken = temp.path().join("broken");
    let script = format!(
        "'{setsid}' sh -c 'trap \"\" PIPE; : > \"{ready}\"; sleep 1; env echo late || : > \"{broken}\"' & \
         while [ ! -e '{ready}' ]; do sleep 0.01; done; echo hi",
        setsid = setsid.display(),
        ready = ready.display(),
        broken = broken.display()
    );
    let started = Instant::now();
    let ran = run_it(sh(temp.path(), &script));
    assert_eq!(ran.result_text(), "hi\nexit 0");
    assert!(
        started.elapsed() < Duration::from_millis(900),
        "{:?}",
        started.elapsed()
    );
    let deadline = Instant::now() + Duration::from_secs(5);
    while !broken.exists() {
        assert!(Instant::now() < deadline, "the pipe was never closed on it");
        std::thread::sleep(Duration::from_millis(10));
    }
}

// ---------------------------------------------------------------------------
// A program writing without pause can't hold off the deadlines

/// A program that writes without pause: `yes`, `prefix` run first.
#[cfg(unix)]
fn chatty(root: &Path, prefix: &str) -> Prepared {
    sh(root, &format!("{prefix}exec yes"))
}

/// The output kept of a chatty program stays within the cap.
#[cfg(unix)]
fn capped(ran: &Ran) {
    assert!(
        ran.output.total() > (HEAD + TAIL) as u64,
        "{}",
        ran.output.total()
    );
    assert!(ran.output.text().len() < HEAD + TAIL + 64);
}

#[cfg(unix)]
#[test]
fn a_chatty_command_is_cancelled() {
    let temp = folder();
    let running = RunningCommand::start(chatty(temp.path(), ""), Limits::secs(60)).unwrap();
    std::thread::sleep(Duration::from_millis(300));
    let stopped = Instant::now();
    running.cancel();
    let ran = running.wait();
    assert_eq!(ran.end, End::Killed(KillReason::Cancelled));
    assert!(
        stopped.elapsed() < Duration::from_secs(3),
        "{:?}",
        stopped.elapsed()
    );
    capped(&ran);
}

#[cfg(unix)]
#[test]
fn a_chatty_command_times_out() {
    let temp = folder();
    let limits = Limits {
        timeout: Duration::from_millis(500),
        grace: Duration::from_millis(500),
    };
    let started = Instant::now();
    let ran = RunningCommand::start(chatty(temp.path(), ""), limits)
        .unwrap()
        .wait();
    assert_eq!(ran.end, End::Killed(KillReason::Timeout));
    assert!(
        started.elapsed() < Duration::from_secs(4),
        "{:?}",
        started.elapsed()
    );
    capped(&ran);
}

/// One that ignores `SIGTERM` is killed when the grace ends, not when it
/// stops writing.
#[cfg(unix)]
#[test]
fn a_chatty_command_ignoring_sigterm_is_killed_after_the_grace() {
    let temp = folder();
    let limits = Limits {
        timeout: Duration::from_millis(300),
        grace: Duration::from_millis(700),
    };
    let started = Instant::now();
    let ran = RunningCommand::start(chatty(temp.path(), "trap '' TERM; "), limits)
        .unwrap()
        .wait();
    assert_eq!(ran.end, End::Killed(KillReason::Timeout));
    let took = started.elapsed();
    assert!(took >= Duration::from_secs(1), "{took:?}");
    assert!(took < Duration::from_secs(5), "{took:?}");
    capped(&ran);
}

/// A process that left the group and writes without pause is read only
/// for the drain: the result comes, and the pipe closed on it ends it.
#[cfg(unix)]
#[test]
fn a_chatty_process_that_left_the_group_is_cut_off_at_the_drain() {
    let Some(setsid) = resolve_program("setsid", &system_path(), &[]) else {
        eprintln!("no setsid here: skipped");
        return;
    };
    let temp = folder();
    let pids = temp.path().join("pid");
    let script = format!(
        "'{setsid}' sh -c 'echo $$ > \"{pids}\"; exec yes' & \
         while [ ! -s '{pids}' ]; do sleep 0.01; done; echo hi",
        setsid = setsid.display(),
        pids = pids.display()
    );
    let started = Instant::now();
    let ran = run_it(sh(temp.path(), &script));
    assert_eq!(ran.end, End::Exited(0));
    assert!(
        started.elapsed() < Duration::from_secs(4),
        "{:?}",
        started.elapsed()
    );
    capped(&ran);
    assert!(gone(pid_in(&pids)), "the pipe closed on it ended it");
}

/// One pump hands back within its wait, however fast the pipe fills.
#[cfg(unix)]
#[test]
fn one_pump_hands_back_however_fast_the_pipe_fills() {
    let (reader, writer) = std::io::pipe().unwrap();
    let yes = resolve_program("yes", &system_path(), &[]).unwrap();
    let mut writing = std::process::Command::new(yes)
        .stdout(writer)
        .stderr(std::process::Stdio::null())
        .spawn()
        .unwrap();
    let mut output = Output::new(reader).unwrap();
    for _ in 0..20 {
        let pumped = Instant::now();
        output.pump(Duration::from_millis(5));
        assert!(
            pumped.elapsed() < Duration::from_millis(500),
            "{:?}",
            pumped.elapsed()
        );
    }
    assert!(output.capture.total() > 0);
    let drained = Instant::now();
    output.drain(Duration::from_millis(100));
    assert!(
        drained.elapsed() < Duration::from_millis(800),
        "{:?}",
        drained.elapsed()
    );
    drop(output.finish());
    // The pipe closed, `yes` ends on its next write.
    let status = writing.wait().unwrap();
    assert!(!status.success());
}

#[cfg(unix)]
#[test]
fn exit_codes_and_programs_that_cannot_start() {
    let temp = folder();
    let ran = run_it(sh(temp.path(), "echo failing >&2; exit 3"));
    assert_eq!(ran.exit_code(), Some(3));
    assert!(!ran.is_error());
    assert_eq!(ran.result_text(), "failing\nexit 3");
    assert_eq!(run_it(sh(temp.path(), "true")).result_text(), "exit 0");

    fs::write(temp.path().join("not-a-program"), "").unwrap();
    let given = prepare(&input("./not-a-program", &[], None), temp.path(), &[], None).unwrap();
    let ran = run_it(given);
    assert!(matches!(ran.end, End::NotStarted(_)), "{:?}", ran.end);
    assert!(ran.is_error());
}

// ---------------------------------------------------------------------------
// Output caps

/// stdout and stderr share one pipe, so they interleave as written.
#[cfg(unix)]
#[test]
fn stdout_and_stderr_interleave_as_written() {
    let temp = folder();
    let ran = run_it(sh(
        temp.path(),
        "echo out1; echo err1 >&2; echo out2; echo err2 >&2",
    ));
    assert_eq!(ran.result_text(), "out1\nerr1\nout2\nerr2\nexit 0");
}

#[cfg(unix)]
#[test]
fn output_up_to_the_cap_is_kept_whole() {
    let mut capture = Capture::default();
    let all: Vec<u8> = (0..HEAD + TAIL).map(|ix| b'a' + (ix % 26) as u8).collect();
    for chunk in all.chunks(1000) {
        capture.push(chunk);
    }
    assert_eq!(capture.total(), (HEAD + TAIL) as u64);
    assert_eq!(capture.text().as_bytes(), &all[..]);
}

/// Past 32 KiB: the first 8 KiB and the last 24 KiB, with how many bytes
/// were left out between; the same however the output was read.
#[cfg(unix)]
#[test]
fn output_past_the_cap_keeps_its_head_and_tail() {
    let all: Vec<u8> = (0..100_000).map(|ix| b'a' + (ix % 26) as u8).collect();
    let omitted = 100_000 - HEAD - TAIL;
    let expected = format!(
        "{}\n[{omitted} bytes omitted]\n{}",
        std::str::from_utf8(&all[..HEAD]).unwrap(),
        std::str::from_utf8(&all[all.len() - TAIL..]).unwrap()
    );
    for chunk in [1, 7, 4096, 30_000, 100_000] {
        let mut capture = Capture::default();
        for part in all.chunks(chunk) {
            capture.push(part);
        }
        assert_eq!(capture.text(), expected, "read {chunk} at a time");
    }
    let mut one_over = Capture::default();
    one_over.push(&all[..HEAD + TAIL + 1]);
    assert!(one_over.text().contains("\n[1 bytes omitted]\n"));
}

/// A cut never splits a character: what it leaves of one counts as
/// omitted.
#[cfg(unix)]
#[test]
fn the_cap_cuts_at_character_boundaries() {
    let mut text = "a".repeat(HEAD - 1);
    text.push('é');
    text.push_str(&"b".repeat(50_000));
    text.push('€');
    text.push_str(&"c".repeat(TAIL - 2));
    let mut capture = Capture::default();
    capture.push(text.as_bytes());
    let shown = capture.text();
    let (head, rest) = shown.split_once('\n').unwrap();
    let (omitted, tail) = rest.split_once('\n').unwrap();
    assert_eq!(head, "a".repeat(HEAD - 1));
    assert_eq!(tail, "c".repeat(TAIL - 2));
    let omitted_bytes = text.len() - (HEAD - 1) - (TAIL - 2);
    assert_eq!(omitted, format!("[{omitted_bytes} bytes omitted]"));
}

/// A real command's output past the cap, from both streams.
#[cfg(unix)]
#[test]
fn a_commands_output_is_capped() {
    let temp = folder();
    let ran = run_it(sh(temp.path(), "seq 1 20000; seq 1 3 >&2"));
    let mut all = String::new();
    for n in 1..=20000 {
        all.push_str(&format!("{n}\n"));
    }
    all.push_str("1\n2\n3\n");
    let omitted = all.len() - HEAD - TAIL;
    assert_eq!(ran.output.total(), all.len() as u64);
    let expected = format!(
        "{}\n[{omitted} bytes omitted]\n{}exit 0",
        &all[..HEAD],
        &all[all.len() - TAIL..]
    );
    assert_eq!(ran.result_text(), expected);
}

/// On Windows too, a process that isn't the command's and holds the pipe
/// is read only for the drain, then the pipe is closed on it: no thread
/// is left reading it.
#[cfg(windows)]
#[test]
fn a_process_holding_the_pipe_is_cut_off_at_the_drain() {
    let (reader, writer) = std::io::pipe().unwrap();
    let mut holder = std::process::Command::new("cmd")
        .args(["/c", "ping -n 4 127.0.0.1 >nul & echo late"])
        .stdout(writer)
        .spawn()
        .unwrap();
    let mut output = Output::new(reader).unwrap();
    let started = Instant::now();
    output.drain(Duration::from_millis(250));
    assert!(
        started.elapsed() < Duration::from_secs(2),
        "{:?}",
        started.elapsed()
    );
    assert!(!output.ended, "the holder still has it");
    drop(output.finish());
    let _ = holder.wait();
}
