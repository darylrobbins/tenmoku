---
task_uid: "01m4sgkstd1692mdad29pkzh7z"
task: "T73"
rev: 2
type: "feature"
complexity: "xs"
criteria:
  - id: "C1"
    text: "New test crates/fe-config/tests/change_colors_muted.rs: in each bundled theme, change_added, change_modified and change_deleted each have HSL saturation at most 0.45 and keep their hue family (added 100-160 degrees, modified 190-230, deleted 340-10). In fastedit-dark each has HSL lightness between 0.35 and 0.55. In fastedit-light each has lightness between 0.55 and 0.80"
    required: true
    manual: false
  - id: "C2"
    text: "change_colors.rs no longer asserts that change colours equal the information or error colours; its test that a theme file's own change colours override the bundled ones still passes unchanged"
    required: true
    manual: false
  - id: "C3"
    text: "Each theme file's comment above the change colours says they are muted on purpose, not derived. No golden PNG changes on helium (Metal goldens are macOS-only); this lane's docs/specs file lists every screenshot scene that shows a change bar, for the Mac to regenerate"
    required: true
    manual: false
  - id: "C4"
    text: "Every other test passes; clippy on fe-config --all-targets -D warnings is clean; nothing outside the scope changes except this lane's docs/specs file"
    required: true
    manual: false
---

# Mute the git change colours in the bundled themes: desaturated added, modified and deleted, no longer copied from the status colours (Mac T595)

Filed on helium from Mac T595 (~/coordination/helium-cards/T595.json; Mac lead message ~/coordination/from-mac-lead-2026-10-13-helium-zj.md). Title, type, complexity, scope, criteria and labels as on the Mac.

On helium (Linux) the screenshot goldens can't be regenerated: they are rendered by Metal and compared only on macOS (TESTING.md). Leave every golden PNG unchanged. Instead, list in this lane's docs/specs file which screenshot scenes show a change bar, so that the Mac regenerates those goldens when it picks this commit. C3 is worded that way on helium. Tune the colours to C1's numbers and the starting points below; on Linux the screenshot render goes through wgpu, which is for looking at a page only.

The user, after T583 landed (2026-10-13): "git change marks are more subtle but still a bright green marker". T583 dropped the row background and lowered the bar's opacity to 0.6, but kept the bundled colours. Those colours were copied from the status colours: dark change_added is #61ef79, a saturated neon green, and light's #2fd04a is the same. On dark #16181d, even at 0.6 it reads as bright. The user's reference (a JetBrains editor) uses a muted, desaturated green bar.

Give the three bundled themes their own muted change colours, the same hues with much lower saturation and moderate lightness, so the bar is noticeable but quiet. Starting points (tune by eye with the screenshot example, and record what you chose):
- fastedit-dark (background #16181d): added #4f8a5b, modified #4e7fae, deleted #b05a64
- fastedit-light (background #ffffff): added #8cc79a, modified #8fb3e0, deleted #e3999f
- fastedit-ember: already muted (#7fb889 #7fa6b8 #e0705f); bring its deleted in line if it stands out, else leave it

They stop being derived from `information` and `error`, so update the comment in each theme file and the change_colors.rs test that pins the derivation. Opacity (GUTTER_ALPHA 0.6) and geometry stay as they are. The lane's position moves in T587, a separate card.

## Implementation notes

Chosen colours (all muted, HSL saturation <= 0.45):

- fastedit-dark: added #4f8a5b, modified #4e7fae, deleted #b05a64 (the starting points).
- fastedit-light: added #8cc79a, modified #9db6d6, deleted #d6a2a7. The suggested #8fb3e0 and #e3999f had saturation 0.57, over C1's 0.45, so both were desaturated.
- fastedit-ember: added and modified unchanged; deleted #e0705f (saturation 0.67) became #c47a72.

Goldens not regenerated (Metal, macOS only). No golden PNG changes.

Screenshot scenes that show a change bar: none. No scene in `crates/fe-gpui/examples/screenshots/scenes.rs` (highlighted_rust, multi_line_selection, multi_cursor, tabs, long_line, three_digit_gutter, markdown_fence, large_c) or `crates/fe-workbench/examples/screenshots/scenes.rs` (workbench) sets a git baseline or opens a repository with changes, so no bar is drawn. Nothing for the Mac to regenerate; if a future scene adds a change bar, its golden needs regenerating then.
