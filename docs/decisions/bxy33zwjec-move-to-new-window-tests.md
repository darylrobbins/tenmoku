# T1949: Move to New Window tests (T1930 F3, F4)

- F4: `a_destination_the_user_used_stays_when_the_load_fails` moved from
  main.rs `mod tests` to `crates/fastedit/src/move_to_window_tests.rs`,
  declared from the base-declared `app_icon_tests.rs` (`#[path]`), so a
  test-only overlay at base discovers it with no main.rs change. It carries
  its own `wait_for` and `window_files` helpers; main.rs only loses the test.
- F3: `a_binary_file_tab_moves_to_a_new_window_with_a_hex_viewer`
  (fe-workbench `tests/move_to_new_window.rs`) already drives the move through
  an opener that really opens the destination window and asks `file_loaded`
  on it, then asserts the destination keeps the hex viewer and the source tab
  closes; it no longer relies on `opened()`'s fake loader for the move.

## Fails with T1930's fix reverted (checked by hand)

Reverted `if untouched {` to always close in `open_window_on` and removed
`|| tab.hex_viewer().is_some()` from `load_of` in fe-workbench open.rs:

- `app_icon_tests::move_to_window_tests::a_destination_the_user_used_stays_when_the_load_fails`
  FAILED (move_to_window_tests.rs:55, the destination window was closed).
- `a_binary_file_tab_moves_to_a_new_window_with_a_hex_viewer` FAILED
  (move_to_new_window.rs:412, `loaded` false).
- Also failing: `a_binary_file_in_a_hex_viewer_counts_as_loaded` and main.rs
  `a_moved_file_opens_alone_in_a_fresh_window_at_its_place`.

All pass with the fix in place.
