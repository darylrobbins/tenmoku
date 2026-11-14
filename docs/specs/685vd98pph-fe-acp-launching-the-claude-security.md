---
task_uid: "01m7azsdj7w4p4ty685vd98pph"
task: "T619"
rev: 5
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "the profile JSON, byte-identical on new, load, resume and respawn"
    required: true
    manual: false
  - id: "C2"
    text: "a changed setting causes a re-load before the next prompt"
    required: true
    manual: false
  - id: "C3"
    text: "class levels mapped to `tools` and ask rules"
    required: true
    manual: false
  - id: "C4"
    text: "each inherited-settings case of § 12"
    required: true
    manual: false
  - id: "C5"
    text: "each state. Known limit (follow-up Mac T1870): a send-side failure on the outgoing channel (request or session/cancel) racing teardown can still be reported as Failed rather than Disconnected; Mac T1870 normalizes it"
    required: true
    manual: false
  - id: "C6"
    text: "a crash mid-turn"
    required: true
    manual: false
  - id: "C7"
    text: "shutdown kills the group"
    required: true
    manual: false
  - id: "C8"
    text: "Carried F6 (high): with `allowManagedPermissionRulesOnly: true` in a managed file or drop-in, Claude Agent refuses to start and names that setting. So the forced asks can never be silently ignored. Test: `claude_profile.rs` places it in each managed source, and asserts the refusal and its reason"
    required: true
    manual: false
  - id: "C9"
    text: "Carried F8: for a root that is itself a linked worktree, the main checkout is resolved from `--git-common-dir`, and its `.claude/settings.local.json` is checked and watched. Test: a widening rule there refuses; the same rule in the linked root's own local file doesn't count as the main checkout's"
    required: true
    manual: false
  - id: "C10"
    text: "Carried F10: every spawn and respawn sets `CLAUDE_CODE_DISABLE_BACKGROUND_TASKS=1`. Test: `launch.rs` reads it back from the fake agent's environment"
    required: true
    manual: false
  - id: "C11"
    text: "Carried F11: smoke step 14 in this record is rewritten to plant `\"touch *\"` after a baseline failure, in the card's own commit to this doc"
    required: true
    manual: false
---

# fe-acp launching, the Claude security profile, the inherited Claude Code settings check, states and the fake agent binary (AI agents AG6, T1779) (Mac T1787)

# fe-acp launching, the Claude security profile, the inherited Claude Code settings check, states and the fake agent binary (AI agents AG6, T1779)

Source: `docs/decisions/50t8wt9we2-ai-agents.md` § 13, card **AG6** — T1779's decision record (AI agents: Claude Agent over ACP, a tool loop for the API chat, permissions and review), approved by the lead with round-4 findings F6–F11 carried into card criteria (§ 14). Read the record's sections the card cites before starting.

Linux-OK: the card can be built and tested on Linux (§ 13; no card in the plan is Mac-only).

## The card, verbatim from § 13

**AG6. fe-acp: launching, states and the fake agent binary.** Size **m**.
Deps: AG5. Linux-OK.

- Files:
  - `crates/fe-acp/src/launch.rs` (new: spawn from given paths with
    `CLAUDE_CODE_EXECUTABLE`, process group, stderr ring, shutdown, the
    `hide_claude_auth` parameter);
  - `crates/fe-acp/src/claude.rs` (new: `claude::profile`, the § 3 security
    profile from the agent settings, with `CLAUDE_TOOLS` and the class
    mapping; sent on new, load, resume and respawn; re-load when it changes;
    the git common and admin dirs from `git rev-parse` into
    `filesystem.denyWrite`; the version floor; `authRequired` and the other
    states of § 4);
  - `crates/fe-acp/src/claude_settings.rs` (new: the inherited-settings
    check):
    - the source table, with the `managed-settings.d` drop-ins;
    - managed preferences failing closed;
    - keep only `sandbox`, `permissions.allow` and
      `permissions.additionalDirectories`;
    - accept only tightening values, network-granting allow rules included;
    - the watcher callback that cancels a turn on a widening change;
  - `crates/fe-acp/src/bin/fe-acp-fake-agent.rs` (new);
  - `crates/fe-acp/src/lib.rs`;
  - `crates/fe-acp/Cargo.toml`.
