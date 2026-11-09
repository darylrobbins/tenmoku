//! Vim's `!` filters and `:r !{cmd}` in a real workbench, through
//! `/bin/sh` (`vim.shell`): run off the UI thread in a trusted folder for
//! the user's keys only, each one undo step; refused, failed, stopped or
//! overtaken by an edit, the text as it was; `%` shell-escaped.

// What only its Unix tests use goes unused elsewhere.
#![cfg_attr(not(unix), allow(unused_imports, dead_code))]

use std::path::Path;
use std::thread;
use std::time::{Duration, Instant};

use fe_command::Actor;
use fe_gpui::EditorView;
use fe_vim::{HostCommand, ShellCommand, ShellPart};
use gpui::{Entity, SharedString, Task, TestAppContext, VisualTestContext};
use kiln_platform::config::{self, BatchSender, ConfigService};

use super::super::toast::Level;
use super::super::vim_host::vim_filter::{
    self, CANCELLED, CHANGED, E194, E499, GRACE, NO_NAME_HERE, NO_NAMES_ON_WINDOWS, OUTPUT_LIMIT,
    Outcome, Platform, Quoting, Run, Shell, TOAST_AFTER, UNKNOWN_SHELL, UNNAMEABLE, UNTRUSTED,
    WATCH, resolve_shell,
};
use super::{active, draw, ex, message, open, run, window};
use crate::Workbench;

/// The user's settings: `/bin/sh` as the shell (`cmd.exe` on Windows), and
/// `limit` ms to run.
fn settings(limit: u64) -> String {
    let shell = if cfg!(windows) { "cmd.exe" } else { "/bin/sh" };
    format!("[vim]\nshell = \"{shell}\"\nfilter_time_limit = {limit}\n")
}

/// A config service holding the user's settings file, and its sender.
#[cfg_attr(not(unix), allow(dead_code))]
struct Settings {
    path: std::path::PathBuf,
    sender: BatchSender,
    _service: Task<()>,
}

impl Settings {
    fn start(dir: &Path, cx: &mut TestAppContext) -> Self {
        let path = dir.join("settings.toml");
        std::fs::write(&path, settings(10_000)).unwrap();
        let store = fe_config::SettingsStore::load(Some(path.clone()), None);
        let (sender, batches) = config::batch_channel();
        let service = cx.update(|cx| {
            ConfigService::new()
                .settings(store.into_inner())
                .start_with(batches, cx)
        });
        Self {
            path,
            sender,
            _service: service,
        }
    }

    /// Sets `vim.filter_time_limit` to `limit`.
    #[cfg(unix)]
    fn limit(&self, limit: u64, cx: &mut VisualTestContext) {
        std::fs::write(&self.path, settings(limit)).unwrap();
        self.sender.send(vec![self.path.clone()]);
        draw(cx);
        let set = cx.update(|_, cx| {
            config::settings::<fe_config::Settings>(cx)
                .vim
                .filter_time_limit
        });
        assert_eq!(set, limit);
    }
}

/// Trusts the workbench's root, as Trust This Folder does.
fn trust(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) {
    workbench.update(cx, |workbench, cx| workbench.set_root_trust(true, cx));
    draw(cx);
    assert!(workbench.read_with(cx, |workbench, _| workbench.root_trusted()));
}

fn text(editor: &Entity<EditorView>, cx: &mut VisualTestContext) -> String {
    editor.read_with(cx, |editor, _| editor.model().text().to_string())
}

/// Draws until `done` holds, sleeping between draws for the command's
/// thread. Panics after 10 s, naming `what`.
fn until(
    cx: &mut VisualTestContext,
    what: &str,
    mut done: impl FnMut(&mut VisualTestContext) -> bool,
) {
    until_within(cx, what, Duration::from_secs(10), &mut done, |_| {
        String::new()
    });
}

/// As [`until`], waiting up to `limit`, and naming what `shown` says on
/// timing out.
fn until_within(
    cx: &mut VisualTestContext,
    what: &str,
    limit: Duration,
    mut done: impl FnMut(&mut VisualTestContext) -> bool,
    shown: impl Fn(&mut VisualTestContext) -> String,
) {
    let deadline = Instant::now() + limit;
    loop {
        draw(cx);
        if done(cx) {
            return;
        }
        if Instant::now() >= deadline {
            panic!("timed out waiting for {what}: {}", shown(cx));
        }
        thread::sleep(Duration::from_millis(10));
    }
}

/// Waits until the vim message is `wanted`.
fn until_message(workbench: &Entity<Workbench>, wanted: &str, cx: &mut VisualTestContext) {
    let mut last = None;
    until(cx, wanted, |cx| {
        last = message(workbench, cx);
        last.as_deref() == Some(wanted)
    });
}

/// Lets a command that would run have time to, and draws.
fn linger(cx: &mut VisualTestContext) {
    thread::sleep(Duration::from_millis(300));
    draw(cx);
}

/// Each toast shown: its level, message and detail.
fn toasts(
    workbench: &Entity<Workbench>,
    cx: &mut VisualTestContext,
) -> Vec<(Level, String, Option<String>)> {
    let layer = workbench.read_with(cx, |workbench, _| workbench.toasts().clone());
    layer.read_with(cx, |layer, _| {
        (layer.queue().shown().iter())
            .map(|entry| {
                let toast = &entry.toast;
                (toast.level, toast.message.clone(), toast.detail.clone())
            })
            .collect()
    })
}

