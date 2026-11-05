# Clean-room and licensing policy for kiln

kiln is licensed under Apache-2.0. Everything in it must be
written for it or permissively licensed, and nothing may derive from
copyleft code. While kiln lives inside a host repository, that
repository's own policy applies as well; this file restates what concerns
kiln, so it holds once kiln stands alone.

## Never read, copy or port

Zed's GPL/AGPL crates. kiln covers the same ground as several of them, so
these matter most:

- `ui` (Zed's widget kit), against kiln-ui;
- `workspace` (panes, docks, the modal layer, persistence), against
  kiln-shell;
- `picker` (the palette and pickers), against kiln-ui and kiln-shell;
- `theme` (theme data and the theme registry), against kiln-core's theme
  and kiln-ui;
- `settings` (settings files, layering and keymaps), against kiln-core's
  config, settings stores and keymaps.

The others are off limits too: `editor`, `rope`, `text`, `language`,
`languages`, `multi_buffer`, `vim`, `lsp`, `project`, `project_panel` and
`fuzzy`, and Zed's highlight queries and keymap JSON. Check a crate's
`LICENSE-*` file when unsure.

Don't open these files, including "just to see how they did it": code
written after reading GPL code is hard to defend as independent. Learn what
an app should do by running it and reading its user documentation, not its
source.

## Allowed sources

- Zed's Apache-2.0 crates: `gpui*` (published as `gpui-pre`), `sum_tree`,
  `collections`, `util` and `refineable`, including GPUI's examples.
  kiln-ui's `TextField` adapts GPUI's `examples/input.rs` (`THIRD_PARTY.md`).
- gpui-kit (Apache-2.0), Floem (MIT; its xi-derived code is Apache-2.0) and
  Lapce (Apache-2.0).
- Lucide icons (ISC, and MIT for those derived from Feather).

When you copy or adapt code:

1. Keep the original copyright and licence header at the top of the file,
   or add one naming the source, commit and licence if the upstream file has
   none (e.g. `// Adapted from gpui-pre 0.3.7 examples/input.rs
   (zed@1a28cff); Apache-2.0`), and say in the module docs what changed.
2. Add a row to `THIRD_PARTY.md` with the source, commit and licence.
3. Apache-2.0 sources: keep their `NOTICE` text, if any, in `NOTICE`.

## MPL-2.0

Allowed only as an **unmodified dependency**, approved crate by crate in
`deny.toml`. Never paste MPL code into kiln's files, because that makes the
whole file MPL. Shipped MPL crates are listed in `THIRD_PARTY.md`.

## Enforcement

`cargo deny check` checks dependencies against the allowlist in
`deny.toml`. Copied code and assets are audited one by one when added,
because `cargo-deny` only sees crate metadata.
