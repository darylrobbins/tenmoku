---
task_uid: "01m7tqngs3zpg123wygm8x7zkc"
task: "T2024"
rev: 3
type: "feature"
complexity: "l"
criteria:
  - id: "C1"
    text: "ConsentGate launches a granted extension server only from \u003cconsent file dir\u003e/extensions-lsp-packages/\u003cpackage_hash\u003e/: a read-only copy (never hard links), hashed from the copy and checked again at launch. The command and in-package arguments are rewritten only in the gate's bound spec. A missing or mismatched snapshot refuses the launch. Known limit (T2032, review F7): an in-package path that is an internal symlink (entrypoint, argument or flag value) may be bound to its canonical target inside the snapshot instead of its own spelling; the decision file's known limits name this. Known limit (T2033, review F10): the extension reload watcher does not fingerprint execute bits, so a chmod-only package update fires no reload and no new consent prompt until another change; the decision file's known limits name this."
    required: true
    manual: false
  - id: "C2"
    text: "A process test in crates/fe-workbench/src/lsp_consent_snapshot_tests.rs (cfg(unix), sh through ConsentGate over fe_lsp::LocalLauncher) rewrites the installed module in place, then checks that the running server and a restart before acceptance both read the old bytes. After the new package is granted, a new launch reads the new bytes."
    required: true
    manual: false
  - id: "C3"
    text: "GC tests in crates/fe-workbench/src/lsp_consent_snapshot_tests.rs: a snapshot survives while a stored grant names it, while it is pending in this session, or while a running process holds its shared lock (including another store user's lock). An unreferenced snapshot is removed after acceptance and process retirement. Abandoned .staging dirs are removed."
    required: true
    manual: false
  - id: "C4"
    text: "A package symlink whose target is absolute into the live extension dir, or relative and escaping the package root, fails closed (not grantable), with a test in lsp_consent_snapshot_tests.rs. Existing lsp_consent*_tests and crates/fastedit/tests/extension_consent.rs pass."
    required: true
    manual: false
  - id: "C5"
    text: "Decision file vmtnmkay62 §1 (~80) and the risk table describe the snapshot store, the hard-link exclusion, the symlink rule, retention/GC and the disk bound, with a revision-disclosure line. Snapshotting and GC run off the main thread."
    required: true
    manual: false
---

# Run granted extension language servers from immutable content-addressed package snapshots (review follow-up of T1978 F3)

Review follow-up of T1978 F3 (high, codex review cqejxn, moved out of T1978's scope by the lead): retaining the old `ServerSpec` does not retain the old package files. `ConsentGate::launch` (crates/fe-workbench/src/lsp_consent.rs ~1011 on T1978's lane) checks `Invocation::of` against the live extension directory and then spawns from that same mutable directory, so (a) a running server's delayed `require`/import or child process loads modules an update changed after the grant, before the user accepts the change, and (b) a check-then-spawn race lets bytes change between the hash and the spawn. T1978's own fix (package_hash, fail-closed fingerprinting, watcher trigger: F1/F2/F4/F5) stays in T1978; this card builds on its `package_hash`.

Do: run every granted extension server from an immutable, content-addressed snapshot of its package.

- **Store.** New module crates/fe-workbench/src/lsp_consent_snapshot.rs, declared from lsp_consent.rs with `#[path]` (not from lib.rs). Snapshots live in `<dir of the consent file>/extensions-lsp-packages/<package_hash>/` (no new setter: derive it from `set_extension_consent_file`'s path; without a consent file, a private per-session temp dir). Made off the main thread during `evaluation`: copy the canonical extension dir into a `.staging-*` dir in the store (std `fs::copy`, which clones on APFS and uses copy_file_range on Linux; never hard links, as an in-place rewrite of the installed file would change the snapshot), hash the staged copy with T1978's package hash, rename it to `<hash>`, then make it read-only (exec bits kept). An existing complete `<hash>` is reused without copying. The invocation's `package_hash` is the snapshot's hash, so what the prompt shows is exactly what the snapshot holds. Symlinks are copied as symlinks, unfollowed (as the hash treats them); one whose target is absolute into the live extension dir, or relative and leaving the package root, makes the package not snapshottable: it fails closed (pending, not grantable, logged), since it would reach back into mutable files.
- **Launch.** `ConsentGate::launch` runs only from the snapshot whose hash is the grant's `package_hash`, re-verified at launch (as the live dir is hashed today): the bound spec rewrites the command and each argument or `--flag=value` that names a file inside the extension dir (the names `Invocation::of` resolves) to the same relative path under the snapshot. Rewrite only in the gate's bound spec; the specs handed to the managers stay as now, so matching by name/command/args is unchanged. A launch while a changed package is pending (crash restart, new window, a later session's `intact` path) runs the granted snapshot's old bytes, not the live dir, and never the new package until it is granted. A missing or mismatched snapshot refuses the launch (fail closed) and wakes re-evaluation.
- **Lifetime and GC.** A snapshot is kept while any of: a Granted entry in the consent file names its hash (any process's), this session's evaluation has it as a server's current or pending invocation, or a process launched from it is running (the `Tracked` process holds a shared lock on `<hash>.lock` until it drops; std `File::lock_shared`). GC runs off the main thread after an evaluation and after a tracked process ends, removes a snapshot only after taking its lock exclusively without blocking and finding no grant naming it, and also removes abandoned `.staging-*` dirs it can lock. Removal failures (Windows open files) are left for the next pass. Two app instances sharing the state dir must never delete each other's running snapshot.
- **Decision file.** docs/decisions/vmtnmkay62-extensions-and-plugins.md §1 (~line 80) and the risk table (~448): granted servers run from a retained immutable snapshot under the state dir, its location, the hard-link exclusion, the symlink rule, retention/GC and disk bound (at most the granted, pending and still-running versions per server, each within T1978's caps), with a revision-disclosure line.

Tests (new file crates/fe-workbench/src/lsp_consent_snapshot_tests.rs, declared with `#[cfg(test)] #[path]` from lsp_consent.rs): the process-level test codex asked for is `#[cfg(unix)]` with `sh` through `ConsentGate` over `fe_lsp::LocalLauncher` (env sets an output path; `server.sh` waits for a stdin line, then sources `$(dirname "$0")/lib/util.sh` and writes what it read). Existing tests in lsp_consent_tests.rs, lsp_consent_package_tests.rs, lsp_consent_relaunch_tests.rs and crates/fastedit/tests/extension_consent.rs may need their expectations adjusted for snapshot paths; keep their intent.

Type feature, not bugfix: the behaviour needs the snapshot store and the base has no API a regression test could fail on.
