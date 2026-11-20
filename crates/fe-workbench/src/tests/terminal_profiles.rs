//! Terminal profiles: `terminal.new` starting the default profile,
//! `terminal.new_with_profile` starting the one it names (or asking which),
//! and the `+` caret's menu of every profile. The sessions are fakes: what
//! each was asked to run is what's checked, so nothing starts a process.

use std::{collections::BTreeMap, fs};

use fe_buffer::Buffer;
use fe_command::{ContextFrame, ContextStack, Invocation};
use fe_config::{TerminalProfileSettings, TerminalSettings};
use fe_gpui::commands::RunCommand;
use fe_terminal_view::{
    TerminalOptions,
    profiles::{self, Detected, Profile},
};
use gpui::{Entity, Focusable as _, Modifiers, TestAppContext, VisualTestContext};

use crate::{
    Launch, TerminalPanel, Workbench,
    terminal_panel::terminal_tabs_tests::{dock_shown, draw, drawn_tabs, terminal, toggle},
    tests::open,
    workbench::Palette,
};

/// The profiles "found" on the test's machine.
fn found() -> Vec<Profile> {
    vec![
        Profile::shell(
            "pwsh",
            "PowerShell 7",
            "/opt/pwsh/pwsh",
            vec!["-NoLogo".into()],
        ),
        Profile::shell("cmd", "Command Prompt", "cmd.exe", Vec::new()),
        Profile::wsl("Ubuntu"),
    ]
}

/// Detection's first stage on Windows: [`found`]'s shells, `wsl.exe` not
/// having answered.
fn shells_found(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) {
    let mut shells = found();
    shells.retain(|profile| profile.program != "wsl.exe");
    land(workbench, shells, false, cx);
}

/// Detection's last stage: everything in [`found`].
fn all_found(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) {
    land(workbench, found(), true, cx);
}

fn land(
    workbench: &Entity<Workbench>,
    profiles: Vec<Profile>,
    done: bool,
    cx: &mut VisualTestContext,
) {
    let found = Detected { profiles, done };
    terminal(workbench, cx).update(cx, |terminal, cx| terminal.detection_landed(found, cx));
    draw(cx);
}

/// A workbench showing `a.rs` from a temporary project, focused, whose
/// terminal panel starts no process; detection has found [`found`].
fn opened(
    cx: &mut TestAppContext,
) -> (Entity<Workbench>, &mut VisualTestContext, tempfile::TempDir) {
    let (workbench, cx, dir) = undetected(cx);
    terminal(&workbench, cx).update(cx, |terminal, cx| terminal.set_detected(found(), cx));
    draw(cx);
    (workbench, cx, dir)
}

/// As [`opened`], with detection still running.
fn undetected(
    cx: &mut TestAppContext,
) -> (Entity<Workbench>, &mut VisualTestContext, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("a.rs");
    fs::write(&path, "fn a() {}\n").unwrap();
    let launch = Launch {
        root: Some(dir.path().to_owned()),
        file: Some((path, Some(Buffer::new("fn a() {}\n").into()))),
    };
    let (workbench, cx, _) = open(cx, launch);
    terminal(&workbench, cx).update(cx, |terminal, _| terminal.fake_sessions = true);
    cx.update(|window, cx| window.focus(&workbench.focus_handle(cx), cx));
    draw(cx);
    (workbench, cx, dir)
}

fn set_settings(
    workbench: &Entity<Workbench>,
    settings: TerminalSettings,
    cx: &mut VisualTestContext,
) {
    workbench.update(cx, |workbench, cx| {
        workbench.set_terminal_settings(&settings, cx)
    });
    draw(cx);
}

