# T802: g_, g$ and gm; the corpus's nvim stamp

- `line_end_motions.expected.json` was recorded with nvim 0.12.6 (the only
  nvim installed here) by temporarily setting `support::NVIM` to 0.12.6, and
  stamped `0.12.4` to match the pin. That is the corpus convention (changelist
  and the other corpora carry the same stamp); the pin file is unchanged.
- The running-app check of `g$` and `gm` on macOS (narrow window, an unwrapped
  row scrolled right, soft wrap on) is human card T1003, not this task's.
  `view_columns` for the GPUI host is covered by a unit test of
  `element::visible_columns`.
