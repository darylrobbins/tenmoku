---
task_uid: "01m7azser04c0rmmchqbcn8a42"
task: "T621"
rev: 2
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "the executable-substitution cases of § 12: with `commands_allow = [\"git status\"]`, a `git` placed in the root, or on a relative `PATH` entry, is not the resolved program. `./git status` asks. A remembered answer asks again once the name resolves elsewhere"
    required: true
    manual: false
  - id: "C2"
    text: "timeout and cancel kill the group"
    required: true
    manual: false
  - id: "C3"
    text: "output caps"
    required: true
    manual: false
---

# run_command executor and agent audit log: bare names resolved on the login PATH to a real path, grant check, timeout, output cap, kill on cancel, JSONL audit with rotation and agent.show_audit_log (AI agents AG8, T1779) (Mac T1789)

# run_command executor and agent audit log: bare names resolved on the login PATH to a real path, grant check, timeout, output cap, kill on cancel, JSONL audit with rotation and agent.show_audit_log (AI agents AG8, T1779)

Source: `docs/decisions/50t8wt9we2-ai-agents.md` § 13, card **AG8** — T1779's decision record (AI agents: Claude Agent over ACP, a tool loop for the API chat, permissions and review), approved by the lead with round-4 findings F6–F11 carried into card criteria (§ 14). Read the record's sections the card cites before starting.

Linux-OK: the card can be built and tested on Linux (§ 13; no card in the plan is Mac-only).

## The card, verbatim from § 13

**AG8. `run_command` executor and audit log.** Size **m**. Deps: AG1, AG2.
Linux-OK (Unix process groups; Windows parts behind `cfg`).

- Files:
  - `crates/fe-workbench/src/agent_command.rs` (new: resolving a bare name
    on the login `PATH`, skipping relative and in-root entries, to a real
    path, and running exactly that path; grant check; timeout; output cap;
    kill on cancel);
  - `crates/fe-workbench/src/agent_audit.rs` (new: the JSONL worker,
    rotation, `agent.show_audit_log`);
  - `crates/fe-workbench/src/lib.rs`.
- New test files: `crates/fe-workbench/src/agent_command_tests.rs` and
  `crates/fe-workbench/src/agent_audit_tests.rs`.
- Tests: the executable-substitution cases of § 12; timeout and cancel kill
  the group; output caps.

## § 12 workbench loop tests: executable substitution, verbatim

- **Executable substitution** (AG8): with `commands_allow = ["git status"]`,
  a `git` placed in the root, or on a relative `PATH` entry, is not the
  resolved program. `./git status` asks. A remembered answer asks again once
  the name resolves elsewhere.

## § 12 Workbench tests, verbatim

### Workbench tests

Workbench tests use GPUI's test platform, and Linux temp folders (`.txt` and
`.go` files, not `.rs`, so no language server starts).

## Plan-wide rules, verbatim from § 13

Cards are size **s** or **m**. Scopes are exact files, and tests are in new
files. "Linux-OK" means the card can be built and tested on Linux. Marks:

- **[chat_panel.rs]** touches `crates/fe-workbench/src/chat_panel.rs`;
- **[settings.rs]** touches `crates/fe-config/src/settings.rs`;
- **[Cargo deps]** adds Cargo dependencies, and needs the `deny.toml`
  licence check.

Cards that touch T604's files depend on **T604** (the picker card).

New fe-workbench modules are declared from `crates/fe-workbench/src/lib.rs`,
not from `chat_panel.rs`. Each card's tests are declared from its own new
module.

Order: wave 1 in parallel; then AG7, AG8 and AG9; then AG10. AG11 waits for
its lanes cards, then AG12 and AG13 follow.

- **Mac-only cards:** none. Every card is Linux-OK.
- **Mac check:** the sandbox's macOS path (Seatbelt) is checked in the
  manual smoke run on a Mac.

T604 in the record is Mac T1780 (the model and effort picker), already on main.

## Scope beyond the card's file list