/// What each session started was asked to run, in start order.
fn started(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Vec<TerminalOptions> {
    terminal(workbench, cx).read_with(cx, |terminal, _| terminal.started_options.clone())
}

fn shell(options: &TerminalOptions) -> Option<(&str, Vec<&str>)> {
    let (program, args) = options.shell.as_ref()?;
    Some((program, args.iter().map(String::as_str).collect()))
}

fn labels(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Vec<String> {
    let tabs = drawn_tabs(workbench, cx);
    tabs.iter().map(ToString::to_string).collect()
}

/// The toasts' messages.
fn toasts(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Vec<String> {
    let layer = workbench.read_with(cx, |workbench, _| workbench.toasts().clone());
    layer.read_with(cx, |layer, _| {
        (layer.queue().shown().iter())
            .map(|entry| entry.toast.message.to_string())
            .collect()
    })
}

/// Runs workbench command `id` with `args`, as a key bound with them would.
fn run_with(id: &str, args: serde_json::Value, cx: &mut VisualTestContext) {
    let context = ContextStack::new().with(ContextFrame::new("workbench"));
    let invocation = Invocation::of(id).unwrap().args(args).context(context);
    cx.update(|window, cx| window.dispatch_action(Box::new(RunCommand(invocation)), cx));
    draw(cx);
}

fn run(id: &str, cx: &mut VisualTestContext) {
    run_with(id, serde_json::Value::Null, cx);
}

/// The user's profile "Dev": `/bin/dev -l -i` in `cwd`, with `STAGE=dev`.
fn dev(cwd: &std::path::Path) -> TerminalProfileSettings {
    TerminalProfileSettings {
        name: "Dev".into(),
        command: "/bin/dev".into(),
        args: vec!["-l".into(), "-i".into()],
        cwd: Some(cwd.to_string_lossy().into_owned()),
        env: BTreeMap::from([("STAGE".into(), "dev".into())]),
        icon: Some("square-terminal".into()),
    }
}

fn panel(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Entity<TerminalPanel> {
    terminal(workbench, cx)
}

#[gpui::test]
fn terminal_new_starts_the_default_profile(cx: &mut TestAppContext) {
    let (workbench, cx, _dir) = opened(cx);
    set_settings(
        &workbench,
        TerminalSettings {
            default_profile: "PowerShell 7".into(),
            ..TerminalSettings::default()
        },
        cx,
    );
    // The first, as the terminal shows, and another.
    toggle(cx);
    run("terminal.new", cx);
    let started = started(&workbench, cx);
    assert_eq!(started.len(), 2);
    for options in &started {
        assert_eq!(shell(options), Some(("/opt/pwsh/pwsh", vec!["-NoLogo"])));
    }
    // Each tab is named after its profile, the program having set no title.
    assert_eq!(labels(&workbench, cx), ["PowerShell 7", "PowerShell 7"]);
    // By its id too, ignoring case.
    set_settings(
        &workbench,
        TerminalSettings {
            default_profile: "WSL:ubuntu".into(),
            ..TerminalSettings::default()
        },
        cx,
    );
    run("terminal.new", cx);
    let last = started_last(&workbench, cx);
    assert_eq!(shell(&last).unwrap().0, "wsl.exe");
    assert_eq!(&shell(&last).unwrap().1[..2], ["-d", "Ubuntu"]);
}

fn started_last(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> TerminalOptions {
    started(workbench, cx).pop().expect("a session started")
}

#[gpui::test]
fn new_with_profile_starts_it_with_its_args_env_and_cwd(cx: &mut TestAppContext) {
    let (workbench, cx, dir) = opened(cx);
    let cwd = dir.path().join("work");
    fs::create_dir(&cwd).unwrap();
    set_settings(
        &workbench,
        TerminalSettings {
            profiles: vec![dev(&cwd)],
            ..TerminalSettings::default()
        },
        cx,
    );
    assert!(!dock_shown(&workbench, cx));
    run_with(
        "terminal.new_with_profile",
        serde_json::json!({ "profile": "Dev" }),
        cx,
    );
    assert!(dock_shown(&workbench, cx));
    let started = started(&workbench, cx);
    assert_eq!(started.len(), 1, "only the profile's session");
    let options = &started[0];
    assert_eq!(shell(options), Some(("/bin/dev", vec!["-l", "-i"])));
    assert_eq!(
        options.env,
        BTreeMap::from([("STAGE".to_owned(), "dev".to_owned())])
    );
    assert_eq!(options.cwd, cwd);
    assert_eq!(labels(&workbench, cx), ["Dev"]);

    // A detected one, beside it, named after it.
    run_with(
        "terminal.new_with_profile",
        serde_json::json!({ "profile": "Command Prompt" }),
        cx,
    );
    assert_eq!(
        shell(&started_last(&workbench, cx)),
        Some(("cmd.exe", vec![]))
    );
    assert_eq!(labels(&workbench, cx), ["Dev", "Command Prompt"]);
    let toasts = toasts(&workbench, cx);
    assert!(toasts.is_empty(), "{toasts:?}");
}

#[gpui::test]
fn a_user_profile_replaces_the_detected_one_of_its_name(cx: &mut TestAppContext) {
    let (workbench, cx, dir) = opened(cx);
    let mut mine = dev(dir.path());
    mine.name = "PowerShell 7".into();
    set_settings(
        &workbench,
        TerminalSettings {
            profiles: vec![mine],
            ..TerminalSettings::default()
        },
        cx,
    );
    let names: Vec<String> = panel(&workbench, cx).read_with(cx, |panel, _| {
        panel.profiles().into_iter().map(|p| p.name).collect()
    });
    assert_eq!(names, ["PowerShell 7", "Command Prompt", "Ubuntu"]);
    run_with(
        "terminal.new_with_profile",
        serde_json::json!({ "profile": "PowerShell 7" }),
        cx,
    );
    assert_eq!(shell(&started_last(&workbench, cx)).unwrap().0, "/bin/dev");
}

#[gpui::test]
fn an_unknown_profile_shows_a_notification_and_starts_nothing(cx: &mut TestAppContext) {
    let (workbench, cx, _dir) = opened(cx);
    run_with(
        "terminal.new_with_profile",
        serde_json::json!({ "profile": "Nope" }),
        cx,
    );
    assert_eq!(toasts(&workbench, cx), ["No terminal profile named Nope"]);
    assert!(started(&workbench, cx).is_empty());
    assert!(!dock_shown(&workbench, cx));

    // With terminals running, none is added.
    toggle(cx);
    assert_eq!(started(&workbench, cx).len(), 1);
    run_with(
        "terminal.new_with_profile",
        serde_json::json!({ "profile": "Nope" }),
        cx,
    );
    assert_eq!(started(&workbench, cx).len(), 1);
    assert_eq!(labels(&workbench, cx), ["Terminal 1"]);
}

#[gpui::test]
fn with_no_default_profile_the_shell_is_as_today(cx: &mut TestAppContext) {
    let (workbench, cx, _dir) = opened(cx);
    // The login shell.
    toggle(cx);
    assert_eq!(started_last(&workbench, cx).shell, None);
    assert_eq!(labels(&workbench, cx), ["Terminal 1"]);
    // terminal.shell, with its args.
    set_settings(
        &workbench,
        TerminalSettings {
            shell: "/bin/fish".into(),
            shell_args: vec!["-l".into()],
            ..TerminalSettings::default()
        },
        cx,
    );
    run("terminal.new", cx);
    let last = started_last(&workbench, cx);
    assert_eq!(shell(&last), Some(("/bin/fish", vec!["-l"])));
    assert!(last.env.is_empty());
    assert_eq!(labels(&workbench, cx), ["Terminal 1", "Terminal 2"]);
}

#[gpui::test]
fn an_unknown_default_profile_says_so_and_runs_the_shell(cx: &mut TestAppContext) {
    let (workbench, cx, _dir) = opened(cx);
    set_settings(
        &workbench,
        TerminalSettings {
            default_profile: "Gone".into(),
            ..TerminalSettings::default()
        },
        cx,
    );
    toggle(cx);
    assert_eq!(toasts(&workbench, cx), ["No terminal profile named Gone"]);
    assert_eq!(started(&workbench, cx).len(), 1);
    assert_eq!(started_last(&workbench, cx).shell, None);
}

#[gpui::test]
fn a_detected_default_profile_waits_for_detection(cx: &mut TestAppContext) {
    let (workbench, cx, _dir) = undetected(cx);
    set_settings(
        &workbench,
        TerminalSettings {
            default_profile: "PowerShell 7".into(),
            ..TerminalSettings::default()
        },
        cx,
    );
    toggle(cx);
    run("terminal.new", cx);
    assert!(started(&workbench, cx).is_empty(), "none until it's found");
    assert!(toasts(&workbench, cx).is_empty());
    terminal(&workbench, cx).update(cx, |terminal, cx| terminal.set_detected(found(), cx));
    draw(cx);
    let started = started(&workbench, cx);
    assert_eq!(started.len(), 2, "each asked for");
    assert!(
        (started.iter()).all(|options| shell(options).unwrap().0 == "/opt/pwsh/pwsh"),
        "{started:?}"
    );
}

#[gpui::test]
fn a_user_profile_never_waits_for_detection(cx: &mut TestAppContext) {
    let (workbench, cx, dir) = undetected(cx);
    set_settings(
        &workbench,
        TerminalSettings {
            default_profile: "Dev".into(),
            profiles: vec![dev(dir.path())],
            ..TerminalSettings::default()
        },
        cx,
    );
    toggle(cx);
    assert_eq!(shell(&started_last(&workbench, cx)).unwrap().0, "/bin/dev");
}

#[gpui::test]
fn without_a_profile_the_command_asks_which(cx: &mut TestAppContext) {
    let (workbench, cx, _dir) = opened(cx);
    run("terminal.new_with_profile", cx);
    let asking = workbench.read_with(cx, |workbench, _| workbench.palette);
    assert_eq!(asking, Some(Palette::Rename));
    assert!(started(&workbench, cx).is_empty());
    cx.simulate_input("ubu");
    draw(cx);
    cx.simulate_keystrokes("enter");
    draw(cx);
    assert!(dock_shown(&workbench, cx));
    let last = started_last(&workbench, cx);
    assert_eq!(&shell(&last).unwrap().1[..2], ["-d", "Ubuntu"]);
    assert_eq!(labels(&workbench, cx), ["Ubuntu"]);
}

/// The names of the open menu's profiles, if it's open.
fn menu(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Option<Vec<String>> {
    panel(workbench, cx).read_with(cx, |panel, _| panel.profile_menu_names())
}

fn click(selector: &'static str, cx: &mut VisualTestContext) {
    let bounds = cx
        .debug_bounds(selector)
        .unwrap_or_else(|| panic!("{selector} is drawn"));
    cx.simulate_click(bounds.center(), Modifiers::none());
    draw(cx);
}

const CARET: &str = "icon-button:terminal-profiles-button";

#[gpui::test]
fn the_caret_lists_every_profile_and_choosing_one_starts_it(cx: &mut TestAppContext) {
    let (workbench, cx, dir) = opened(cx);
    set_settings(
        &workbench,
        TerminalSettings {
            profiles: vec![dev(dir.path())],
            ..TerminalSettings::default()
        },
        cx,
    );
    toggle(cx);
    assert_eq!(started(&workbench, cx).len(), 1);

    // One session: the dock strip's caret.
    click(CARET, cx);
    let every = ["PowerShell 7", "Command Prompt", "Ubuntu", "Dev"];
    assert_eq!(
        menu(&workbench, cx).as_deref(),
        Some(&every.map(String::from)[..])
    );
    for name in every {
        let row: &'static str = format!("menu-item:{name}").leak();
        assert!(cx.debug_bounds(row).is_some(), "{row} is drawn");
    }
    click("menu-item:Ubuntu", cx);
    assert_eq!(menu(&workbench, cx), None, "choosing closes it");
    assert_eq!(started(&workbench, cx).len(), 2);
    let last = started_last(&workbench, cx);
    assert_eq!(&shell(&last).unwrap().1[..2], ["-d", "Ubuntu"]);
    assert_eq!(labels(&workbench, cx), ["Terminal 1", "Ubuntu"]);

    // Two: the panel's own strip has it.
    assert!(panel(&workbench, cx).read_with(cx, |panel, _| panel.strip_drawn()));
    click(CARET, cx);
    assert!(menu(&workbench, cx).is_some());
    // A second click on the caret closes it, starting nothing.
    click(CARET, cx);
    assert_eq!(menu(&workbench, cx), None);
    click(CARET, cx);
    click("menu-item:Dev", cx);
    assert_eq!(shell(&started_last(&workbench, cx)).unwrap().0, "/bin/dev");
    assert_eq!(labels(&workbench, cx), ["Terminal 1", "Ubuntu", "Dev"]);
    assert_eq!(started(&workbench, cx).len(), 3);
}

#[gpui::test]
fn the_menu_takes_its_keys_and_escape_closes_it(cx: &mut TestAppContext) {
    let (workbench, cx, _dir) = opened(cx);
    toggle(cx);
    click(CARET, cx);
    assert!(menu(&workbench, cx).is_some());
    cx.simulate_keystrokes("escape");
    draw(cx);
    assert_eq!(menu(&workbench, cx), None);
    assert_eq!(started(&workbench, cx).len(), 1);

    click(CARET, cx);
    cx.simulate_keystrokes("down down enter");
    draw(cx);
    assert_eq!(menu(&workbench, cx), None);
    assert_eq!(
        shell(&started_last(&workbench, cx)),
        Some(("cmd.exe", vec![]))
    );

    // A click outside it closes it.
    click(CARET, cx);
    assert!(menu(&workbench, cx).is_some());
    let session = cx.debug_bounds("terminal-session").unwrap();
    cx.simulate_click(
        session.bottom_right() - gpui::point(gpui::px(4.), gpui::px(4.)),
        Modifiers::none(),
    );
    draw(cx);
    assert_eq!(menu(&workbench, cx), None);
    assert_eq!(started(&workbench, cx).len(), 2);
}

#[gpui::test]
fn the_menu_says_when_no_profile_is_known(cx: &mut TestAppContext) {
    let (workbench, cx, _dir) = undetected(cx);
    toggle(cx);
    click(CARET, cx);
    assert_eq!(menu(&workbench, cx), Some(Vec::new()));
    assert!(cx.debug_bounds("menu-item:No profiles found").is_some());
    click("menu-item:No profiles found", cx);
    assert_eq!(started(&workbench, cx).len(), 1);
}

#[gpui::test]
fn a_shell_default_starts_without_waiting_for_wsl(cx: &mut TestAppContext) {
    let (workbench, cx, _dir) = undetected(cx);
    set_settings(
        &workbench,
        TerminalSettings {
            default_profile: "PowerShell 7".into(),
            ..TerminalSettings::default()
        },
        cx,
    );
    toggle(cx);
    assert!(started(&workbench, cx).is_empty());
    // The shells land while wsl.exe is still stalled: it starts.
    shells_found(&workbench, cx);
    assert_eq!(
        shell(&started_last(&workbench, cx)),
        Some(("/opt/pwsh/pwsh", vec!["-NoLogo"]))
    );
    // Another shell starts at once, WSL still unanswered.
    run_with(
        "terminal.new_with_profile",
        serde_json::json!({ "profile": "Command Prompt" }),
        cx,
    );
    assert_eq!(
        shell(&started_last(&workbench, cx)),
        Some(("cmd.exe", vec![]))
    );
    // A distro's waits for wsl.exe's answer, then starts.
    run_with(
        "terminal.new_with_profile",
        serde_json::json!({ "profile": "Ubuntu" }),
        cx,
    );
    assert_eq!(started(&workbench, cx).len(), 2);
    assert!(toasts(&workbench, cx).is_empty());
    all_found(&workbench, cx);
    assert_eq!(started(&workbench, cx).len(), 3);
    assert_eq!(
        &shell(&started_last(&workbench, cx)).unwrap().1[..2],
        ["-d", "Ubuntu"]
    );
    assert_eq!(
        labels(&workbench, cx),
        ["PowerShell 7", "Command Prompt", "Ubuntu"]
    );
}

#[gpui::test]
fn an_unknown_profile_asked_for_while_detecting_starts_nothing(cx: &mut TestAppContext) {
    let (workbench, cx, _dir) = undetected(cx);
    run_with(
        "terminal.new_with_profile",
        serde_json::json!({ "profile": "Nope" }),
        cx,
    );
    assert!(started(&workbench, cx).is_empty());
    shells_found(&workbench, cx);
    assert!(started(&workbench, cx).is_empty(), "WSL may have it");
    assert!(toasts(&workbench, cx).is_empty());
    all_found(&workbench, cx);
    draw(cx);
    assert_eq!(toasts(&workbench, cx), ["No terminal profile named Nope"]);
    assert!(
        started(&workbench, cx).is_empty(),
        "not the default shell either"
    );
    assert!(!dock_shown(&workbench, cx));

    // With a terminal running, it stays as it is.
    toggle(cx);
    assert_eq!(started(&workbench, cx).len(), 1);
    run_with(
        "terminal.new_with_profile",
        serde_json::json!({ "profile": "Nope" }),
        cx,
    );
    assert_eq!(started(&workbench, cx).len(), 1);
    assert!(dock_shown(&workbench, cx));
}

#[gpui::test]
fn the_menu_lists_profiles_found_while_it_is_open(cx: &mut TestAppContext) {
    let (workbench, cx, _dir) = undetected(cx);
    toggle(cx);
    click(CARET, cx);
    assert_eq!(menu(&workbench, cx), Some(Vec::new()));
    shells_found(&workbench, cx);
    assert_eq!(
        menu(&workbench, cx).as_deref(),
        Some(&["PowerShell 7", "Command Prompt"].map(String::from)[..])
    );
    assert!(cx.debug_bounds("menu-item:Command Prompt").is_some());
    // The highlight stays on its profile as more land.
    cx.simulate_keystrokes("down down");
    draw(cx);
    all_found(&workbench, cx);
    let every = ["PowerShell 7", "Command Prompt", "Ubuntu"];
    assert_eq!(
        menu(&workbench, cx).as_deref(),
        Some(&every.map(String::from)[..])
    );
    cx.simulate_keystrokes("enter");
    draw(cx);
    assert_eq!(menu(&workbench, cx), None);
    assert_eq!(
        shell(&started_last(&workbench, cx)),
        Some(("cmd.exe", vec![]))
    );
}

#[gpui::test]
fn the_picker_lists_profiles_found_while_it_is_open(cx: &mut TestAppContext) {
    let (workbench, cx, _dir) = undetected(cx);
    run("terminal.new_with_profile", cx);
    shells_found(&workbench, cx);
    // Its query stays as they land.
    cx.simulate_input("ubu");
    draw(cx);
    cx.simulate_keystrokes("enter");
    draw(cx);
    assert!(started(&workbench, cx).is_empty(), "no Ubuntu yet");
    all_found(&workbench, cx);
    cx.simulate_keystrokes("enter");
    draw(cx);
    assert!(dock_shown(&workbench, cx));
    let last = started_last(&workbench, cx);
    assert_eq!(&shell(&last).unwrap().1[..2], ["-d", "Ubuntu"]);
}

#[gpui::test]
fn the_menu_lists_the_profiles_a_settings_change_gives(cx: &mut TestAppContext) {
    let (workbench, cx, dir) = opened(cx);
    let with = |profiles| TerminalSettings {
        profiles,
        ..TerminalSettings::default()
    };
    set_settings(&workbench, with(vec![dev(dir.path())]), cx);
    toggle(cx);
    click(CARET, cx);
    // Up from none is the last row: Dev.
    cx.simulate_keystrokes("up");
    draw(cx);
    let mut alpha = dev(dir.path());
    alpha.name = "Alpha".into();
    alpha.command = "/bin/alpha".into();
    panel_settings(&workbench, with(vec![alpha, dev(dir.path())]), cx);
    let every = ["PowerShell 7", "Command Prompt", "Ubuntu", "Alpha", "Dev"];
    assert_eq!(
        menu(&workbench, cx).as_deref(),
        Some(&every.map(String::from)[..])
    );
    assert!(
        cx.debug_bounds("menu-item:Alpha").is_some(),
        "its row is drawn"
    );
    // The highlight stays on Dev, a row further down now.
    cx.simulate_keystrokes("enter");
    draw(cx);
    assert_eq!(menu(&workbench, cx), None);
    assert_eq!(shell(&started_last(&workbench, cx)).unwrap().0, "/bin/dev");
    // A profile the settings drop leaves it.
    toggle_menu_with(&workbench, with(Vec::new()), cx);
    assert!(cx.debug_bounds("menu-item:Dev").is_none(), "no row for Dev");
    assert!(cx.debug_bounds("menu-item:Ubuntu").is_some());
}

/// Opens the caret's menu, then applies `settings` with it open.
fn toggle_menu_with(
    workbench: &Entity<Workbench>,
    settings: TerminalSettings,
    cx: &mut VisualTestContext,
) {
    click(CARET, cx);
    assert!(menu(workbench, cx).is_some());
    panel_settings(workbench, settings, cx);
}

/// Applies `settings` to the terminal panel alone, nothing else in the
/// window changing: the panel itself has to draw again.
fn panel_settings(
    workbench: &Entity<Workbench>,
    settings: TerminalSettings,
    cx: &mut VisualTestContext,
) {
    let panel = panel(workbench, cx);
    cx.run_until_parked();
    let drawn = std::rc::Rc::new(std::cell::Cell::new(false));
    let seen = std::rc::Rc::clone(&drawn);
    let _watch = cx.update(|_, cx| cx.observe(&panel, move |_, _| seen.set(true)));
    panel.update(cx, |panel, cx| panel.set_settings(&settings, cx));
    assert!(drawn.get(), "the panel asks to be drawn again");
    draw(cx);
}

/// The workbench tests that failed on Windows (a11y, grammar prompts, hot
/// exit and others) built a terminal panel under the test platform, whose
/// detecting thread landed off the test scheduler. Nothing may detect here:
/// were a thread detecting, its landing would wake the panel's task off the
/// scheduler and fail this test as nondeterministic as it ends.
#[gpui::test]
fn the_test_platform_detects_nothing_off_the_executor(cx: &mut TestAppContext) {
    let (workbench, cx, _dir) = undetected(cx);
    toggle(cx);
    run("terminal.new", cx);
    assert_eq!(started(&workbench, cx).len(), 2);
    // What a detecting thread would have done by now, it has.
    std::thread::sleep(std::time::Duration::from_millis(200));
    cx.run_until_parked();
    draw(cx);
    assert!(
        !profiles::detection_started(),
        "no detection runs under the test platform"
    );
    let found: Vec<String> = panel(&workbench, cx).read_with(cx, |panel, _| {
        panel.profiles().into_iter().map(|p| p.name).collect()
    });
    assert!(found.is_empty(), "only what the test lands: {found:?}");
}
