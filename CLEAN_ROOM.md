# Clean-room and licensing policy

fastedit is proprietary. Everything we ship must be ours or permissively licensed.

## Never read, copy or port

Zed's GPL/AGPL crates. That includes `editor`, `rope`, `text`, `language`,
`languages`, `multi_buffer`, `vim`, `lsp`, `project`, `project_panel`, `theme`,
`settings`, `ui`, `workspace`, `picker` and `fuzzy`. Check the crate's `LICENSE-*`
file when unsure. Don't open these files, including "just to see how they did
it": code written after reading GPL code is hard to defend as independent.

Zed's Apache-2.0 crates (`gpui*`, `sum_tree`, `collections`, `util`,
`refineable`) are fine. Zed's highlight queries are GPL and also off limits.

## Copying permissive code

Allowed sources include gpui-kit (Apache-2.0), Floem (MIT; its xi-derived
buffer code is Apache-2.0), Lapce (Apache-2.0), tree-sitter grammar repos (MIT)
and nvim-treesitter queries (Apache-2.0).

When you copy code:

1. Keep the original copyright and license header at the top of the file. Add
   one if the upstream file had none (e.g.
   `// Adapted from longbridge/gpui-kit@3467e64, crates/…; Apache-2.0`).
2. Add a row to `THIRD_PARTY.md` with the source, commit and license.
3. Apache-2.0 sources: keep their `NOTICE` text, if any, in our `NOTICE`.

## MPL-2.0

Allowed only as an **unmodified dependency**, approved crate by crate in
`deny.toml`. Never paste MPL code into our files, because that makes the whole
file MPL. Never patch an MPL crate without deciding to publish the patch.
Shipped MPL crates are listed in `THIRD_PARTY.md`.

## Enforcement

`cargo deny check` runs in CI with the allowlist in `deny.toml`. Grammars and
query files are audited one by one when added, because `cargo-deny` only sees
crate metadata.
