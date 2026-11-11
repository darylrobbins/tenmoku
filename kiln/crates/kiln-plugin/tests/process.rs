// SPDX-License-Identifier: Apache-2.0

//! Spawning programs for a plugin: only by a grant, only the file recorded for it, with
//! arguments as given, a minimal environment, and output and time capped. The programs are real
//! ones from the system, copied into temporary directories where their identity can be made to
//! change, and scripts: on Unix `printf`, `env` and `sleep` from `/usr/bin` and `#!/bin/sh`
//! scripts; on Windows, which has none of those three, `whoami`, `sort` and `ping` from
//! `System32` and `.cmd` scripts, which run through `cmd.exe`.

#[path = "support/script.rs"]
mod support;

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use kiln_command::capability::{Capability, Grants};
use kiln_plugin::grants::{
    Decision, GrantRecord, GrantStore, GrantStoreError, ProgramIdentity, SuspendReason, evaluate,
    resolve_program,
};
use kiln_plugin::limits::Limits;
#[cfg(unix)]
use kiln_plugin::process::SYSTEM_PATH;
use kiln_plugin::process::{SpawnError, State, minimal_environment};

const ID: &str = "acme.tool";

fn grants(capabilities: &[&str]) -> Grants {
    capabilities
        .iter()
        .map(|capability| Capability::parse(capability).unwrap())
        .collect()
}

/// A directory of its own under cargo's temporary directory for tests.
fn temp_dir(name: &str) -> PathBuf {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("kiln-plugin-process-{}-{name}", std::process::id()));
    fs::remove_dir_all(&dir).ok();
    fs::create_dir_all(&dir).unwrap();
    dir
}

/// The system's `name`, copied into `dir` under its own name (a multi-call binary picks what it
/// runs as by that), and bound as a grant would bind it.
fn copied(dir: &Path, name: &str) -> ProgramIdentity {
    support::system_program(dir, name);
    resolve_program(name, dir.as_os_str()).unwrap()
}

/// The script `name` in `dir`, `sh` on Unix and `cmd` on Windows, bound as a grant would bind it.
fn script(dir: &Path, name: &str, sh: &str, cmd: &str) -> ProgramIdentity {
    support::script(dir, name, sh, cmd);
    resolve_program(name, dir.as_os_str()).unwrap()
}

/// `printf` in `dir`, bound: the system's on Unix. Windows has none, so there it is a script that
/// writes each of its arguments on a line of its own, as `cmd.exe` was given it: in the quotes std
/// put it in, with each `"` in it doubled, if it needed them.
fn printf(dir: &Path) -> ProgramIdentity {
    if cfg!(windows) {
        let each = ":next\nif [%1]==[] exit /b 0\necho(%1\nshift\ngoto next";
        script(dir, "printf", "", each)
    } else {
        copied(dir, "printf")
    }
}

/// What [`printf`] writes given `word` alone: the word itself on Unix, a line on Windows.
fn printed(word: &str) -> String {
    if cfg!(windows) {
        format!("{word}\r\n")
    } else {
        word.to_owned()
    }
}

/// The system's `sleep` in `dir`, bound; Windows has none that runs without a console, so there
/// it is `ping`, given [`nap`]'s arguments.
fn sleeper(dir: &Path) -> ProgramIdentity {
    copied(dir, if cfg!(windows) { "ping" } else { "sleep" })
}

/// [`sleeper`]'s arguments for a sleep of `seconds`: on Windows, pings of this machine a second
/// apart, the first at once.
fn nap(seconds: u32) -> Vec<String> {
    if cfg!(windows) {
        args(&["-n", &(seconds + 1).to_string(), "127.0.0.1"])
    } else {
        vec![seconds.to_string()]
    }
}

