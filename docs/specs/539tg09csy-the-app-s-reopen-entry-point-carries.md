---
task_uid: "01m7yrap9gx2cf6z539tg09csy"
task: "T734"
rev: 3
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "launch_windows_in's windows carry Option\u003cProjectLocation\u003e roots and main.rs reports a remote root with the existing unsupported message, opening no window for it; local reopen behaviour unchanged"
    required: true
    manual: false
  - id: "C2"
    text: "a test in crates/fastedit/tests/location_handoff.rs drives the reopen entry point main.rs calls with a saved remote window and sees the report and no window"
    required: true
    manual: false
---

# The app's reopen entry point carries ProjectLocation roots: launch_windows_in and main.rs, remote roots reported at window creation (T725 review F2 follow-up) (Mac T2079)

# The app's reopen entry point carries ProjectLocation roots

Follow-up from T725 (Mac T2052) review round 2, finding F2 (medium). T725 made `reopen::launch_locations_in` carry `Option<ProjectLocation>` roots. The app's real reopen entry point, `reopen::launch_windows_in`, is called only from `crates/fastedit/src/main.rs` (around line 766, which assigns `Launch::root: Option<PathBuf>`), and still takes and returns `PathBuf` roots, because main.rs was out of T725's scope (T729 and T662 held it).

Change: `launch_windows_in` (or its replacement) returns windows whose root is `Option<ProjectLocation>`, and main.rs maps a remote root to the existing "remote folders aren't supported yet" report at window creation, opening no window for it, as the handoff path already does. Local behaviour is unchanged.

Test: a reopen of a saved session holding a remote window, driven through the entry point main.rs calls, reports the error and opens no window for it.
