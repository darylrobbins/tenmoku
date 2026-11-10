---
task_uid: "01m6vvtfz9d7aphwwz7g7vh4fh"
task: "T1538"
rev: 2
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "Tests in tests::title_bar_b3 (cargo nextest run -p fe-workbench) pass: the bar is 38 px; left to right back, forward, project (folder name), branch (current branch), no slash between them; search and settings at the right; each opens what the spec says; each has an accessible name and a tooltip with its key"
    required: true
    manual: false
  - id: "C2"
    text: "kiln-shell's title_bar and window_frame tests pass with the lights at (14, 12); the breadcrumbs row has no arrows"
    required: true
    manual: false
  - id: "C3"
    text: "cargo nextest run -p fe-workbench --test l10n passes (ui_text_is_in_the_catalogue, keys_used_here_exist) with this card's files in l10n-allow.toml's migrated list"
    required: true
    manual: false
  - id: "C4"
    text: "On the Mac: cargo run -p fe-workbench --example screenshots matches every golden; the ones this card claims were regenerated with UPDATE_SCREENSHOTS=1 in this change and looked at (the five fe-workbench goldens); the chrome audit scene's probes on the bar pass"
    required: true
    manual: false
---

# Title bar in B3: 38 px, traffic lights at (14, 12), back/forward, project and branch widgets side by side, search, settings

# Title bar in B3: 38 px, back/forward, project ▾ and branch ▾, search, settings

Spec: `docs/design/design-system.md#workbench-anatomy` (Title bar row; the
Measure table's 38 px) and `#agent-provenance` (the chip is not built here).
Audit findings CH6, TB1–TB4 (the held card P14).

## Change

- 38 px bar on `surface.chrome`, traffic lights at (14, 12)
  (`kiln/crates/kiln-platform/src/window_options.rs` `TITLE_BAR_HEIGHT`,
  `TRAFFIC_LIGHTS`, `TRAFFIC_LIGHT_INSET`), no line between it and the tab
  strip (one band, TB3).
- Left: back / forward (moved from the breadcrumbs row, which loses them),
  then the project widget (folder name ▾, opens Open Recent) and the branch
  widget (branch ▾, opens the branch picker), side by side with no slash.
  Right: search (opens Go to File / the palette as today's search key does) and
  a settings gear (Open Settings). Each is a ghost button with a hover fill, a
  tooltip naming its key, and an accessible name.
- The run widget and the agent chip are not in this card.
- Files: `kiln/crates/kiln-shell/src/title_bar.rs` (slots for left and right
  items), `crates/fe-workbench/src/title_bar.rs`,
  `crates/fe-workbench/src/workbench_chrome.rs`,
  `crates/fe-workbench/src/git_branch.rs` (the widget; the status-bar item goes
  in the status bar card), `kiln/crates/kiln-ui/src/breadcrumbs.rs` (drop the
  arrows).
- Existing tests that pin the 32 px bar and the light positions follow
  (`crates/fe-workbench/src/tests/title_bar.rs`, `tests/window_frame.rs`,
  `kiln/crates/kiln-shell/tests/title_bar.rs`, `tests/window_frame.rs`).
- Regenerate the five fe-workbench goldens on the Mac; add the probe points for
  the bar to `audit/chrome.rs`.

## i18n

This card is the one that moves these files' text into the catalogue
(`docs/decisions/xte5e7hqsw-i18n-design.md` §§ 4–5 and 10): every
user-visible string in `title_bar.rs`, `workbench_chrome.rs` and `git_branch.rs`, the new ones and the old, goes to
`crates/fe-workbench/l10n/en-US/chrome.ftl` (keys `workbench-chrome-…`), through `tr!` / `text!`, whole messages, plural
selectors for counts; add the file to `resources!` in the crate's
`src/l10n.rs` and the files to `l10n-allow.toml`'s `migrated` list. Text the
design rewrites follows the design; text it keeps stays byte-equal. Tooltips
and accessible names count.

Platform: **Mac-only** (traffic lights, goldens).
