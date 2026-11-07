---
task_uid: "01m6sh1a9rh7k133w0gnt26d7y"
task: "T488"
rev: 1
type: "chore"
complexity: "xs"
criteria:
  - id: "C1"
    text: "start() in plugin_host_tests.rs sets PluginHost limits.call_deadline to 30 s; a_stuck_plugin_does_not_freeze_the_window, a_granted_program_that_outlasts_the_call_deadline_completes and a_command_runs_only_in_its_context_with_args_that_fit keep CALL_DEADLINE through one shared helper; until's timeout message includes plugin_states; no production code changes"
    required: true
    manual: false
  - id: "C2"
    text: "cargo nextest run -p fe-workbench --lib -E 'test(/plugin_host::plugin_host_tests::(a_kept_version_reloading_follows_every_root_change|a_spawn_in_flight_ends_before_the_plugin_unloads)$/)' --stress-count 30 --retries 0 passes under load (24 yes burners)"
    required: true
    manual: false
  - id: "C3"
    text: "cargo nextest run -p fe-workbench -E 'test(/plugin_host::plugin_host_tests::/)' passes and cargo clippy -p fe-workbench --all-targets -- -D warnings is clean"
    required: true
    manual: false
---

# Flake: fe-workbench plugin_host_tests a_kept_version_reloading_follows_every_root_change and a_spawn_in_flight_ends_before_the_plugin_unloads time out under load (plugin init overruns the 100 ms CALL_DEADLINE); give tests a 30 s call deadline

# Flake: plugin_host_tests time out waiting for a plugin to run under gate load

## Problem
`plugin_host::plugin_host_tests::a_kept_version_reloading_follows_every_root_change` failed a gate at load average ~57 (run 01m6sf54s1jadb9mt3dyx9kk0h): "timed out waiting for the plugin to run" at plugin_host_tests.rs:181 after 30.7 s, at its first `until(cx, "the plugin to run", running(TOOL))`. Every other plugin_host test passed in 0.3-4 s in the same run. `a_spawn_in_flight_ends_before_the_plugin_unloads` failed the same way under load (T474's finish, "timed out waiting for the spawning plugin to run", same line).

## Cause
The 30 s `until` deadline isn't what's slow. The plugin's load runs `instantiate` and `init` through `Guest::guarded` (plugin_host.rs, ~2275), under the host's `limits.call_deadline`, which is kiln-plugin's `CALL_DEADLINE`, 100 ms of wall-clock time. When a starved thread pushes instantiate+init past 100 ms, the load fails ("a call ran past its 100ms deadline"), the slot goes to `PluginState::Stopped` (StopReason::Load), and nothing loads it again. `until` then polls a state that will never become Running, for 30 s. Test-only: production enforcing the limit is by design. A separate, longer init deadline would be a feature card and isn't part of this one.

## Change (test-only, crates/fe-workbench/src/plugin_host_tests.rs)
- `start()` gives tests a generous call deadline, `cx.global_mut::<PluginHost>().limits.call_deadline = Duration::from_secs(30)`, the way plugin_bridge_tests.rs:217 already does. Tests whose subject is the call deadline keep the real `CALL_DEADLINE` through a second helper (for example `start_with_call_deadline`): `a_stuck_plugin_does_not_freeze_the_window`, `a_granted_program_that_outlasts_the_call_deadline_completes`, and `a_command_runs_only_in_its_context_with_args_that_fit` (it sleeps `CALL_DEADLINE * 2` to show a refused spin never times out).
- `a_spawn_in_flight_ends_before_the_plugin_unloads` takes the generous deadline. Its first leg ("a program run has its own, longer limit") duplicates `a_granted_program_that_outlasts_the_call_deadline_completes`, so drop that leg (or keep it with a comment that it no longer tests the limit). The leg that unloads mid-spawn is what the test is for.
- `until`'s timeout message includes `plugin_states(cx)`, so a future failure names the stop reason and doesn't just say "timed out".
- No production code changes.

## Known limits
The three tests that keep the 100 ms deadline still load their plugin under it, so they stay load-sensitive. Fixing that needs a production init deadline (a separate card).
