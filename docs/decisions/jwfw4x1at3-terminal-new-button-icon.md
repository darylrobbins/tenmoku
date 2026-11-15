# Terminal panel: the + button is a 16 px icon button (T387)

## Status

Done on Linux, 2026-11-15. The goldens are left for the Mac lead (below).

## Change

`terminal_panel.rs` `tab_strip` draws `+` as
`IconButton::new("terminal-new-button", Icon::Plus, ..)`: the 22 px box and 16 px
`plus.svg` icon of the kill button beside it. `Icon::Plus` and `plus.svg` had
landed with the project panel's New menu (T1769), so none was added. The
wrapper div keeps its `terminal-new` id and debug selector; the tooltip
(`New Terminal` + the `terminal.new` key), the click dispatch and the
accessible name are unchanged. The `terminal-new-button` literal's
`l10n-allow.toml` entry matched nothing any more and is removed.

The tests in `tests/terminal_plus.rs` assert the size against kill, the icon's
path and size, the accessible name (a Button named "New Terminal") and the
tooltip's text, the label plus the resolved `terminal.new` key, read from the
text the strip's tooltip builder produced (`last_strip_tooltip`, test-only).

## kiln-ui's tests

The gate's affected-test stage doesn't select kiln-ui for an fe-workbench-only
change, so the run is here:

```text
$ cargo nextest run -p fe-workbench -p kiln-ui
     Summary [ 268.864s] 3449 tests run: 3449 passed, 3 skipped
```

## Goldens that move (Metal-only; the Mac lead regenerates them)

The strip's right end (the `+` glyph, 16 px icon in a 22 px box instead of a
12 px "+" label) is the only change. From `crates/fe-workbench/examples/screenshots/`,
run by `cargo run -p fe-workbench --example screenshots -- audit target/ui-audit`:

- `audit/terminal.rs`: `terminal_dark.png`, `terminal_light.png`
- `audit/terminal_session.rs`: `terminal_session_dark.png`,
  `terminal_session_light.png`

No other surface draws the terminal panel's tab strip.
