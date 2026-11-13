//! Credential prompts for a push, in a headless window: a repository whose
//! `origin` is reached by a fake ssh (its `core.sshCommand`) that asks
//! `GIT_ASKPASS` for a password, as git's own prompts do, and runs git's
//! command on this machine if the answer is right. The helper git runs is
//! this test binary, rerun as [`askpass_helper`], asking over the socket as
//! the app's `--askpass` mode does. Pushes that may prompt run on threads
//! of their own, so the tests wait on them in real time.
#![cfg(unix)]

use std::{
    ffi::OsString,
    fs,
    os::unix::fs::PermissionsExt as _,
    path::{Path, PathBuf},
    process::Command,
    time::Duration,
};

use fe_gpui::{EditorSettings, EditorTheme};
use gpui::{Entity, Focusable as _, TestAppContext, VisualTestContext};

use crate::{
    Launch, Workbench,
    git_askpass::{CredentialPrompt, GIT_ASKPASS, prompts},
    workbench::{
        git_commands::{GitCommand, git_remote::failure},
        toast::Level,
    },
};

const SECRET: &str = "s3cret-Pa55";
/// What the fake ssh asks.
const ASKED: &str = "Password for 'ssh://host':";

/// Set, with the prompt, when git runs this binary as its askpass helper.
const HELPER_SOCKET: &str = "FE_WORKBENCH_ASKPASS_TEST_SOCKET";
const HELPER_PROMPT: &str = "FE_WORKBENCH_ASKPASS_TEST_PROMPT";

/// The askpass helper, when git runs this binary as one: asks over the
/// socket and writes the answer to fd 3, which the script made git's pipe
/// (stdout is the test harness's), then exits as `--askpass` would.
#[test]
fn askpass_helper() {
    let (Some(socket), Some(prompt)) = (
        std::env::var_os(HELPER_SOCKET),
        std::env::var(HELPER_PROMPT).ok(),
    ) else {
        return;
    };
    let mut out = (fs::OpenOptions::new().write(true))
        .open("/dev/fd/3")
        .unwrap();
    let args = [socket, prompt.into()];
    std::process::exit(fe_git::askpass::helper(args, &mut out));
}

/// What the workbench's askpass runs: this binary, as [`askpass_helper`],
/// given the socket and the prompt.
fn helper() -> Vec<OsString> {
    let script = format!(
        "{HELPER_SOCKET}=\"$1\" {HELPER_PROMPT}=\"$2\" exec \"$0\" \
         tests::git_askpass::askpass_helper --exact --nocapture --test-threads=1 \
         3>&1 >/dev/null 2>/dev/null"
    );
    vec![
        "/bin/sh".into(),
        "-c".into(),
        script.into(),
        std::env::current_exe().unwrap().into(),
    ]
}

/// Runs `git` in `dir`, with the developer's own config shut out; its
/// output, trimmed.
fn git(dir: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .args(["-c", "init.defaultBranch=main"])
        .args(args)
        .current_dir(dir)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .output()
        .expect("run git");
    assert!(
        output.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}

/// A repository with a local commit to push to its bare `origin`, reached
/// through the fake ssh, and the state directory its prompts' sockets go
/// in.
struct Fixture {
    _dir: tempfile::TempDir,
    work: PathBuf,
    origin: PathBuf,
    state: PathBuf,
}

fn fixture() -> Fixture {
    fixture_with(Ssh::Asks)
}

/// What the fake ssh does.
enum Ssh {
    /// Asks, and gets in with [`SECRET`].
    Asks,
    /// Asks, then stops the helper after a moment, unanswered, and gets in
    /// another way, as OpenSSH does with a notice once a key's touched.
    StopsAsking,
}

fn fixture_with(ssh: Ssh) -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let base = dunce::canonicalize(dir.path()).unwrap();
    git(&base, &["init", "-q", "--bare", "origin.git"]);
    let asks = match ssh {
        Ssh::Asks => format!(
            "pass=$(\"$GIT_ASKPASS\" \"{ASKED} \") || {{ echo 'Permission denied (password).' >&2; exit 255; }}\n\
             [ \"$pass\" = '{SECRET}' ] || {{ echo 'Permission denied (password).' >&2; exit 255; }}\n"
        ),
        Ssh::StopsAsking => format!(
            "\"$GIT_ASKPASS\" \"{ASKED} \" >/dev/null & helper=$!\n\
             sleep 1; kill $helper; wait\n"
        ),
    };
    let ssh = base.join("fake-ssh");
    fs::write(
        &ssh,
        format!("#!/bin/sh\nfor arg; do last=$arg; done\n{asks}exec sh -c \"$last\"\n"),
    )
    .unwrap();
    fs::set_permissions(&ssh, fs::Permissions::from_mode(0o755)).unwrap();
    let work = base.join("work");
    fs::create_dir(&work).unwrap();
    git(&work, &["init", "-q"]);
    for (key, value) in [
        ("user.name", "t"),
        ("user.email", "t@t"),
        ("commit.gpgsign", "false"),
        ("push.gpgsign", "false"),
        ("core.sshCommand", ssh.to_str().unwrap()),
        ("ssh.variant", "simple"),
    ] {
        git(&work, &["config", key, value]);
    }
    fs::write(work.join("a.txt"), "one\n").unwrap();
    git(&work, &["add", "."]);
    git(&work, &["commit", "-q", "-m", "one"]);
    git(&work, &["remote", "add", "origin", "../origin.git"]);
    git(&work, &["push", "-q", "-u", "origin", "main"]);
    let url = format!("ssh://host{}", base.join("origin.git").display());
    git(&work, &["remote", "set-url", "origin", &url]);
    fs::write(work.join("a.txt"), "two\n").unwrap();
    git(&work, &["commit", "-q", "-am", "two"]);
    Fixture {
        origin: base.join("origin.git"),
        state: base.join("state"),
        work,
        _dir: dir,
    }
}

