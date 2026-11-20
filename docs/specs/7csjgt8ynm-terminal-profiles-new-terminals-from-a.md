---
task_uid: "01m7t1bv43yh4pt87csjgt8ynm"
task: "T696"
rev: 9
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "fe-terminal tests/profiles.rs: wsl.exe -l -q output in UTF-16LE with a BOM, CRLF line ends and blank lines parses to exactly the distro names, and each WSL profile runs wsl.exe -d \u003cdistro\u003e"
    required: true
    manual: false
  - id: "C2"
    text: "fe-terminal tests/profiles.rs: with a fake probe, Windows detection yields PowerShell 7, Windows PowerShell 5.1, Command Prompt, Git Bash and one profile per WSL distro in that order, omits each absent shell, and a wsl.exe that times out or fails yields no WSL profiles"
    required: true
    manual: false
  - id: "C3"
    text: "fe-terminal tests/profiles.rs: /etc/shells parsing skips comments, missing files and nologin, and the Default profile is the login shell"
    required: true
    manual: false
  - id: "C4"
    text: "fe-terminal tests/profiles.rs: WSL cwd mapping turns C:\\a\\b into /mnt/c/a/b and \\\\wsl.localhost\\D\\x and \\\\wsl$\\D\\x into /x, and an unmappable cwd passes no --cd"
    required: true
    manual: false
  - id: "C5"
    text: "fe-config tests/terminal_profiles.rs: terminal.profiles (name, command, args, cwd, env, icon) and terminal.default_profile parse; a user profile replaces a detected one of the same name; a project file's profiles and default_profile are dropped with a warning; terminal.shell keeps today's behaviour when no profile is set"
    required: true
    manual: false
  - id: "C6"
    text: "fe-workbench tests/terminal_profiles.rs: terminal.new starts the default profile; terminal.new_with_profile with {profile} starts that profile with its args, env and cwd and names the tab after it; an unknown profile shows a notification and starts nothing; with no default_profile set the shell is the same as today"
    required: true
    manual: false
  - id: "C7"
    text: "fe-workbench tests/terminal_profiles.rs and tests/terminal_plus.rs: the + keeps its size and its click; its caret opens a menu listing every profile; choosing one starts that profile"
    required: true
    manual: false
  - id: "C8"
    text: "The fe-config and fe-workbench l10n lints and typed_command_ids_tests and command_catalog_tests pass with the new strings and command"
    required: true
    manual: false
  - id: "C9"
    text: "Windows CI (draft PR, Windows job) passes the fe-terminal profiles tests, including a real-probe smoke test that finds the Command Prompt profile and spawns it through ConPTY to echo a marker, and fe-workbench tests::terminal_profiles"
    required: true
    manual: false
  - id: "C10"
    text: "crates/fe-terminal/src/session.rs is not modified, and the existing terminal_tabs_tests, terminal_plus and fe-terminal session tests pass on Linux (helium gate); the macOS gate runs them when the batch lands on the Mac"
    required: true
    manual: false
  - id: "C11"
    text: "Under the GPUI test platform no terminal-profile detection thread wakes the test scheduler: detection runs on the app's background executor with an injectable probe, or does not start under the test platform. A fe-workbench test proves it. The Windows run's fe-workbench FAIL set equals main's at the same base, and its log has no 'Detected activity on thread'"
    required: true
    manual: false
---

# Terminal profiles: new terminals from a detected or user-defined profile (PowerShell 7, Windows PowerShell, Command Prompt, Git Bash, each WSL distro; /etc/shells on macOS and Linux), a profile menu on the + button and a default profile setting (Mac T1975)

# Terminal profiles

The user's request: "On Windows, for the terminal, we should be able to create terminals for different
profiles - like Powershell, CMD, WSL2 shell - like you can in the windows terminal app".

## Today

`TerminalPanel::shell()` (crates/fe-workbench/src/terminal_panel.rs, ~l.1300) returns
`terminal.shell`/`terminal.shell_args` from settings, or None for the login shell (macOS
`/usr/bin/login -flp`, the passwd shell elsewhere, the ConPTY default on Windows). The tab strip's `+`
(`terminal-new-button`) dispatches `terminal.new` (workbench_chrome.rs ~l.634). Project files cannot set
`terminal.shell` (`drop_terminal_shell`, PROJECT_TERMINAL_SHELL in fe-config settings.rs).

## What to build

One cross-platform model; only detection differs per OS.