/// The primary caret's row.
fn row(editor: &Entity<EditorView>, cx: &mut VisualTestContext) -> u32 {
    editor.read_with(cx, |editor, _| {
        let model = editor.model();
        model
            .text()
            .offset_to_point(model.primary_selection().head())
            .row
    })
}

#[gpui::test]
// POSIX shell scripts: Windows filters through cmd.exe (see
// filter_runs_through_cmd).
#[cfg(unix)]
fn filter_runs_through_the_shell(cx: &mut TestAppContext) {
    // The command's thread wakes the task waiting for it.
    cx.executor().allow_parking();
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("root");
    std::fs::create_dir(&root).unwrap();
    std::fs::write(root.join("a.txt"), "c\na\nb\n").unwrap();
    let _settings = Settings::start(dir.path(), cx);
    let (workbench, mut vcx) = window(cx, &root, &["a.txt"]);
    let cx = &mut vcx;
    trust(&workbench, cx);
    let a = active(&workbench, cx);

    // :%!sort sorts the buffer, one undo step.
    ex("%!sort", cx);
    until(cx, "sorted", |cx| text(&a, cx) == "a\nb\nc\n");
    cx.simulate_keystrokes("u");
    assert_eq!(text(&a, cx), "c\na\nb\n");

    // 2!! opens :.,.+1! for the two rows from the caret's.
    cx.simulate_keystrokes("g g 2 ! !");
    cx.simulate_input("tr a-z A-Z");
    cx.simulate_keystrokes("enter");
    until(cx, "upcased", |cx| text(&a, cx) == "C\nA\nb\n");
    cx.simulate_keystrokes("u");
    assert_eq!(text(&a, cx), "c\na\nb\n");

    // :r !cmd puts its rows below the caret's line.
    cx.simulate_keystrokes("g g");
    ex("r !printf 'x\\ny\\n'", cx);
    until(cx, "read", |cx| text(&a, cx) == "c\nx\ny\na\nb\n");
    cx.simulate_keystrokes("u");
    assert_eq!(text(&a, cx), "c\na\nb\n");
    // Below the last line, the file keeps its final line break.
    ex("$r !echo z", cx);
    until(cx, "read at the end", |cx| text(&a, cx) == "c\na\nb\nz\n");
    cx.simulate_keystrokes("u");
    assert_eq!(text(&a, cx), "c\na\nb\n");
    // :$! filters the last line, not the empty row after its line break.
    ex("$!tr a-z A-Z", cx);
    until(cx, "the last line upcased", |cx| {
        text(&a, cx) == "c\na\nB\n"
    });
    cx.simulate_keystrokes("u");
    assert_eq!(text(&a, cx), "c\na\nb\n");

    // A slow command leaves the window working while it runs.
    cx.simulate_keystrokes("g g");
    ex("%!sleep 1; sort", cx);
    assert_eq!(text(&a, cx), "c\na\nb\n");
    cx.simulate_keystrokes("j");
    draw(cx);
    assert_eq!(row(&a, cx), 1, "the caret moved while the command ran");
    assert_eq!(text(&a, cx), "c\na\nb\n");
    until(cx, "sorted after the sleep", |cx| {
        text(&a, cx) == "a\nb\nc\n"
    });
}