/// A window of a workbench rooted at `root`, focused, with prompts going
/// through [`helper`] and their sockets in `state`.
fn window(
    cx: &mut TestAppContext,
    root: &Path,
    state: &Path,
) -> (Entity<Workbench>, VisualTestContext) {
    // The push, and the askpass's socket, run on threads of their own.
    cx.executor().allow_parking();
    cx.update(|cx| {
        if !cx.has_global::<crate::Commands>() {
            fe_gpui::init(cx);
            crate::init(cx);
        }
        crate::set_git_askpass(cx, helper(), state.to_owned());
    });
    let launch = Launch {
        root: Some(root.to_owned()),
        file: None,
    };
    let window = cx.add_window(move |window, cx| {
        let theme = EditorTheme::default_dark(cx);
        Workbench::new(launch, theme, EditorSettings::default(), window, cx)
    });
    let workbench = window.root(cx).unwrap();
    let mut vcx = VisualTestContext::from_window(window.into(), cx);
    activate(&workbench, &mut vcx);
    (workbench, vcx)
}

fn activate(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) {
    switch_to(cx);
    cx.update(|window, cx| window.focus(&workbench.focus_handle(cx), cx));
    draw(cx);
}

/// Makes `cx`'s window the active one, as the user going back to it would,
/// focus staying where it was in it.
fn switch_to(cx: &mut VisualTestContext) {
    cx.update(|window, _| window.activate_window());
    draw(cx);
}

/// Lets work land, then draws a frame.
fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.run_until_parked();
}

