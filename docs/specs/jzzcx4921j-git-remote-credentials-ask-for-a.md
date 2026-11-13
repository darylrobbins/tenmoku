---
task_uid: "01m78dyx5weq37eejzzcx4921j"
task: "T334"
rev: 1
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "Tests in remote_askpass (cargo nextest run -p fe-git --test remote_askpass) pass against a local fake askpass helper: a_credential_request_reaches_the_prompt_and_the_answer_reaches_git, cancelling_fails_the_op_with_the_old_credentials_message, no_prompt_is_attempted_when_askpass_is_unset_by_the_caller, nothing_secret_is_logged_or_in_the_toast"
    required: true
    manual: false
  - id: "C2"
    text: "Tests in tests::git_askpass (cargo nextest run -p fe-workbench) pass: a_request_opens_a_masked_input_naming_the_remote, answering_continues_the_push, escape_cancels_and_reports, one_prompt_at_a_time_across_windows"
    required: true
    manual: false
  - id: "C3"
    text: "remote_signing, pull_signing_policy and git_remote_tests still pass"
    required: true
    manual: false
---

# Git remote credentials: ask for a username/password or ssh passphrase in a prompt instead of failing (Mac T1748)

# Credential prompts for fetch/pull/push

Follows the git sync UI card. Today remote.rs runs git with no stdin, GIT_TERMINAL_PROMPT=0, GIT_ASKPASS="" and ssh BatchMode, so a remote that wants credentials git cannot find fails with a message (RemoteError credentials variant, git.ftl). Credential helpers (osxkeychain, GCM) and ssh-agent keys still work; this card covers what they cannot answer.

## Do
- GIT_ASKPASS/SSH_ASKPASS point to a small helper (the app's own executable in an `--askpass` mode, or a script written to the state dir; decide, and say why in the review) that talks to the running app over a per-operation unix socket in the state dir (mode 600, removed at the end). The app shows a masked input (kiln-shell prompt) naming the prompt text git gave and returns the answer. The answer is never written to logs, toasts or disk. Cancel = failure with today's message.
- Keep the non-interactive default when no UI is attached (tests, headless). ssh: replace BatchMode with askpass only when the user has no GIT_SSH_COMMAND/core.sshCommand of their own.
- One prompt at a time across windows.
- Windows is out of scope (state it in the module doc); gate the unix socket by cfg and keep the old path elsewhere.

Platform: macOS/Linux; not Windows.

## Implementation notes

- **Helper: the app's own executable (`tenmoku --askpass <socket> <prompt>`), reached through a two-line `#!/bin/sh` script written into each command's private directory.** Git and ssh run `GIT_ASKPASS`/`SSH_ASKPASS` as a bare program with the prompt as its only argument, so something has to supply the `--askpass` flag and the socket path; the script does that and nothing else. A script that talks to the socket itself was rejected: no unix-socket client (`nc -U`, `socat`, python) is on every Mac and Linux. `main` checks for `--askpass` before the log file, IPC or anything else starts.
- fe-git `askpass` module: `Askpass::new(helper, dir, answer)` and `Repo::set_askpass`. Each remote command with one gets `<state>/askpass-<pid>-<n>/` (mode 700) holding the socket `s` (mode 600) and the script. A thread answers on the socket by calling the caller's `answer` function. The directory is removed when git exits. A `Repo` has no askpass by default, so every existing caller and test keeps today's env (`GIT_ASKPASS=""`, ssh `BatchMode=yes`). If the session can't be set up (bind fails, say), the command falls back to that same non-interactive env.
- ssh: `SSH_ASKPASS` plus `SSH_ASKPASS_REQUIRE=force` replace `BatchMode` only when there's no `GIT_SSH`, `GIT_SSH_COMMAND` or `core.sshCommand`. A user's own ssh command runs as before. Git's own `GIT_ASKPASS` prompts (https) go through the askpass either way.
- Workbench: `git_askpass.rs`. The app calls `set_git_askpass` (Unix only). Without it, as in tests and headless runs, nothing prompts. A global queue shows one prompt at a time across windows, each in the window its command started in, as a modal over the modal layer: a `PaletteFrame` with a masked `TextField`. Enter answers. Escape, focus leaving in the window, or another modal cancels, which fails with today's `workbench-git-remote-auth` message. The window going inactive (fetching a password elsewhere) hides the prompt; it shows again on reactivation. A closed window cancels its prompt. The field is cleared once it's answered.
- A command with an askpass runs on its own thread instead of the background pool, since it may wait on the user indefinitely.
- Tests use their own test binary as the helper (`askpass_helper`, rerun through `sh -c` with the answer on fd 3, since libtest owns stdout). fe-git uses a Basic-auth dumb-http server and a fake `ssh` on `PATH`. The workbench uses a fake `core.sshCommand` that calls `$GIT_ASKPASS`.
- Review round 1:
  - **F1:** with an askpass, git runs with `-c credential.helper= -c credential.helper=!<session script>`. That script answers `get` with `git credential fill` (the user's helpers, with the session's `-c` config and every prompt off) and passes `erase` on as `git credential reject`. It drops `store`, so stored credentials still answer but a prompted one is never kept.
  - **F2:** git runs in its own process group. A cancel kills the group (git, ssh, hooks, the helper), and every later prompt is refused unasked. The command fails with `RemoteError::Auth`, whose output is git's text or "the credential prompt was cancelled".
  - **F3:** the answer function gets a `Withdrawn`. The helper keeps its connection open, so its end means it went (OpenSSH stops one once a notice's done), and so does the session ending. The workbench drops a withdrawn request from the queue or closes its prompt. `SSH_ASKPASS_PROMPT=none` notices end in the helper without asking.
  - **F4:** each command's askpass watches `on_window_closed` and cancels that window's prompts, both waiting and showing.
  - **F5:** the session directory is made absolute.
- Review round 2:
  - **F2:** a cancel that lands before git's process group is registered still stops it. `cancel` stores the flag and then reads the group; `started` stores the group and then reads the flag (both SeqCst), so one of them sees the other.
  - **F3:** a request is registered under the same lock teardown takes, with `done` checked there. A session that ends while a prompt is still arriving asks nothing.
  - **F4:** a request whose window closed before it arrived is dropped, which cancels it.
  - **F6:** the credential script's lookup answers fill's prompts empty (`GIT_ASKPASS=true`) and drops the empty fields, so a helper's partial answer is used and git asks only for the rest. A helper's `quit` is passed on.
  - **F7:** every connection is shut down after its reply or withdrawal, so a helper and its watch thread never outlive the session.
- Not covered: ssh's unknown-host-key question also arrives through `SSH_ASKPASS` and is shown masked (batch mode refused it before); answering `yes` works.