#[gpui::test]
#[cfg(unix)]
fn filter_refusals_keep_the_buffer(cx: &mut TestAppContext) {
    // The command's thread wakes the task waiting for it.
    cx.executor().allow_parking();
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("root");
    std::fs::create_dir(&root).unwrap();
    std::fs::write(root.join("a.txt"), "c\na\nb\n").unwrap();
    let settings = Settings::start(dir.path(), cx);
    let (workbench, mut vcx) = window(cx, &root, &["a.txt"]);
    let cx = &mut vcx;
    let a = active(&workbench, cx);
    let unchanged = "c\na\nb\n";

    // Untrusted, nothing runs.
    ex("%!touch ran", cx);
    linger(cx);
    assert_eq!(message(&workbench, cx).as_deref(), Some(UNTRUSTED));
    assert!(!root.join("ran").exists());
    assert_eq!(text(&a, cx), unchanged);

    // Trusted, a filter anyone but the user asked for runs nothing.
    trust(&workbench, cx);
    let host = HostCommand::Filter {
        rows: 0..3,
        command: ShellCommand {
            parts: vec![ShellPart::Text("touch ran".into())],
        },
        actor: Actor::Host("a plugin".into()),
    };
    let weak = a.downgrade();
    workbench.update_in(cx, |workbench, window, cx| {
        workbench.run_vim_host_command(&weak, host, window, cx)
    });
    linger(cx);
    assert_eq!(
        message(&workbench, cx).as_deref(),
        Some(fe_vim::messages::shell_refused().as_str())
    );
    assert!(!root.join("ran").exists());

    // A file reached through a symlink to a folder outside the root is
    // outside it.
    let outside = dir.path().join("outside");
    std::fs::create_dir(&outside).unwrap();
    std::fs::write(outside.join("f.txt"), "f\n").unwrap();
    std::os::unix::fs::symlink(&outside, root.join("link")).unwrap();
    open(&workbench, root.join("link/f.txt"), cx);
    let linked = active(&workbench, cx);
    assert_ne!(linked, a);
    ex(".!touch ran", cx);
    linger(cx);
    assert_eq!(message(&workbench, cx).as_deref(), Some(UNTRUSTED));
    assert!(!root.join("ran").exists() && !outside.join("ran").exists());
    assert_eq!(text(&linked, cx), "f\n");
    run("workbench.close_tab", cx);
    assert_eq!(active(&workbench, cx), a);

    // A failing command changes nothing, and shows its stderr.
    ex("%!sh -c 'echo out; echo err >&2; exit 3'", cx);
    until_message(&workbench, "shell returned 3", cx);
    assert_eq!(text(&a, cx), unchanged);
    let failed = toasts(&workbench, cx);
    let toast = (failed.iter())
        .find(|(level, _, _)| *level == Level::Error)
        .unwrap_or_else(|| panic!("an error toast: {failed:?}"));
    assert!(toast.1.ends_with("exited with 3"), "{toast:?}");
    assert!(
        toast
            .2
            .as_deref()
            .is_some_and(|detail| detail.contains("err")),
        "{toast:?}"
    );
    assert!(!toast.2.as_deref().unwrap().contains("out"), "{toast:?}");

    // An edit while it runs keeps the edit, not the output.
    cx.simulate_keystrokes("g g");
    ex("%!sleep 1; sort", cx);
    cx.simulate_keystrokes("x");
    draw(cx);
    assert_eq!(text(&a, cx), "\na\nb\n");
    until_message(&workbench, CHANGED, cx);
    assert_eq!(text(&a, cx), "\na\nb\n");
    cx.simulate_keystrokes("u");
    draw(cx);
    assert_eq!(text(&a, cx), unchanged);

    // Output that lands while vim is in insert mode isn't put in that
    // insert's undo step: it changes nothing.
    ex("%!sleep 0.5; sort", cx);
    cx.simulate_keystrokes("i");
    until_message(&workbench, vim_filter::NOT_NORMAL, cx);
    assert_eq!(text(&a, cx), unchanged);
    cx.simulate_keystrokes("z escape u");
    draw(cx);
    assert_eq!(text(&a, cx), unchanged, "u undoes the typing alone");

    // Past the time limit it's killed, changing nothing.
    settings.limit(200, cx);
    let started = Instant::now();
    ex("%!sleep 5; sort", cx);
    until(cx, "the time limit", |cx| {
        message(&workbench, cx).is_some_and(|said| said.contains("ran past 200 ms"))
    });
    assert!(
        started.elapsed() < Duration::from_secs(4),
        "{:?}",
        started.elapsed()
    );
    assert_eq!(text(&a, cx), unchanged);

    // Cancelled, the same.
    settings.limit(10_000, cx);
    let started = Instant::now();
    ex("%!sleep 5; sort", cx);
    linger(cx);
    run(vim_filter::CANCEL, cx);
    until_message(&workbench, CANCELLED, cx);
    assert!(
        started.elapsed() < Duration::from_secs(4),
        "{:?}",
        started.elapsed()
    );
    assert_eq!(text(&a, cx), unchanged);
}

#[gpui::test]
// POSIX shell scripts: Windows filters through cmd.exe (see
// filter_runs_through_cmd).
#[cfg(unix)]
fn file_names_are_shell_escaped(cx: &mut TestAppContext) {
    // The command's thread wakes the task waiting for it.
    cx.executor().allow_parking();
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("root");
    std::fs::create_dir(&root).unwrap();
    let name = "a b; rm -rf x.txt";
    std::fs::write(root.join(name), "row\n").unwrap();
    std::fs::write(root.join("x.txt"), "x\n").unwrap();
    let _settings = Settings::start(dir.path(), cx);
    let (workbench, mut vcx) = window(cx, &root, &[name]);
    let cx = &mut vcx;
    trust(&workbench, cx);
    let a = active(&workbench, cx);

    ex(".!echo %", cx);
    until(cx, "the name echoed", |cx| {
        text(&a, cx) == format!("{name}\n")
    });
    assert!(root.join("x.txt").exists());

    ex(".!echo #", cx);
    linger(cx);
    assert_eq!(message(&workbench, cx).as_deref(), Some(E194));
    assert_eq!(text(&a, cx), format!("{name}\n"));

    // A name that is a command substitution stays a name, and % inside
    // quotes, where its quotes would be text, is refused.
    let hostile = "$(touch pwned)'`touch pwned`'";
    std::fs::write(root.join(hostile), "row\n").unwrap();
    open(&workbench, root.join(hostile), cx);
    let h = active(&workbench, cx);
    ex(".!echo %", cx);
    until(cx, "the hostile name echoed", |cx| {
        text(&h, cx) == format!("{hostile}\n")
    });
    for quoted in [
        ".!echo \"%\"",
        ".!echo '%'",
        ".!echo `echo %`",
        ".!echo $%",
        // Quotes nested in a case pattern inside a substitution.
        ".!echo \"$(case x in x) echo \"%\";; esac)\"",
    ] {
        ex(quoted, cx);
        linger(cx);
        assert_eq!(
            message(&workbench, cx).as_deref(),
            Some(NO_NAME_HERE),
            "{quoted}"
        );
    }
    assert_eq!(text(&h, cx), format!("{hostile}\n"));
    assert!(!root.join("pwned").exists());

    // A name with line breaks can't end a comment (`\#` is a literal #)
    // and run what follows: it isn't put in at all.
    let lines = "safe\ntouch pwned\n#";
    std::fs::write(root.join(lines), "row\n").unwrap();
    open(&workbench, root.join(lines), cx);
    let l = active(&workbench, cx);
    ex(".!echo \\# %", cx);
    linger(cx);
    assert_eq!(message(&workbench, cx).as_deref(), Some(NO_NAME_HERE));
    ex(".!echo %", cx);
    linger(cx);
    assert_eq!(message(&workbench, cx).as_deref(), Some(UNNAMEABLE));
    assert_eq!(text(&l, cx), "row\n");
    assert!(!root.join("pwned").exists());

    // An untitled buffer has no name for %.
    run("workbench.new_file", cx);
    let untitled = active(&workbench, cx);
    assert_ne!(untitled, a);
    ex(".!echo %", cx);
    linger(cx);
    let said: Option<SharedString> = message(&workbench, cx);
    assert_eq!(said.as_deref(), Some(E499));
    assert_eq!(text(&untitled, cx), "");
}

