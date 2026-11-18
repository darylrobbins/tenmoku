---
task_uid: "01m7q3aava4f2d9r6bpaa357dd"
task: "T691"
rev: 1
type: "bugfix"
complexity: "s"
criteria:
  - id: "C1"
    text: "tests::python_servers::one_launch::a_lead_not_found_launches_once_per_open passes and fails at base on every run: a launcher refusing ty as NotFound, set_trust holding the open's second ask until ty's Stopped is seen (+100 ms); on hearing that Stopped, status_at(ty, root) is it, ty launched once, and ty's statuses are exactly [Starting, Stopped(why)] from one ServerId"
    required: true
    manual: false
  - id: "C2"
    text: "LspManager::open's pass over documents waiting for the servers it placed starts no instance: one waiting after an earlier failure goes to the instance this open started if it still runs, else keeps waiting"
    required: true
    manual: false
  - id: "C3"
    text: "tests::python_servers::missing_lead_names_binary_and_install passes 20 consecutive runs alone and 5 under the whole fe-lsp suite, with the evidence in the decision file"
    required: true
    manual: false
  - id: "C4"
    text: "cargo nextest run -p fe-lsp passes and the gate's vet stage passes"
    required: true
    manual: false
---

# Fix the flaky test tests::python_servers::missing_lead_names_binary_and_install: open relaunches a lead whose launch already failed, so the status reads Starting after its Stopped

`tests::python_servers::missing_lead_names_binary_and_install` (T684) fails about 2 runs in 5 even alone, on whichever case's open loses the race (ty or pyright; pyrefly, the first, rarely):
`assertion left == right failed: ty / left: Some(Starting) / right: Some(Stopped(Some("ty not found on PATH, ...")))` at crates/fe-lsp/src/tests/python_servers.rs:594.

Root cause (production, crates/fe-lsp/src/manager.rs at base 8576d16d5): `LspManager::open` places each server, then runs "documents waiting for the same servers go to them too" (manager.rs:1575-1582, `self.place_waiting(&specs, Some(&only))`). place_waiting -> place_for -> target -> start_server (manager.rs:1979-1986, 921-935) calls `self.prune()` first. A lead whose launch fails at once (not found) has already ended its thread by then, so prune drops it, turns the just-opened document's place back to Waiting (manager.rs:910-917), and start_server launches a second instance at the same (name, root) inside the same open, setting `latest` to its `Starting` (manager.rs:954-958). The first instance's `Stopped` event still reaches the host, which then reads `status_at` = Starting (server.rs:407-411 keeps the newest instance's status). Logged at base: every open in that test after the first starts the lead twice (ids 0 and 2) and ruff twice. In the app, a missing server is launched (and resolved, login shell included) twice per open, and the status item briefly shows "starting" again after the stop.

Fix: the pass at the end of `open` must start no instance; it only moves documents already waiting for (name, root) onto the instance running there, if one still runs (as `running_place` does, the way place_waiting's own second pass already avoids a loop). A prototype that replaces the place_waiting call there with `waiting_for(Some(&only))` + `running_place` + `set_place` passes the whole fe-lsp suite (381) and 25/25 runs of both tests: /home/daryl/code/fastedit-lanes/.worktrees/python-lead-one-launch.patch (also has the regression test). Use it as a starting point; keep documents waiting after an earlier failure placed on the instance this open starts.

Regression test (fails at base every run): `tests::python_servers::one_launch::a_lead_not_found_launches_once_per_open`, in crates/fe-lsp/src/tests/python_servers/one_launch.rs, declared with `mod one_launch;` from python_servers.rs. A launcher refusing ty as NotFound and counting its launches; `set_trust` blocks the open's second trust ask (on the manager's thread, after ty was started) until the probe has seen ty's Stopped plus 100 ms, so ty's thread has ended before the pass, deterministically. Asserts status_at("ty") is that Stopped on hearing it, one launch, and ty's statuses are exactly [Starting, Stopped(why)] from one ServerId. Base APIs only. tests/mod.rs stays out of scope.

## Implementation notes

`LspManager::open`'s closing pass now only moves documents already waiting for (name, root) onto the instance running there (`waiting_for` + `running_place` + `set_place`); it starts no instance, so a lead that failed at once is no longer relaunched within the same open.

Evidence:
- `missing_lead_names_binary_and_install` alone: 20/20 passes.
- Whole `cargo nextest run -p fe-lsp`: 5/5 passes.
- `one_launch::a_lead_not_found_launches_once_per_open` with manager.rs at base: failed 3/3; with the fix: passes.
