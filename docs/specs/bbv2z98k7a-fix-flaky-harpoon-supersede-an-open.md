---
task_uid: "01m6t74h4swa7987bbv2z98k7a"
task: "T497"
rev: 1
type: "chore"
complexity: "xs"
criteria:
  - id: "C1"
    text: "harpoon_supersede::an_open_while_a_jump_is_pending_beats_the_jump passes"
    required: true
    manual: false
  - id: "C2"
    text: "harpoon_supersede::a_dismissed_list_picker_does_not_open_its_row passes"
    required: true
    manual: false
  - id: "C3"
    text: "cargo nextest run -p fe-workbench --test harpoon_supersede passes 20 runs in a row under load (stress -c $(nproc) or a concurrent gate), and no output mentions config-watcher"
    required: true
    manual: false
  - id: "C4"
    text: "Only crates/fe-workbench/tests/harpoon_supersede.rs changes; serially() also removes HOME, as palette_ghost_text.rs does, so no user-layer ConfigWatcher starts"
    required: true
    manual: false
---

# Fix flaky harpoon_supersede::an_open_while_a_jump_is_pending_beats_the_jump: config-watcher thread wakes the test scheduler (unset HOME in serially())

## Problem

`fe-workbench::harpoon_supersede an_open_while_a_jump_is_pending_beats_the_jump`
fails now and then under gate load (seen in T495's finish, gate run
01m6t5vmwzg8xf1m4s7zqtpk2r, test.fastedit.log ~4375-4470). The panic:

    Detected activity on thread Some("config-watcher") ThreadId(5), but test
    scheduler is running on Some("an_open_while_a_jump_is_pending_beats_the_jump")
    ThreadId(2). Your test is not deterministic.

The test came over from the Mac in T1422 (mac/main 50c76b40, helium ff26694b).

## Cause

The backtrace goes through `fe_workbench::lsp::user_layer`
(crates/fe-workbench/src/lsp.rs:189-233). Its `if cfg!(test) { return; }`
guard (lsp.rs:203) does nothing in an integration test, because the library
is built without `cfg(test)` there. `settings_path` (src/open.rs:326) then
falls back to `fe_config::user_settings_path()`, which is the developer's
real `$HOME/.config/tenmoku/settings.toml`. So a real `ConfigWatcher` starts.
Its thread always sends the watched files once at startup
(kiln/crates/kiln-core/src/watch.rs:128-129, `sink(first)`), and every 3 s
after that. That send, `wake.send_blocking(())` at lsp.rs:207, wakes the
main-thread task in `user_layer` from the "config-watcher" thread. If the
task is already parked on `changes.recv()` at that moment, the gpui test
scheduler panics. Load makes that more likely: the thread starts late, or
the test runs past 3 s (it took 3.25 s when it failed).

`serially()` in crates/fe-workbench/tests/harpoon_supersede.rs:20-29 removes
only `TENMOKU_CONFIG_DIR` and `XDG_CONFIG_HOME` and leaves `HOME` set. The
other fe-workbench integration tests also remove `HOME`
(palette_ghost_text.rs:27, hot_exit_rootless.rs:37, grammar_prompt.rs:28,
find_replace_interrupted.rs:50, git_log_restore_first_list.rs:27). With no
config dir at all, `settings_path` is `None`, `lsp.user` is never made, and
no watcher thread starts. palette_ghost_text.rs:23-27 explains why.

This is not a production bug. Only gpui's test scheduler rejects wakes from
another thread. The real app is meant to hear from this watcher.

## Fix (test only)

In `serially()` in crates/fe-workbench/tests/harpoon_supersede.rs, also
remove `HOME`, as the other tests do. Keep setting `TENMOKU_STATE_DIR`: the
marks file and harpoon state read it (fe-config paths.rs:89), so they don't
need `HOME`. Update the comment to say why: with no config dir, no user-layer
watcher thread starts. The second test in the file,
`a_dismissed_list_picker_does_not_open_its_row`, goes through the same
`serially()`, so the change fixes it too. Leave the existing "Kept, not
removed" scratch-dir comments alone; they are about the root's file watcher.

Don't change lsp.rs, open.rs or kiln. Don't add a regression test.
Don't use `allow_parking()` as the fix: it doesn't stop wakes from
another thread.

## Verify

    for i in $(seq 20); do cargo nextest run -p fe-workbench --test harpoon_supersede || break; done

Run it with the machine under load (for example `stress -c $(nproc)`, or
alongside another lane's gate). Then check that no output says
"config-watcher".