/// The script `name` in `dir`, bound, that creates `marker` if it ever runs.
fn marking(dir: &Path, name: &str, marker: &Path) -> ProgramIdentity {
    let path = marker.display();
    script(
        dir,
        name,
        &format!("touch '{path}'"),
        &format!(r#"type nul > "{path}""#),
    )
}

/// A record granting `granted`, with `programs` bound.
fn record(granted: &[&str], programs: &[&ProgramIdentity]) -> GrantRecord {
    GrantRecord {
        version: Some((1, 0, 0)),
        granted: grants(granted),
        programs: programs
            .iter()
            .map(|identity| (identity.name.clone(), (*identity).clone()))
            .collect::<BTreeMap<_, _>>(),
        ..GrantRecord::default()
    }
}

fn args(args: &[&str]) -> Vec<String> {
    args.iter().map(|&arg| arg.to_owned()).collect()
}

fn stdout(state: &mut State, program: &str, arguments: &[&str]) -> String {
    let output = state.spawn(program, &args(arguments), None).unwrap();
    assert_eq!(output.status, Some(0), "{output:?}");
    String::from_utf8(output.stdout).unwrap()
}

#[test]
fn a_granted_program_runs_by_its_recorded_path() {
    let _serial = serially();
    let dir = temp_dir("recorded");
    // A copy of the system's `printf`, given a format; Windows has none, so there `whoami`, which
    // says what it says when run from `System32`.
    let (name, arguments, says) = if cfg!(windows) {
        let output = std::process::Command::new("whoami").output().unwrap();
        ("whoami", vec![], String::from_utf8(output.stdout).unwrap())
    } else {
        ("printf", vec!["%s-%s", "a", "b"], "a-b".to_owned())
    };
    let granted = format!("process.spawn:{name}");
    let copy = support::system_program(&dir, name);
    let program = resolve_program(name, dir.as_os_str()).unwrap();
    assert_eq!(program.path, copy);
    let mut state = State::new(record(&[granted.as_str()], &[&program]), &Limits::default());
    assert_eq!(stdout(&mut state, name, &arguments), says);

    // A program of its own of that name, which no PATH holds: a name looked up again would find
    // the system's.
    let shadow = temp_dir("recorded-shadow");
    let recorded = script(
        &shadow,
        name,
        r#"echo "recorded: $0""#,
        "echo recorded: %~f0",
    );
    let mut state = State::new(
        record(&[granted.as_str()], &[&recorded]),
        &Limits::default(),
    );
    let line_end = if cfg!(windows) { "\r\n" } else { "\n" };
    assert_eq!(
        stdout(&mut state, name, &["%s", "x"]),
        format!("recorded: {}{line_end}", recorded.path.display())
    );

    // `process.spawn:*` runs a name bound to a program the same way.
    let mut state = State::new(
        record(&["process.spawn:*"], &[&recorded]),
        &Limits::default(),
    );
    assert!(stdout(&mut state, name, &[]).starts_with("recorded: "));
}

#[test]
fn an_ungranted_program_is_refused() {
    let _serial = serially();
    let dir = temp_dir("ungranted");
    let printf = printf(&dir);
    let marker = dir.join("ran");
    let tool = marking(&dir, "tool", &marker);
    let refused = |state: &mut State, program: &str| match state.spawn(program, &[], None) {
        Err(SpawnError::NotGranted { program: named }) => assert_eq!(named, program),
        other => panic!("{program} was not refused: {other:?}"),
    };

    // Bound but not granted: the grant decides, not the binding.
    let mut state = State::new(
        record(&["process.spawn:printf"], &[&printf, &tool]),
        &Limits::default(),
    );
    refused(&mut state, "tool");
    refused(&mut state, "sleep");
    refused(&mut state, "");
    refused(&mut state, &tool.path.display().to_string());
    // `process.spawn:*` with nothing bound to the name: it is never looked up.
    let mut state = State::new(record(&["process.spawn:*"], &[]), &Limits::default());
    refused(&mut state, "tool");
    refused(&mut state, "printf");
    // Nothing granted at all.
    refused(&mut State::default(), "printf");
    assert!(!marker.exists(), "an ungranted program ran");
}

#[test]
fn a_changed_binary_suspends_the_grant_before_spawn() {
    let _serial = serially();
    changed_binary_suspends("named", &["process.spawn:printf"]);
    // `process.spawn:*` still covers the name, but a suspended program asks again all the same.
    changed_binary_suspends("wildcard", &["process.spawn:*", "process.spawn:printf"]);
}

/// Grants `granted` through a store, binding `printf`, then overwrites the program: the next
/// spawn is refused, and the store's next decision on the same request asks about `printf`.
fn changed_binary_suspends(case: &str, granted: &[&str]) {
    let dir = temp_dir(&format!("changed-{case}"));
    let printf = printf(&dir);
    let marker = dir.join("ran");
    let requested = grants(granted);
    let store = GrantStore::new(dir.join("state/plugin-grants.toml"));
    store
        .grant(
            ID,
            (1, 0, 0),
            &requested,
            std::slice::from_ref(&printf),
            true,
        )
        .unwrap();
    let mut state = State::new(store.get(ID).unwrap().unwrap(), &Limits::default())
        .recorded_in(store.clone(), ID);
    assert_eq!(
        stdout(&mut state, "printf", &["ok"]),
        printed("ok"),
        "{case}"
    );

    // Overwritten in place: same path, same inode, different content.
    assert_eq!(marking(&dir, "printf", &marker).path, printf.path);
    for _ in 0..2 {
        match state.spawn("printf", &args(&["again"]), None) {
            Err(SpawnError::Suspended { program, suspended }) => {
                assert_eq!(program, "printf");
                assert_eq!(suspended.reason, SuspendReason::Changed);
            }
            other => panic!("{case}: not suspended: {other:?}"),
        }
    }
    assert!(!marker.exists(), "{case}: the changed program ran");

    // The grant is withdrawn, here and in the store: loading again asks about the program.
    let escalation = Decision::Escalation {
        added: grants(&["process.spawn:printf"]),
        keep_running: Some((1, 0, 0)),
    };
    assert_eq!(state.record().suspended["printf"], SuspendReason::Changed);
    assert_eq!(
        evaluate(Some(state.record()), &requested),
        escalation,
        "{case}"
    );
    let stored = store.get(ID).unwrap().unwrap();
    assert_eq!(stored.suspended["printf"], SuspendReason::Changed);
    assert_eq!(evaluate(Some(&stored), &requested), escalation, "{case}");
    let decided = store.decide(ID, (1, 0, 0), &requested, dir.as_os_str());
    assert_eq!(decided.unwrap(), escalation, "{case}");
}

#[test]
fn a_removed_binary_is_suspended_as_missing() {
    let _serial = serially();
    let dir = temp_dir("removed");
    let printf = printf(&dir);
    let mut state = State::new(
        record(&["process.spawn:printf"], &[&printf]),
        &Limits::default(),
    );
    fs::remove_file(&printf.path).unwrap();
    match state.spawn("printf", &[], None) {
        Err(SpawnError::Suspended { suspended, .. }) => {
            assert_eq!(suspended.reason, SuspendReason::Missing);
        }
        other => panic!("not suspended: {other:?}"),
    }
}

#[test]
fn arguments_are_not_shell_parsed() {
    let _serial = serially();
    let dir = temp_dir("arguments");
    let printf = printf(&dir);
    let mut state = State::new(
        record(&["process.spawn:printf"], &[&printf]),
        &Limits::default(),
    );
    #[cfg(unix)]
    {
        let literal = [
            "; echo pwned",
            "$(touch pwned)",
            "`id`",
            "a b",
            "*",
            "'\"",
            "",
        ];
        let mut arguments = vec!["%s|"];
        arguments.extend(literal);
        assert_eq!(
            stdout(&mut state, "printf", &arguments),
            literal.map(|arg| format!("{arg}|")).concat()
        );
    }
    // A batch file's arguments are read again by `cmd.exe`, the shell. Std quotes each that holds
    // what `cmd.exe` would act on, and keeps a `%` from expanding, so each reaches the script
    // whole, as written; none runs a command.
    #[cfg(windows)]
    {
        let literal = [
            "& echo pwned> pwned",
            "| echo pwned",
            "%PATH%",
            "^",
            "\"",
            "a b",
            "*",
            "",
        ];
        let printed = stdout(&mut state, "printf", &literal);
        let each: Vec<String> = printed.lines().map(unquoted).collect();
        assert_eq!(each, literal, "{printed}");
    }
    assert!(!Path::new("pwned").exists());

    // Stdin is the plugin's bytes, as they are. Windows has no `cat`: there `sort`, given one
    // line, writes it as it was (text, though, without a NUL).
    let (cat, bytes) = if cfg!(windows) {
        (copied(&dir, "sort"), &b"; echo pwned & echo pwned\r\n"[..])
    } else {
        let cat = script(&dir, "cat", "exec /bin/cat", "");
        (cat, &b"; echo pwned\n\0bytes"[..])
    };
    let granted = format!("process.spawn:{}", cat.name);
    let mut state = State::new(record(&[granted.as_str()], &[&cat]), &Limits::default());
    let output = state.spawn(&cat.name, &[], Some(bytes)).unwrap();
    assert_eq!(output.stdout, bytes);
}

/// An argument as the Windows [`printf`] script writes it: out of std's quotes, if it is in them,
/// with each `"` in it no longer doubled.
#[cfg(windows)]
fn unquoted(line: &str) -> String {
    match line
        .strip_prefix('"')
        .and_then(|line| line.strip_suffix('"'))
    {
        Some(quoted) => quoted.replace("\"\"", "\""),
        None => line.to_owned(),
    }
}

#[test]
fn the_environment_is_minimal() {
    let _serial = serially();
    let dir = temp_dir("environment");
    // Windows has no `env`: there a script lists the environment with `cmd.exe`'s `set`.
    let env = if cfg!(windows) {
        script(&dir, "env", "", "set")
    } else {
        copied(&dir, "env")
    };
    let mut state = State::new(record(&["process.spawn:env"], &[&env]), &Limits::default());
    let printed = stdout(&mut state, "env", &[]);
    let variables: BTreeMap<&str, &str> = printed
        .lines()
        .map(|line| line.split_once('=').unwrap())
        .collect();
    #[cfg(unix)]
    let (passed, path) = {
        for name in variables.keys() {
            assert!(
                matches!(*name, "PATH" | "HOME" | "LANG" | "TMPDIR" | "TERM")
                    || name.starts_with("LC_"),
                "{name} reached the program:\n{printed}"
            );
        }
        (["HOME", "LANG", "TERM"], SYSTEM_PATH.to_owned())
    };
    #[cfg(windows)]
    let (passed, path) = {
        // Besides what it's given, `cmd.exe` sets `PROMPT`, and `COMSPEC` to itself.
        let given = [
            "PATH",
            "SystemRoot",
            "windir",
            "SystemDrive",
            "USERPROFILE",
            "APPDATA",
            "LOCALAPPDATA",
            "TEMP",
            "TMP",
            "PATHEXT",
            "ProgramData",
            "NUMBER_OF_PROCESSORS",
            "PROCESSOR_ARCHITECTURE",
        ];
        for name in variables.keys() {
            assert!(
                given
                    .iter()
                    .chain(&["COMSPEC", "PROMPT"])
                    .any(|given| given.eq_ignore_ascii_case(name)),
                "{name} reached the program:\n{printed}"
            );
        }
        let path = kiln_core::platform::system_path();
        (
            ["SystemRoot", "USERPROFILE", "TEMP"],
            path.into_string().unwrap(),
        )
    };
    assert_eq!(variables["PATH"], path);
    // What does pass is the app's own value.
    for name in passed {
        assert_eq!(
            variables.get(name).copied(),
            std::env::var(name).ok().as_deref(),
            "{name}"
        );
    }
    // Cargo set these for this test: none of them reaches the program.
    assert!(std::env::var_os("CARGO_MANIFEST_DIR").is_some());
    assert!(!variables.contains_key("CARGO_MANIFEST_DIR"));
}

#[test]
fn the_minimal_environment_keeps_only_its_variables() {
    let _serial = serially();
    #[cfg(unix)]
    let (app, kept) = (
        [
            ("PATH", "/home/me/bin:/usr/bin"),
            ("HOME", "/home/me"),
            ("LANG", "en_CA.UTF-8"),
            ("LC_ALL", "C"),
            ("LC_CTYPE", "C.UTF-8"),
            ("TMPDIR", "/tmp/me"),
            ("TERM", "xterm"),
            ("SSH_AUTH_SOCK", "/tmp/agent"),
            ("LD_PRELOAD", "/tmp/evil.so"),
            ("DYLD_INSERT_LIBRARIES", "/tmp/evil.dylib"),
            ("CLASS", "LC_X"),
        ],
        vec![
            ("PATH", SYSTEM_PATH),
            ("HOME", "/home/me"),
            ("LANG", "en_CA.UTF-8"),
            ("LC_ALL", "C"),
            ("LC_CTYPE", "C.UTF-8"),
            ("TMPDIR", "/tmp/me"),
            ("TERM", "xterm"),
        ],
    );
    // Windows' own names, any case, of which Unix's mean nothing there.
    #[cfg(windows)]
    let (app, kept) = (
        [
            ("Path", r"C:\Users\me\bin;C:\Windows\System32"),
            ("SYSTEMROOT", r"D:\Windows"),
            ("USERPROFILE", r"C:\Users\me"),
            ("temp", r"C:\Users\me\AppData\Local\Temp"),
            ("PATHEXT", ".COM;.EXE;.BAT;.CMD"),
            ("HOME", r"C:\Users\me"),
            ("LANG", "en_CA.UTF-8"),
            ("SSH_AUTH_SOCK", r"\\.\pipe\openssh-ssh-agent"),
            ("PSModulePath", r"C:\Users\me\Documents\PowerShell\Modules"),
        ],
        vec![
            (
                "PATH",
                r"D:\Windows\System32;D:\Windows;D:\Windows\System32\Wbem",
            ),
            ("SystemRoot", r"D:\Windows"),
            ("USERPROFILE", r"C:\Users\me"),
            ("TEMP", r"C:\Users\me\AppData\Local\Temp"),
            ("PATHEXT", ".COM;.EXE;.BAT;.CMD"),
        ],
    );
    let minimal = minimal_environment(app.map(|(name, value)| (name.into(), value.into())));
    let minimal: Vec<_> = minimal
        .iter()
        .map(|(name, value)| (name.to_str().unwrap(), value.to_str().unwrap()))
        .collect();
    assert_eq!(minimal, kept);
}

#[test]
fn output_over_the_cap_and_runs_over_the_deadline_are_killed() {
    let _serial = serially();
    let dir = temp_dir("limits");
    let sleep = sleeper(&dir);
    // A megabyte of zeros. `printf` reuses its format for each argument: a megabyte from a few
    // hundred short ones, well inside any system's argument limit. On Windows a script writes
    // lines of zeros until it's stopped, more than the cap at the first.
    let (printf, flood) = if cfg!(windows) {
        let lines = format!(":again\necho {}\ngoto again", "0".repeat(4096));
        (script(&dir, "printf", "", &lines), Vec::new())
    } else {
        let mut arguments = vec!["%04096d"];
        arguments.extend(["0"; 256]);
        (copied(&dir, "printf"), args(&arguments))
    };
    let limits = Limits {
        max_read_bytes: 16,
        ..Limits::default()
    };
    let sleep_granted = format!("process.spawn:{}", sleep.name);
    let record = record(
        &[sleep_granted.as_str(), "process.spawn:printf"],
        &[&sleep, &printf],
    );
    // The deadline, without the cap: Windows' sleeper, `ping`, writes more than 16 bytes at once.
    let mut state =
        State::new(record.clone(), &Limits::default()).with_deadline(Duration::from_millis(300));

    let started = Instant::now();
    match state.spawn(&sleep.name, &nap(5), None) {
        Err(SpawnError::TimedOut { program, deadline }) => {
            assert_eq!(program, sleep.name);
            assert_eq!(deadline, Duration::from_millis(300));
        }
        other => panic!("not killed: {other:?}"),
    }
    assert!(
        started.elapsed() < Duration::from_secs(2),
        "{:?}",
        started.elapsed()
    );

    // Output is cut at the cap. A megabyte fills the pipe and blocks: the program is killed at
    // the cap, not left to the deadline, which would fail the call.
    let mut state = State::new(record, &limits).with_deadline(Duration::from_millis(1500));
    let output = state.spawn("printf", &flood, None).unwrap();
    assert_eq!(output.stdout, b"0000000000000000");
    assert_eq!(output.status, None);
}

#[test]
fn nothing_outlives_a_call_that_ends() {
    let _serial = serially();
    let dir = temp_dir("outlives");
    let sleep = sleeper(&dir);
    let printf = printf(&dir);
    // Leaves a program of its own holding stdout and stderr open after it exits.
    // The background program is far longer than the elapsed bound below, so a call that waited for
    // it would blow that bound. It is not the call's child, so the call's kill does not reach it:
    // it records its pid and `Reaper` kills it when the test ends, however it ends. On Windows it
    // is `ping`, in the call's job object, which ends with the call.
    let background_pid = dir.join("background.pid");
    let holder = script(
        &dir,
        "holder",
        &format!(
            "sleep 30 &\necho $! > '{}'\necho started",
            background_pid.display()
        ),
        "start \"\" /b ping -n 31 127.0.0.1\necho started",
    );
    let sleep_granted = format!("process.spawn:{}", sleep.name);
    let record = record(
        &[
            sleep_granted.as_str(),
            "process.spawn:printf",
            "process.spawn:holder",
        ],
        &[&sleep, &printf, &holder],
    );
    let mut state =
        State::new(record.clone(), &Limits::default()).with_deadline(Duration::from_millis(300));
    let before = threads();
    let unread = vec![b'x'; 1 << 20];
    let _reaper = Reaper(background_pid);

    let started = Instant::now();
    let held = state.spawn("holder", &[], None);
    assert!(matches!(held, Err(SpawnError::TimedOut { .. })), "{held:?}");
    let ignored = state.spawn(&sleep.name, &nap(30), Some(&unread));
    assert!(
        matches!(ignored, Err(SpawnError::TimedOut { .. })),
        "{ignored:?}"
    );
    // Nominally about 600 ms. The bound leaves seconds of headroom for a loaded machine, yet stays
    // well under the 30 s sleeps, so a call that waited for either would fail it. The `sleep 30`
    // given stdin is the call's own child, which the call's kill ends.
    assert!(
        started.elapsed() < Duration::from_secs(10),
        "{:?}",
        started.elapsed()
    );

    // A program that ends without reading its stdin ends the call. This is the success path, so
    // it gets a deadline a loaded machine cannot miss.
    let mut state = State::new(record, &Limits::default()).with_deadline(Duration::from_secs(30));
    let output = state
        .spawn("printf", &args(&["ok"]), Some(&unread))
        .unwrap();
    assert_eq!(output.stdout, printed("ok").as_bytes());
    assert_eq!(threads(), before, "a call left a thread behind");
}

/// Kills the program a pid file names when dropped, if there is one.
struct Reaper(std::path::PathBuf);

impl Drop for Reaper {
    fn drop(&mut self) {
        if let Ok(pid) = fs::read_to_string(&self.0) {
            drop(std::process::Command::new("kill").arg(pid.trim()).status());
        }
    }
}

/// One test at a time: `nothing_outlives_a_call_that_ends` counts this
/// process's threads, which the others' calls would add to and take from
/// meanwhile when `cargo test` runs them side by side.
fn serially() -> MutexGuard<'static, ()> {
    static SERIAL: Mutex<()> = Mutex::new(());
    SERIAL.lock().unwrap_or_else(PoisonError::into_inner)
}

