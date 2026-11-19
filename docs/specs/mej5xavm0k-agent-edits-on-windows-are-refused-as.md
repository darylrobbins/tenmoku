---
task_uid: "01m7rgvttaxjphs1mej5xavm0k"
task: "T695"
rev: 6
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "The cause is written up in the decision file before the fix, with the evidence helium can give (the failing job's raw-tempdir/canonicalized split, GH runners' RUNNER~1 TEMP); the GH CI windows run after landing confirms it"
    required: true
    manual: false
  - id: "C2"
    text: "chat_tool_loop::tests::edits_run_in_the_calls_order_whatever_is_answered_first and chat_tool_loop::tests::a_hand_in_under_way_ends_only_its_own_turn pass on the gate; the GH CI windows Test job after landing checks them on Windows (a lane can't run Windows CI)"
    required: true
    manual: false
  - id: "C3"
    text: "A regression test in crates/fe-workbench/src/tests/chat_edit_root_spelling.rs proposes an edit under a root reached by another spelling of the same folder (Windows: an 8.3 short name or different letter case) and the edit is located inside the root; if that spelling can be made on macOS (case-insensitive APFS) so it fails at base on the gate, respec to add it as --regression-test"
    required: true
    manual: false
  - id: "C4"
    text: "Real escapes are still refused: the chat_apply, agent_tools and chat_context tests for symlinks out of the root, .. and .git/secret names pass on the gate"
    required: true
    manual: false
---

# Agent edits on Windows are refused as outside the folder when the root is spelled differently from its canonical form (chat_apply::locate compares real_path against dunce::canonicalize) (Mac T1972)

# Agent edits refused as "outside the folder" on Windows

GH CI 44405849017 on 8ba6f008c, windows Test: chat_tool_loop_tests.rs:661 edits_run_in_the_calls_order_whatever_is_answered_first gets `ToolResult { content: "it's outside the folder", is_error: true }` for an edit of `notes.txt` directly under the root. a_hand_in_under_way_ends_only_its_own_turn (:543, `left: None`) uses the same propose_edit flow.

Suspected cause (not confirmed; there's no Windows box here): these tests pass the raw `tempfile::tempdir()` path as the root. `chat_apply::locate` (chat_apply.rs:427) and `agent_tools::resolve` (:174) compare `real_path(root.join(rel))`, which follows links part by part but keeps each part's spelling, against `dunce::canonicalize(root)`, which expands 8.3 short names and fixes letter case. If the runner's TEMP is spelled differently from its canonical form (e.g. `C:\Users\RUNNER~1\...`), `starts_with` fails. This fits: the chat tests that do pass on Windows (chat_apply_tests.rs:75, agent_tools_tests.rs:45) `dunce::canonicalize(temp_dir())` first. A user who opens a folder by a short or differently-cased path would get every agent edit refused.

Fix: compare spellings resolved the same way, e.g. run the root through `real_path` too, or canonicalize the located path, keeping the link-escape and `..` guarantees. First confirm the cause on GH CI, for example by temporarily logging `real` and `real_root` in a CI run. If the cause turns out to be test-only, stop and respec as a chore.

## Helium filing note

- Lent from the Mac as y149. No dependency. None of T1953's (helium T694) files.
- **Linux can't make the Windows spellings** (8.3 short names, letter case on a case-sensitive disk). Confirm the
  cause first (C1): read real_path and dunce::canonicalize side by side and say in the decision file whether any
  alias Linux can make (a symlinked root or ancestor, a `.`/`//` spelling, a bind-mount-free trick) hits the same
  compare. If none does, say so there; the GH CI windows run after landing checks C2.
- **Regression test:** the card has none. Add one (`rgt task respec --regression-test`) only if it fails at base
  on Linux. Then don't put it in a new `src/tests/<y>.rs` declared from `src/tests/mod.rs`: the regression check
  runs 0 tests at base for that. Make it a submodule of a test module base already has (`src/tests/<x>.rs` +
  `src/tests/<x>/*.rs`), list the parent and `<x>/**` as `--test-support-path`, and name it by its full path
  (`tests::<x>::<y>::<name>`). Respec before the next finish (refused while one runs).
- **tests/mod.rs:** keep your change to the one `mod` line (argon holds Mac T1966, which adds a helper there).
- Builds and loops: CARGO_TARGET_DIR inside your worktree, never /tmp. Ask the lead before rgt finish if anything
  in the criteria can't be met as written.
