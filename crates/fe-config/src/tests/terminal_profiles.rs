//! `terminal.profiles` and `terminal.default_profile`: parsed from the user's
//! file, merged over the profiles found on the machine, and dropped from a
//! project file with a warning; `terminal.shell` unchanged without them.

use std::collections::BTreeMap;

use crate::{PROJECT_TERMINAL_SHELL, SettingsStore, TerminalProfileSettings, TerminalSettings};

use super::write;

fn load(user: &str, project: Option<&str>) -> SettingsStore {
    let dir = tempfile::tempdir().unwrap();
    let user = write(&dir, "settings.toml", user);
    let project = project.map(|text| write(&dir, "project.toml", text));
    SettingsStore::load(Some(user), project)
}

const PROFILES: &str = r#"
[terminal]
default_profile = "Dev"

[[terminal.profiles]]
name = "Dev"
command = "/bin/zsh"
args = ["-l", "-i"]
cwd = "/work"
env = { STAGE = "dev", EDITOR = "vi" }
icon = "square-terminal"

[[terminal.profiles]]
name = "Command Prompt"
command = 'C:\tools\cmd.exe'
"#;

#[test]
fn profiles_and_the_default_parse() {
    let store = load(PROFILES, None);
    assert_eq!(store.diagnostics(), []);
    let terminal = &store.settings().terminal;
    assert_eq!(terminal.default_profile, "Dev");
    assert_eq!(
        terminal.profiles,
        [
            TerminalProfileSettings {
                name: "Dev".into(),
                command: "/bin/zsh".into(),
                args: vec!["-l".into(), "-i".into()],
                cwd: Some("/work".into()),
                env: BTreeMap::from([
                    ("EDITOR".into(), "vi".into()),
                    ("STAGE".into(), "dev".into())
                ]),
                icon: Some("square-terminal".into()),
            },
            TerminalProfileSettings {
                name: "Command Prompt".into(),
                command: r"C:\tools\cmd.exe".into(),
                ..TerminalProfileSettings::default()
            },
        ]
    );
}

#[test]
fn a_bad_profile_is_an_error_and_keeps_the_last_good() {
    let store = load("[[terminal.profiles]]\nname = 1\n", None);
    let [diagnostic] = store.diagnostics() else {
        panic!("{:#?}", store.diagnostics());
    };
    assert!(diagnostic.is_error(), "{diagnostic}");
    assert!(store.settings().terminal.profiles.is_empty());
}

/// A detected profile, as the tests have it: its name and what it runs.
#[derive(Debug, PartialEq)]
struct Found(String, String);

fn found(name: &str, program: &str) -> Found {
    Found(name.into(), program.into())
}

fn merged(terminal: &TerminalSettings, detected: Vec<Found>) -> Vec<Found> {
    terminal.profiles_over(
        detected,
        |profile| &profile.0,
        |user| Found(user.name.clone(), user.command.clone()),
    )
}

#[test]
fn a_user_profile_replaces_a_detected_one_of_its_name() {
    let store = load(PROFILES, None);
    let detected = vec![
        found("PowerShell 7", "pwsh.exe"),
        found("Command Prompt", r"C:\WINDOWS\system32\cmd.exe"),
        found("Ubuntu", "wsl.exe"),
    ];
    assert_eq!(
        merged(&store.settings().terminal, detected),
        [
            found("PowerShell 7", "pwsh.exe"),
            // In its place, the user's.
            found("Command Prompt", r"C:\tools\cmd.exe"),
            found("Ubuntu", "wsl.exe"),
            // The user's others after.
            found("Dev", "/bin/zsh"),
        ]
    );
}

#[test]
fn a_profile_without_a_name_or_command_is_left_out_and_the_later_of_two_wins() {
    let store = load(
        "[[terminal.profiles]]\nname = \"\"\ncommand = \"/bin/sh\"\n\n\
         [[terminal.profiles]]\nname = \"x\"\n\n\
         [[terminal.profiles]]\nname = \"Two\"\ncommand = \"/bin/first\"\n\n\
         [[terminal.profiles]]\nname = \"two\"\ncommand = \"/bin/second\"\n",
        None,
    );
    assert_eq!(store.diagnostics(), []);
    assert_eq!(
        merged(&store.settings().terminal, vec![found("x", "/bin/x")]),
        [found("x", "/bin/x"), found("two", "/bin/second")]
    );
}

#[test]
fn a_project_file_cannot_set_profiles_or_the_default() {
    let store = load(
        "[terminal]\ndefault_profile = \"Mine\"\n",
        Some(
            "[terminal]\ndefault_profile = \"Evil\"\nscrollback = 42\n\n\
             [[terminal.profiles]]\nname = \"Evil\"\ncommand = \"/bin/evil\"\n",
        ),
    );
    let terminal = &store.settings().terminal;
    assert_eq!(terminal.default_profile, "Mine");
    assert!(terminal.profiles.is_empty());
    assert_eq!(terminal.scrollback, 42);
    let mut keys: Vec<_> = (store.diagnostics().iter())
        .map(|d| {
            assert!(!d.is_error(), "{d:?}");
            assert_eq!(d.message, PROJECT_TERMINAL_SHELL);
            d.key.clone().unwrap()
        })
        .collect();
    keys.sort();
    assert_eq!(keys, ["terminal.default_profile", "terminal.profiles"]);
}

#[test]
fn the_shell_works_as_before_without_a_profile() {
    let store = load(
        "[terminal]\nshell = \"/bin/fish\"\nshell_args = [\"-l\"]\n",
        None,
    );
    assert_eq!(store.diagnostics(), []);
    let terminal = &store.settings().terminal;
    assert_eq!(terminal.shell, "/bin/fish");
    assert_eq!(terminal.shell_args, ["-l"]);
    assert_eq!(terminal.default_profile, "");
    assert!(terminal.profiles.is_empty());
    // Nothing of the user's: the detected profiles as they were.
    assert_eq!(
        merged(terminal, vec![found("Default", "/bin/zsh")]),
        [found("Default", "/bin/zsh")]
    );
}
