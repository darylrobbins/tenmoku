---
task_uid: "01m5fv84rgw5t0n819m3kzx25x"
task: "T264"
rev: 1
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "grants::an_unknown_plugin_needs_a_prompt: no record gives Prompt(NewPlugin) and loads nothing"
    required: true
    manual: false
  - id: "C2"
    text: "grants::the_same_request_loads_from_the_record: an equal granted set gives Load"
    required: true
    manual: false
  - id: "C3"
    text: "grants::new_or_wider_capabilities_are_an_escalation: a new capability, a new net host and net:* replacing a named host each give Escalation naming what was added"
    required: true
    manual: false
  - id: "C4"
    text: "grants::requesting_less_applies_without_a_prompt: a narrower request gives Load and shrinks the record"
    required: true
    manual: false
  - id: "C5"
    text: "grants::the_old_version_keeps_running_while_an_escalation_is_pending: Escalation carries keep_running = the installed version"
    required: true
    manual: false
  - id: "C6"
    text: "grants::a_declined_request_is_not_asked_again_until_it_changes: after a decline the same request gives Declined and a different one prompts"
    required: true
    manual: false
  - id: "C7"
    text: "grants::wildcards_need_their_own_acceptance: granting net:* or process.spawn:* without accept_wildcards is refused"
    required: true
    manual: false
  - id: "C8"
    text: "grants::a_spawn_grant_records_the_absolute_path_and_identity: resolve_program records the absolute path, size and SHA-256 of the program found on the given PATH"
    required: true
    manual: false
  - id: "C9"
    text: "grants::a_changed_or_missing_program_suspends_the_grant: after the file content changes, or it is removed, check_program returns Suspended and evaluate asks again"
    required: true
    manual: false
  - id: "C10"
    text: "grants::the_store_round_trips_under_a_lock_and_forgets: set, reopen, read back, forget removes the record; two writers do not corrupt the file"
    required: true
    manual: false
  - id: "C11"
    text: "cargo nextest run -p kiln-plugin and cargo clippy -p kiln-plugin --all-targets -- -D warnings pass"
    required: true
    manual: false
---

# Keep plugin grants in a locked state file with program identities, and decide load, prompt or escalation as a pure state machine that suspends a grant when its program changes

## Problem

Capabilities are granted by the user at install (§7), and an update that asks for more must stop and ask before it loads
(decision 3), while the old version keeps running. A spawn grant is bound to the absolute path and file identity of the
program, and is suspended when the file changes (decision 1). None of that state exists yet; `Grants` (T963) is only the
value type.

## Change

New `kiln/crates/kiln-plugin/src/grants.rs`:

- `GrantStore` over `<state dir>/plugin-grants.toml` (path passed in), written under a lock and atomically like
  `ExtensionSet::license_notices` in `kiln-core` (reuse its lock and write helpers). One record per plugin id: the
  installed version, the granted `Grants`, each `process.spawn:<program>` grant's recorded absolute path and file
  identity (device, inode, size, modified time, SHA-256 of the content), whether wildcards were separately accepted, and
  the last declined request (so the same request is not asked again).
- A pure state machine `evaluate(record: Option<&GrantRecord>, requested: &Grants) -> Decision`:
  `Prompt(NewPlugin)`, `Load(granted)`, `Escalation { added, keep_running: Option<version> }` (any capability or wider
  scope the installed version lacks; narrower or equal applies silently, shrinking the record without a prompt),
  `Declined`. `needs_wildcard_acceptance(added)` is true for `net:*` or `process.spawn:*`, and granting those
  requires `accept_wildcards = true` as its own argument.
- Program identity: `resolve_program(name, path_var) -> ProgramIdentity` looks the name up on the given `PATH` and
  records the absolute path and identity; `check_program(&ProgramIdentity) -> Result<(), Suspended>` returns a
  `Suspended { reason: Changed | Missing }` that turns the plugin's grants into `Escalation` again.
- `forget(id)` removes a plugin's record (uninstall uses it).

## Notes

Prompt text and combination warnings are P7; enforcing spawn at run time is P13. Use `std::os::unix::fs::MetadataExt`
(macOS and Linux only). Tests use a tempdir state file and copies of a small binary or script as the "program".

Project rules: keep tests lean and fast (hostile and process fixtures bounded to under 2 s each); new tests go in new files; the scope lists exact files.

## Implementation notes

- `kiln_core::trust`'s `locked` and `write` are `pub(crate)`, and `kiln-core` is outside this card's scope, so
  `grants.rs` keeps local copies of both (same `<file>.lock`, temp-file-and-rename, 0600/0700 modes).
- `evaluate` is pure; `GrantStore::decide` wraps it and records the shrink (and the new version) on `Load`.
  `GrantStore::grant`, `decline` and `check_programs` are the other writers.
- Coverage is `Grants::allows`: `net:*` covers a named host and `process.spawn:*` a named program. Narrowing
  `process.spawn:*` to `process.spawn:<program>` is a silent `Load`; `GrantStore::decide` takes a `PATH` value and binds
  the program to what it resolves to there. If any named program can't be resolved, `decide` errs with
  `GrantStoreError::Unresolved` and leaves the record untouched, version included: nothing loads, so no capability is
  kept past a narrowing that did load. (Review round 1, F1; round 2, F3.)
- Program files are opened once, with `O_NONBLOCK` (a FIFO swapped in can't block while the grants lock is held), and
  every check (regular file, executable, identity, hash) is made on the opened descriptor. `std` doesn't name
  `O_NONBLOCK` and `libc` isn't a dependency here, so its per-platform values are local constants. (Review round 1, F2.)
- A suspended spawn grant is removed from `granted` (and its identity dropped), with the reason kept in the record's
  `suspended` map for the P7 prompt; the next `evaluate` names it in `Escalation.added`.
- A record can exist with `version = None`: a new plugin whose first request was declined.
- `resolve_program` skips relative `PATH` entries and records the path as found (symlinks kept, so argv[0]-sensitive
  programs such as rustup proxies still work); the identity follows the symlink to the file.
