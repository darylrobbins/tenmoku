---
task_uid: "01m74pvv2r21mbhfjzy0csdf6m"
task: "T347"
rev: 1
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "On the render path Tenmoku uses (Shell::render, the docks, Workbench::render), the activity rail, side panel, editor area and bottom dock are labelled landmarks and the status bar is a Region labelled \"Status bar\", with the roles section 6 gives"
    required: true
    manual: false
  - id: "C2"
    text: "Every IconButton::new and ToggleButton::new caller passes a label, and toggle buttons and controls carry their toggled and disabled states as section 6 specifies"
    required: true
    manual: false
  - id: "C3"
    text: "Tests in kiln/crates/kiln-ui/src/tests/a11y_chrome.rs and crates/fe-workbench/tests/a11y_landmarks.rs assert the landmarks' roles and labels and that every focusable control the recorder sees has a label; both pass, and cargo build -p kiln --examples succeeds"
    required: true
    manual: false
  - id: "C4"
    text: "The A11Y-1 dump or AT-SPI probe of a running Tenmoku window lists the five landmarks, quoted in the finish notes"
    required: true
    manual: false
  - id: "C5"
    text: "The finish notes list this card's section 12 human-check lines (H-numbers and text), for the A11Y-11 VoiceOver pass"
    required: true
    manual: false
---

# A11Y-3 Landmarks and labelled chrome: rail, side panel, editor area, dock and status bar regions, and a label on every icon and toggle button (T294 design) (Mac T1669)

# A11Y-3 Landmarks and labelled chrome

Spec: `docs/decisions/qenw9xxdzk-accessibility-design.md`, the **A11Y-3 Landmarks and labelled chrome** row of section 11 "Proposed card breakdown", and section 6 "Chrome semantics".
The user approved the design's platform path (section 3, option (a): gpui-pre 0.3.9's
AccessKit, no vendoring) and its first slice (section 10) as written on 2026-11-09.

Build what that row and those sections specify, with the tests section 9 "Testing strategy"
assigns (shape 1 for text elements, shape 2 for chrome). If the design's text differs from
this card's criteria, the design wins.

Platform: **Linux-OK**.

## Implementation notes

- **Landmarks** go through A11Y-2's `kiln_ui::node`, so the recorder sees them:
  - `ActivityRail` is the `Navigation` "Activity bar" (`kiln-ui-activity-bar`).
  - A side dock's panel column (`dock-column`) is a `Complementary` named by the shown panel's title; the bottom dock's column (`dock-bottom-column`) is the `Region` "Panel" (`kiln-shell-dock-panel`). Both exist only while the panel shows, so a hidden dock is no empty landmark. `DockState::render` gets no `Window` and `crates/fe-workbench/src/dock.rs` (out of scope) calls it, so dock.rs wraps the column in a private `Landmark` `RenderOnce`, which gets one, rather than changing the signature.
  - The shell's centre (`shell-centre`) is the `Main` "Editor" (`kiln-shell-editor`).
  - `StatusBarFrame` is the `Region` "Status bar" (`kiln-ui-status-bar`), never `Status` (section 6). `StatusBarFrame::region` gives the same landmark to whatever stands in the bar's place, which fe-workbench's `StatusBar` uses for vim's command line. It's an associated fn so `kiln-ui/src/lib.rs` needs no new export.
- **Buttons and controls**: `IconButton`, `ToggleButton`, `RailButton` and controls.rs's `Button`, `Checkbox` and `Switch` set their role, name and toggled state through `node`/`node_with` instead of GPUI's setters, so the recorder lists them. `Control` still adds AccessKit's disabled flag (`States` has no `disabled`, and `a11y.rs` isn't in scope); `b3_keyboard.rs` checks it. `RailButton` gains `shortcut()` (`aria_keyshortcuts`), which the dock fills from the keymap, and asserts a name like `IconButton`.
- **Required label** (review round 1, F3): `IconButton::new(id, icon, name, theme)`, `ToggleButton::new(id, caption, name, theme)` and `ToggleButton::icon_only(id, icon, name)` take the accessible name as an argument, and their `name()` setters are gone, so a caller can't leave one out; a blank name is still a debug assertion. A toggle's caption is what it shows (`Aa`), its name what a screen reader reads ("Match Case"); a toggle whose name isn't its caption shows the name in its tooltip. Every caller moved its `.name(..)` into the constructor, which reaches past the card's list: `crates/fe-workbench/src/title_bar.rs`, `crates/fe-workbench/src/run_widget.rs`, `kiln/crates/kiln-ui/src/tests/b3_controls.rs`, `kiln/crates/kiln-ui/src/tests/b3_keyboard.rs` and the gallery's `pages/controls.rs`. In `search_panel.rs` and `toast.rs` (Mac T1546 and T1556 overlap) only the button constructor calls changed, though rustfmt rewrapped some of the calls around them. The search panel's symbol toggles (`…`, `AB`, `×`, the options, Replace), the terminal find options and the gallery's chrome page (its rail buttons too) gained names.
- **Toggled icon buttons** (F2): `IconButton::selected(..)` makes the button a toggle, so a screen reader hears the find bar's and the search panel's Replace chevrons on or off. A button never given `selected` stays a plain button.
- **Caller labels** (F1): `Button`, `Checkbox` and `Switch` without a name or label set their role and state but not their label, so a label the caller set through `aria_label`, as the setup wizard does for its switches, stands. GPUI keeps that label `pub(crate)`, so the control can't read it to record it.
- **Out-of-scope files**: kiln-ui and kiln-shell lint every literal (`l10n-allow.toml`, `all = true`), so the four landmark names are catalogue messages: `kiln/crates/kiln-ui/l10n/en-US/kiln-ui.ftl` and `kiln/crates/kiln-shell/l10n/en-US/kiln-shell.ftl` gain two keys each.
- **Recorder caveat**: the recorder keeps only the last draw's nodes, so a view drawn from its cache (the status bar is) drops out. `a11y_landmarks.rs` reads a frame after `window.refresh()`, which draws every view.
- **C4, the running app**: `driver.sh a11y-tree` on Linux, with the Terminal rail button clicked so the bottom dock shows, lists five AT-SPI landmarks (role 110): Project, Activity bar, Editor, Panel, Status bar. The A11Y-1 dump of that window: `Complementary "Project"` (`dock-column`), `Navigation "Activity bar"` (`dock-rail`), `Main "Editor"` (`shell-centre`), `Region "Panel"` (`dock-bottom-column`), `Region "Status bar"` (`status-bar`), and the announcer still `Status`. Rail buttons are toggle buttons with `toggled` and `keyboard_shortcut` (Project: `Ctrl+Shift+E`). The driver's keys can't dismiss the trust prompt, so the run clicked Don't Trust first.
- **C5, for A11Y-11**: H1′ (Inspector, chrome: the rail, side panel, editor area and bottom dock show `AXLandmark*` subroles with labels, the status bar `AXLandmarkRegion` "Status bar", every button has a title) and H2′ (VO-U › Landmarks lists Activity bar, Explorer, Editor, Panel and Status bar, and choosing one moves the VoiceOver cursor there). The side panel's landmark takes its panel's title, which is "Project" in Tenmoku today where H2′ says "Explorer".
