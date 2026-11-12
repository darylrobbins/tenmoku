---
task_uid: "01m71sz46pvccf81s84acpgezq"
task: "T1695"
rev: 2
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "`kiln_plugin::paths` maps host to guest and back as pure functions: `C:\\Users\\d\\work\\a.rs` \u003c-\u003e `/C:/Users/d/work/a.rs`, drive letter upper-cased, identity on Unix."
    required: true
    manual: false
  - id: "C2"
    text: "New tests in kiln/crates/kiln-plugin/tests/paths.rs, run on every OS as pure functions: `paths_drive_path_round_trips`, `paths_lowercase_drive_is_uppercased`, `paths_guest_backslash_is_invalid_argument`, `paths_guest_without_drive_prefix_is_invalid_argument_on_windows_rules`, `paths_unc_and_verbatim_roots_are_skipped_with_a_warning`, `paths_identity_on_unix_rules`."
    required: true
    manual: false
  - id: "C3"
    text: "Preopens are registered under guest paths (fs.rs), and `events.buffer-opened.path`, `buffer-saved.path`, `buffer.snapshot.path` and every other path in the WIT cross the boundary through the mapping both ways (events.rs, crates/fe-plugin/src/editor.rs, crates/fe-workbench/src/plugin_events.rs); tests in crates/fe-plugin/tests/editor.rs cover a snapshot path and an edit path with Windows-style host paths via the pure mapping."
    required: true
    manual: false
  - id: "C4"
    text: "Roots over 260 characters keep `\\\\?\\` and are warned about; UNC roots are not preopened (Q7 follows the design's proposal: unavailable until decided), with a log warning."
    required: true
    manual: false
  - id: "C5"
    text: "On Unix every existing fs, events and editor test passes unchanged."
    required: true
    manual: false
---

# Guest paths: host paths mapped to /C:/... in preopens and every WIT path (P6, W-d)

# Guest paths: host paths mapped to /C:/... in preopens and every WIT path (P6, W-d)

Draft id P6 of the plugins stage 3 plan (docs/decisions/dqsjfernkt-plugins-stage-3.md, section 6). The design is approved by the user (2026-11-09); questions Q1 to Q7 follow the design's proposals.

Section 2.2 row W-d. Root context rule: a path outside every preopened root is refused after mapping, never passed through as a host string.

Read the design sections named above before starting. Do not widen --scope; if a file outside it is needed, stop and say so.

## Implementation notes

- `kiln_plugin::paths` is pure string code over a `Rules` value (`Unix` or `Windows`;
  `Rules::HOST` is this OS's), so `tests/paths.rs` runs both sets of rules on every OS.
  `to_guest` / `root_to_guest` give `Unmapped` (not absolute, UNC, device, long root) for a
  path with no guest form; `to_host` gives `invalid-argument`. `paths::root` and
  `paths::guest` are the logging wrappers (a warning for a root, debug for a buffer's path,
  which recurs on every event).
- Where each path crosses: `fs::configure` preopens each root at `paths::root`; fe-workbench's
  `plugin_events::path_of` gives `buffer-opened.path` and `buffer-saved.path` as guest paths
  before they're pushed (an open whose path has no guest form names none, a save to one isn't
  sent), and kiln's event queue passes them on unchanged, documented as guest paths, so
  `kiln-plugin/tests/events.rs`'s `/work/…` fixtures hold on every OS; fe-plugin's
  `buffer.read` maps `snapshot.path` from the bridge's host path. `plugin_bridge.rs`'s
  `BufferText.path` (out of scope) still uses `display()`; for a UTF-8 path that is the same
  string.
- A bare drive (`D:`, the current directory on D) is not a root: it has no guest path, and
  `/D:` maps back to nothing (review f6p67t F1).
- Guest to host: no WIT import takes a path from a plugin yet, so nothing calls the reverse
  map at runtime. `paths::to_host_within(guest, rules, roots)` applies the root context rule
  (`..`/`.` parts are `invalid-argument`, a path outside every mappable root is `denied`) for
  `navigation.open` (A8) to call.
- Roots (C4): a Windows root still in `\\?\` form or over 260 UTF-16 units, and every UNC root
  (`\\server\share`, `\\?\UNC\…`), is not preopened, with a warning. A buffer's file over 260
  characters under a short root keeps its `\\?\` on the host and drops it in its guest path.
- `tests/fs.rs` gives the guest mapped paths and no longer ignores its three tests on Windows,
  whose ignore reason was the host-path preopen this card replaces. They run only in CI's
  `windows` job; `dotdot_and_symlinks_cannot_leave_the_root` also needs that runner to create
  symlinks, which the design says its admin runner can.
- `editor::State::with_path_rules` lets fe-plugin's tests read Windows host paths on any OS.
