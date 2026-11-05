---
task_uid: "01m6ab3dxqyxkmtjaapwshcge8"
task: "T1267"
rev: 1
type: "chore"
complexity: "s"
criteria:
  - id: "C1"
    text: "LICENSE exists and is byte-identical to LICENSES/Apache-2.0.txt: cmp LICENSE LICENSES/Apache-2.0.txt prints nothing"
    required: true
    manual: false
  - id: "C2"
    text: "NOTICE starts with \"Tenmoku\" and \"Licensed under the Apache License, Version 2.0\", has no \"All rights reserved\", still lists GPUI, gpui-kit, VS Code, Catppuccin, nvim-treesitter and Lucide, and has an app-icon entry"
    required: true
    manual: false
  - id: "C3"
    text: "Cargo.toml [workspace.package] and fuzz/Cargo.toml say license = \"Apache-2.0\"; rg -n \"LicenseRef-Proprietary\" Cargo.toml fuzz/Cargo.toml CLEAN_ROOM.md deny.toml prints nothing; rg -n -i \"proprietary\" CLEAN_ROOM.md deny.toml prints nothing"
    required: true
    manual: false
  - id: "C4"
    text: "THIRD_PARTY.md has a section \"Bundled assets audit\" with one line each for fonts, app icon, themes, icons, grammars and queries, and vendored crates, and says for each whether any file lacked a source and licence; any gap found is fixed with a row or header in this card"
    required: true
    manual: false
  - id: "C5"
    text: "cargo nextest run -p fastedit --test license_files passes (new file crates/fastedit/tests/license_files.rs): LICENSE equals LICENSES/Apache-2.0.txt; the workspace manifest (parsed with the toml crate) and fuzz/Cargo.toml say Apache-2.0; NOTICE has no \"All rights reserved\"; every file in crates/fe-config/themes/ not named tenmoku-* is named in both THIRD_PARTY.md and NOTICE"
    required: true
    manual: false
  - id: "C6"
    text: "cargo deny check licenses bans sources passes and the deny.toml diff is comments only: git diff -U0 deny.toml shows only lines starting with + or - #; the allow list is untouched"
    required: true
    manual: false
  - id: "C7"
    text: "git diff --stat against the base lists only the eight scope paths"
    required: true
    manual: false
---

# Relicense Tenmoku's own code to Apache-2.0: add LICENSE, rewrite NOTICE, set the workspace licence, and audit every bundled asset class in THIRD_PARTY.md

## Goal

Tenmoku's own code becomes Apache-2.0 (user decision, "First public release" checklist). This card ships
the licence files, flips the root workspace's licence metadata and policy text, and audits every bundled
asset class. kiln's licence flip (329 SPDX headers) is the next card, not this one.

## Facts found

- No `LICENSE` file exists. `LICENSES/Apache-2.0.txt` and `LICENSES/MIT.txt` do (the texts third-party rows ship).
- `NOTICE` says "Copyright (c) 2026 Daryl Robbins. All rights reserved." and already lists GPUI, gpui-kit, VS Code,
  Catppuccin, nvim-treesitter queries and Lucide. `THIRD_PARTY.md` has per-file rows with commits for adapted code,
  themes, icons (Lucide, in `kiln/crates/kiln-ui/assets/icons/`) and a grammar table (every grammar MIT).
- Licence metadata still says proprietary: `Cargo.toml` line 12 `license = "LicenseRef-Proprietary"` (inherited by all
  `crates/*` via `license.workspace = true`), `fuzz/Cargo.toml` line 8, `CLEAN_ROOM.md` line 3
  ("fastedit is proprietary"), `deny.toml` comments at lines 1 and 42. kiln's seven-plus manifests and its SPDX
  headers are out of scope here.
- Root `crates/**/*.rs` carry no SPDX headers (only `kiln/` does), so no header edits here.
- No font files are bundled (rg/fd find no .ttf/.otf/.woff); the only non-code assets are
  `crates/fastedit/assets/icon/fastedit.svg` and `fastedit-1024.png` (the app icon, the author's own work), the
  themes in `crates/fe-config/themes/`, `crates/fe-syntax/queries/**`, the Lucide icons and the vendored
  `vendor/tree-sitter` and `vendor/tree-sitter-yaml` (both have a LICENSE file).
- `regatta.toml` `human_paths = ["deny.toml"]`: a deny.toml edit, even a comment, waits for the user. Keep it to
  the two comment lines so approval is one glance.

## Changes

1. `LICENSE`: the exact bytes of `LICENSES/Apache-2.0.txt`.
2. `NOTICE`: first lines become `Tenmoku` / `Copyright 2026 Daryl Robbins` / `Licensed under the Apache License,
   Version 2.0; see LICENSE.` Drop "All rights reserved". Keep every third-party entry. Add an entry for the app icon
   (own work, same licence) so the audit leaves nothing implicit.
3. `Cargo.toml` `[workspace.package]` `license = "Apache-2.0"`; `fuzz/Cargo.toml` the same.
4. `CLEAN_ROOM.md` line 3 and `deny.toml` lines 1 and 42: say Tenmoku is Apache-2.0 (the allowlist stays
   permissive; do not widen it).
5. `THIRD_PARTY.md`: add a short "Bundled assets audit (2026-10-31)" section, one line per class: fonts (none
   bundled; text renders in system fonts), app icon, themes (tenmoku-* own; the rest rows above), icons (Lucide),
   grammars and queries, vendored crates. Where a class has a gap (a shipped file with no row and no header naming
   its source and licence) fix it in this card: add the row or the header, never silence it. State the result
   either way.
6. New `crates/fastedit/tests/license_files.rs` (flat file in that crate's tests dir, like `app_name.rs`).

## Not here

kiln manifests, headers and `kiln/NOTICE` (next card). Contributor docs and README (separate card). Anything in
`.github/`.
