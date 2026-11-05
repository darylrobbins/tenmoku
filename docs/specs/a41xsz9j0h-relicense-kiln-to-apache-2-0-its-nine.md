---
task_uid: "01m6ab498c22qtr0a41xsz9j0h"
task: "T1268"
rev: 1
type: "chore"
complexity: "m"
criteria:
  - id: "C1"
    text: "rg -n \"LicenseRef-Proprietary\" kiln scripts/check_kiln_boundary.py scripts/kiln_standalone.py prints nothing, and rg -n -i \"proprietary|not yet open source|all rights reserved\" kiln --glob \"!**/target/**\" prints only lines that are about someone else's code (justify each in the hand-in) or nothing"
    required: true
    manual: false
  - id: "C2"
    text: "every kiln manifest says license = \"Apache-2.0\" and every kiln .rs file starts with \"// SPDX-License-Identifier: Apache-2.0\": python3 scripts/check_kiln_boundary.py exits 0, and python3 scripts/check_kiln_boundary.py --self-test (or the script's existing self-test entry point) exits 0"
    required: true
    manual: false
  - id: "C3"
    text: "git diff of each kiln .rs file is exactly its first line (git diff --numstat -- \"kiln/**/*.rs\" shows 1 added, 1 deleted for every file)"
    required: true
    manual: false
  - id: "C4"
    text: "kiln/LICENSE equals LICENSES/Apache-2.0.txt, kiln/NOTICE no longer says \"All rights reserved\", kiln/README.md Licence section says Apache-2.0 and no longer says \"not yet open source\""
    required: true
    manual: false
  - id: "C5"
    text: "python3 scripts/kiln_standalone.py (its existing dry-run or check mode) still exits 0 and now lists LICENSE as shipped"
    required: true
    manual: false
  - id: "C6"
    text: "cargo clippy --workspace passes (headers are comments; this proves nothing else moved)"
    required: true
    manual: false
---

# Relicense kiln to Apache-2.0: its nine manifests, the SPDX header of every kiln .rs file, the boundary check, and kiln's LICENSE, NOTICE and README

## Goal

kiln is part of the public repository and Tenmoku links it, so it must carry the same licence as the app:
Apache-2.0. Today every kiln crate is `LicenseRef-Proprietary` and every kiln `.rs` file starts with
`// SPDX-License-Identifier: LicenseRef-Proprietary`. This is the mechanical flip kiln/README.md's Licence
section describes ("one `license` line per crate, the SPDX headers, and the licence texts").

## Facts found

- 329 `.rs` files under `kiln/` all have the header; `scripts/check_kiln_boundary.py` (`check_files`,
  `check_licences`, and the `self_test` fixture at line ~119) requires header and manifest to agree, so all three
  change together or CI's boundary check fails.
- Nine kiln manifests carry the licence line (`rg -l LicenseRef-Proprietary kiln -g '*.toml'`).
- `kiln/NOTICE` begins "All rights reserved"; `kiln/LICENSES/` holds only `Apache-2.0.txt`; `kiln/README.md`
  "## Licence" says "not yet open source"; `kiln/CLEAN_ROOM.md` and `kiln/deny.toml` say proprietary;
  `scripts/kiln_standalone.py` lines 265-267 name `LICENCE_FILES` as shipped only after "the licence flip".
- Depends on the root card (LICENSE text, NOTICE wording). Touches every kiln source file, so land it when no kiln
  lane is running; blocked kiln cards (T1234) will need a rebase after.

## Changes

1. Each kiln manifest `license = "Apache-2.0"`.
2. Every `kiln/**/*.rs` header line becomes `// SPDX-License-Identifier: Apache-2.0` (one scripted pass; no other
   byte of any file changes).
3. `scripts/check_kiln_boundary.py` expects `Apache-2.0` (code and self-test fixture).
4. `kiln/LICENSE` (same bytes as `LICENSES/Apache-2.0.txt`), `kiln/NOTICE` header as the root's, `kiln/README.md`
   Licence section, `kiln/CLEAN_ROOM.md` and `kiln/deny.toml` first-line text. `kiln/deny.toml` is a deny.toml:
   comment-only edit, goes to the user for approval (human_paths matches by name; check).
5. `scripts/kiln_standalone.py`: ship `LICENSE` now (drop the "until the licence flip" comment), keep its behaviour
   otherwise.

## Implementation notes

- `.wat` fixtures: the 16 under `kiln-plugin/tests/fixtures` carried the old SPDX header too, and C1 needs them
  flipped. Same one-line change as the `.rs` files.
- `check_kiln_boundary.py` used to accept any SPDX identifier and any licence the crates agreed on. It now requires
  `Apache-2.0` in both, which also covers what the "disagree" check did, so that check is gone.
- T1259 F1 (`kiln-shell/src/layout.rs:587`, file-launch windows rewrite the layout on every notification) is real and
  still open. It isn't fixed here: C3 and Change 2 allow only the header line in every kiln `.rs` file. It needs its
  own card.
- `docs/embedder-audit.md` still says kiln is `LicenseRef-Proprietary`. It's outside this card's scope.
