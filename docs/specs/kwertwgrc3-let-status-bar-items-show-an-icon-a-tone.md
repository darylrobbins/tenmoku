---
task_uid: "01m4sjfhe58xnswvkwertwgrc3"
task: "T75"
rev: 1
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "kiln-shell test status_style::builders_carry_style: StatusItem::new leaves icon None, tone Default, fill None and tooltip None; values set with .icon/.tone/.fill/.tooltip survive StatusItems::set, a replacing set and merged"
    required: true
    manual: false
  - id: "C2"
    text: "kiln-shell test status_style::entry_colors: kiln_ui::status_entry_colors gives a fill's background and foreground (text and icon) for a filled entry, and otherwise no background, text.primary text and an icon of status.error/warning/info/success by tone (text.primary for Default), for Theme::dark() and Theme::light() tokens"
    required: true
    manual: false
  - id: "C3"
    text: "kiln-shell test status_style::icon_and_tooltip_draw: in a window drawing status_bar, an item with an icon has a status-icon:{id} bound inside and left of its status-item:{id}; an item without one has none; hovering an item with a tooltip past the tooltip delay shows a tooltip with its text"
    required: true
    manual: false
  - id: "C4"
    text: "Icon::ALL has 40 icons including CircleX, TriangleAlert, Server and Lock, each SVG byte-identical to Lucide's icons/\u003cname\u003e.svg at 500620a2e8123f8d1db191538886dc0c223f69a9; THIRD_PARTY.md and kiln/THIRD_PARTY.md list them (40 SVGs) with their Feather/ISC status; cargo test -p kiln-ui and cargo test -p fastedit --test third_party pass"
    required: true
    manual: false
  - id: "C5"
    text: "The workspace builds; cargo test -p kiln-shell -p fe-workbench passes; cargo clippy --workspace --all-targets -- -D warnings is clean; cargo run -p fe-workbench --example screenshots matches the existing golden (plain items draw as before)"
    required: true
    manual: false
---

# Let status bar items show an icon, a tone, a filled segment and a tooltip in kiln-shell and kiln-ui (Mac T598)

Filed on helium from Mac T598 (~/coordination/helium-cards/T598.json; Mac lead message ~/coordination/from-mac-lead-2026-10-13-helium-zk.md). Title, type, complexity, scope, criteria and labels as on the Mac.

## Problem

The user (2026-10-13): "Colored vim modes. Click on the line/col to goto line. Maybe some fancier toolbar icons.
Let's make it a fancier status bar than just pure text."

A status item is text only. `kiln_shell::StatusItem` (`kiln/crates/kiln-shell/src/status.rs`) has `id`, `side`,
`priority`, `text` and `command`; `status_entry` hands id and text to `kiln_ui::StatusEntry`
(`kiln/crates/kiln-ui/src/status_bar.rs`), which draws a padded label with a hover background when clickable. There
is no way for an item to show an icon, a coloured segment (a vim mode pill) or a tooltip. kiln-ui bundles Lucide
icons (`kiln/crates/kiln-ui/src/file_icons.rs`, `Icon`), but none fit diagnostics, a language server or a read-only
file. This card is the framework half; fastedit's items use it in two later cards.

## Change

**kiln-ui** (`kiln/crates/kiln-ui/src/status_bar.rs`):

- `pub enum StatusTone { #[default] Default, Error, Warning, Info, Success }` and
  `pub struct StatusFill { pub background: Hsla, pub foreground: Hsla }` (Clone, Copy, Debug, PartialEq).
- `pub struct StatusEntryColors { pub background: Option<Hsla>, pub text: Hsla, pub icon: Hsla }` and
  `pub fn status_entry_colors(tone: StatusTone, fill: Option<StatusFill>, colors: &Tokens<Hsla>) -> StatusEntryColors`:
  with a fill, background `Some(fill.background)`, text and icon `fill.foreground`; without, background `None`,
  text `text.primary`, icon `status.error` / `status.warning` / `status.info` / `status.success` by tone, or
  `text.primary` for `Default`.
