//! The agents' audit trail ([`super`]): its lines, its file and folder
//! modes, rotation at 1 MiB with one kept, the worker's order, nothing
//! written for an untrusted folder, and `agent.show_audit_log`.

use std::{
    fs,
    path::{Path, PathBuf},
    time::{Duration, UNIX_EPOCH},
};

use fe_chat::{ChatId, permissions::Class};
use fe_command::{ContextFrame, ContextStack, Invocation};
use fe_gpui::commands::RunCommand;
use gpui::{Entity, TestAppContext, VisualTestContext};
use serde_json::{Value, json};

use super::*;
use crate::{
    Launch, Workbench,
    agent_command::{Capture, Program},
    chat_panel::ChatStateDir,
    tests::open,
};

/// A folder under the real temp folder.
fn folder() -> tempfile::TempDir {
    let temp = dunce::canonicalize(std::env::temp_dir()).unwrap();
    tempfile::tempdir_in(temp).unwrap()
}

fn git_status() -> Prepared {
    Prepared {
        argv: vec!["git".to_owned(), "status".to_owned()],
        program: Program::Resolved(PathBuf::from("/usr/bin/git")),
        cwd: PathBuf::from("/work/project"),
        path: None,
    }
}

fn ran(end: End) -> Ran {
    Ran {
        output: Capture::default(),
        end,
        timeout: Duration::from_secs(120),
        elapsed: Duration::from_secs(1),
    }
}

fn parsed(entry: &Entry) -> Value {
    let line = entry.line();
    assert!(!line.contains('\n'), "one line: {line}");
    serde_json::from_str(&line).unwrap()
}

/// What a line holds but its time.
fn timeless(entry: &Entry) -> Value {
    let mut value = parsed(entry);
    let time = value.as_object_mut().unwrap().remove("time").unwrap();
    assert!(time.as_str().unwrap().ends_with('Z'), "{time}");
    value
}

// ---------------------------------------------------------------------------
// Lines

#[test]
fn a_commands_line_holds_its_argv_resolved_program_and_end() {
    let chat = ChatId::new();
    let by = Decider::Rule("git status".to_owned());
    let exited = Entry::command(chat, &git_status(), Verdict::Allow, &by)
        .ended(&ran(End::Exited(0)), Killed::Stop);
    assert_eq!(
        timeless(&exited),
        json!({
            "agent": "fastedit.chat",
            "session": chat.to_string(),
            "class": "run_command",
            "target": ["git", "status"],
            "decision": "allow",
            "decided_by": "rule:git status",
            "program": "/usr/bin/git",
            "exit": 0,
        })
    );

    let timed_out = Entry::command(chat, &git_status(), Verdict::Allow, &Decider::User)
        .ended(&ran(End::Killed(KillReason::Timeout)), Killed::Stop);
    let value = timeless(&timed_out);
    assert_eq!(value["killed"], "timeout");
    assert_eq!(value["decided_by"], "user");
    assert!(value.get("exit").is_none());

    for (cancelled, why) in [
        (Killed::Stop, "stop"),
        (Killed::TrustWithdrawn, "trust_withdrawn"),
    ] {
        let killed = Entry::command(chat, &git_status(), Verdict::Allow, &Decider::Session)
            .ended(&ran(End::Killed(KillReason::Cancelled)), cancelled);
        assert_eq!(timeless(&killed)["killed"], why);
    }

    let denied = Entry::command(chat, &git_status(), Verdict::Deny, &Decider::Hard);
    let value = timeless(&denied);
    assert_eq!(value["decision"], "deny");
    assert_eq!(value["decided_by"], "hard");
    assert!(value.get("exit").is_none() && value.get("killed").is_none());
}

/// A command a signal of its own ended records the signal, and one that
/// never started records why: neither reads as a line with no end.
#[test]
fn a_signal_or_a_failed_start_is_recorded() {
    let chat = ChatId::new();
    let signalled = Entry::command(chat, &git_status(), Verdict::Allow, &Decider::User)
        .ended(&ran(End::Signalled(11)), Killed::Stop);
    let value = timeless(&signalled);
    assert_eq!(value["signal"], 11);
    assert!(value.get("exit").is_none() && value.get("killed").is_none());

    let why = "`/usr/bin/git` couldn't start: Permission denied (os error 13)";
    let failed = Entry::command(chat, &git_status(), Verdict::Allow, &Decider::User)
        .ended(&ran(End::NotStarted(why.to_owned())), Killed::Stop);
    let value = timeless(&failed);
    assert_eq!(value["not_started"], why);
    assert!(value.get("exit").is_none() && value.get("signal").is_none());
}