- **crates/fe-terminal/src/profiles.rs** (pure, headless): `Profile { id, name, program, args, cwd,
  env, icon, kind: Shell | Wsl { distro } }`, a `ProfileProbe` trait (PATH lookup, file exists,
  command output) so tests fake it, `detect_windows`, `detect_unix`, `parse_wsl_list`, `wsl_cwd`.
  - Windows, in this order, each only when present: PowerShell 7 (`pwsh` on PATH or
    `Program Files\PowerShell\7`), Windows PowerShell 5.1 (`System32\WindowsPowerShell\v1.0`),
    Command Prompt (`%ComSpec%`), Git Bash (`Program Files\Git\bin\bash.exe --login -i`), then one
    profile per WSL distro from `wsl.exe -l -q` (UTF-16LE: strip BOM, NULs, CRLF, blank lines;
    program `wsl.exe`, args `-d <distro>`). Run `wsl.exe` with a timeout; a timeout or error means no
    WSL profiles, never a hang.
  - macOS/Linux: "Default" (the login shell, today's behaviour) plus one profile per `/etc/shells`
    entry (skip comments, missing files and nologin).
  - Detection runs off the UI thread and is cached; it never blocks opening a terminal.
- **WSL working folder:** a Windows cwd `C:\a\b` maps to `/mnt/c/a/b`; `\\wsl$\D\x` and
  `\\wsl.localhost\D\x` map to `/x`; pass it as `--cd <path>`. An unmappable cwd passes no `--cd`.
  Environment is not carried into WSL (WSLENV is out of scope; say so in the decision file).
- **Settings (fe-config, user-only):** `terminal.default_profile` (name; empty keeps today's
  behaviour, so the Windows default does not change unless the user picks one) and
  `terminal.profiles` (user-defined: name, command, args, cwd, env, icon). A user profile replaces a
  detected one of the same name. Project files' `profiles`/`default_profile` are dropped with a
  warning, like `terminal.shell` today. Existing `terminal.shell`/`shell_args` still work when no
  profile is set.
- **UI:** the `+` keeps its size and click (`terminal.new` = the default profile) and gains a caret that
  opens a popup menu listing every profile (follow the existing tab context-menu pattern). New
  command `terminal.new_with_profile`, "New Terminal with Profile…", opens a picker; it takes an
  optional `{profile}` argument so it can be bound. No default key binding. A new tab is named after
  its profile until the program sets a title. Icons come from the existing icon set only.
- A profile resolves to the existing `TerminalOptions` (shell, env, cwd). Do NOT edit
  crates/fe-terminal/src/session.rs or the console-title handling (`program_title`,
  `TITLE_PREFIX`); T1974 owns them. An elevated console's "Administrator: " prefix is handled there.
- All new strings in terminal.ftl / settings.ftl; no hard-coded English.
- Write docs/decisions/<lane>-terminal-profiles.md.

## Evidence

Windows behaviour (C9) comes from a draft PR's Windows CI job: commit, give the lead the sha, and the
lead pushes it to `ci/t<n>-windows` and opens the PR. Do not push yourself. Quote the job's PASS lines
for the named tests in the criteria file.

## Helium filing note

- Lent from the Mac as y150, the user's request. No dependency.
- **Never modify crates/fe-terminal/src/session.rs** (Mac T1974 owns it) nor the console-title handling.
- **Windows evidence (C9) comes before the review round** (the user's choice: a draft PR per card). When the
  code is committed and the Linux tests pass, tell the lead the commit SHA (`herdr agent prompt wR7:p1 ...`) and
  wait: the Mac lead pushes it to origin `ci/t1975-windows` and sends back the Windows job lines. Quote the PASS
  lines for the named tests in the criteria file, then finish. If the Windows job fails, fix it and send the new
  SHA. Never push to origin yourself.
- **C10 on helium** covers Linux only; the macOS gate runs at landing on the Mac.
- Settings: workbench-wide settings reach the panel only through workbench_root.rs reload_settings; if
  `terminal.default_profile`/`terminal.profiles` need to reach TerminalPanel at runtime and that file is out
  of scope, ask the lead for a respec first (`rgt task respec --scope` replaces the list).
- Run `wsl.exe` and every probe off the UI thread with a timeout (a dedicated thread, not a blocking executor
  worker). Headless tests use a fake ProfileProbe; the real-probe ConPTY smoke test (C9) is `#[cfg(windows)]`.
- Builds and loops: CARGO_TARGET_DIR inside your worktree, never /tmp. Ask the lead before rgt finish if
  anything in the criteria can't be met as written.

## Implementation notes

- Scope widened (respec) for files the change needs: `workbench_commands.rs` (the
  command beside `terminal.new`), `fe-terminal-view/src/lib.rs` (re-exports
  `profiles`; fe-workbench has no fe-terminal dependency), fe-config's generated
  `settings.schema.json`, `settings-template.toml` and its qps-ploc fixture,
  `settings_model.rs` (the settings editor's user-only keys), `l10n-allow.toml`
  (the widened `PROJECT_TERMINAL_SHELL` text), fe-workbench's `commands.ftl` (the
  command's title), and `tests/terminal_single_header.rs` plus the screenshot
  audit's `terminal_session.rs`, which pinned the strip order `+, kill` (now `+`,
  caret, kill).
- The Default profile on macOS/Linux is `ProfileKind::Login`, a third kind beside
  `Shell` and `Wsl`, so it runs exactly today's login shell
  (docs/decisions/7csjgt8ynm-terminal-profiles.md).
- An unknown `terminal.default_profile` toasts and still starts today's shell; only
  an explicit `{profile}` that's unknown starts nothing.