/// Draws until `done`, as the push's thread and the helper go on.
fn wait_until(
    what: &str,
    cx: &mut VisualTestContext,
    mut done: impl FnMut(&mut VisualTestContext) -> bool,
) {
    for _ in 0..1000 {
        draw(cx);
        if done(cx) {
            return;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    panic!("still waiting for {what} after 10 s");
}

fn push(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) {
    cx.update(|window, cx| {
        workbench.update(cx, |workbench, cx| {
            workbench.run_git_command(GitCommand::Push, window, cx)
        })
    });
    draw(cx);
}

/// The prompt showing or hidden, once there is one.
fn prompt(cx: &mut VisualTestContext) -> Entity<CredentialPrompt> {
    let mut prompt = None;
    wait_until("a prompt", cx, |cx| {
        prompt = cx.update(|_, cx| prompts(cx).0);
        prompt.is_some()
    });
    prompt.unwrap()
}

/// Whether `workbench`'s window shows the prompt.
fn shows_prompt(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> bool {
    workbench.read_with(cx, |workbench, cx| {
        workbench.shell().read(cx).modals().read(cx).modal() == Some(GIT_ASKPASS)
    })
}

/// The toasts shown other than the progress one, once there's one, in any
/// of `workbenches`' windows: they show in the active window, whichever
/// that is. Emptied.
fn told_in(workbenches: &[&Entity<Workbench>], cx: &mut VisualTestContext) -> Vec<(Level, String)> {
    let layers: Vec<_> = (workbenches.iter())
        .map(|workbench| workbench.read_with(cx, |workbench, _| workbench.toasts().clone()))
        .collect();
    let mut shown = Vec::new();
    wait_until("the push to finish", cx, |cx| {
        shown = (layers.iter())
            .flat_map(|layer| {
                layer.read_with(cx, |layer, _| {
                    (layer.queue().shown().iter())
                        .map(|entry| (entry.toast.level, entry.toast.message.clone()))
                        .filter(|(_, message)| message != "Pushing…")
                        .collect::<Vec<_>>()
                })
            })
            .collect();
        !shown.is_empty()
    });
    for layer in layers {
        cx.update(|_, cx| layer.update(cx, |layer, cx| layer.dismiss_all(cx)));
    }
    shown
}

fn told(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Vec<(Level, String)> {
    told_in(&[workbench], cx)
}

/// Nothing's left of the push's prompts in `state`.
fn nothing_left(state: &Path) -> bool {
    fs::read_dir(state).map_or(true, |mut entries| entries.next().is_none())
}

fn type_answer(text: &str, cx: &mut VisualTestContext) {
    cx.simulate_input(text);
    draw(cx);
}

#[gpui::test]
fn a_request_opens_a_masked_input_naming_the_remote(cx: &mut TestAppContext) {
    let fixture = fixture();
    let (workbench, mut cx) = window(cx, &fixture.work, &fixture.state);
    let cx = &mut cx;

    push(&workbench, cx);
    let prompt = prompt(cx);
    assert!(shows_prompt(&workbench, cx));
    let [title, status, placeholder] = prompt.read_with(cx, |prompt, cx| prompt.shown_text(cx));
    assert_eq!(title, "Sign in to origin");
    assert!(status.starts_with("origin asks"), "{status}");
    assert_eq!(placeholder, ASKED);
    assert!(prompt.read_with(cx, |prompt, cx| prompt.is_masked(cx)));
    // Typing goes to it.
    type_answer("abc", cx);
    assert_eq!(prompt.read_with(cx, |prompt, cx| prompt.typed(cx)), "abc");

    cx.simulate_keystrokes("escape");
    told(&workbench, cx);
}

#[gpui::test]
fn answering_continues_the_push(cx: &mut TestAppContext) {
    let fixture = fixture();
    let (workbench, mut cx) = window(cx, &fixture.work, &fixture.state);
    let cx = &mut cx;

    push(&workbench, cx);
    let prompt = prompt(cx);
    type_answer(SECRET, cx);
    cx.simulate_keystrokes("enter");
    assert_eq!(told(&workbench, cx), [(Level::Info, "Pushed".to_owned())]);
    assert_eq!(
        git(&fixture.origin, &["rev-parse", "main"]),
        git(&fixture.work, &["rev-parse", "HEAD"])
    );
    // Gone, and what was typed with it.
    assert!(!shows_prompt(&workbench, cx));
    assert_eq!(prompt.read_with(cx, |prompt, cx| prompt.typed(cx)), "");
    assert!(cx.update(|_, cx| prompts(cx).0.is_none()));
    assert!(nothing_left(&fixture.state));
}

#[gpui::test]
fn escape_cancels_and_reports(cx: &mut TestAppContext) {
    let fixture = fixture();
    let (workbench, mut cx) = window(cx, &fixture.work, &fixture.state);
    let cx = &mut cx;
    let before = git(&fixture.origin, &["rev-parse", "main"]);

    push(&workbench, cx);
    prompt(cx);
    type_answer("not sent", cx);
    cx.simulate_keystrokes("escape");
    let told = told(&workbench, cx);
    // As it said before there were prompts.
    let auth = fe_git::RemoteError::Auth {
        output: String::new(),
    };
    let sentence = failure(GitCommand::Push, auth, &fixture.work).message;
    match told.as_slice() {
        [(Level::Error, message)] => assert_eq!(message, &sentence),
        _ => panic!("not one error: {told:?}"),
    }
    assert!(!told[0].1.contains("not sent"));
    assert!(!shows_prompt(&workbench, cx));
    assert!(cx.update(|_, cx| prompts(cx).0.is_none()));
    assert_eq!(git(&fixture.origin, &["rev-parse", "main"]), before);
    assert!(nothing_left(&fixture.state));
}

/// Two windows, into two repositories, push at once: the second's prompt
/// waits for the first's to be answered. The first's, hidden as its window
/// goes inactive, shows again once it's active.
#[gpui::test]
fn one_prompt_at_a_time_across_windows(cx: &mut TestAppContext) {
    let (one, two) = (fixture(), fixture());
    let (first, mut first_cx) = window(cx, &one.work, &one.state);
    push(&first, &mut first_cx);
    let first_prompt = prompt(&mut first_cx);
    assert!(shows_prompt(&first, &mut first_cx));

    let (second, mut second_cx) = window(cx, &two.work, &two.state);
    push(&second, &mut second_cx);
    wait_until("the second to wait", &mut second_cx, |cx| {
        cx.update(|_, cx| prompts(cx).1) == 1
    });
    assert!(!shows_prompt(&second, &mut second_cx));
    assert_eq!(cx.update(|cx| prompts(cx).0), Some(first_prompt.clone()));

    switch_to(&mut first_cx);
    assert!(shows_prompt(&first, &mut first_cx));
    type_answer(SECRET, &mut first_cx);
    first_cx.simulate_keystrokes("enter");
    let pushed = [(Level::Info, "Pushed".to_owned())];
    assert_eq!(told_in(&[&first, &second], &mut first_cx), pushed);

    // Then the second's, in its own window.
    switch_to(&mut second_cx);
    let second_prompt = prompt(&mut second_cx);
    assert_ne!(second_prompt, first_prompt);
    assert!(shows_prompt(&second, &mut second_cx));
    assert!(!shows_prompt(&first, &mut first_cx));
    type_answer(SECRET, &mut second_cx);
    second_cx.simulate_keystrokes("enter");
    assert_eq!(told_in(&[&first, &second], &mut second_cx), pushed);
    for fixture in [&one, &two] {
        assert_eq!(
            git(&fixture.origin, &["rev-parse", "main"]),
            git(&fixture.work, &["rev-parse", "HEAD"])
        );
    }
}

/// A prompt git stops waiting for, its helper gone, closes, and the push
/// goes on.
#[gpui::test]
fn a_prompt_no_one_waits_for_closes(cx: &mut TestAppContext) {
    let fixture = fixture_with(Ssh::StopsAsking);
    let (workbench, mut cx) = window(cx, &fixture.work, &fixture.state);
    let cx = &mut cx;

    push(&workbench, cx);
    prompt(cx);
    assert!(shows_prompt(&workbench, cx));
    assert_eq!(told(&workbench, cx), [(Level::Info, "Pushed".to_owned())]);
    assert!(!shows_prompt(&workbench, cx));
    assert!(cx.update(|_, cx| prompts(cx).0.is_none()));
    assert!(nothing_left(&fixture.state));
}

/// A window closed while its prompt waits behind another's cancels it, its
/// push failing at once, and the other goes on.
#[gpui::test]
fn closing_a_window_cancels_the_prompt_waiting_in_it(cx: &mut TestAppContext) {
    let (one, two) = (fixture(), fixture());
    let before = git(&two.origin, &["rev-parse", "main"]);
    let (first, mut first_cx) = window(cx, &one.work, &one.state);
    push(&first, &mut first_cx);
    let first_prompt = prompt(&mut first_cx);
    let (second, mut second_cx) = window(cx, &two.work, &two.state);
    push(&second, &mut second_cx);
    wait_until("the second to wait", &mut second_cx, |cx| {
        cx.update(|_, cx| prompts(cx).1) == 1
    });

    second_cx.update(|window, _| window.remove_window());
    switch_to(&mut first_cx);
    let told = told_in(&[&first], &mut first_cx);
    match told.as_slice() {
        [(Level::Error, message)] => assert!(message.starts_with("Can't push"), "{message}"),
        _ => panic!("not one error: {told:?}"),
    }
    assert_eq!(cx.update(prompts), (Some(first_prompt), 0));
    assert_eq!(git(&two.origin, &["rev-parse", "main"]), before);
    assert!(nothing_left(&two.state));

    type_answer(SECRET, &mut first_cx);
    first_cx.simulate_keystrokes("enter");
    let pushed = [(Level::Info, "Pushed".to_owned())];
    assert_eq!(told_in(&[&first], &mut first_cx), pushed);
}

/// A window closed before its push asks cancels the prompt as it comes,
/// rather than queueing it behind another window's.
#[gpui::test]
fn a_prompt_asked_after_its_window_closed_is_cancelled(cx: &mut TestAppContext) {
    let (one, two) = (fixture(), fixture());
    let before = git(&two.origin, &["rev-parse", "main"]);
    let (first, mut first_cx) = window(cx, &one.work, &one.state);
    push(&first, &mut first_cx);
    let first_prompt = prompt(&mut first_cx);
    let (second, mut second_cx) = window(cx, &two.work, &two.state);
    cx.update(Workbench::hold_git_work);
    push(&second, &mut second_cx);
    second_cx.update(|window, _| window.remove_window());
    switch_to(&mut first_cx);
    cx.update(|cx| Workbench::release_git_work(0, cx));

    let told = told_in(&[&first], &mut first_cx);
    match told.as_slice() {
        [(Level::Error, message)] => assert!(message.starts_with("Can't push"), "{message}"),
        _ => panic!("not one error: {told:?}"),
    }
    assert_eq!(cx.update(prompts), (Some(first_prompt), 0));
    assert_eq!(git(&two.origin, &["rev-parse", "main"]), before);
    assert!(nothing_left(&two.state));
}