#[gpui::test]
#[cfg(windows)]
fn filter_runs_through_cmd(cx: &mut TestAppContext) {
    // The command's thread wakes the task waiting for it.
    cx.executor().allow_parking();
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("root");
    std::fs::create_dir(&root).unwrap();
    // An odd number of bytes: sort.exe takes some even-length input with
    // no BOM for UTF-16, as `IsTextUnicode` guesses.
    std::fs::write(root.join("a.txt"), "c\na\nbb\n").unwrap();
    let _settings = Settings::start(dir.path(), cx);
    let (workbench, mut vcx) = window(cx, &root, &["a.txt"]);
    let cx = &mut vcx;
    trust(&workbench, cx);
    let a = active(&workbench, cx);

    // :%!sort sorts the buffer through Windows' own sort, one undo step,
    // its CRLF rows read as the buffer's. Its first cmd.exe can take a
    // while on a loaded CI runner.
    ex("%!sort", cx);
    let shown =
        |cx: &mut VisualTestContext| format!("{:?}, {:?}", text(&a, cx), message(&workbench, cx));
    let sorted = |cx: &mut VisualTestContext| text(&a, cx) == "a\nbb\nc\n";
    until_within(cx, "sorted", Duration::from_secs(60), sorted, shown);
    cx.simulate_keystrokes("u");
    assert_eq!(text(&a, cx), "c\na\nbb\n");

    // :r !cmd puts its rows below the caret's line.
    ex("$r !echo z", cx);
    until(cx, "read at the end", |cx| text(&a, cx) == "c\na\nbb\nz\n");
    cx.simulate_keystrokes("u");
    assert_eq!(text(&a, cx), "c\na\nbb\n");

    // A file name isn't put in a cmd.exe command line.
    ex(".!echo %", cx);
    linger(cx);
    assert_eq!(
        message(&workbench, cx).as_deref(),
        Some(NO_NAMES_ON_WINDOWS)
    );
    assert_eq!(text(&a, cx), "c\na\nbb\n");
}

#[test]
fn shell_resolution() {
    fn env(vars: &'static [(&'static str, &'static str)]) -> impl Fn(&str) -> Option<String> {
        move |name| {
            (vars.iter())
                .find(|(var, _)| *var == name)
                .map(|(_, value)| value.to_string())
        }
    }
    let shell = |program: &str, flag, platform| Shell {
        program: program.to_string(),
        flag,
        platform,
    };
    use Platform::{Unix, Windows};

    // The setting first, then $SHELL if absolute, else /bin/sh.
    let fish = env(&[("SHELL", "/usr/bin/fish")]);
    assert_eq!(
        resolve_shell("/bin/zsh", Unix, &fish),
        shell("/bin/zsh", "-c", Unix)
    );
    assert_eq!(
        resolve_shell("", Unix, &fish),
        shell("/usr/bin/fish", "-c", Unix)
    );
    assert_eq!(
        resolve_shell("", Unix, env(&[])),
        shell("/bin/sh", "-c", Unix)
    );
    let relative = env(&[("SHELL", "fish")]);
    assert_eq!(
        resolve_shell("", Unix, relative),
        shell("/bin/sh", "-c", Unix)
    );

    // Windows, as data on any host: %COMSPEC%, else cmd.exe, with /C.
    let comspec = env(&[
        ("COMSPEC", r"C:\Windows\system32\cmd.exe"),
        ("SHELL", "/bin/bash"),
    ]);
    let cmd = resolve_shell("", Windows, comspec);
    assert_eq!(cmd, shell(r"C:\Windows\system32\cmd.exe", "/C", Windows));
    assert_eq!(
        resolve_shell("", Windows, env(&[])),
        shell("cmd.exe", "/C", Windows)
    );
    let pwsh = resolve_shell("pwsh.exe", Windows, env(&[]));
    assert_eq!(pwsh, shell("pwsh.exe", "/C", Windows));

    // On Windows % and # are refused; plain text goes as it is.
    let parts = |parts: Vec<ShellPart>| ShellCommand { parts };
    let text = |text: &str| ShellPart::Text(text.to_string());
    let current = parts(vec![text("type "), ShellPart::CurrentFile]);
    let alternate = parts(vec![text("type "), ShellPart::AlternateFile]);
    assert_eq!(cmd.line(&current, Some("a.txt")), Err(NO_NAMES_ON_WINDOWS));
    assert_eq!(
        cmd.line(&alternate, Some("a.txt")),
        Err(NO_NAMES_ON_WINDOWS)
    );
    assert_eq!(
        cmd.line(&parts(vec![text("sort")]), None),
        Ok("sort".to_string())
    );

    // On Unix % is single-quoted, ' and all; # and an untitled % fail.
    let sh = resolve_shell("/bin/sh", Unix, env(&[]));
    assert_eq!(
        sh.line(&current, Some("it's a b")),
        Ok(r"type 'it'\''s a b'".to_string())
    );
    assert_eq!(sh.line(&current, None), Err(E499));
    assert_eq!(sh.line(&alternate, Some("a.txt")), Err(E194));
}