/// A real command killed by a segfault of its own, through the executor.
#[cfg(unix)]
#[test]
fn a_command_a_signal_ended_is_recorded_with_it() {
    use crate::agent_command::{Limits, RunningCommand, prepare};
    let root = folder();
    let input = fe_chat::RunCommandInput {
        program: "sh".to_owned(),
        args: vec!["-c".to_owned(), "kill -SEGV $$".to_owned()],
        cwd: None,
    };
    let path = std::ffi::OsString::from("/usr/bin:/bin");
    let prepared = prepare(&input, root.path(), &[], Some(&path)).unwrap();
    let ran = RunningCommand::start(prepared.clone(), Limits::secs(30))
        .unwrap()
        .wait();
    assert_eq!(ran.end, End::Signalled(libc::SIGSEGV));
    let entry = Entry::command(ChatId::new(), &prepared, Verdict::Allow, &Decider::User)
        .ended(&ran, Killed::Stop);
    assert_eq!(timeless(&entry)["signal"], libc::SIGSEGV);
}

#[test]
fn other_requests_record_their_class_and_target() {
    let read = Entry::new(
        CLAUDE_AGENT,
        "session-1",
        Class::Read,
        Target::Text("src/lib.go".to_owned()),
        Verdict::Cancelled,
        &Decider::User,
    );
    assert_eq!(
        timeless(&read),
        json!({
            "agent": "acp.claude-agent",
            "session": "session-1",
            "class": "read",
            "target": "src/lib.go",
            "decision": "cancelled",
            "decided_by": "user",
        })
    );
    let names: Vec<&str> = Class::ALL.into_iter().map(class_name).collect();
    assert_eq!(
        names,
        [
            "read",
            "search",
            "propose_edit",
            "run_command",
            "fetch",
            "other"
        ]
    );
}

#[test]
fn times_are_rfc3339_in_utc() {
    let at = |secs: u64, millis: u64| {
        rfc3339(UNIX_EPOCH + Duration::from_secs(secs) + Duration::from_millis(millis))
    };
    assert_eq!(at(0, 0), "1970-01-01T00:00:00.000Z");
    assert_eq!(at(951_782_400, 5), "2000-02-29T00:00:00.005Z");
    assert_eq!(at(1_709_251_199, 999), "2024-02-29T23:59:59.999Z");
    assert_eq!(at(1_794_647_107, 123), "2026-11-14T09:05:07.123Z");
}

// ---------------------------------------------------------------------------
// The file

#[test]
fn the_log_is_under_the_state_dir_by_the_roots_layout_key() {
    let file = audit_file(Path::new("/state"), Path::new("/work/project"));
    let key = kiln::core::layout::layout_key(Path::new("/work/project"));
    assert_eq!(
        file,
        Path::new("/state/agents").join(key).join("audit.jsonl")
    );
    assert_eq!(rotated(&file), file.with_file_name("audit.jsonl.1"));
}

#[cfg(unix)]
#[test]
fn the_file_is_0600_in_a_0700_folder() {
    use std::os::unix::fs::PermissionsExt as _;
    let state = folder();
    let file = audit_file(state.path(), Path::new("/work/project"));
    append(&file, "{}").unwrap();
    let mode = |path: &Path| fs::metadata(path).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode(&file), 0o600);
    assert_eq!(mode(file.parent().unwrap()), 0o700);
    // One left wider is narrowed on the next line.
    fs::set_permissions(&file, fs::Permissions::from_mode(0o644)).unwrap();
    append(&file, "{}").unwrap();
    assert_eq!(mode(&file), 0o600);
    assert_eq!(fs::read_to_string(&file).unwrap(), "{}\n{}\n");
}

/// A line that would take the log past 1 MiB moves it to `.1` first; the
/// next rotation replaces that one. No line is split, and reading the log
/// gives the kept `.1` and then the log.
#[test]
fn the_log_rotates_at_a_mebibyte_keeping_one() {
    let state = folder();
    let file = audit_file(state.path(), Path::new("/work/project"));
    let line = |n: usize| format!("{{\"n\":{n:06},\"pad\":\"{}\"}}", "x".repeat(1000));
    let per_line = line(0).len() as u64 + 1;
    let per_file = (MAX_LOG / per_line) as usize;
    for n in 0..per_file {
        append(&file, &line(n)).unwrap();
    }
    assert!(!rotated(&file).exists(), "{per_file} lines fit");
    append(&file, &line(per_file)).unwrap();
    assert!(rotated(&file).exists());
    let lines = |path: &Path| -> Vec<String> {
        fs::read_to_string(path)
            .unwrap()
            .lines()
            .map(str::to_owned)
            .collect()
    };
    assert_eq!(
        lines(&rotated(&file)),
        (0..per_file).map(line).collect::<Vec<_>>()
    );
    assert_eq!(lines(&file), [line(per_file)]);

    for n in per_file + 1..2 * per_file + 1 {
        append(&file, &line(n)).unwrap();
    }
    assert!(fs::metadata(&file).unwrap().len() <= MAX_LOG);
    assert_eq!(
        lines(&rotated(&file)),
        (per_file..2 * per_file).map(line).collect::<Vec<_>>(),
        "the second rotation replaced the first's"
    );
    assert_eq!(lines(&file), [line(2 * per_file)]);
    let mut names: Vec<String> = fs::read_dir(file.parent().unwrap())
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    assert_eq!(names, ["audit.jsonl", "audit.jsonl.1"]);
    let kept = read_log(&file).unwrap();
    assert_eq!(kept.lines().count(), per_file + 1);
    assert!(kept.starts_with(&line(per_file)));
    assert!(kept.ends_with(&format!("{}\n", line(2 * per_file))));
}

