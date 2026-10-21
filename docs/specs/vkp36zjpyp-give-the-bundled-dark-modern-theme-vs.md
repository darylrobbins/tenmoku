---
task_uid: "01m5f7cc0pjnfnx0vkp36zjpyp"
task: "T258"
rev: 2
type: "bugfix"
complexity: "s"
criteria:
  - id: "C1"
    text: "vscode-dark-modern.toml has kiln_theme = 1 and every token group, and parses with no diagnostics"
    required: true
    manual: false
  - id: "C2"
    text: "surface.panel #181818, surface.background #1F1F1F, border.default #2B2B2B, tab.inactive #181818, tab.active #1F1F1F, accent.base #0078D4"
    required: true
    manual: false
  - id: "C3"
    text: "Editor and syntax colours are unchanged, and the existing Dark Modern tests pass"
    required: true
    manual: false
---

# Give the bundled Dark Modern theme VS Code Dark Modern's workbench chrome through kiln's token groups, as Dark 2026 has (Mac T1027)

## Problem (user bug: "the whole editor doesn't match the theme")

The user runs `theme.name = "Dark Modern"`. Only the editor follows it:
`crates/fe-config/themes/vscode-dark-modern.toml` says "The rest of the window's colours are Kiln
Dark's" and has no kiln token groups. Pixels from the user's screenshot:
- the title bar, tab bar, breadcrumbs, side bar, activity bar and status bar are Kiln Dark's #121418;
- the borders are #1F2228/#21242A;
- the editor is Dark Modern #1F1F1F.

VS Code Dark Modern's chrome (dark_modern.json at ab8f31f0, already credited in NOTICE/THIRD_PARTY):
- title bar, activity bar, side bar, status bar, panel, tabs bar and inactive tab: #181818;
- active tab #1F1F1F, with #FFFFFF text and a #0078D4 top border;
- inactive tab text #9D9D9D;
- every border (`titleBar`/`sideBar`/`statusBar`/`tab`/`panel` `.border`): #2B2B2B;
- focusBorder #0078D4, button #0078D4 (hover #026EC1);
- foreground #CCCCCC, descriptionForeground #9D9D9D;
- widget.border #313131, editorWidget #202020, input #313131, quickInput #222222, menu #1F1F1F;
- badge #616161, textLink #4DAAFC, icon #CCCCCC.

## Fix

Give `vscode-dark-modern.toml` the chrome the way T1017 did for Dark 2026
(`crates/fe-config/themes/vscode-2026-dark.toml`):
- add `kiln_theme = 1`;
- fill `[surface] [text] [border] [accent] [state] [status] [list] [tab] [scrollbar]` from the
  values above, plus dark_modern.json's list/scrollbar keys, or dark_vs/dark_plus defaults where
  dark_modern sets none, saying so in the header;
- update the header comment. Editor and syntax colours are unchanged.

Note: the user's VS Code is on Dark 2026, not Dark Modern. Setting `theme.name = "Dark 2026"` in
fastedit already gives matching editor, token and chrome colours (T1017). This card makes Dark
Modern complete for users who pick it.

## Tests

- New `crates/fe-config/tests/vscode_dark_modern_chrome.rs`:
  - the file parses with no diagnostics;
  - `tokens.surface.background`/`panel`, `border.default`, `tab.active`/`inactive` and
    `accent.base` equal the dark_modern.json values above;
  - they differ from Kiln Dark's (#121418 panel).
- Existing `vscode_dark_modern_theme.rs` and `bundled_themes.rs` stay green.