- `StatusEntry` gains `.icon(Icon)`, `.tone(StatusTone)`, `.fill(StatusFill)` and `.tooltip(SharedString)`, and
  draws with `status_entry_colors(…, &cx.theme().colors)`:
  - an icon: `svg().path(icon.path())`, 14 px square, tinted the icon colour, before the label with a 4 px gap,
    debug selector `status-icon:{id}`;
  - a fill: the entry's whole height in the background colour (square corners, as a lualine/airline segment),
    label in the foreground colour at semibold weight, 12 px side padding instead of 8; a filled entry keeps its
    fill on hover;
  - a tooltip: GPUI's `.tooltip` with `kiln_ui::Tooltip::new(text, None::<SharedString>, …)`.
  - An entry with none of these draws exactly as today (same padding, colours, hover).
- Bundle four more Lucide icons, unmodified from the pinned commit THIRD_PARTY.md names (1.52.0 @
  `500620a2e8123f8d1db191538886dc0c223f69a9`, `icons/<name>.svg`): `circle-x` (`Icon::CircleX`), `triangle-alert`
  (`Icon::TriangleAlert`), `server` (`Icon::Server`), `lock` (`Icon::Lock`), under a `// The status bar's; not
  mapped from names.` comment in the `icons!` list. Update the `Icon::ALL.len()` assertion in
  `kiln/crates/kiln-ui/src/tests/assets.rs` (36 to 40).
- Export `StatusTone`, `StatusFill`, `StatusEntryColors` and `status_entry_colors` from
  `kiln/crates/kiln-ui/src/lib.rs`.

**kiln-shell** (`kiln/crates/kiln-shell/src/status.rs`, `kiln/crates/kiln-shell/src/lib.rs`):

- `StatusItem` gains `pub icon: Option<Icon>`, `pub tone: StatusTone`, `pub fill: Option<StatusFill>`,
  `pub tooltip: Option<SharedString>`; `StatusItem::new` leaves them empty/default; builders `.icon()`, `.tone()`,
  `.fill()`, `.tooltip()` like `.command()`. `status_entry` passes them to `StatusEntry`.
- Re-export `kiln_ui::{Icon, StatusTone, StatusFill}` from kiln-shell (`pub use` beside `StatusItem` in lib.rs),
  so fe-gpui, which depends on kiln-shell and not kiln-ui, can use them.

**fastedit's one breaking site:** `crates/fe-workbench/src/status_bar.rs`'s `From<StatusItem> for
kiln_shell::StatusItem` builds the kiln struct by literal; build it with `kiln_shell::StatusItem::new` and
`command` instead. fastedit's own `StatusItem` mirror does not gain the new fields (host items that want them build
`kiln_shell::StatusItem` directly). No other fastedit file changes; the editor's items were moved to builders by
the go-to-line card this depends on.

**Licensing:** add the four files to the Icons row's Files cell and the "36 SVGs" count (40) in both
`THIRD_PARTY.md` and `kiln/THIRD_PARTY.md`. For each, check Lucide's `LICENSE` Feather list and
`icons/<name>.json` contributors at the pinned commit, as the existing text does, and list it under the
Feather-derived (MIT) bullets or leave it ISC only. `kiln/crates/kiln-ui/assets/icons/LICENSE` doesn't change.
No new source, so NOTICE doesn't change.

No separators between items and no change to plain items' look: the workbench screenshot golden must not change.

## Tests

New file `kiln/crates/kiln-shell/tests/status_style.rs` (an integration test; no mod.rs edit).

## Implementation notes

- Licensing: all four new icons are in Lucide's `LICENSE` Feather list at the pinned commit, `lock` and
  `server` by name, `circle-x` as `x-circle` and `triangle-alert` as `alert-triangle`, so both THIRD_PARTY.md
  files list them as Feather-derived (MIT). The pinned `LICENSE` is byte-identical to the bundled copy.
- Tooltip in tests: GPUI's tooltip view isn't reachable from a test, so `StatusEntry` wraps
  `kiln_ui::Tooltip` in a small view under the debug selector `status-tooltip:{id}:{text}`; C3 checks it's
  absent before the hover, present past the 500 ms delay, and gone once the pointer moves to another item.
- Plain entries: the entry now sets its text colour explicitly, `text.primary`, the colour it inherited from
  the bar before, and adds the 4 px gap only with an icon, so a plain entry draws as before. The screenshot
  golden is Metal-only; `cargo run -p fe-workbench --example screenshots` skips on Linux.