- `crates/fe-workbench/l10n/en-US/commands.ftl` and `crates/fe-workbench/src/command_catalog_tests.rs`: the new `agent.show_audit_log` command needs a title key and its catalogue pin (as T1751 did for its commands).
- `crates/fe-workbench/l10n-allow.toml`: audit JSONL field names are literals under the `all = true` lint.

## Notes

- Tests go in `crates/fe-workbench/src/agent_command_tests.rs` and `crates/fe-workbench/src/agent_audit_tests.rs`.
- If opening the audit log from `agent.show_audit_log` needs a workbench call site outside this scope, respec rather than widen silently.

## Implementation notes

- **What AG10 calls.** `agent_command::prepare` (cwd through `chat_apply::locate`, the program resolved on `login_path()`, which runs the login shell once, so off the main thread), then `decide(grants, policy, chat, prepared)`, then `RunningCommand::start(prepared, Limits::secs(command_timeout_secs))`. `RunningCommand::cancel` (or dropping it) kills the group; `finished().await` gives a `Ran`, whose `result_text()` and `is_error()` are the tool result. A cancelled command's result is replaced by the loop's `NotRun` text.
- **Resolution** skips relative and empty `PATH` entries, entries inside the root or a window folder (as written, through a link, or really), and a match whose real path is inside one. A program given with a path runs from `cwd` as written, with no resolved path, so it asks every time.
- **Grants** are checked as `process.spawn:<the program's file name>`. A program that isn't granted is denied with `decided_by: "hard"`. Otherwise the decision's provenance comes from `Policy::decide_with_reason` (added to `crates/fe-chat/src/permissions.rs` by a respec after review round 1, F5). It returns `policy` for the class level, `rule:<pattern>` for the pattern as written, `session` for a remembered answer, and `hard` for a forced ask. `decide` keeps its old behaviour and delegates to it.
- **argv[0]** is the name asked for (`CommandExt::arg0`), while the resolved real path is what's executed. A multicall program such as rustup's `cargo` therefore dispatches as asked (review F1).
- **Group lifetime.** On a timeout or a cancel, the group gets SIGTERM. SIGKILL follows once no member is left alive, or at the end of the grace. On Linux, "alive" means a non-zombie member found by scanning `/proc` for the group id. On other Unix systems it is `kill(-group, 0)`, so there the leader's zombie keeps the grace running to its end. A member cleaning up therefore keeps the full grace even when the leader dies at once (F3). When the leader exits on its own, whatever it left in its group is killed. The leader is reaped only after the group's last signal.
- **The pipe** is read without blocking on the runner's thread (`poll` with `O_NONBLOCK` on Unix; on Windows only what `PeekNamedPipe` says is waiting), so no platform has a reader thread. One pump reads at most `PUMP_BYTES` (256 KiB) or until its wait ends, whichever comes first, before the runner looks at the exit, cancel, timeout and grace again, so a program writing without pause can't hold them off (review round 2, F7). Once the group is gone, output is read for `DRAIN` (250 ms) more, then the pipe is closed. A process that left the group holding it gets EPIPE (Unix) or a broken pipe (Windows) and keeps nothing of Tenmoku's (F4).
- **Output** goes to a single pipe shared by stdout and stderr, so the streams interleave in the order they were written. The capture keeps 8 KiB of head and 24 KiB of tail, and never more. A cut that would split a character counts the cut bytes as omitted.
- **Audit.** `agent_audit::destination(root, cx)` is taken when a request is decided. It is `None` for an untrusted folder or when there's no state directory (`ChatStateDir` under test). A command's line is written through that destination once the command has ended (`Entry::command(..).ended(&ran, Killed::Stop | Killed::TrustWithdrawn)`), so a kill caused by trust withdrawal is still logged (F2). `record(root, ..)` is for requests decided now. A command that a signal of its own ended records `signal`, and one that never started records `not_started` (F6). `agent.show_audit_log` is declared on its own struct and handled by an app-level listener registered from `lib.rs` `init`, so no workbench call site outside the scope was needed. It shows `.1` and then the current log in a read-only tab, after every line queued before it has been written.
