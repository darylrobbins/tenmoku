---
task_uid: "01m6vvvnk0jzmbbydxgfv86swr"
task: "T1556"
rev: 3
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "Tests in tests::toast_b3 (cargo nextest run -p fe-workbench) pass: 340 px wide (narrower only when the editor area minus both 12 px margins is narrower), anchored 12 px inside the editor area bottom right and above the bottom panel, never over the rail or a dock, including in the frame where a dock opens, resizes or the bottom panel toggles; surface.raised, radius 10, the float shadow, no left border; the icon colour follows the level; more than two actions is a debug assertion for toasts with an action. Known limit: choice-only toasts (Toast::choices: the grammar prompt three choices, LSP showMessageRequest actions from the server) are exempt until T1743 adds a More overflow; dismiss has an accessible name"
    required: true
    manual: false
  - id: "C2"
    text: "toast_tests and toast_routing_tests still pass (merging, expiry, Choices unchanged)"
    required: true
    manual: false
  - id: "C3"
    text: "cargo nextest run -p fe-workbench --test l10n passes (ui_text_is_in_the_catalogue, keys_used_here_exist) with this card's files in l10n-allow.toml's migrated list"
    required: true
    manual: false
---

# Toasts in B3: 340 px in the editor area above the panel, overlay surface, source-coloured icon, title, one line, two actions at most, no coloured border

# Toasts in B3

Spec: `docs/design/design-system.md#notifications-and-agents` (Toast). Audit
findings TO1–TO3 (the held card P3).

## Change (`crates/fe-workbench/src/toast.rs`)

- Lower right inside the editor area, above the bottom panel, 12 px from its
  edges (not over the rail or dock), 340 px wide.
- The overlay surface with the float shadow and hairline; radius 10; no
  coloured left border.
- An icon in the source's colour (error shinsha, warning kiseto, info ruri,
  success celadon), a title, one detail line, at most two actions, and a
  dismiss button with an accessible name.
- The existing behaviour (merging ×N, errors stay, Choices, dismiss all)
  unchanged.
- Agent toasts and the inbox are separate work.

## i18n

This card is the one that moves these files' text into the catalogue
(`docs/decisions/xte5e7hqsw-i18n-design.md` §§ 4–5 and 10): every
user-visible string in `toast.rs`, the new ones and the old, goes to
`crates/fe-workbench/l10n/en-US/toast.ftl` (keys `workbench-toast-…`), through `tr!` / `text!`, whole messages, plural
selectors for counts; add the file to `resources!` in the crate's
`src/l10n.rs` and the files to `l10n-allow.toml`'s `migrated` list. Text the
design rewrites follows the design; text it keeps stays byte-equal. Tooltips
and accessible names count.

Platform: **Linux-OK**.

## Implementation notes

- The layer anchors to the union of the panes' content bounds (`Workbench::editor_area`), 12 px in; before panes are laid out, or over the setup sheet, it keeps the window's bottom right.
- Info has no dedicated icon in the bundled set, so it uses `CircleDot`; `Level::Success` was added (celadon, `CircleCheck`).
- Two-action debug assertion applies to an action beside choices; a question's choices alone may still exceed two (the grammar prompt asks three) and wrap. The title keeps its 8-line clamp (the small-window test needs wrapping); the detail is one line.
- The audit scene's probe moved to the raised surface beside the icon; its coordinates were estimated, not run.
