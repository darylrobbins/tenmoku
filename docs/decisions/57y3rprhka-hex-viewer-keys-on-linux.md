# Hex viewer Find and Copy keys on Linux (T678)

- **The cause is the test harness; the product is right.** `fe_gpui::init`
  installs `DEFAULT_MACOS` on every host, so in tests `secondary` is `cmd` and
  find-next is `cmd-g`. The 7 tests sent Linux's `ctrl-f`, `ctrl-c` and `f3`,
  which that keymap does not bind in `HexViewer`.
- The Linux bindings are correct and pinned by
  `crates/fe-keymap/tests/hex_viewer_keys.rs` (they resolve in `HexViewer`; the
  editor keeps ctrl-f and ctrl-c). No binding changed, so macOS and every other
  Linux context are as before.
- The tests now install the host-resolved built-in keymap and send the host's own keys, so
  they exercise Linux ctrl-f/ctrl-c/f3 and macOS cmd keys end to end; the 7
  `#[ignore]`s are gone.
