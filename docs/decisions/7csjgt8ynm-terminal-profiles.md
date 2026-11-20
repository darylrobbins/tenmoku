# Terminal profiles: design calls (T696, Mac T1975)

New terminals can start from a *profile*, as Windows Terminal's do: PowerShell
7, Windows PowerShell, Command Prompt, Git Bash and each WSL distro on Windows;
the login shell and each `/etc/shells` entry on macOS and Linux; and the
user's own. The model is `crates/fe-terminal/src/profiles.rs` (headless, its
module docs are the reference); the panel side is `TerminalPanel` in
`crates/fe-workbench/src/terminal_panel.rs`.

- **One model, detection per OS.** `Profile { id, name, program, args, cwd,
  env, icon, kind }`; `kind` is `Login`, `Shell` or `Wsl { distro }`. `Login`
  is a third kind beside the spec's two: the macOS/Linux "Default" profile has
  to run exactly what a terminal with no profile runs today (`TerminalOptions.
  shell = None`: `/usr/bin/login -flp` on macOS, the passwd shell elsewhere),
  which no program and arguments can stand for. Its `program` (`$SHELL`, else
  `/bin/sh`) only names it.
- **A probe in front of the machine.** `ProfileProbe` answers PATH lookups,
  file existence, environment variables, file reads and a program's output
  with a timeout; tests fake it, `SystemProbe` asks the machine. `detect_windows`
  and `detect_unix` are plain functions of a probe, so both are tested on every
  OS. Windows paths are built with `\` whatever the host's separator
  (`windows_join`), so detection reads the same in a Linux test.
- **Windows order and presence.** PowerShell 7 (`pwsh` on PATH, else
  `%ProgramFiles%\PowerShell\7\pwsh.exe`), Windows PowerShell 5.1
  (`%SystemRoot%\System32\WindowsPowerShell\v1.0\powershell.exe`), Command
  Prompt (`%ComSpec%`), Git Bash (`%ProgramFiles%\Git\bin\bash.exe --login -i`),
  then `wsl.exe -l -q`'s distros, each only when it's there. The default
  Windows terminal is unchanged: with `terminal.default_profile` empty a
  terminal still runs `terminal.shell` or ConPTY's default.
- **`wsl.exe` can't hang detection.** It runs with stdin and stderr closed,
  `CREATE_NO_WINDOW`, its stdout read on a thread of its own, and is killed
  after `WSL_TIMEOUT` (5 s). A timeout, a failure to start or a non-zero exit
  (no WSL, no distros) all mean no WSL profiles. Its output is UTF-16LE (BOM,
  CRLF, blank lines), or UTF-8 under `WSL_UTF8=1`; `parse_wsl_list` takes
  both. Every distro listed is a profile, `docker-desktop` ones included.
- **Off the UI thread, once per process, in stages.** `profiles::detected`
  runs detection on a dedicated thread (never an executor worker, which a 5 s
  wait would hold) the first time it's asked, caches the result for the
  process and hands it to every caller stage by stage
  (`detect_in_stages`): on Windows the shells first, found by looking at
  files and the environment with no program run, then those and the WSL
  distros once `wsl.exe` has answered or timed out (`done`). A caller after
  a stage has it at once and the later ones as they land. macOS and Linux
  run no program, so theirs is one stage. A distro installed later shows
  after a restart. The panel asks as it's made and holds what's found so far
  in `detected`, and whether more may come in `detecting`.
- **Nothing detects under the GPUI test platform.** The panel starts
  detection only when `!cx.is_test()` (as `project_panel.rs` and
  `update_check.rs` decide), so in every crate's tests, not just
  fe-workbench's own (`cfg!(test)` is off in integration tests and the
  `tenmoku` binary's), no detecting thread runs. Its landing would wake the
  panel's task from off the test scheduler, which the scheduler fails as
  nondeterministic ("Detected activity on thread terminal-profiles"): on
  Windows, where `wsl.exe` makes detection slow enough to land after the
  task waits, that failed every test that builds a workbench. Tests give the
  panel what detection found with `TerminalPanel::detection_landed`, and
  `profiles::detection_started` says whether anything detected. Running
  detection on `cx.background_executor()` instead was the alternative: it
  would hold an executor worker for up to `WSL_TIMEOUT`, and under the test
  scheduler run the real probe (files, `wsl.exe`) on the test thread unless
  every test injected a fake one. So under the test platform the panel's
  profiles are only the user's until a test lands some, and a terminal asked
  for a detected profile there waits until one does.
- **Never blocks opening a terminal, and a shell never waits on WSL.** A
  session asked for a profile that neither the user's list nor what's found
  so far has waits in `waiting` while detection runs, and is tried again as
  each stage lands (the panel's next frame). So a detected shell's terminal
  starts with the first stage, before `wsl.exe` is even run, and only a WSL
  distro's (or an unknown name) waits for `wsl.exe`, at most `WSL_TIMEOUT`;
  the UI never waits. Anything else starts at once: no default profile, a
  user profile, or a profile already found. If the thread can't be spawned,
  detection is done having found nothing.
- **Unknown names.** `terminal.new_with_profile {profile}` with a name no
  profile has shows a toast and starts nothing. Asked for while detection
  runs, the dock shows with the request waiting; once detection finishes
  without it, the toast shows and, if the panel has no session, it hides
  the dock (`TerminalPanelEvent::Exited`) instead of starting the default
  shell in its place. An unknown
  `terminal.default_profile` shows the same toast but still starts a session
  with today's shell, so a stale setting never leaves the terminal unable to
  open (and `TerminalPanel::start`, which runs as the panel draws, can't loop).
- **Names, ids and labels.** A profile is picked by its name or its id
  (`pwsh`, `wsl:Ubuntu`, `shell:/bin/zsh`, `user:<name>`), ignoring ASCII
  case. Detected names are the canonical English product names, which is what
  `default_profile` and a user profile's name match, so a setting means the
  same in every locale. The menu, picker and tab show "Default" and "Command
  Prompt" from terminal.ftl; the rest are product or distro names, shown as
  they are. An `/etc/shells` entry is named for its file, by its path if an
  earlier entry took that name (`/bin/bash` and `/usr/bin/bash`).
- **User profiles (fe-config).** `[[terminal.profiles]]` with `name`, `command`,
  `args`, `cwd`, `env`, `icon`; `TerminalSettings::profiles_over` lays them
  over the detected list, replacing one of the same name in place and
  appending the rest. One without a name or command is left out; of two with
  one name the later wins. It is generic over the profile type, because
  fe-config can't depend on fe-terminal. Both keys are user-only: a project
  file's are dropped with the `PROJECT_TERMINAL_SHELL` warning (its text now
  names all four keys), and the settings editor treats them as user-only.
- **Settings reach the panel as today.** Both keys are `TerminalSettings`
  fields, which `Workbench::reload_settings` already hands the panel
  (`set_settings`); no new path.
- **Resolving to a session.** `Profile::options(cwd, scrollback)` gives the
  `TerminalOptions` (shell, env, cwd) the session runs; `session.rs` and the
  console-title handling are untouched (Mac T1974 owns them). A profile's own
  `cwd` wins if it's a directory. A tab is named after its profile until its
  program sets a title; with no profile it's `Terminal <n>` as before.
- **WSL working folder.** `wsl_cwd` maps `C:\a\b` to `/mnt/c/a/b`, and
  `\\wsl$\<distro>\x` or `\\wsl.localhost\<distro>\x` to `/x` *for that distro
  only* (another distro's share doesn't exist inside this one), passed as
  `--cd`. Anything else (a network share, a relative path) passes no `--cd`,
  and WSL starts in its own default. A profile's own `cwd` starting with `/`
  or `~` is the distro's and goes to `--cd` as it is. The session's own cwd
  stays the Windows folder, which ConPTY needs.
- **Environment isn't carried into WSL.** A WSL profile's `env` reaches
  `wsl.exe`, not the distro: forwarding it needs `WSLENV`, which is out of
  scope.
- **UI.** The `+` keeps its size, place and click (`terminal.new`, the
  default profile). A caret (`terminal-profiles-button`, `chevron-down`, the
  same 22 px icon button) follows it, before kill, in both headers: the dock
  strip's `PaneAction` with `anchored` (as the project tree's `+` menu), and
  the panel's own strip, which records the caret's bounds as it prepaints.
  The menu follows the tree's New menu and the Graph's row menu: a
  `MenuPanel` under the caret, Up/Down/Enter/Escape while the panel holds
  focus, closed by a click outside it (a press on the caret is its own
  click's, which closes it) or focus leaving the panel. With no profile known
  it has one disabled row. Its rows are the profiles there are as it draws,
  so ones detection finds while it's open appear, the highlight held by
  profile id. The command's picker lists them again on
  `TerminalPanelEvent::ProfilesChanged` (a detection stage, a settings
  change), keeping its query and selected profile. `MenuEntry` has no icons, so a profile's `icon`
  (a name from kiln-ui's icon set, else `square-terminal`) shows in
  `terminal.new_with_profile`'s picker rows.
- **The command.** `terminal.new_with_profile` ("New Terminal with Profile…")
  sits beside `terminal.new` in `workbench_commands.rs`, reading its optional
  `profile` argument from the invocation. Without it, a picker lists every
  profile, filtered by the query. No default key; fastedit.toml has a
  commented example binding with `args = { profile = "…" }`.