// ---------------------------------------------------------------------------
// The worker

fn entry(n: usize) -> Entry {
    Entry::new(
        API_CHAT,
        "chat",
        Class::Search,
        Target::Text(format!("pattern {n}")),
        Verdict::Allow,
        &Decider::Policy,
    )
}

fn targets(text: &str) -> Vec<String> {
    text.lines()
        .map(|line| {
            let value: Value = serde_json::from_str(line).unwrap();
            value["target"].as_str().unwrap().to_owned()
        })
        .collect()
}

/// The by-hand worker touches the disk only when run, in the order asked,
/// and a read sees every write queued before it.
#[test]
fn the_worker_writes_in_order_and_reads_after_queued_writes() {
    let state = folder();
    let file = audit_file(state.path(), Path::new("/work/project"));
    let log = AuditLog::new(true);
    for n in 0..3 {
        assert!(log.write(file.clone(), &entry(n)));
    }
    let reply = log.read(file.clone());
    assert!(!file.exists(), "nothing written before the worker runs");
    log.run_pending();
    let text = reply.recv_blocking().unwrap().unwrap();
    assert_eq!(targets(&text), ["pattern 0", "pattern 1", "pattern 2"]);
}

/// The threaded worker, shut down, writes what was queued first, then
/// takes nothing more.
#[test]
fn shutting_the_worker_down_writes_what_was_queued() {
    let state = folder();
    let file = audit_file(state.path(), Path::new("/work/project"));
    let log = AuditLog::new(false);
    for n in 0..50 {
        log.write(file.clone(), &entry(n));
    }
    log.shutdown();
    assert_eq!(targets(&read_log(&file).unwrap()).len(), 50);
    assert!(!log.write(file.clone(), &entry(50)));
    assert!(log.read(file.clone()).recv_blocking().is_err());
    assert_eq!(targets(&read_log(&file).unwrap()).len(), 50);
}

// ---------------------------------------------------------------------------
// In the workbench

fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.run_until_parked();
}

/// A workbench on a folder of a `.txt` file, with its state directory in
/// `state`.
fn opened<'a>(
    root: &Path,
    state: &Path,
    cx: &'a mut TestAppContext,
) -> (Entity<Workbench>, &'a mut VisualTestContext) {
    cx.executor().allow_parking();
    fs::write(root.join("notes.txt"), "notes\n").unwrap();
    cx.update(|cx| cx.set_global(ChatStateDir(Some(state.to_owned()))));
    let launch = Launch {
        root: Some(root.to_owned()),
        file: None,
    };
    let (workbench, cx, _) = open(cx, launch);
    draw(cx);
    (workbench, cx)
}

