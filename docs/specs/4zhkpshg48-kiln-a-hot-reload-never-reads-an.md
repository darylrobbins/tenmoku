---
task_uid: "01m564gvkfhdrpk94zhkpshg48"
task: "T190"
rev: 2
type: "chore"
complexity: "s"
criteria:
  - id: "C1"
    text: "kiln integration test a_theme_swapped_for_an_outside_symlink_is_not_read (tests/extension_themes.rs) passes: after a loaded extension's theme file is replaced by a symlink to a file outside the extension directory and the rescan runs, the outside file's colours are never applied, an error diagnostic names the theme file, and the extension's other theme still loads (a guard test: T143's read_contained already fixed the bug at the base)"
    required: true
    manual: false
  - id: "C2"
    text: "cargo nextest run -p kiln passes with no existing test edited; cargo clippy -p kiln --all-targets -- -D warnings is clean; scripts/check_kiln_boundary.py passes; no Cargo changes"
    required: true
    manual: false
---

# kiln: test that a hot reload never reads an extension theme swapped for a symlink outside the extension directory, and document read_themes' check (T142 review F1, already fixed by T143)

Argon follow-up to T142 (Mac T842), review 4wfq31, finding F1 (medium, path containment). It's a real bug, so it's filed under note d. It depends on T143 (Mac T843) only because both change `kiln/crates/kiln/src/extensions.rs`.

## Problem

Extension manifests reject any contributed path that resolves outside the extension's directory. But on a hot reload (T141's rescan), a manifest that fails validation keeps its last good version. So if a valid theme file is replaced by a symlink to a file outside the extension directory:

1. The rescan rejects the manifest with a containment error.
2. The last good manifest is kept.
3. `read_themes` (`kiln/crates/kiln/src/extensions.rs`, around line 193) opens the cached path, follows the symlink, and loads the outside file anyway.

## Change

- Before `read_themes` reads a contributed theme, resolve its current path (canonicalize, following symlinks) and check that it is still under the extension's canonical directory. This includes themes coming from a kept last-good manifest.
- A theme that fails the check is not read. It gets one error diagnostic that names the file and says it resolves outside the extension. Its own last good theme is kept, as for a theme file that fails to parse, so the user's chosen theme doesn't flip on a bad edit. No new data is read from outside.
- Other themes of the same extension, and other extensions, are unaffected.
- Doc comment on `read_themes` states the check.

New tests go in the existing `kiln/crates/kiln/tests/extension_themes.rs`. Any new kiln file needs the SPDX header, and `scripts/check_kiln_boundary.py` must still pass.
