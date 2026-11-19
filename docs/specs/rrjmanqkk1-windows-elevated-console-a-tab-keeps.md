---
task_uid: "01m7s2b5jy0xh5barrjmanqkk1"
task: "T1974"
rev: 2
type: "chore"
complexity: "s"
criteria:
  - id: "C1"
    text: "The cause is confirmed (or replaced) and written up in the decision file before the fix"
    required: true
    manual: false
  - id: "C2"
    text: "GH CI windows Test passes fe-terminal session::console_title_tests::a_title_before_any_default_loses_the_prefix_once_learned: a #[cfg(windows)] test that sends Event::Title \"Administrator:  build\" through session A's Listener, then \"Administrator: C:\\Windows\\system32\\cmd.exe\" through session B's, and asserts A's last TerminalEvent::Title is \"build\""
    required: true
    manual: false
  - id: "C3"
    text: "GH CI windows Test passes fe-workbench terminal_panel::terminal_tabs_tests::tab_strip in two windows Test runs of the draft PR (a rerun of the job counts), with terminal_tabs_tests.rs unchanged"
    required: true
    manual: false
  - id: "C4"
    text: "macOS and Linux behaviour is unchanged: the existing console_title_tests and the fe-terminal and fe-workbench terminal tests pass on the gate with no expectation edits"
    required: true
    manual: false
---

# Windows elevated console: a tab keeps "Administrator: build" when its shell's title arrives before any session's default title taught the prefix (tab_strip intermittent)

# Elevated title prefix kept when a title beats every default title (Windows)

GH CI run 44451597170 (job 135316626648, sha 4b534410a, draft PR #4), windows Test: `fe-workbench terminal_panel::terminal_tabs_tests::tab_strip` panicked at terminal_tabs_tests.rs:81, "timed out waiting for the shell to set its title: ["Terminal 1", "Administrator:  build"]". It passed on windows in run 44439492559 (PR #3) and isn't in the 8ba6f008c windows baseline failures, so it's intermittent.

## Cause

The title did arrive. It still had the elevated console's prefix, so this isn't a late title. The GH runner is elevated, so conhost puts `Administrator: ` before every title. `program_title` (crates/fe-terminal/src/session.rs:225) drops that prefix only once `TITLE_PREFIX` (:194, process-wide since 863c2eda1) has been learned from some session's default title, `Administrator: C:\...\cmd.exe`. The test types `title build` into session 2 as soon as its pty exists (`started` waits only for the spawn). ConPTY coalesces title changes before its first frame. So session 2's first title event can be `Administrator:  build`, and it can arrive before session 1's console has reported its default. That title goes out unstripped (:260-264). No later title event comes for session 2, so its label stays wrong even after the prefix is learned. Users see the same thing: an elevated app plus a shell or profile that sets the title at once (cmd /k title, oh-my-posh) gives an "Administrator:  ..." tab.

## Fix (production, fe-terminal session.rs)

Stop the result depending on event order. Suggested: each Listener keeps its last raw console title. When `TITLE_PREFIX` is first learned, or changes, every live session reruns `program_title` on its raw title and sends a fresh `TerminalEvent::Title` if the label changes. That could use a process-wide registry of weak handles next to `TITLE_PREFIX`, or another design you prefer. Known limit, which is fine to keep: if no session in the process ever reports a default title, nothing can be learned. Optionally, a non-elevated process (TokenElevation) can treat the prefix as empty from the start. If that needs windows-sys in fe-terminal, it's in scope (Cargo.toml, Cargo.lock).

Don't loosen the test's `done` predicate: "Administrator:  build" really is the wrong label. Behaviour off Windows can't change, because `is_console_default_title` is false there.

Regression test: a `#[cfg(windows)]` unit test in session.rs `console_title_tests`. It drives two sessions' Listeners (their channels, no pty needed) with `Event::Title`: first "Administrator:  build" on A, then `Administrator: C:\Windows\system32\cmd.exe` on B. It asserts that A's last Title event is "build". It can't fail at the Mac base, so it isn't a --regression-test. Collect its evidence on the draft PR's windows CI job, as T1967 did.

Record the confirmed (or replaced) cause in the decision file before the fix.