fn toasts(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Vec<String> {
    let layer = workbench.read_with(cx, |workbench, _| workbench.toasts().clone());
    layer.read_with(cx, |layer, _| {
        (layer.queue().shown().iter())
            .map(|entry| entry.toast.message.clone())
            .collect()
    })
}

fn show_audit_log(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) {
    // An action reaches the root's listeners only from focus within.
    cx.update(|window, cx| {
        if window.focused(cx).is_none() {
            window.focus(&workbench.focus_handle(cx), cx);
        }
    });
    let run = RunCommand(
        Invocation::of("agent.show_audit_log")
            .unwrap()
            .context(ContextStack::new().with(ContextFrame::new("workbench"))),
    );
    cx.update(|window, cx| window.dispatch_action(Box::new(run), cx));
    draw(cx);
    cx.update(|_, cx| AuditLog::of(cx).run_pending());
    draw(cx);
}

/// An untrusted folder writes nothing; once trusted, its lines are queued
/// and written.
#[gpui::test]
fn an_untrusted_folder_writes_nothing(cx: &mut TestAppContext) {
    let (root, state) = (folder(), folder());
    let (_workbench, cx) = opened(root.path(), state.path(), cx);
    let file = cx.update(|_, cx| file_for(root.path(), cx)).unwrap();
    assert!(file.starts_with(state.path().join("agents")));

    assert!(!cx.update(|_, cx| record(root.path(), &entry(0), cx)));
    cx.update(|_, cx| AuditLog::of(cx).run_pending());
    assert!(!file.exists());

    cx.update(|_, cx| kiln_shell::trust::decide(root.path(), true, cx));
    assert!(cx.update(|_, cx| record(root.path(), &entry(1), cx)));
    cx.update(|_, cx| AuditLog::of(cx).run_pending());
    assert_eq!(targets(&fs::read_to_string(&file).unwrap()), ["pattern 1"]);

    cx.update(|_, cx| kiln_shell::trust::decide(root.path(), false, cx));
    assert!(!cx.update(|_, cx| record(root.path(), &entry(2), cx)));
}

/// A command started while the folder was trusted keeps its line when
/// trust is withdrawn and the command is killed for it: its destination
/// was taken while trusted. A request made after that writes nothing.
#[cfg(unix)]
#[gpui::test]
fn a_command_killed_as_trust_is_withdrawn_keeps_its_line(cx: &mut TestAppContext) {
    use crate::agent_command::{Limits, RunningCommand, prepare};
    let (root, state) = (folder(), folder());
    let (_workbench, cx) = opened(root.path(), state.path(), cx);
    cx.update(|_, cx| kiln_shell::trust::decide(root.path(), true, cx));
    let kept = cx
        .update(|_, cx| destination(root.path(), cx))
        .expect("trusted");

    let input = fe_chat::RunCommandInput {
        program: "sleep".to_owned(),
        args: vec!["30".to_owned()],
        cwd: None,
    };
    let path = std::ffi::OsString::from("/usr/bin:/bin");
    let prepared = prepare(&input, root.path(), &[], Some(&path)).unwrap();
    let running = RunningCommand::start(prepared.clone(), Limits::secs(60)).unwrap();
    let chat = ChatId::new();

    cx.update(|_, cx| kiln_shell::trust::decide(root.path(), false, cx));
    assert!(cx.update(|_, cx| destination(root.path(), cx)).is_none());
    running.cancel();
    let ran = running.wait();
    let killed = Entry::command(chat, &prepared, Verdict::Allow, &Decider::User)
        .ended(&ran, Killed::TrustWithdrawn);
    assert!(cx.update(|_, cx| kept.record(&killed, cx)));
    assert!(!cx.update(|_, cx| record(root.path(), &entry(9), cx)));
    cx.update(|_, cx| AuditLog::of(cx).run_pending());

    let text = fs::read_to_string(kept.file()).unwrap();
    let lines: Vec<Value> = text
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(lines.len(), 1, "{text}");
    assert_eq!(lines[0]["killed"], "trust_withdrawn");
    assert_eq!(lines[0]["target"], json!(["sleep", "30"]));
}

/// `agent.show_audit_log` opens the folder's log, the kept `.1` first, in
/// a read-only tab.
#[gpui::test]
fn show_audit_log_opens_the_log_read_only(cx: &mut TestAppContext) {
    let (root, state) = (folder(), folder());
    let (workbench, cx) = opened(root.path(), state.path(), cx);
    let file = cx.update(|_, cx| file_for(root.path(), cx)).unwrap();
    append(&rotated(&file), "{\"older\":1}").unwrap();
    append(&file, "{\"newer\":2}").unwrap();

    show_audit_log(&workbench, cx);
    let editor = workbench.read_with(cx, |workbench, cx| workbench.active_editor(cx).cloned());
    let editor = editor.expect("the log is open");
    editor.read_with(cx, |editor, _| {
        assert!(editor.is_read_only());
        assert_eq!(
            editor.model().buffer().text().to_string(),
            "{\"older\":1}\n{\"newer\":2}\n"
        );
        assert!(!editor.model().is_dirty());
    });
    assert_eq!(toasts(&workbench, cx), Vec::<String>::new());
}

/// With nothing logged, a toast says so and no tab opens.
#[gpui::test]
fn show_audit_log_with_nothing_logged_says_so(cx: &mut TestAppContext) {
    let (root, state) = (folder(), folder());
    let (workbench, cx) = opened(root.path(), state.path(), cx);
    let before = workbench.read_with(cx, |workbench, cx| workbench.open_editors(cx).len());
    show_audit_log(&workbench, cx);
    assert_eq!(
        toasts(&workbench, cx),
        ["No agent actions are logged for this folder yet"]
    );
    let after = workbench.read_with(cx, |workbench, cx| workbench.open_editors(cx).len());
    assert_eq!(after, before);
}
