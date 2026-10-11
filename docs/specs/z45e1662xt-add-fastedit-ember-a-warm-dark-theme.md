---
task_uid: "01m4p1wpc8ghspghz45e1662xt"
task: "T6"
rev: 1
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "fe-config test ember_theme::parses_cleanly: ThemeFile::parse of fastedit-ember.toml with fe_config::theme::EXTENSIONS gives no diagnostics, name \"fastedit Ember\", appearance Dark; its tokens include every key Kiln Dark's DARK_SOURCE sets; its editor table has exactly EditorColors::KEYS; its syntax captures are fastedit Dark's 25"
    required: true
    manual: false
  - id: "C2"
    text: "fe-config test ember_theme::looks_different: EditorPalette::for_theme(ThemeData::from_file(ember)) returns no warnings; editor.background equals the theme's surface.background; editor background, foreground, cursor, selection and the keyword, string and function colours all differ from EditorPalette::fastedit_dark(), and surface.background and accent.base differ from ThemeData::dark()'s"
    required: true
    manual: false
---

# Add fastedit Ember, a warm dark theme file users can try as their theme.toml (Mac T372)

Filed on helium from Mac T372 (~/coordination/helium-cards/T372.json; Mac lead message ~/coordination/from-mac-lead-2026-10-11-helium-a.md). Title, type, complexity, scope, criteria and labels as on the Mac.

## Problem

The user asked (2026-10-11): "let's create a second theme we can try out".

What exists, or is about to:

- **fastedit Dark**: `crates/fe-config/themes/fastedit-dark.toml` (editor and syntax colours) over kiln's
  Kiln Dark (`kiln/crates/kiln-core/themes/dark.toml`, chrome tokens). A cool blue-grey One Dark look:
  background `#16181d`, accent `#4f7cc8`, keyword `#c678dd`.
- **fastedit Light**: T245 (mergeable, branch `lane/55y8hx1s3p`) adds
  `crates/fe-config/themes/fastedit-light.toml` (a One Light look) over Kiln Light, picked with
  `[theme] mode = "light"` once T246 and T247 land.

So a light theme is already coming, and a second *light* theme wouldn't be a new look. Both existing themes are
the same cool, blue-accented family. This card adds a genuinely different one: **fastedit Ember**, a warm dark
theme (brown-black surfaces, amber accent, warm syntax hues).

How it's selected is fixed by `docs/decisions/app-framework.md` §8 (added by T245; user, 2026-10-11: "Use
kiln's theme service"): kiln has a dark/light pair, a `ThemeMode` and **one user theme file**,
`<config_dir>/theme.toml` (`fe_config::paths::user_theme_path`), laid over the dark or the light base as its
`appearance` says. "There's no themes directory or picker by name." So a second theme is a complete theme
file the user copies to `theme.toml`. With `appearance = "dark"` it replaces dark only, so `mode = "dark"`
shows Ember and `mode = "light"` still shows fastedit Light: the user can flip between the two.

## Change

**One new file, `crates/fe-config/themes/fastedit-ember.toml`**, a full kiln theme file. No code changes: it is
not compiled into the binary and no base, setting or picker changes.

- Header comment: what it is, and how to try it: copy it to `~/.config/fastedit/theme.toml` (or
  `$XDG_CONFIG_HOME/fastedit/theme.toml`) with `mode = "dark"` (the default); it reloads live; delete the file
  to go back to fastedit Dark.
- `kiln_theme = 1`, `name = "fastedit Ember"`, `appearance = "dark"`.
- **Chrome tokens**: set every token key that Kiln Dark's `dark.toml` sets (`[surface]`, `[text]`, `[border]`,
  `[accent]`, `[state]`, `[status]`, `[list]`, `[tab]`), so nothing on screen keeps Kiln Dark's blue-grey.
  Use token tables, not the legacy `[ui]` alias table. Anchors: `surface.background = "#1c1814"`,
  `surface.panel = "#16120f"`, `border.default = "#2e2722"`, `text.primary = "#a8998a"`,
  `text.muted = "#5e544a"`, `accent.base = "#d08b3e"`, `text.on_accent = "#1c1814"`; `list.folder_icon` and
  `list.chevron` warm, not blue. The `state` overlays may keep Kiln Dark's white alphas.
- **`[editor]`**: all 13 `EditorColors::KEYS`. `background` equals `surface.background`; `foreground =
  "#e3d6c3"`; `cursor = "#f0a050"`; `selection` an amber tint (`#d08b3e40`); `popover_background` /
  `popover_border` warm greys a step lighter than `surface.background` (`#26201b`, `#3a312a`); diagnostic colours match `[status]`.
- **`[syntax]`**: the same 25 captures as fastedit Dark, in the same order, in warm hues of the implementer's
  choosing (terracotta keywords, gold functions, sage strings, for instance). `keyword`, `function`, `type`,
  `string`, `number` and `comment` are pairwise distinct, and `comment` is the dimmest.

**Test**: `crates/fe-config/tests/ember_theme.rs`, a new file. Load the theme with
`include_str!("../themes/fastedit-ember.toml")` and `kiln_core::theme::ThemeFile::parse(…, fe_config::theme::EXTENSIONS)`,
the way T245's `tests/editor_palette.rs` does, and Kiln Dark from `kiln_core::theme::DARK_SOURCE`.

Not in this card: a command or CLI flag that installs the file, bundled-theme selection by name (that would
reverse §8), and any change to fe-config's code, kiln, fe-gpui, the host or the workbench.

**Running-app check**: seeing Ember needs T246 (the user theme file wired into kiln) and T247 (workbench chrome
and open editors following the theme). The lead files that human check separately, or adds an Ember step to
T248.