- New test files: `crates/fe-acp/tests/launch.rs` and
  `crates/fe-acp/tests/claude_profile.rs`.
- Tests:
  - the profile JSON, byte-identical on new, load, resume and respawn;
  - a changed setting causes a re-load before the next prompt;
  - class levels mapped to `tools` and ask rules;
  - each inherited-settings case of § 12;
  - each state;
  - a crash mid-turn;
  - shutdown kills the group.
- **Carried acceptance criteria** (§ 14, "Open questions carried to
  implementation"):
  - **F6 (high):** with `allowManagedPermissionRulesOnly: true` in a
    managed file or drop-in, Claude Agent refuses to start and names that
    setting. So the forced asks can never be silently ignored. Test:
    `claude_profile.rs` places it in each managed source, and asserts the
    refusal and its reason.
  - **F8:** for a root that is itself a linked worktree, the main checkout
    is resolved from `--git-common-dir`, and its
    `.claude/settings.local.json` is checked and watched. Test: a widening
    rule there refuses; the same rule in the linked root's own local file
    doesn't count as the main checkout's.
  - **F10:** every spawn and respawn sets
    `CLAUDE_CODE_DISABLE_BACKGROUND_TASKS=1`. Test: `launch.rs` reads it
    back from the fake agent's environment.
  - **F11:** smoke step 14 in this record is rewritten to plant `"touch *"`
    after a baseline failure, in the card's own commit to this doc.

## Carried findings, verbatim from § 14

The fourth review of this record (round 4) found F6–F11. The approver
(Mac lead) took the record as proposed, with these carried to the cards
named below. Each is an acceptance criterion of that card in § 13. The
text of §§ 3, 6 and 12 above is not yet amended for them: where they
differ, the card's criterion wins.

- **F6 (high): managed `allowManagedPermissionRulesOnly`.**
  - **Gap.** Set in managed settings, it makes Claude Code ignore
    permission rules from flag settings, which are this design's forced asks
    and deny-list rules (SDK `sdk.d.ts:7797`). § 3's check doesn't read it.
  - **Fix.** The inherited-settings check also reads
    `allowManagedPermissionRulesOnly` from every managed source (file,
    drop-ins), and Claude Agent refuses to start, with that reason, when it
    is true.
  - **Card:** AG6 (`claude_settings.rs`).

- **F8 (medium): the main checkout when the root is a linked worktree.**
  - **Gap.** When the opened root is itself a linked worktree,
    `<root>/.claude/settings.local.json` is not the main checkout's local
    file, which Claude Code loads.
  - **Fix.** Resolve the main checkout as the parent of `git rev-parse
    --path-format=absolute --git-common-dir`. Check and watch its
    `.claude/settings.local.json`. Keep the opened root's
    `.claude/settings.json` as the Update-lane source.
  - **Card:** AG6 (`claude_settings.rs`), with a linked-root settings test.

- **F10 (medium): background commands outlive cancel and freezing.**
  - **Gap.** Claude Code's Bash can run commands in the background,
    including after its own timeouts. They survive `session/cancel`
    (adapter `README.md:88-98`), and can keep writing the lane after its
    revision is frozen.
  - **Fix.** The adapter is always spawned with
    `CLAUDE_CODE_DISABLE_BACKGROUND_TASKS=1` (SDK `sdk.d.ts:3363`), on every
    spawn and respawn, so nothing is backgrounded. A detached process a
    command starts on its own is a stated limit. It is bounded by the
    sandbox to the worktree, and anything it writes after a freeze shows in
    the next revision.
  - **Card:** AG6 (`launch.rs`), with a test in AG11 that a background
    request runs in the foreground and ends before the turn's freeze.
  - **Where this is placed.** The approver's note put F10 on the API
    tool-loop card. But the gap is in Claude Code's Bash under the Claude
    Agent launcher: the API loop's `run_command` has no backgrounding. So it
    is placed on AG6 and AG11, and the approver can move it.

- **F11 (medium): smoke step 14 can't show the escape.**
  - **Gap.** `excludedCommands: ["touch"]` matches only `touch` without
    arguments, so `touch ../x` stays sandboxed.
  - **Fix.** Plant `"touch *"`. First show the same model-issued `touch ../x`
    fails under the baseline sandbox, then show it escapes after the source
    gains the setting.
  - **Card:** AG6, whose criteria include rewriting smoke step 14 this way
    before the first manual smoke run.

## § 12 The fake ACP agent, verbatim

### The fake ACP agent

`crates/fe-acp/src/bin/fe-acp-fake-agent.rs` is built with the crate's
`Agent.v1()` (`src/role/acp.rs:312`; `examples/simple_agent.rs` is the
pattern). It plays a JSON script given as its argument:

- updates to send;
- permission requests, and the option it expects back;
- `authRequired` on new or prompt;
- a stop reason;
- exiting mid-turn.

It records what it received.

- In-process tests run the same agent over `Channel::duplex()`
  (`src/jsonrpc.rs:7380`).
- Process tests (spawn, crash, shutdown, stderr) run the binary through
  `CARGO_BIN_EXE_fe-acp-fake-agent`.
- fe-acp's tests assert:
  - the `initialize` capabilities;
  - the security profile of § 3, as JSON, **byte-identical** on
    `session/new`, `session/load`, `session/resume` and the load after a
    crash;
  - a settings change that changes the profile sends `session/load` with the
    new one before the next prompt;
  - each class level mapping: a `deny` class's tools absent from `tools`, an
    `ask` class's tools in the ask rules;
  - the inherited-settings check, over a fake home and a fake main checkout
    and worktree. Each accepted key passes, and each widening case refuses
    with its file and key named. The widening cases are:
    - in `sandbox`: `excludedCommands`, `filesystem.allowWrite`,
      `filesystem.disabled`, `enabled: false`,
      `allowUnsandboxedCommands: true`, `network.allowUnixSockets` (a Docker
      socket), `network.allowAllUnixSockets`, `network.allowLocalBinding`, a
      loopback `allowedDomains` entry, and an unknown key;
    - `permissions.additionalDirectories`;
    - `permissions.allow` with bare `Edit`, `Edit(//etc/**)`, `Edit(~/x)`,
      `Write(../x)`, and a relative `Edit(src/**)` in the **user's** file.
      The same relative rule in a project file is accepted;
    - `permissions.allow` network grants: `WebFetch(domain:localhost)`,
      `WebFetch(domain:127.0.0.1)`, `WebFetch(domain:*)` and a bare
      `WebFetch` are refused, and `WebFetch(domain:docs.rs)` is accepted;
    - each case placed in each source of § 3's table: the main checkout's
      `settings.local.json`, and a file in a fake `managed-settings.d`
      directory, included;
    - a managed-preferences domain present in the fake macOS location
      refuses, naming the source as uninspectable;
    - a file that doesn't parse.

    Keys outside the three it reads, such as `env`, are never kept;
  - a settings file changed to a widening value mid-turn cancels the turn,
    answers pending requests `Cancelled`, and refuses the next prompt. So
    does a widening file added to `managed-settings.d`;
  - the git directories in `filesystem.denyWrite`, over temporary
    repositories:
    - for a main-checkout root, the common dir and the lane's admin dir;
    - for a root that is itself a linked worktree, those plus the root's
      own admin dir;
    - a `rev-parse` failure refuses the session;
  - with `mcp_servers = true`: `other = "ask"` puts `mcp__*` in the ask
    rules and `strictMcpConfig` is off; `other = "deny"` sets
    `strictMcpConfig: true` despite the opt-in;
  - `Cancelled` answers after cancel;
  - the state for each failure in § 4's table.

## § 12 manual smoke run, step 14 (F11 rewrites it), verbatim

14. **The source table matches this Claude Code version.** It is done in
    two parts, matching the table's last column.
    - **Sources the session loads now** (every row marked "yes", except
      managed preferences). For each, in turn:
      1. plant `"sandbox": {"excludedCommands": ["touch"]}` in it;
      2. run `claude` by hand in a lane worktree and confirm `touch ../x`
         now escapes the sandbox, which shows Claude Code reads that file;
      3. confirm Claude Agent refuses to start, naming that file.

      A source Claude Code reads that the table lacks is a bug in
      `claude_settings.rs`, fixed before the tested version is raised.
    - **Files checked only ahead of Update lane** (the main checkout's
      `.claude/settings.json`). Plant the same value there, and confirm
      Claude Agent refuses to start, naming that file. The current session
      isn't expected to honour it, since Claude Code reads the worktree's
      copy. Then run Update lane by hand and confirm the worktree's copy now
      carries it.

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

- `docs/decisions/50t8wt9we2-ai-agents.md`: F11 makes the card rewrite smoke step 14 in its own commit to this record.
- `Cargo.lock`: `crates/fe-acp/Cargo.toml` may gain workspace dependencies (a file watcher, process-group calls) for launch and the settings watcher.

## Notes

- Tests go in `crates/fe-acp/tests/launch.rs` and `crates/fe-acp/tests/claude_profile.rs`; process tests run the binary through `CARGO_BIN_EXE_fe-acp-fake-agent` (§ 12).
## Round-3 decision (approver, Mac lead): fix F7 and F8, do not waive them

F8 lets a queued public load/resume skip the settings re-check after it waits, so a widening settings file added meanwhile gets through after settings_changed refused the session. Design 50t8wt9we2-ai-agents.md section 3 "Settings changed during a turn" says a failing change refuses further prompts until the check passes again: a sandbox hole, not a known limit.

- F8: re-check SettingsCheck after each wait in queued load/resume (claude.rs:815), with tests for a widening file added while queued.
- F7: the reconfiguration callback holds `Weak<Inner>` (claude.rs:553), with a drop-without-shutdown test that the process group is reaped.

Then rgt finish once. Keep the scope; if the review still demands more, say so in the finish note.
## Round-5 decision (approver, Mac lead)

Only F11 (medium) remains open: send-side failures on the outgoing channel can still surface as Failed instead of Disconnected (client.rs ~l.469/519/550). Per the Mac lead's rule it moves to follow-up Mac T1870 and C5 is a Known limit naming it. Make no further code change for F11. Dispute F11 as "carried to follow-up Mac T1870 (known limit on C5)" and run rgt finish once.

## Implementation notes

- **Scope widened** (respec) to `crates/fe-acp/src/client.rs`, for
  `AcpClient::resume_session` and `close_session` (C1's resume and § 4's
  shutdown need them, and `AcpClient`'s request method is private), the
  `_auth/status_update` kind (§ 4's "Not logged in"), and
  `is_disconnect`. That inherited T618's medium F2, which is fixed:
  `prompt` now ends its turn in the connection's ordered response callback
  (`on_receiving_result`), not when the caller's future drops.
  `crates/fe-acp/tests/client_turns.rs` (also added by the respec) holds
  its two regressions, which time out against the old `prompt`.
- **The auth status** is kept on the client (`AcpClient::auth_kind`,
  `ClaudeAgent::auth_state`) rather than as a new `Incoming` variant, which
  would break `tests/client.rs`'s exhaustive matches. Only `kind` is
  deserialized; the email and organisation never are.
- **The watcher callback** is `ClaudeAgent::settings_changed`, defined in
  `claude_settings.rs`; `SettingsCheck::watched` (and
  `ClaudeAgent::watched`) lists what AG11's watcher must watch.
- **Re-load before a prompt** uses `session/load` as § 3 says, so the
  adapter replays the history onto the `Incoming` stream; AG11 should drop
  those replayed updates for an open chat.
- **`allowManagedPermissionRulesOnly`** is read at the top level and under
  `permissions`, from each managed file and drop-in; any value but `false`
  or `null` refuses.
- **`sandbox_allow_write`** entries that are relative, or inside the root or
  the git directories, are dropped from the profile with a warning (§ 3,
  "Where the rule can't hold").
- **The fake agent** is `src/bin/fe-acp-fake-agent.rs`; in-process tests
  include it with `#[path]` and run `serve` over `Channel::duplex()`.
- **Smoke step 14** (F11) is rewritten in its own commit to the record. It
  also names the opened root's `.claude/settings.json` as the Update-lane
  file, per F8.
- **Review round 1** (F1–F6), fixed:
  - F1: `sandbox_allow_write` paths are resolved (`..` and symlinks, as
    far as they exist; dangling links refused) before the overlap check
    against the root, the worktree and the git directories, and passed
    resolved.
  - F2: a wildcard domain passes only as `*.<suffix>` with no port, whose
    suffix is a host name whose last label isn't numeric in any
    `inet_aton` form and isn't `localhost`; anything else with `*` refuses,
    as does a host that isn't a plain name, an address or `host:port`.
  - F3 and F4: `ClaudeAgent::prompt` re-checks the settings, compares the
    profile and sends the prompt together under the sessions' lock, through
    the new synchronous `AcpClient::start_prompt`; `set_settings` and the
    watcher take the same lock, and the watcher cancels by the client's
    protocol-owned `turn_running`, not by the caller's future. After a
    re-load, everything is checked again.
  - F5: source discovery fails closed on any error but absence, naming the
    path.
  - F6: stderr is drained in bounded chunks and decoded lossily.
- **Review round 2** (F1, F4), fixed:
  - F1: path resolution resumes after every component, so a `..` out of a
    missing component back to an existing link is followed again.
  - F4: each session tracks the profile the adapter is known to run
    (`applied`, set only by an answered new, load or resume) and whether a
    load or resume is in flight. Loads and resumes, the public ones and a
    prompt's re-load alike, go through the new `AcpClient::start_load_session`
    and `start_resume_session`, whose response callbacks settle that state
    on the dispatch loop, whatever became of the caller; one that's never
    answered leaves no profile applied. Nothing is admitted while one is in
    flight, and only one is in flight per session.
- **Review round 3** (F7, F8), fixed per the approver's round-3 decision:
  - F7: `Reconfiguring` holds `Weak<Inner>` and settles only when it
    upgrades, so an unanswered load no longer keeps the client, connection
    and process alive. `launch.rs` drops every handle during a held-open
    re-load, without shutdown, and checks the group is killed and reaped.
  - F8: a queued public load or resume checks the inherited settings again
    under the sessions' lock after each wait, and refuses (recording the
    refusal) as a prompt would. Tests cover a queued load and a queued
    resume with a widening file added while they wait; `ClaudeAgent::queued`
    (hidden, for tests) says when one is waiting.
- **Rev 4 review, round 1** (F9–F11), fixed in the lane, awaiting the
  approver's word before another finish (the rev-4 decision said finish
  once):
  - F9: `sandbox_allow_write` entries with glob syntax (`* ? [ ] { } \`)
    are dropped before resolution, since the sandbox reads them as
    patterns on macOS and a literal overlap check can't bound them.
  - F10: requests go through `prepare_request(…).on_receiving_result(…)`,
    so ordered consumption is registered before publication; a test runs
    the queued-turn case 50 times with the connection, the fake and the
    test on separate threads.
  - F11: `is_disconnect` also recognises the SDK's
    `is_connection_terminated`; a cached connection whose driver has ended
    is refused as a disconnect, and so is a callback registration that
    fails on it. Tests cover every request after the driver has fully
    ended, and a transport failure (the fake closes its stdout) while the
    adapter keeps running.
- **Rev 4 review, round 2** (F9, F11), fixed:
  - F9: the resolved path is checked for glob syntax too, before it is
    passed, so a glob-free link to a folder named like a pattern
    (`rep*/**`) is dropped. And a lane whose root, worktree or git
    directories have glob characters is refused (Sandbox loosened), since
    the profile's `denyWrite` and deny rules name them.
  - F11: a request's failure, a callback registration's, and a cancel
    notification's are disconnects whenever the connection is ending,
    which the client tells by whether its task queue still takes a no-op
    task (the SDK closes it first as it tears down, before the termination
    is published). A test holds the output draining open during teardown.
- **Rev 5** (the approver's round-5 decision): F9 and F10 are confirmed
  fixed. F11's remaining case (a send-side failure on the outgoing
  channel racing teardown, reported as Failed) is disputed as carried to
  follow-up Mac T1870, the Known limit C5 now names; no further code
  change for it here.