#[test]
fn names_go_only_in_plain_commands() {
    let text = |text: &str| ShellPart::Text(text.to_string());
    let around = |before: &str, after: &str| ShellCommand {
        parts: vec![text(before), ShellPart::CurrentFile, text(after)],
    };
    let sh = resolve_shell("/bin/sh", Platform::Unix, |_| None);
    let fish = resolve_shell("/usr/bin/fish", Platform::Unix, |_| None);
    let line = |shell: &Shell, before: &str, after: &str| {
        shell.line(&around(before, after), Some("it's \\ $(x)"))
    };

    // In a command that opens nothing, it's one quoted word.
    let posix = r"'it'\''s \ $(x)'";
    for (before, after) in [
        ("cat ", ""),
        ("cat --file=", " | sort"),
        ("x=1; cat ", "; echo done"),
        ("wc -l <", " >out && echo ok"),
        ("ls ~/ ", " *.txt"),
    ] {
        let expected = format!("{before}{posix}{after}");
        assert_eq!(line(&sh, before, after), Ok(expected), "{before}%{after}");
    }
    let quoted = r"'it\'s \\ $(x)'";
    assert_eq!(line(&fish, "cat ", ""), Ok(format!("cat {quoted}")));

    // A command that opens quotes, a substitution, an expansion or a group
    // anywhere, before or after %, has none put in; nor does % after ~ or
    // a leading =.
    for (before, after) in [
        ("echo '", "'"),
        ("echo \"", "\""),
        ("echo `cat ", "`"),
        ("echo $(cat ", ")"),
        ("echo \"$(case x in x) echo \"", "\";; esac)\""),
        ("echo ${x:-", "}"),
        ("echo $((", "))"),
        ("echo $", ""),
        ("echo \\", ""),
        ("(cat ", ")"),
        ("ls *(e:", ":)"),
        ("echo {a,", "}"),
        ("cat ", " 'x'"),
        ("cat ", " $HOME"),
        ("echo ~", ""),
        ("echo =", ""),
        ("x; =", ""),
        ("echo # ", ""),
        ("cat <<EOF ", ""),
    ] {
        assert_eq!(
            line(&sh, before, after),
            Err(NO_NAME_HERE),
            "{before}%{after}"
        );
        assert_eq!(
            line(&fish, before, after),
            Err(NO_NAME_HERE),
            "{before}%{after}"
        );
    }

    // Nor does a name with a line break or another control character.
    for name in ["a\nb", "a\rb", "a\tb", "a\u{1b}b"] {
        let quoted = sh.line(&around("cat ", ""), Some(name));
        assert_eq!(quoted, Err(UNNAMEABLE), "{name:?}");
    }

    // Another shell's quoting isn't built.
    let nu = resolve_shell("/usr/bin/nu", Platform::Unix, |_| None);
    assert_eq!(line(&nu, "cat ", ""), Err(UNKNOWN_SHELL));
    assert_eq!(Quoting::of("/bin/zsh"), Some(Quoting::Posix));
    assert_eq!(Quoting::of("fish"), Some(Quoting::Fish));
    assert_eq!(Quoting::of("/bin/tcsh"), None);
    // With no %, any command runs in any shell, as typed.
    let typed = ShellCommand {
        parts: vec![text("echo \"$(date)\" | tr -d '\\n'")],
    };
    assert_eq!(
        nu.line(&typed, None),
        Ok("echo \"$(date)\" | tr -d '\\n'".to_string())
    );
}

