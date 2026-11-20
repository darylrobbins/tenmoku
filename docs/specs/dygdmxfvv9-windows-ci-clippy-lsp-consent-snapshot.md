---
task_uid: "01m7wxrra7xtj0gkdygdmxfvv9"
task: "T720"
rev: 1
type: "chore"
complexity: "xs"
criteria:
  - id: "C1"
    text: "windows-msvc cargo clippy -p fe-workbench --all-targets -- -D warnings reports no error in lsp_consent_snapshot.rs"
    required: true
    manual: false
  - id: "C2"
    text: "macOS cargo clippy -p fe-workbench --all-targets -- -D warnings stays clean and the lsp_consent_snapshot tests pass"
    required: true
    manual: false
---

# Windows CI Clippy: lsp_consent_snapshot.rs unused_mut, set_readonly(false) and cfg(unix)-only test helpers (from T2024) (Mac T2037)

Windows CI Clippy fails on main since 811182dba (T2024), in crates/fe-workbench/src/lsp_consent_snapshot.rs (main's Windows run 44713576380, job 136211140362; also PR #9 r4 run 44742026836). Four errors under `cargo clippy --workspace --all-targets -- -D warnings` on windows-msvc:

- :114 `let mut builder` is only mutated under `cfg(unix)` (unused_mut).
- :732 `permissions.set_readonly(false)` in the `cfg(not(unix))` arm (clippy::permissions_set_readonly_false). On Windows this clears the read-only attribute, which is the intent: keep the call and allow the lint on that statement with a one-line reason, or use an equivalent that clippy accepts.
- :772 `pause_after_open` and :794 `collect_unkept` are dead code in the Windows test build: gate them (and their statics/helpers only they use) with `cfg(all(test, unix))` to match their callers.

No behaviour change. Like T2028 and T1962. Prove C1 with windows-msvc clippy (helium runs it with stubbed CC/AR); it unblocks helium T696 (Mac T1975) r5 on PR #9.

## Helium filing note

- Lent from the Mac as y155. Builds and loops: CARGO_TARGET_DIR inside your worktree, never /tmp.
- Criteria tests need full cargo test paths.
- Ask the lead (`herdr agent prompt wR7:p1 ...`) before rgt finish if a criterion can't be met as written.
  Run rgt finish once, and wait for its result before you report.
- **Windows and macOS clippy on Linux:** install the target (`rustup target add x86_64-pc-windows-msvc`,
  `aarch64-apple-darwin`), stub CC and AR with a script that exits 0 (e.g. `CC_x86_64_pc_windows_msvc=true
  AR_x86_64_pc_windows_msvc=true`), and run `cargo clippy -p fe-workbench --all-targets --target <triple> -- -D warnings`.
  For the macOS target, touch gpui-pre-apple's stitched_shaders.metal if its build script asks for it. Paste
  the clippy summaries into the finish note: C1 is the windows-msvc run, and C2 is the aarch64-apple-darwin
  clippy plus the lsp_consent_snapshot tests run natively on Linux (the macOS tests run when the batch lands).
- Fix only the four Windows findings: unused_mut :114 (cfg the binding or the mutation), set_readonly(false) :732
  (use a platform-correct permission change, or an allow with a reason if it's deliberate), and the
  cfg(unix)-only test helpers :772/:794. Behaviour doesn't change.
- The lead batches this one alone, so it lands fast and unblocks T696's Windows evidence on PR #9.
