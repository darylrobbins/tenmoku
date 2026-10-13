# Search highlighting's time limit (T363)

- One `Deadline::after(time_limit())` per painted frame, shared by every row of `match_rects`
  (`matches_reaching_until`); once it runs out the frame paints what it found and searches no more rows.
- `FrameLayout::highlight_ran_out` holds the pattern's `regex_str` (its translated source plus flags) and the text
  version of a frame that ran out. The next frame takes it from the view's last layout and searches nothing while
  both match, so a keystroke doesn't spend the limit again. An edit or a different pattern clears it.
- The deadline is sampled once more after each row's search, since the clock is only read between regex calls.
- Find's off-thread count has its own `stop` flag and needs no limit. In `crates/fe-gpui/src/view/find.rs`:

  ```rust
  ranges.extend(matches_in_until(text, start..end, pattern, room, &mut Deadline::never()));
  ```

  `matches_in` keeps its signature and behaviour: it calls `matches_in_until(.., &mut Deadline::new())`.