#[test]
#[cfg(unix)]
fn containment_follows_symlinks_dangling_or_not() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("root");
    let outside = dir.path().join("outside");
    for folder in [&root, &outside, &root.join("sub")] {
        std::fs::create_dir(folder).unwrap();
    }
    std::fs::write(root.join("in.txt"), "").unwrap();
    std::fs::write(root.join("sub/x.txt"), "").unwrap();
    std::fs::write(outside.join("f.txt"), "").unwrap();
    let link = |to: &Path, at: &str| std::os::unix::fs::symlink(to, root.join(at)).unwrap();
    link(&outside, "out");
    link(&outside.join("missing.txt"), "dangling.txt");
    link(&outside.join("missing"), "dangling");
    link(&root.join("sub"), "inner");
    let within = |path: &str| vim_filter::within(&root.join(path), &root);

    assert_eq!(within("in.txt"), Some("in.txt".into()));
    assert_eq!(
        within("new.txt"),
        Some("new.txt".into()),
        "a file not made yet"
    );
    assert_eq!(within("inner/x.txt"), Some("inner/x.txt".into()));
    // Through links out of the root, to something or nothing.
    assert_eq!(within("out/f.txt"), None);
    assert_eq!(within("out/new.txt"), None);
    assert_eq!(within("dangling.txt"), None);
    assert_eq!(within("dangling/new.txt"), None);
    // A file in a folder not made yet isn't worked out.
    assert_eq!(within("nowhere/new.txt"), None);
    assert_eq!(within("../outside/f.txt"), None);
}

/// The real shells, where installed, take each name back as it is.
#[test]
fn quoted_names_come_back_from_the_shells() {
    let names = [
        "plain",
        "a b; rm -rf x",
        "it's",
        "$(touch pwned)",
        "`x`",
        r"back\slash",
        "\"q\"",
        "new\nline",
        "''",
    ];
    let dir = tempfile::tempdir().unwrap();
    for (program, quoting) in [
        ("/bin/sh", Quoting::Posix),
        ("/bin/bash", Quoting::Posix),
        ("/bin/zsh", Quoting::Posix),
        ("/usr/bin/fish", Quoting::Fish),
    ] {
        if !Path::new(program).exists() {
            continue;
        }
        for name in names {
            let out = std::process::Command::new(program)
                .arg("-c")
                .arg(format!("printf %s {}", quoting.quote(name)))
                .current_dir(dir.path())
                .output()
                .unwrap();
            assert_eq!(
                String::from_utf8_lossy(&out.stdout),
                name,
                "{program}: {name:?}"
            );
        }
        assert!(!dir.path().join("pwned").exists(), "{program}");
    }
}

#[test]
#[cfg(unix)]
fn output_past_the_cap_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let run = |command: &str| {
        vim_filter::run(Run {
            shell: resolve_shell("/bin/sh", Platform::Unix, |_| None),
            line: command.to_string(),
            cwd: dir.path().to_owned(),
            input: None,
            limit: Duration::from_secs(20),
            cancel: Default::default(),
            group: Default::default(),
        })
    };
    // A command that writes one byte past the cap and ends at once.
    let past = format!("head -c {} /dev/zero", OUTPUT_LIMIT + 1);
    assert_eq!(run(&past), Outcome::TooMuch);
    let past = format!("head -c {} /dev/zero >&2", OUTPUT_LIMIT + 1);
    assert_eq!(run(&past), Outcome::TooMuch);
    // Past it as the shell ends, what it left in the background (with its
    // streams elsewhere and SIGTERM ignored) is killed with it.
    let past = format!(
        "(trap '' TERM; exec sleep 30) </dev/null >/dev/null 2>&1 & echo $! >pid; \
         head -c {} /dev/zero",
        OUTPUT_LIMIT + 1
    );
    assert_eq!(run(&past), Outcome::TooMuch);
    let pid = pid_in(&dir.path().join("pid"));
    let deadline = Instant::now() + GRACE + Duration::from_secs(5);
    while alive(pid) {
        assert!(Instant::now() < deadline, "{pid} outlived the cap");
        thread::sleep(Duration::from_millis(20));
    }
    // The cap itself goes.
    let at = format!("head -c {OUTPUT_LIMIT} /dev/zero");
    match run(&at) {
        Outcome::Exited {
            code: 0, stdout, ..
        } => assert_eq!(stdout.len(), OUTPUT_LIMIT),
        other => panic!("{other:?}"),
    }
}

/// Whether process `pid` is still there, not counting a zombie its reaper
/// hasn't come for.
#[cfg(unix)]
fn alive(pid: i32) -> bool {
    // SAFETY: signal 0 only asks.
    if unsafe { libc::kill(pid, 0) } != 0 {
        return false;
    }
    let stat = std::process::Command::new("ps")
        .args(["-o", "stat=", "-p", &pid.to_string()])
        .output()
        .unwrap();
    let stat = String::from_utf8_lossy(&stat.stdout);
    !stat.trim().is_empty() && !stat.trim_start().starts_with('Z')
}

/// The pid the command wrote to `file`, once it has.
#[cfg(unix)]
fn pid_in(file: &Path) -> i32 {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if let Ok(pid) = std::fs::read_to_string(file)
            && let Ok(pid) = pid.trim().parse()
        {
            return pid;
        }
        assert!(Instant::now() < deadline, "no pid in {}", file.display());
        thread::sleep(Duration::from_millis(10));
    }
}

