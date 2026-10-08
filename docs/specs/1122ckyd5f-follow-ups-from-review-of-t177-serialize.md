---
task_uid: "01m4ee1xydj5h6zp1122ckyd5f"
task: "T201"
rev: 2
type: "chore"
complexity: "m"
criteria:
  - id: "C1"
    text: "fe-config's trust store offers a locked reload-apply-save: an exclusive lock on a stable lock file beside the trust file (created 0600 if missing, never removed) is held from the load through the save, so two writers, in one process or two, never interleave."
    required: true
    manual: false
  - id: "C2"
    text: "fe-workbench's trust writer (trust.rs write) uses it for every write; a write still logs and keeps the session's decision on failure, and a bad trust file is still left as it is."
    required: true
    manual: false
  - id: "C3"
    text: "crates/fe-config/tests/trust_lock.rs: a deterministic overlapping-writers test (channels, no sleeps) in which a revocation saved while another writer holds the lock for an unrelated decision survives, and the unrelated decision is kept too."
    required: true
    manual: false
  - id: "C4"
    text: "Existing tests in crates/fe-config/tests/workspace_trust.rs and crates/fe-workbench/src/trust_tests.rs pass unchanged."
    required: true
    manual: false
---

# Follow-ups from review of T177: serialize trust-file writes across fastedit instances with a lock file (F6)

Finding F6 of the review of T177 (below merge.block_severity). The review's other finding, F5 (fe-lsp routed opens drop a queued didSave), is split out to its own card.

- F6 medium crates/fe-workbench/src/trust.rs:72: Overlapping writers in separate application instances can still restore a revoked trust decision. Instance A loads the file for an unrelated decision, instance B saves a revocation, then A saves its stale whole-file snapshot over B's result. Atomic rename prevents partial files but does not serialize read-modify-write operations. This predates the current fixes and is separate from replaying already committed decisions.
  fix: Serialize the complete reload, apply, and save operation across processes using a stable lock file, and add a deterministic overlapping-writers test that preserves the revocation.
  instances: crates/fe-workbench/src/trust.rs:72

Still open on main (86d226d): fe-workbench's `write` (trust.rs:71) does `TrustStore::load`, `decide`, `save` with no lock.

## Fix

- crates/fe-config/src/trust.rs: a locked read-modify-write, e.g. `TrustStore::update(path, |store| …) -> io::Result<()>`: open (create, mode 0600, directory 0700 as `save` does) a stable lock file beside the trust file (`trust.toml.lock`, never deleted or renamed), take an exclusive lock with `std::fs::File::lock` (flock on macOS, so two opens in one process exclude each other too), then load, apply, save, and unlock on drop. A bad file stays untouched, as today.
- crates/fe-workbench/src/trust.rs: `write` goes through it. It already runs off the main thread, so blocking on the lock is fine.

## Tests

New file crates/fe-config/tests/trust_lock.rs (an integration test, no mod line). Deterministic, with channels, no sleeps: writer A takes the lock and, inside its closure, has loaded the file and decided an unrelated folder; writer B, on another thread, revokes folder X (trusted beforehand) and is shown to be still waiting while A holds the lock; A finishes, then B; the file ends with X untrusted and A's decision kept.
