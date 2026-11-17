---
task_uid: "01m7nhvs4eh6wz4657y3rprhka"
task: "T678"
rev: 2
type: "chore"
complexity: "s"
criteria:
  - id: "C1"
    text: "On Linux, the 7 hex viewer tests named in the spec pass (they fail at base there): cargo nextest run -p fe-gpui hex_viewer and -p fe-workbench tests::hex_viewer"
    required: true
    manual: false
  - id: "C2"
    text: "The fix is in the product's bindings or key dispatch unless the cause is shown to be the test harness; the decision file says which and why"
    required: true
    manual: false
  - id: "C3"
    text: "macOS behaviour is unchanged: the same tests still pass on macOS (the Mac gate re-checks), and no other binding loses ctrl-f or ctrl-c on Linux outside the hex viewer"
    required: true
    manual: false
---

# Hex viewer: Find and Copy keys don't fire on Linux (ctrl-f, ctrl-c), failing 7 hex viewer tests there (Mac T1931)

## Problem

T1895 (hex viewer, on main since 9160b6a63) fails 7 tests on Linux, deterministically (helium gate
run 01m7nee1eychrg7dkwm375gy65, log test.fastedit.log on helium):

- fe-gpui hex_viewer::find_tests::{a_new_query_stops_the_running_search, find_wraps_and_reports_not_found,
  hex_query_finds_bytes_with_nul_and_newline, text_query_matches_case_by_toggle}
- fe-gpui hex_viewer::tests::{copy_follows_the_focused_column, copy_over_the_limit_is_refused}
- fe-workbench tests::hex_viewer::the_header_inputs_take_the_find_bar_keys

Each sends a `secondary()` keystroke (ctrl on Linux) and the hex.find / hex.copy command doesn't
fire. The bindings are `secondary-f` / `secondary-c` in context "HexViewer"
(crates/fe-keymap/keymaps/fastedit.toml ~2875), so on Linux something else takes ctrl-f / ctrl-c
first (a broader-context binding, vim's <C-f>, a text-input binding) or the tests' keymap isn't the
Linux one. Find the cause and fix it in the product (or the test harness if the product is right),
so the hex viewer's Find and Copy work with ctrl on Linux and cmd on macOS.

Helium holds its mac/main sync at 3c81d92ba until this lands (a sync past 9160b6a63 fails its gate).

## Helium filing note

- Helium main carries the 7 tests above with `#[ignore = "T1931 (helium T678): ..."]` (added in the
  sync merge of mac/main 9160b6a63, so that sync could gate). Removing those 7 ignore lines is part
  of this fix; the Mac's copy has no ignores. The spec's "helium holds its sync at 3c81d92ba" is past.
- The card's regression tests are the 6 assert-style ones (full module paths, no crate prefix).
  `a_new_query_stops_the_running_search` fails at base with an `expect` panic, which the regression
  check reads as a compile error, so it is a criterion only. In the criteria file, write lib unit
  tests by their bare module path (e.g. `hex_viewer::tests::copy_follows_the_focused_column`).
- Kill only processes you started, by PID; never pkill/killall by name (other lanes' gates share
  this host).

## Implementation notes

Cause: the test harness, not the product. `fe_gpui::init` (and so the workbench
tests' `open`) installs `DEFAULT_MACOS`, the keymap resolved for macOS, on every
host, where `secondary` is `cmd` and the find-next key is `cmd-g`. The tests
sent `ctrl-f`/`ctrl-c`/`f3` on Linux, which that keymap doesn't bind in
`HexViewer`. The product's Linux bindings (`ctrl-f`, `ctrl-c`, `f3`) are right
and already pinned by `crates/fe-keymap/tests/hex_viewer_keys.rs`, which also
checks the editor keeps ctrl-f/ctrl-c. Fix: the 7 tests now install the built-in keymap resolved for the host
(`KeymapLayer::parse_for(.., Os::current())`, via `init_with_keymap`) and send
the host's own keys (ctrl on Linux, cmd on macOS), so they exercise the Linux
bindings end to end; the 7 `#[ignore]`s are removed. No binding changed, so macOS and other Linux contexts are untouched.