#[gpui::test]
#[cfg(unix)]
fn a_stopped_commands_group_dies(cx: &mut TestAppContext) {
    // The command's thread wakes the task waiting for it.
    cx.executor().allow_parking();
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("root");
    std::fs::create_dir(&root).unwrap();
    std::fs::write(root.join("a.txt"), "a\n").unwrap();
    let _settings = Settings::start(dir.path(), cx);
    let (workbench, mut vcx) = window(cx, &root, &["a.txt"]);
    let cx = &mut vcx;
    trust(&workbench, cx);
    let a = active(&workbench, cx);

    // The shell and what it starts ignore SIGTERM, which has its stream go
    // nowhere the filter reads.
    ex(
        ".!trap '' TERM; sleep 30 </dev/null >/dev/null 2>&1 & echo $! >pid; wait",
        cx,
    );
    let pid = pid_in(&root.join("pid"));
    assert!(alive(pid));
    run(vim_filter::CANCEL, cx);
    until_message(&workbench, CANCELLED, cx);
    assert_eq!(text(&a, cx), "a\n");
    let deadline = Instant::now() + GRACE + Duration::from_secs(5);
    while alive(pid) {
        assert!(Instant::now() < deadline, "{pid} outlived SIGKILL");
        thread::sleep(Duration::from_millis(20));
    }
}

#[gpui::test]
#[cfg(unix)]
fn closing_the_tab_or_the_window_cancels(cx: &mut TestAppContext) {
    // The command's thread wakes the task waiting for it.
    cx.executor().allow_parking();
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("root");
    std::fs::create_dir(&root).unwrap();
    std::fs::write(root.join("a.txt"), "a\n").unwrap();
    std::fs::write(root.join("b.txt"), "b\n").unwrap();
    let _settings = Settings::start(dir.path(), cx);
    let (workbench, mut vcx) = window(cx, &root, &["a.txt", "b.txt"]);
    trust(&workbench, &mut vcx);
    let b = active(&workbench, &mut vcx);
    let gone = |pid: i32, cx: &mut TestAppContext| {
        let deadline = Instant::now() + GRACE + Duration::from_secs(5);
        loop {
            cx.executor().advance_clock(WATCH);
            cx.run_until_parked();
            let running = cx.update(|cx| vim_filter::running(cx));
            if !alive(pid) && running == 0 {
                return;
            }
            assert!(
                Instant::now() < deadline,
                "{pid} still runs, or {running} commands do"
            );
            thread::sleep(Duration::from_millis(20));
        }
    };

    // The tab closes while something else holds its editor (`b`, as a
    // pending save would).
    ex(".!echo $$ >b.pid; exec sleep 30", &mut vcx);
    let pid = pid_in(&root.join("b.pid"));
    run("workbench.close_tab", &mut vcx);
    let a = active(&workbench, &mut vcx);
    assert_ne!(a, b);
    gone(pid, cx);
    assert_eq!(text(&b, &mut vcx), "b\n");

    // The window closes, its workbench and editor still held, before
    // another watch.
    ex(".!echo $$ >a.pid; exec sleep 30", &mut vcx);
    let pid = pid_in(&root.join("a.pid"));
    vcx.update(|window, _| window.remove_window());
    let deadline = Instant::now() + GRACE + Duration::from_secs(5);
    while alive(pid) {
        assert!(Instant::now() < deadline, "{pid} outlived its window");
        thread::sleep(Duration::from_millis(20));
        cx.run_until_parked();
    }
    drop((a, b, workbench));
}

#[gpui::test]
#[cfg(unix)]
fn quitting_stops_every_command(cx: &mut TestAppContext) {
    // The command's thread wakes the task waiting for it.
    cx.executor().allow_parking();
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("root");
    std::fs::create_dir(&root).unwrap();
    std::fs::write(root.join("a.txt"), "a\n").unwrap();
    let _settings = Settings::start(dir.path(), cx);
    let (workbench, mut vcx) = window(cx, &root, &["a.txt"]);
    trust(&workbench, &mut vcx);

    // It ignores SIGTERM, and what it starts has its streams elsewhere.
    let command = ".!trap '' TERM; sleep 30 </dev/null >/dev/null 2>&1 & echo $! >pid; wait";
    ex(command, &mut vcx);
    let pid = pid_in(&root.join("pid"));
    assert!(alive(pid));
    // The app quits at once, as it does when its last window closes.
    cx.update(|cx| cx.shutdown());
    // Killed before quitting went on; only its reaper may lag.
    let deadline = Instant::now() + Duration::from_millis(500);
    while alive(pid) {
        assert!(Instant::now() < deadline, "{pid} outlived the app");
        thread::sleep(Duration::from_millis(10));
    }
}

