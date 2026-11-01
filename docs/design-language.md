# Tenmoku's design language

What the code does today, not what it should do. Every rule names the path and
symbol it comes from, written `path:symbol`, so it can be checked and so it goes
stale visibly. Things that disagree are listed under [Gaps](#gaps) and left
alone here; later cards pick them up. Pixel values are logical pixels. Counts
are from the tree when this was written (T1273), to show which values carry
weight, and aren't kept up to date.

Paths starting `kiln/` are the framework; the rest are the app.

## Colour tokens

A window's colours come from two tables, each mainly for its own surfaces:

- **kiln's semantic tokens** paint the chrome: panels, tabs, trees, bars, menus.
  They're declared in `kiln/crates/kiln-core/src/theme/tokens.rs:tokens!`, one
  group per theme file table (`TOKEN_GROUPS`), each key `group.name`
  (`TOKEN_KEYS`). `kiln/crates/kiln-ui/src/theme.rs:Theme` resolves them to
  GPUI's colours once per change, and widgets read them with `cx.theme().colors`.
  The older flat `kiln/crates/kiln-ui/src/theme.rs:UiTheme`, read with
  `ui_theme(cx)`, is a view of the same tokens (`Theme::ui_theme`).
- **The editor palette** paints the text area and what's drawn over it: a theme
  file's `[editor]` and `[syntax]` tables,
  `crates/fe-config/src/theme.rs:EditorPalette`, turned into
  `crates/fe-gpui/src/lib.rs:EditorTheme` by
  `crates/fe-gpui/src/theme.rs:EditorTheme::from_palette`.

The tables below list each token, what it's for, and, where a theme may leave it
out, what it follows (`kiln/crates/kiln-core/src/theme/tokens.rs:RULES`). "Root"
means no rule: every bundled base sets it.

### surface

What things sit on. `kiln/crates/kiln-core/src/theme/tokens.rs:Surface`.

| Token | For | If left out |
| --- | --- | --- |
| `surface.background` | The window's and the main content's background | Root |
| `surface.panel` | Side panels, bars and inactive tabs | Root |
| `surface.raised` | What floats over content: menus, popovers, palettes | `surface.background` 8% towards `text.primary` |
| `surface.overlay` | The dimming behind a modal | Root |

### text

`kiln/crates/kiln-core/src/theme/tokens.rs:Text`.

| Token | For | If left out |
| --- | --- | --- |
| `text.primary` | Body text in the chrome | Root |
| `text.muted` | Secondary text, placeholders and ignored entries | `text.primary` 50% towards `surface.background` |
| `text.disabled` | Text of what can't be used | Same as `text.muted` |
| `text.accent` | Links and accented labels | Same as `accent.base` |
| `text.on_accent` | Text and marks on an accent fill, as on a primary button | Root |

### border

`kiln/crates/kiln-core/src/theme/tokens.rs:Border`.

| Token | For | If left out |
| --- | --- | --- |
| `border.default` | Lines between regions | Root |
| `border.subtle` | Separators inside a panel | `border.default` 50% towards `surface.background` |
| `border.focused` | Around the control that has focus | Same as `accent.base` |

### accent

`kiln/crates/kiln-core/src/theme/tokens.rs:Accent`.

| Token | For | If left out |
| --- | --- | --- |
| `accent.base` | The accent colour | Root |
| `accent.hover` | The accent under the pointer | `accent.base` at alpha `d9` |
| `accent.subtle` | A tint of the accent, as behind a selection | `accent.base` at alpha `33` |

### state

Interaction states, as overlays on whatever is underneath. `kiln/crates/kiln-core/src/theme/tokens.rs:State`.

| Token | For | If left out |
| --- | --- | --- |
| `state.hover` | Under the pointer | Root |
| `state.pressed` | While pressed | Root |
| `state.selected` | The selection | Same as `accent.subtle` |
| `state.selected_inactive` | The selection in a view that doesn't have focus | Root |
| `state.focus_ring` | Around what has focus | `accent.base` at alpha `aa` |
| `state.drop_target` | Where a drag would drop | Same as `accent.base` |
| `state.dirty` | The mark on something with unsaved changes | Same as `status.warning` |
| `state.disabled` | The fill of a disabled control that's otherwise filled with a colour, such as a primary button: neutral, so its text reads | `surface.panel` 25% towards `text.muted` |

### status

`kiln/crates/kiln-core/src/theme/tokens.rs:Status`.

| Token | For | If left out |
| --- | --- | --- |
| `status.error` | Errors | Root |
| `status.warning` | Warnings | Root |
| `status.info` | Information | Same as `accent.base` |
| `status.success` | Success | Root |

### list

Rows in trees and lists. `kiln/crates/kiln-core/src/theme/tokens.rs:List`.

| Token | For | If left out |
| --- | --- | --- |
| `list.row_hover` | A row under the pointer | Same as `state.hover` |
| `list.row_selected` | The selected row | Same as `state.selected` |
| `list.row_selected_inactive` | The selected row while its list doesn't have focus | Same as `state.selected_inactive` |
| `list.row_focus_border` | The outline of the selected row while its list has focus | Same as `state.focus_ring` |
| `list.indent_guide` | The vertical lines marking a row's ancestor levels | Root |
| `list.chevron` | A row's disclosure chevron | `text.primary` 50% towards `text.muted` |
| `list.icon` | A leaf row's icon | Same as `text.primary` |
| `list.folder_icon` | A folder row's icon | Same as `accent.base` |

### tab

`kiln/crates/kiln-core/src/theme/tokens.rs:Tab`.

| Token | For | If left out |
| --- | --- | --- |
| `tab.active` | Behind the active tab | Same as `surface.background` |
| `tab.active_text` | The active tab's label | Root |
| `tab.inactive` | Behind an inactive tab | Same as `surface.panel` |

### scrollbar

A scrolling view's scrollbar. `kiln/crates/kiln-core/src/theme/tokens.rs:Scrollbar`.

| Token | For | If left out |
| --- | --- | --- |
| `scrollbar.thumb` | The thumb | `text.muted` at alpha `80` |
| `scrollbar.thumb_hover` | The thumb while it's hovered or dragged | `text.primary` at alpha `99` |

### Editor and syntax colours

`crates/fe-config/src/theme.rs:EditorColors` is the `[editor]` table, its keys in
`EditorColors::KEYS`, each with a doc comment there:

- Text area: `background`, `foreground`, `gutter_foreground`,
  `gutter_active_foreground`, `active_line`, `selection`, `cursor`.
- Diagnostics: `error`, `warning`, `information`, `hint`.
- Popovers: `popover_background`, `popover_border`.
- Vim jumps: `jump_label`, `jump_label_preview`, `jump_label_foreground`.
- Git and diffs: `change_added`, `change_modified`, `change_deleted`,
  `diff_added_row`, `diff_removed_row`, `diff_added_word`, `diff_removed_word`,
  `diff_filler`.
- Marks in the text: `indent_guide`, `indent_guide_active`, `whitespace`, `ruler`,
  `bracket_1`, `bracket_2`, `bracket_3`.
- Sticky scroll: `sticky_scroll_background`, `sticky_scroll_border`,
  `sticky_scroll_shadow`, `sticky_scroll_hover`.
- Vim's status segment: `vim_mode_normal`, `vim_mode_insert`, `vim_mode_visual`,
  `vim_mode_replace`, `vim_mode_command`, `vim_mode_foreground`.
- Minimap: `minimap_background`, `minimap_slider`, `minimap_slider_hover`,
  `minimap_slider_active`, `minimap_selection`, `minimap_find_match`,
  `minimap_error`, `minimap_warning`, `minimap_gutter_added`,
  `minimap_gutter_modified`, `minimap_gutter_deleted`.

`[syntax]` (`crates/fe-config/src/theme.rs:EditorPalette::syntax`) maps a highlight
capture's dotted name to a colour. A capture takes its longest listed prefix, so
`function.macro` falls back to `function`; a capture with none uses `foreground`
(`crates/fe-gpui/src/lib.rs:EditorTheme::syntax`).

### How the two tables relate

- **Neither wins over the other: they're separate keys.** `surface.background` and
  `[editor] background` don't affect each other, and a theme keeps them equal by
  writing both. All three Tenmoku themes do (`#16181d`, `#ffffff`, `#1c1814`).
- **The division has exceptions,** where a view takes from both tables:
  - `crates/fe-markdown-view/src/style.rs:MarkdownStyle::new` takes body text,
    background and code colours from the editor, and muted text, borders, links and
    code backgrounds from the chrome.
  - `crates/fe-workbench/src/terminal_panel.rs:style` builds the terminal's from the
    editor's theme.
  - `crates/fe-workbench/src/git_log.rs:FileColors::new` colours the log's file-change
    labels from the palette, in the chrome.
  - The status bar's vim segment and vim's command line are chrome in `vim_mode_*`
    colours: `crates/fe-gpui/src/pane_item.rs:vim_mode_fill`,
    `crates/fe-workbench/src/vim_line.rs:render_line`.
  - `crates/fe-gpui/src/diff_view.rs` draws its toolbar in `ui_theme(cx)`, in the
    editor crate.
  - `crates/fe-chat-view/src/view.rs` reads both.
- **What a theme may leave out.** A kiln token it omits keeps the base's colour
  unless its rule reads a token the theme changed, in which case it follows that
  (`kiln/crates/kiln-core/src/theme/tokens.rs:resolve`). An `[editor]` or `[syntax]`
  key it omits takes the bundled palette's for its appearance
  (`crates/fe-config/src/theme.rs:EditorPalette::for_theme`). An unknown `[editor]` key
  is dropped with a warning. `[syntax]` takes any well-formed capture name, a new one
  included, and warns only on a malformed name or a value that isn't a colour
  (`crates/fe-config/src/theme.rs:EditorPalette::syntax_colors`).
- **The Tenmoku themes.** `crates/fe-config/themes/tenmoku-dark.toml` and
  `crates/fe-config/themes/tenmoku-light.toml` set only `[editor]` and `[syntax]`;
  their chrome is `kiln/crates/kiln-core/themes/dark.toml` and
  `kiln/crates/kiln-core/themes/light.toml`.
  `crates/fe-config/themes/tenmoku-ember.toml` is complete, with kiln's tables as
  well.
- **Applying one.** `crates/fastedit/src/theme.rs` turns kiln's active theme into a
  palette and sets it with `crates/fe-gpui/src/theme.rs:set_editor_palette`.

### The token probe

`crates/fe-workbench/examples/screenshots/token_probe.rs` fails
`cargo run -p fe-workbench --example screenshots` when a colour in the workbench
scene bypasses the theme. It renders the scene under two probe themes that replace
every kiln token (`Tokens::map`) and every `[editor]` and `[syntax]` colour with a
distinct one. Theme A's channels are all at most 100 and theme B's all at least
155, so no blend of A's colours can land on a blend of B's. A pixel the same in
both captures (within the harness's `CHANNEL_TOLERANCE`, ±2/255) took nothing from
the theme.

It prints the count and each region's bounding box and writes
`target/screenshots/workbench_tokens.diff.png`, those pixels in magenta over the
dimmed A capture. It fails above `ALLOWED_FIXED_PIXELS`, which is **0**: everything
the scene draws follows the theme today. A new hard-coded colour there has to be
fixed, or named beside that constant and under [Gaps](#gaps) with the constant
raised. Like the other scenes it runs only on macOS. Unlike the golden comparison it
isn't excused by `SCREENSHOTS_ALLOW_OTHER_SCALE`, so it holds on CI's 1× Mac: the
allowance is in device pixels, and zero is zero at any scale.

It sees only the one scene, and only opaque literals; see [Gaps](#gaps). What it
measured is in `docs/decisions/77nkyz5y3r-token-probe.md`.

## Spacing

There's no spacing scale in code: each widget has its own constants or literals.
GPUI's helpers are in quarter-rems and nothing calls `set_rem_size`, so a rem is
GPUI's default 16 px: `px_1` is 4, `px_2` 8, `px_3` 12, `gap_0p5` 2.

**The de-facto scale is 2, 4, 6, 8, 12, 16, 24.** By use of the helpers: `gap_2`
(8) 51, `gap_1` (4) 34, `px_3` (12) 33, `px_2` (8) 31, `py_1` (4) 18, `py_2` (8) 13,
`px_1` (4) 11, `px_6` (24) 7, `gap_0p5` (2) 6.

### Row and bar heights

| Value | What | Source |
| --- | --- | --- |
| 18 | A settings row's line | `crates/fe-workbench/src/settings_editor.rs:LINE_HEIGHT` |
| 22 | A view's header in a stack | `kiln/crates/kiln-shell/src/view_stack.rs:VIEW_HEADER_HEIGHT` |
| 22 | A peek's header | `crates/fe-workbench/src/peek.rs:HEADER_HEIGHT` |
| 22, 26, 32 | Small, medium and large buttons | `kiln/crates/kiln-ui/src/controls.rs:ButtonSize::height` |
| 24 | Every list row; the status bar | `kiln/crates/kiln-ui/src/theme.rs:ROW_HEIGHT` |
| 24 | The breadcrumbs' toolbar | `kiln/crates/kiln-ui/src/breadcrumbs.rs:TOOLBAR_HEIGHT` |
| 24 | A settings select | `crates/fe-workbench/src/settings_editor_controls.rs:SELECT_HEIGHT` |
| 26 | A shortcuts row | `crates/fe-workbench/src/shortcuts_editor.rs:LINE_HEIGHT` |
| 28 | The menu bar | `kiln/crates/kiln-ui/src/menu_bar.rs:MENU_BAR_HEIGHT` |
| 30 | The tab bar | `kiln/crates/kiln-ui/src/tab_bar.rs:TAB_HEIGHT` |
| 30 | A settings control | `crates/fe-workbench/src/settings_editor.rs:CONTROL_HEIGHT` |
| 32 | The title bar | `kiln/crates/kiln-platform/src/window_options.rs:TITLE_BAR_HEIGHT` |
| 36 | The activity rail's width, and its square buttons | `kiln/crates/kiln-ui/src/activity_rail.rs:RAIL_WIDTH` |

### Paddings and gaps

| Value | What | Source |
| --- | --- | --- |
| 2 | Between a tree row's chevron and icon | `kiln/crates/kiln-ui/src/tree_row.rs:CHEVRON_GAP` |
| 4 | A tree row's selection box inset from the list | `kiln/crates/kiln-ui/src/tree_row.rs:BOX_INSET` |
| 6 | A tree row's left padding; its gaps around icon, badge and detail | `kiln/crates/kiln-ui/src/tree_row.rs:PADDING`, `ICON_GAP`, `BADGE_GAP`, `DETAIL_GAP` |
| 14 | One level of tree indent | `kiln/crates/kiln-ui/src/tree_row.rs:TREE_INDENT` |
| 8 or 12 | A status bar entry, plain or filled | `kiln/crates/kiln-ui/src/status_bar.rs` (`px_2`, `px_3`) |
| 4, 10, 12, 16, 24 | Settings: row gap, row padding, row inset, section gap, control gap | `crates/fe-workbench/src/settings_editor_layout.rs:ROW_GAP`, `ROW_PADDING`, `ROW_INSET`, `SECTION_GAP`, `CONTROL_GAP` |
| 26, 16 | Markdown: around the column, between blocks | `crates/fe-markdown-view/src/style.rs:MarkdownStyle::PADDING`, `BLOCK_GAP` |
| 20, 5 | Markdown table cells, across and down | `crates/fe-markdown-view/src/table.rs:CELL_PAD_PX`, `CELL_PAD_Y_PX` |
| 16 | The commit card's margin | `crates/fe-workbench/src/git_log.rs:CARD_MARGIN` |
| 8 | A hover popover's distance from the window's edge | `crates/fe-gpui/src/hover.rs:WINDOW_MARGIN` |
| 48 | Above the palette | `kiln/crates/kiln-ui/src/palette.rs:PaletteFrame` (`pt`) |

### Widths and handles

| Value | What | Source |
| --- | --- | --- |
| 600 | The palette | `kiln/crates/kiln-ui/src/palette.rs:PALETTE_WIDTH` |
| 420 | A prompt | `kiln/crates/kiln-ui/src/prompt.rs:PROMPT_WIDTH` |
| 240 | The project panel; a peek's list | `crates/fe-workbench/src/project_panel.rs:PANEL_WIDTH`, `crates/fe-workbench/src/peek.rs:LIST_WIDTH` |
| 480 | The commit card | `crates/fe-workbench/src/git_log.rs:CARD_WIDTH` |
| 880 | The Markdown column at its widest | `crates/fe-markdown-view/src/style.rs:MarkdownStyle::MAX_WIDTH` |
| 760 | The settings column | `crates/fe-workbench/src/settings_editor_layout.rs:COLUMN_WIDTH` |
| 8 | A scrollbar | `kiln/crates/kiln-ui/src/scroll_area.rs:SCROLLBAR_WIDTH` |
| 5 | Resize handles of panes, docks and stacked views | `kiln/crates/kiln-shell/src/pane_group.rs:HANDLE`, `kiln/crates/kiln-shell/src/dock.rs:HANDLE`, `kiln/crates/kiln-shell/src/view_stack.rs:DIVIDER_HANDLE` |
| 6 | A split pane's handle | `kiln/crates/kiln-ui/src/split_pane.rs:HANDLE` |
| 1, 2 | Pane borders; a peek's border and a Markdown table's | `kiln/crates/kiln-shell/src/pane_group.rs:BORDER`, `crates/fe-workbench/src/peek.rs:BORDER`, `crates/fe-markdown-view/src/table.rs:TABLE_BORDER_PX` |

### Radii and shadows

GPUI's `rounded_sm` is a quarter rem, 4 px, and `rounded_md` is 6 px
(the `gpui-pre-macros` crate's border radius tokens).

| Value | Uses | What |
| --- | --- | --- |
| 4 | `rounded_sm` 23, `rounded(px(4.))` 5 | Small controls, icon buttons, crumbs, a tree row's selection (`kiln/crates/kiln-ui/src/tree_row.rs:TreeRow`), code blocks |
| 6 | `rounded_md` 18 | What floats: the palette, prompts (`kiln/crates/kiln-ui/src/palette.rs:PaletteFrame`, `kiln/crates/kiln-ui/src/prompt.rs`) |
| 3 | `rounded(px(3.))` 3 | See [Gaps](#gaps) |
| Full | `rounded_full` 5 | Avatars, dots, a switch's knob |

Shadows are `shadow_lg` on the palette, prompts, the menu panel and the shortcuts
dialog, and `shadow_md` on toasts and the settings select's menu. Sticky scroll's
is a gradient 3 px high (`crates/fe-gpui/src/element/sticky_scroll.rs:SHADOW_HEIGHT`),
in the theme's `sticky_scroll_shadow`.

## Type

### Fonts

- **Chrome:** the system UI font. GPUI's default text style is `.SystemUIFont`, and
  nothing in the chrome sets another. `crates/fe-gpui/src/hover.rs:UI_FONT` names
  it where editor popovers need to go back to it.
- **Editor:** `editor.font_family`
  (`crates/fe-config/src/settings.rs:EditorSettings`), unset by default, which takes
  the first installed of `kiln/crates/kiln-platform/src/fonts.rs:MONO_FAMILIES`
  (JetBrains Mono, SF Mono, Menlo, DejaVu Sans Mono, Ubuntu Mono, Liberation Mono,
  Noto Sans Mono), or else `mono_last_resort`.
- **Terminal:** `terminal.font_family` and `font_size`
  (`crates/fe-config/src/settings.rs:TerminalSettings`), the editor's when unset.

### Sizes and weights by role

| Role | Size | Weight, line height | Source |
| --- | --- | --- | --- |
| Editor text | 13, from settings | Line height 1.6 | `crates/fe-config/src/settings.rs:EditorSettings` |
| Editor popovers, completion, which-key | The editor's | | `crates/fe-gpui/src/completion_menu.rs`, `crates/fe-gpui/src/hover.rs`, `crates/fe-gpui/src/vim/which_key.rs` |
| Completion label | The editor's | Medium | `crates/fe-gpui/src/completion_menu.rs` |
| Terminal | The editor's | Line height 1.25 | `crates/fe-terminal-view/src/style.rs:TerminalStyle::LINE_HEIGHT` |
| kiln's body text | 13 | | `kiln/crates/kiln-shell/src/root.rs` |
| Tenmoku's chrome (tabs, panels, status bar, title bar inherit it) | 12 | | `crates/fe-workbench/src/workbench.rs:Workbench::render` |
| Panel rows | 12 | | `crates/fe-workbench/src/project_panel.rs`, `crates/fe-workbench/src/git_panel.rs`, `crates/fe-workbench/src/outline_panel.rs`, `crates/fe-workbench/src/problems_panel.rs`, `crates/fe-workbench/src/hierarchy_panel.rs`, `crates/fe-workbench/src/references_panel.rs`, `crates/fe-workbench/src/find_bar.rs`, `crates/fe-workbench/src/terminal_find.rs` |
| A filled status segment | Inherits 12 | Semibold | `kiln/crates/kiln-ui/src/status_bar.rs:StatusEntry` |
| Palette | 12 | | `kiln/crates/kiln-ui/src/palette.rs:PaletteFrame` |
| Tooltip | 12 | | `kiln/crates/kiln-ui/src/tooltip.rs:Tooltip` |
| Buttons | 11, 12, 13 by size | | `kiln/crates/kiln-ui/src/controls.rs:ButtonSize::text` |
| Prompt | 13, its message 14 | | `kiln/crates/kiln-ui/src/prompt.rs:Prompt` |
| Toast | `text_sm` 14, its detail `text_xs` 12 | | `crates/fe-workbench/src/toast.rs` |
| Settings page heading | 15 | Semibold | `crates/fe-workbench/src/settings_editor_layout.rs` |
| Settings row text; secondary text | 12; 11 | Semibold titles | `crates/fe-workbench/src/settings_editor_layout.rs` |
| Git log labels, avatar initials | 11 | Bold on `HEAD` and initials | `crates/fe-workbench/src/git_log.rs` |
| Commit card | 12 | Bold author and summary | `crates/fe-workbench/src/git_log.rs:CARD_TEXT` |
| Dense two-line rows | 11 over 10 | | `crates/fe-workbench/src/extensions_picker.rs`, `crates/fe-workbench/src/config_problems.rs` |
| Markdown body | 14 | Line height 1.6 | `crates/fe-markdown-view/src/style.rs:MarkdownStyle::new` |
| Markdown headings | h1 to h6: 2, 1.5, 1.25, 1, 0.875, 0.85 em | Semibold; h6 muted | `crates/fe-markdown-view/src/style.rs:MarkdownStyle::heading_size`, `HEADING_WEIGHT`, `heading_color` |
| Markdown code | The editor's | The editor's line height | `crates/fe-markdown-view/src/style.rs:MarkdownStyle::code_font` |
| Hover and completion docs | The editor's less 1 | Line height 1.4 | `crates/fe-markdown-view/src/style.rs:MarkdownStyle::compact` |

Bold otherwise marks matched text and the argument being typed
(`kiln/crates/kiln-ui/src/highlighted_label.rs`,
`crates/fe-gpui/src/signature_help.rs`), and vim's jump labels
(`crates/fe-gpui/src/element.rs`). A preview tab's label is italic
(`kiln/crates/kiln-ui/src/tab_bar.rs:Tab`).

By use, the literal sizes are 12 (24 times), 11 (8), 13 (4), 10 (2), 14 (1) and 15
(1).

## Icons

- **Set:** Lucide, unmodified, 68 SVGs in `kiln/crates/kiln-ui/assets/icons/`, named
  by `kiln/crates/kiln-ui/src/file_icons.rs:Icon`. `FILE_NAMES` and `EXTENSIONS` map a
  file to its icon. An app can swap the set with `set_icon_set`
  (`kiln/crates/kiln-ui/src/file_icons.rs:IconSet`, default `LucideIcons`).
- **Licence:** ISC, with Feather's MIT for the icons derived from it:
  `kiln/crates/kiln-ui/assets/icons/LICENSE`, `THIRD_PARTY.md`, `NOTICE`.
- **Drawing:** monochrome strokes, which GPUI's `svg()` draws as an alpha mask in the
  element's text colour (`kiln/crates/kiln-ui/src/file_icons.rs`). So an icon's colour
  is always a `text_color` call, and an icon has no colours of its own.

### Sizes and tints

| Size | Where | Tint | Source |
| --- | --- | --- | --- |
| 18 | Activity rail | `tab.active_text` highlighted, else `list.icon` | `kiln/crates/kiln-ui/src/activity_rail.rs:ICON_SIZE`, `RailButton` |
| 16 | Tree rows | `list.icon`, `list.folder_icon`; chevron `list.chevron`; an unreadable row `status.error` | `kiln/crates/kiln-ui/src/tree_row.rs:ICON_SIZE`, `TreeRow` |
| 16 | Icon buttons (in a 22 box) | `list.icon`, disabled `text.muted` | `kiln/crates/kiln-ui/src/icon_button.rs:IconButton` |
| 14 | Status bar | The entry's | `kiln/crates/kiln-ui/src/status_bar.rs:ICON_SIZE` |
| 14 | Tabs | `list.icon` | `kiln/crates/kiln-ui/src/tab_bar.rs:Tab` |
| 14 | Crumbs | `list.icon` | `kiln/crates/kiln-ui/src/breadcrumbs.rs` |
| 14 | Button icon | The button's text | `kiln/crates/kiln-ui/src/controls.rs` |
| 14 | Window controls | `text.muted` | `kiln/crates/kiln-shell/src/window_frame.rs` |
| 12 | Crumb separators | `text.muted` | `kiln/crates/kiln-ui/src/breadcrumbs.rs` |
| 11 | Git log ref labels | The label's | `crates/fe-workbench/src/git_log.rs` |
| 11 | A Markdown task's check (an image, not `svg()`) | One of two baked SVGs, by the link colour's lightness | `crates/fe-markdown-view/src/blocks.rs` |
| 10 | A checkbox's check | `text.on_accent` | `kiln/crates/kiln-ui/src/controls.rs` |

## Motion

**None.** There's no animation with a duration or an easing anywhere: no
`with_animation` or `Animation::new` in `crates/` or `kiln/crates/`. Things appear
and disappear in one frame, a theme change included
(`kiln/crates/kiln-ui/src/theme.rs:activate`). The caret doesn't blink, and a
terminal's request to blink is ignored
(`crates/fe-terminal/src/session.rs`, `CursorBlinkingChange`).

Two things look like motion and aren't animation:

- **Frames asked for to finish work,** with no timing or easing of their own: a
  layout still settling (`crates/fe-gpui/src/element.rs`), minimap tiles being drawn
  (`crates/fe-gpui/src/element/minimap.rs`), a terminal scrolling while a drag is held
  past its edge (`crates/fe-terminal-view/src/element.rs`).
- **Delays before something appears or leaves,** which then happens at once:

| Delay | What | Source |
| --- | --- | --- |
| 200 ms | Which-key's popup | `crates/fe-gpui/src/vim/which_key.rs` (`delay`) |
| 300 ms | A hover popover showing, and hiding | `crates/fe-gpui/src/hover.rs:HOVER_DELAY`, `HOVER_HIDE_DELAY` |
| 500 ms | A tooltip | GPUI's `DEFAULT_TOOLTIP_SHOW_DELAY` |
| 1.5 s | The commit card's copy button showing a check | `crates/fe-workbench/src/git_log.rs:COPIED_FOR` |
| 6 s | A toast | `crates/fe-workbench/src/toast.rs:LIFETIME` |

## Gaps

Found while writing this. None is fixed here. The probe measures **no** hard-coded
pixel in the workbench scene (`ALLOWED_FIXED_PIXELS` is 0), so there's no region to
list for it; the hard-coded colours below are in things that scene doesn't draw.

### What the probe can't see

- Anything the one scene doesn't draw. It has the tree, tabs, breadcrumbs, editor,
  minimap, rail and status bar, but no other panel, dialog, terminal, Git view or
  Markdown preview: `crates/fe-workbench/examples/screenshots/scenes.rs:capture`.
- A translucent hard-coded colour, whose blend with the theme differs between the
  captures: `crates/fe-workbench/examples/screenshots/token_probe.rs:identical`.

### Hard-coded colours

- The lightbulb's `#FFCC00` and `#3794FF`: `crates/fe-gpui/src/element.rs:paint_lightbulb`.
- The Git graph's five lane colours: `crates/fe-workbench/src/git_log.rs:GRAPH_COLORS`.
- White initials on a hashed avatar colour, whatever the theme:
  `crates/fe-workbench/src/git_log.rs:avatar_color`, and `gpui::white()` beside it.
- A hunk's backgrounds, "until the themes have diff colours", though they now do:
  `crates/fe-gpui/src/element/proposal.rs:ADDED`, `REMOVED`.
- The 16 ANSI colours, chosen by the background's lightness, not in any theme:
  `crates/fe-terminal-view/src/style.rs:TerminalStyle::ansi_dark`, `ansi_light`.
- A Markdown task's check is one of two SVGs with their colour baked in, picked by the
  link colour's lightness: `crates/fe-markdown-view/src/blocks.rs:CHECK_SVG_DARK`,
  `CHECK_SVG_LIGHT`.
- An image's checkerboard greys, computed from the background rather than a token:
  `crates/fe-gpui/src/image_viewer.rs:checker_colors`.
- Added files' letters turn the accent green with a fixed hue rather than use
  `status.success`: `crates/fe-workbench/src/git_panel.rs:ADDED_HUE`.
- A find match is `foreground` at a fixed alpha, with no key of its own:
  `crates/fe-gpui/src/element.rs:search_match_color`.
- A second copy of Kiln Dark's colours in literals, which match it today only
  because they're kept by hand: `kiln/crates/kiln-ui/src/theme.rs:UiTheme::default_dark`,
  `UI_OVERLAY`.

### Two colours for one role

- Git's change colours by table: the log's file labels take the palette
  (`crates/fe-workbench/src/git_log.rs:FileColors::new`), the panel's letters the chrome's
  tokens (`crates/fe-workbench/src/git_panel.rs:letter_color`).
- Warnings in Tenmoku Dark: `status.warning` `#d19a66`, `[editor] warning` `#e5c07b`.
  Light and Ember use one colour for both:
  `kiln/crates/kiln-core/themes/dark.toml`, `crates/fe-config/themes/tenmoku-dark.toml`.
- The selection's tint in Tenmoku Dark: `accent.subtle` at alpha `33`, `[editor]
  selection` at `55`; Light uses `33` for both:
  `crates/fe-config/themes/tenmoku-dark.toml`.
- Body text in the dark pair: `text.primary` `#8b8f98` in the chrome, `[editor]
  foreground` `#c8ccd4` in the text, and `tab.active_text` matching the second:
  `kiln/crates/kiln-core/themes/dark.toml`, `crates/fe-config/themes/tenmoku-dark.toml`.
- Two ways to read one token, `cx.theme()` and the older `ui_theme(cx)`, both in
  current use: `kiln/crates/kiln-ui/src/theme.rs:UiTheme`.

### Spacing

- Resize handles are 5 in three places and 6 in `kiln/crates/kiln-ui/src/split_pane.rs:HANDLE`.
- A header is 22 by two separate constants:
  `kiln/crates/kiln-shell/src/view_stack.rs:VIEW_HEADER_HEIGHT`,
  `crates/fe-workbench/src/peek.rs:HEADER_HEIGHT`.
- Rows beside `ROW_HEIGHT` with their own height: 26 in
  `crates/fe-workbench/src/shortcuts_editor.rs:LINE_HEIGHT`, 18 in
  `crates/fe-workbench/src/settings_editor.rs:LINE_HEIGHT`, and a literal 24 in
  `kiln/crates/kiln-ui/src/menu_bar.rs`.
- Off the scale, each once: `crates/fe-workbench/src/settings_editor_layout.rs:ROW_PADDING` 10,
  `crates/fe-markdown-view/src/table.rs:CELL_PAD_PX` 20,
  `crates/fe-markdown-view/src/style.rs:MarkdownStyle::PADDING` 26.
- Row indents as literals, each once: `pl(px(22.))` in
  `crates/fe-workbench/src/ai_changes_panel.rs`, `pl(px(44.))` in
  `crates/fe-workbench/src/problems_panel.rs`.
- A status bar entry's padding depends on its fill, 8 or 12:
  `kiln/crates/kiln-ui/src/status_bar.rs:StatusEntry`.
- `rounded(px(4.))` spells out `rounded_sm`:
  `kiln/crates/kiln-ui/src/tree_row.rs`, `crates/fe-markdown-view/src/blocks.rs`,
  `crates/fe-markdown-view/src/image.rs`, `crates/fe-gpui/src/image_viewer.rs`.
- A radius of 3, off the others, three times: `crates/fe-markdown-view/src/blocks.rs`,
  `crates/fe-gpui/src/file_viewer.rs`.

### Type

- Two chrome sizes: kiln's root says 13, Tenmoku's workbench 12, so a kiln widget that
  doesn't set its own differs by host: `kiln/crates/kiln-shell/src/root.rs`,
  `crates/fe-workbench/src/workbench.rs:Workbench::render`.
- No role names a size: every size is a literal at its use, as in
  `crates/fe-workbench/src/project_panel.rs`, `crates/fe-workbench/src/git_panel.rs`.
- Used once: 15 in `crates/fe-workbench/src/settings_editor_layout.rs`, 14 in
  `kiln/crates/kiln-ui/src/prompt.rs`.
- The only rem-sized text in the app, 14 and 12 by another route:
  `crates/fe-workbench/src/toast.rs` (`text_sm`, `text_xs`).
- 10 px, the smallest, in two dense lists only:
  `crates/fe-workbench/src/extensions_picker.rs`, `crates/fe-workbench/src/config_problems.rs`.

### Icons

- `ICON_SIZE` names three different sizes: 16 in `kiln/crates/kiln-ui/src/tree_row.rs`,
  14 in `kiln/crates/kiln-ui/src/status_bar.rs`, 18 in
  `kiln/crates/kiln-ui/src/activity_rail.rs`.
- Icons sized by a bare literal at the use, beside those constants, as in
  `kiln/crates/kiln-ui/src/tab_bar.rs:Tab`, `kiln/crates/kiln-ui/src/breadcrumbs.rs`,
  `kiln/crates/kiln-ui/src/controls.rs`.
- Sizes used at one or two places: 11 in `crates/fe-workbench/src/git_log.rs` and
  `crates/fe-markdown-view/src/blocks.rs`, 10 in `kiln/crates/kiln-ui/src/controls.rs`.
