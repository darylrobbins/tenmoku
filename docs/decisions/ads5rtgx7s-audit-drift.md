# UI audit drift on main (T1880)

`cargo run -p fe-workbench --example screenshots -- audit target/ui-audit`
exited 1 on main at bf4b4df14: 22 captures failed their token probes. They
come from 13 distinct probes, since most fail in both appearances and the
chrome scene is also captured once per bundled theme. After this card the
audit exits 0 on the Mac: 71 captures, every probe passing.

## What failed, and why

"Probe" means the probe's expectation or point was stale or never right, and
the UI is as intended. "Scene" means the scene didn't set up what its probe
assumed. No probe caught a UI regression, so no bug is filed.

| # | Scene (captures) | Probe | Expected | Found (dark / light) | Cause | Wrong | Fix |
|---|---|---|---|---|---|---|---|
| 1 | `chrome` (dark, light), `yohen`, `hakuji` | (600, 45), the tab strip right of the tabs | `surface.panel` | `#120e0b` / `#f2f2f4` = `surface.chrome` | 347372f56 (T1542) draws the pane's tab row on `surface.chrome`, as the design's anatomy says (chrome: title bar, rail, tab strip, status bar). chrome.rs's comment, last rewritten by 94d7c3aea (T1543) on a base without T1542, still said "still `surface.panel`". Only Yohen and Hakuji set `surface.chrome`. Every other bundled theme falls back to `surface.panel` (`Rule::Same`), which is why the other theme captures passed. | Probe | Expect `surface.chrome` |
| 2 | `settings` (dark, light) | (501, 354), the accent dot before Rail position | `accent.base` | `#ece3d5` / `#24282d`, label text | 728bb8fb0 (T1555) measured the scene on Linux and never ran it on the Mac (its spec says so). The page reads the user file from the config directory (`open::settings_path`, `TENMOKU_CONFIG_DIR`, isolated by the audit). The scene's settings go to `settings.toml` beside the project, so no row ever showed as changed. Then 8b2453cfa (T1581) added Colour-blind mode as the first Common row, which moved Rail position from 354 to 422. | Scene | A first step copies the scene's settings file to the user file. The probe is now (501, 422). The light capture now also shows the Hakuji card as chosen. |
| 3 | `shortcuts_editor` (dark, light) | (900, 140), below the search box | `surface.background` | `#e0874c` / `#a84a16` = the focus ring | 347372f56 (T1542) made tabs 34 px, not 30, so the page moved down 4 px. y = 140 now falls on the focused field's 2 px ring (bebcac80d, T1518), which spans 105–141. | Probe | (900, 146) |
| 4 | `changes` (dark, light) | (180, 125), the empty message box right of its placeholder | `surface.background` | `#231c18` / `#f6f7f9` = `[editor] active_line` | 888a3b9fb (T1553) adapted the point from git_panel's on Linux, "not yet checked by a capture" (its spec). The box has been an `EditorView` since a6cc74059 (T795), and its first row carries the editor's current-line band (111–131). | Probe | (180, 150), below the first row |
| 5 | `git_log` (dark, light) | (250, 465), the commit card | `surface.raised` | `#16110e` / `#f4f4f6`, the card's shadow over the project tree | 9acbe3f45 (T1548) moved the Git log into the bottom dock, so the card now floats over the dock (about 293–571 × 318–506). ca2431939 (T1551) wrote the sidebar coordinates on Linux, from a base before T1548. | Probe | (450, 487). The passing (174, 540) "panel below the graph" point was on the project tree, so it moved to (800, 520) in the bottom panel. |
| 6 | `git_log_card` (dark, light) | (250, 465), the commit card | `surface.raised` | as #5 | as #5. The scene is #5's twin, with the same steps. | Probe | as #5 |
| 7 | `problems_panel` (dark, light) | (400, 400), the editor below the code | `surface.background` | `#17120f` / `#f7f7f9` = `surface.panel` | 9acbe3f45 (T1548) put Problems in the bottom dock, which covers y ≥ 326 of the centre. | Probe | (700, 165), on the code's empty 4th row. The passing (174, 400) panel point was on the project tree, so it moved to (640, 560) in the dock. |
| 8 | `problems` (dark, light) | (253.5, 140), README.md's count badge | `list.row_hover` over `surface.panel` | `#17120f` / `#f7f7f9`, the project tree | 9acbe3f45 (T1548), as #7. 19a795869 (T1547) measured the rows in the sidebar. | Probe | (651, 403) |
| 9 | `problems` (dark, light) | (50, 164), the selected row's indent | `list.row_selected` over `surface.panel` | as #8 | as #8 | Probe | (290, 427) |
| 10 | `problems` (dark, light) | (400, 400), the editor | `surface.background` | `surface.panel` | as #7 | Probe | (700, 165). The two passing tree points, (174, 400) and (50, 212), moved into the dock: (700, 376) between the scope switch and the filters, and (290, 475), the warning row's indent. |
| 11 | `terminal_session` (dark, light) | (600, 351), the panel's header | `surface.panel` | `#120e0b` / `#f2f2f4` = `surface.chrome` | 9acbe3f45 (T1548) gave the bottom dock its own 34 px strip on `surface.chrome` (kiln-shell `dock.rs`). | Probe | Expect `surface.chrome` |
| 12 | `terminal` (dark, light) | (296, 377), the active tab's left padding | `tab.active` | `#8e877e` / `#848789`, the spinner's edge | 97e1fd192 (T1552) assumed a 248 px sidebar. It is 240 (`PANEL_WIDTH` since T63), so tabs start at 282 and the spinner at about 295. The probe never held on the Mac. | Probe | (288, 377). The comment now says 240 / 282. |

#8–#10 are three probes in one scene, so the table has 12 rows for the 13
probes. They fail in both appearances: 2 × 11 + 2 for the Yohen and Hakuji
theme captures of #1 = 22 failing captures, the "~22" the spec counted.

## Notes

- **Two patterns cover every failure.** The scene was measured on Linux and
  never checked by a Mac capture (#2, #4, #5/#6, #12, and #8–#10's
  coordinates). Or a B3 layout card moved a surface after the scene was
  written: the tab strip to chrome and 34 px (T1542), and Problems / Git
  log / Terminal into the bottom dock (T1548). The audit isn't a gate stage,
  so nothing caught either.
- **The commit box's first-row band** (#4) is the editor's current line,
  drawn because the box is an editor. It isn't a regression: the box has
  drawn it since T795. Whether an unfocused commit box should show it is a
  design question, so no bug is filed for it.
- **The Settings scene's user file stays** in the audit's isolated config
  directory after the scene. Later scenes don't read it, and it holds only
  `workbench.rail = "left"`, the default, plus the light theme mode after
  the light capture. Every later capture passes with it there.