/// How many threads this process has, where the system says.
fn threads() -> Option<usize> {
    fs::read_dir("/proc/self/task").ok().map(Iterator::count)
}

#[test]
fn a_suspension_holds_until_the_program_is_granted_again() {
    let _serial = serially();
    let dir = temp_dir("suspension");
    let printf = printf(&dir);
    let store = GrantStore::new(dir.join("state/plugin-grants.toml"));
    let path_var = dir.as_os_str();
    let both = grants(&["process.spawn:*", "process.spawn:printf"]);
    let asks_about_printf = |version| Decision::Escalation {
        added: grants(&["process.spawn:printf"]),
        keep_running: Some(version),
    };
    store
        .grant(ID, (1, 0, 0), &both, std::slice::from_ref(&printf), true)
        .unwrap();
    fs::write(&printf.path, "#!/bin/sh\n").unwrap();
    assert_eq!(store.check_programs(ID).unwrap().len(), 1);

    // Dropping the name while `process.spawn:*` still covers it keeps the suspension, so naming
    // it again asks.
    let wildcard = grants(&["process.spawn:*"]);
    let decided = store.decide(ID, (1, 0, 0), &wildcard, path_var).unwrap();
    assert_eq!(decided, Decision::Load(wildcard.clone()));
    assert_eq!(
        store.get(ID).unwrap().unwrap().suspended["printf"],
        SuspendReason::Changed
    );
    let mut state = State::new(store.get(ID).unwrap().unwrap(), &Limits::default());
    let spawned = state.spawn("printf", &[], None);
    assert!(
        matches!(spawned, Err(SpawnError::Suspended { .. })),
        "{spawned:?}"
    );
    let decided = store.decide(ID, (1, 0, 0), &both, path_var).unwrap();
    assert_eq!(decided, asks_about_printf((1, 0, 0)));

    // Granting something unrelated keeps it too.
    let unrelated = grants(&["process.spawn:*", "buffer.read"]);
    store.grant(ID, (2, 0, 0), &unrelated, &[], true).unwrap();
    assert!(
        store
            .get(ID)
            .unwrap()
            .unwrap()
            .suspended
            .contains_key("printf")
    );
    let with_printf = grants(&["process.spawn:*", "process.spawn:printf", "buffer.read"]);
    let decided = store.decide(ID, (2, 0, 0), &with_printf, path_var).unwrap();
    assert_eq!(decided, asks_about_printf((2, 0, 0)));
    // Naming it at a prompt needs the program bound anew; nothing else lifts it.
    let refused = store.grant(ID, (2, 0, 0), &with_printf, &[], true);
    assert!(
        matches!(&refused, Err(GrantStoreError::Suspended(program)) if program == "printf"),
        "{refused:?}"
    );
    assert!(
        store
            .get(ID)
            .unwrap()
            .unwrap()
            .suspended
            .contains_key("printf")
    );

    // Binding it anew at a prompt lifts the suspension.
    let rebound = resolve_program("printf", path_var).unwrap();
    let record = store
        .grant(ID, (2, 0, 0), &with_printf, &[rebound], true)
        .unwrap();
    assert!(record.suspended.is_empty());
    let decided = store.decide(ID, (2, 0, 0), &with_printf, path_var).unwrap();
    assert_eq!(decided, Decision::Load(with_printf));

    // With nothing left to cover it, a suspension has nothing to hold.
    fs::write(&printf.path, "#!/bin/sh\nexit 1\n").unwrap();
    assert_eq!(store.check_programs(ID).unwrap().len(), 1);
    let unrelated = grants(&["buffer.read"]);
    let decided = store.decide(ID, (2, 0, 0), &unrelated, path_var).unwrap();
    assert_eq!(decided, Decision::Load(unrelated));
    assert!(store.get(ID).unwrap().unwrap().suspended.is_empty());
}