#[gpui::test]
// POSIX shell scripts: Windows filters through cmd.exe (see
// filter_runs_through_cmd).
#[cfg(unix)]
fn toasts_show_in_the_commands_window(cx: &mut TestAppContext) {
    // The command's thread wakes the task waiting for it.
    cx.executor().allow_parking();
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("root");
    std::fs::create_dir(&root).unwrap();
    std::fs::write(root.join("a.txt"), "a\n").unwrap();
    let _settings = Settings::start(dir.path(), cx);
    let (workbench, mut vcx) = window(cx, &root, &["a.txt"]);
    trust(&workbench, &mut vcx);
    let a = active(&workbench, &mut vcx);
    // Another window, active.
    let other = cx.add_window(|window, cx| {
        let theme = fe_gpui::EditorTheme::default_dark(cx);
        let launch = crate::Launch {
            root: None,
            file: None,
        };
        Workbench::new(
            launch,
            theme,
            fe_gpui::EditorSettings::default(),
            window,
            cx,
        )
    });
    let elsewhere = other.root(cx).unwrap();
    let mut other = VisualTestContext::from_window(other.into(), cx);
    other.update(|window, _| window.activate_window());
    draw(&mut other);
    let offer = |toasts: &[(Level, String, Option<String>)]| {
        (toasts.iter())
            .any(|(level, said, _)| *level == Level::Info && said.starts_with("Filtering through"))
    };

    // The offer to cancel, then the failure, show in the command's window.
    ex(".!sleep 1; echo err >&2; exit 3", &mut vcx);
    vcx.executor().advance_clock(TOAST_AFTER + WATCH);
    draw(&mut vcx);
    assert!(
        offer(&toasts(&workbench, &mut vcx)),
        "{:?}",
        toasts(&workbench, &mut vcx)
    );
    until_message(&workbench, "shell returned 3", &mut vcx);
    let shown = toasts(&workbench, &mut vcx);
    assert!(!offer(&shown), "the offer goes with the command: {shown:?}");
    assert!(
        (shown.iter())
            .any(|(level, _, detail)| *level == Level::Error && detail.as_deref() == Some("err\n")),
        "{shown:?}"
    );
    assert_eq!(toasts(&elsewhere, &mut other), []);

    // Its Cancel button cancels it.
    ex(".!sleep 30", &mut vcx);
    vcx.executor().advance_clock(TOAST_AFTER + WATCH);
    draw(&mut vcx);
    let layer = workbench.read_with(&vcx, |workbench, _| workbench.toasts().clone());
    let choices = layer.read_with(&vcx, |layer, _| {
        (layer.queue().shown().iter())
            .find_map(|entry| entry.toast.choices.clone())
            .expect("the offer to cancel")
    });
    assert_eq!(choices.titles(), ["Cancel"]);
    choices.choose(0);
    until_message(&workbench, CANCELLED, &mut vcx);
    assert!(!offer(&toasts(&workbench, &mut vcx)));
    assert_eq!(text(&a, &mut vcx), "a\n");
}

/// A workbench on a trusted root holding `a.txt`, through `/bin/sh`.
#[cfg(unix)]
fn trusted(
    dir: &Path,
    cx: &mut TestAppContext,
) -> (
    Settings,
    std::path::PathBuf,
    Entity<Workbench>,
    VisualTestContext,
) {
    let root = dir.join("root");
    std::fs::create_dir(&root).unwrap();
    std::fs::write(root.join("a.txt"), "a\n").unwrap();
    let settings = Settings::start(dir, cx);
    let (workbench, mut vcx) = window(cx, &root, &["a.txt"]);
    trust(&workbench, &mut vcx);
    (settings, root, workbench, vcx)
}

/// Waits a little for `pid` to go, as only its reaper may lag once the
/// app has quit.
#[cfg(unix)]
fn reaped(pid: i32) {
    let deadline = Instant::now() + Duration::from_millis(500);
    while alive(pid) {
        assert!(Instant::now() < deadline, "{pid} outlived the app");
        thread::sleep(Duration::from_millis(10));
    }
}

/// What ignores SIGTERM, with its streams elsewhere, writing its pid.
#[cfg(unix)]
const STUBBORN: &str = ".!trap '' TERM; sleep 30 </dev/null >/dev/null 2>&1 & echo $! >pid; wait";

#[gpui::test]
#[cfg(unix)]
fn quitting_right_after_a_command_is_typed_stops_it(cx: &mut TestAppContext) {
    // The command's thread wakes the task waiting for it.
    cx.executor().allow_parking();
    let dir = tempfile::tempdir().unwrap();
    let (_settings, root, _workbench, mut vcx) = trusted(dir.path(), cx);
    ex(STUBBORN, &mut vcx);
    // At once: its thread may not have started the shell yet.
    cx.update(|cx| cx.shutdown());
    // What started is gone, and nothing starts after.
    thread::sleep(Duration::from_millis(300));
    if let Ok(pid) = std::fs::read_to_string(root.join("pid")) {
        reaped(pid.trim().parse().unwrap());
    }
    thread::sleep(Duration::from_millis(300));
    assert!(!root.join("pid").exists() || !alive(pid_in(&root.join("pid"))));
}

#[gpui::test]
#[cfg(unix)]
fn quitting_while_a_cancelled_command_stops_kills_it(cx: &mut TestAppContext) {
    // The command's thread wakes the task waiting for it.
    cx.executor().allow_parking();
    let dir = tempfile::tempdir().unwrap();
    let (_settings, root, workbench, mut vcx) = trusted(dir.path(), cx);
    ex(STUBBORN, &mut vcx);
    let pid = pid_in(&root.join("pid"));
    // Cancelled, and said so, while its group still waits for SIGKILL.
    run(vim_filter::CANCEL, &mut vcx);
    until_message(&workbench, CANCELLED, &mut vcx);
    assert!(alive(pid), "SIGTERM is ignored");
    cx.update(|cx| cx.shutdown());
    reaped(pid);
}
