---
task_uid: "01m77zvrxz24w2gdbt4vw6a22q"
task: "T1735"
rev: 2
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "`tests::add_ons::reconcile::add_on_joining_on_reload_opens_at_current_text` and `tests::add_ons::reconcile::removed_add_on_gets_did_close_alone` pass"
    required: true
    manual: false
  - id: "C2"
    text: "`tests::add_ons::reconcile::untrusted_add_on_root_starts_only_the_primary` and `tests::add_ons::reconcile::enabled_false_drops_each_server` pass"
    required: true
    manual: false
  - id: "C3"
    text: "`tests::add_ons::reconcile::restart_of_one_server_leaves_the_other_running` passes"
    required: true
    manual: false
---

# Reconcile add-on servers one by one on settings, trust and spec changes, and restart a single server by name and root (LSP add-ons A5, T1645)

# Reconcile add-on servers one by one on settings, trust and spec changes, and restart a single server by name and root (LSP add-ons A5, T1645)

Spec: §4 (joins, removals, reroot), §6.2 (restart by (name, root)), §7 (Sync), §8 row A5.

Row: reconcile per server: settings, trust, extension and host specs adding or removing add-ons, reroot per server, and `restart(name, root)` of one.

Test file: crates/fe-lsp/src/tests/add_ons/reconcile.rs (join at latest text, removal `didClose`, untrusted add-on root, `enabled = false` on each).

Read the design sections named above before starting. Do not widen --scope; if a file outside it is needed, stop and say so.

## Implementation notes

- **Scope.** `crates/fe-lsp/src/tests/add_ons.rs` was added to the scope (respec) for the `mod reconcile;` line; `tree` and `vars_spec` in `tests/add_ons/lead_unavailable.rs` became `pub(super)` for reuse. A thread-name allow entry was appended to `crates/fe-lsp/l10n-allow.toml` (shared append-only).
- **Most of A5 was already in A4's reconcile** (`reroot`, `release`, `regroup`, per-name places): joins at the hub's latest text, trust per root, `enabled = false` per server and `restart(name, root)` of one already worked; the tests pin them, for settings, host and extension specs.
- **A removed server's `didClose` raced its stop.** `release` queues `Command::Close`, and `retire` then fired the stop, which the actor selects ahead of commands, so a removed or turned-off server usually shut down without `didClose`. `Handle::closed` marks a server a document was closed on alone; `Handle::retire` then sends the stop behind the queued commands (a `Command::Documents` closure, which the actor takes only once the outbox, holding that `didClose`, is handed on), or after `CLOSE_GRACE` (2 s real time, on a thread), whichever is first, so a server that stopped reading can't hold up its shutdown or a restart waiting on it for longer. A launch in progress is still cancelled at once. Dropping the closure unrun (between sessions, closed channel) fires the stop too. Server.rs is unchanged.
- **Reroot per server was missing.** `reroot_one` compared server names only, so a server whose root changed (a marker added or removed) kept the document on the old root's instance. It now releases (keeps waiting) each kept server whose root moved: `didClose` there, which runs on, and the document opens on the new root's instance at the current text at the end of the reconcile. Test: `reroot_moves_only_the_server_whose_root_changed`.
