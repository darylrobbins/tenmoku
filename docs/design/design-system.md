# Tenmoku design system

Written 2026-11-07 by Daryl Robbins (revision 19, direction B3). This is the
**should** for Tenmoku's look and feel; `docs/design-language.md` records what
the code does today, and the cards that implement this file bring the two
together.

## Principles

Tenmoku looks like one product, not a host for plugins: every pane, panel and
popup uses the same few parts, colours and keys. It sits in the JetBrains New
UI family (a real IDE, a rail, docked tool windows) and parts ways with it
where agents, Vim and review come in.

- **One meaning per colour.** Each hue carries exactly one job. The rust accent
  is you; silver-blue is agents; green, red, ochre and blue are added, removed,
  warning and modified. Nothing is decoration.
- **Agents are authors, not a feature.** Their lines, commits, terminals, tests
  and changes carry their mark everywhere, and their work reaches your tree
  only through review.
- **Keyboard first, mouse welcome.** Every pane action has a key, and menus
  show it, so the mouse teaches the keyboard. Vim's own ⌃w window commands are
  used where they exist; the ␣ leader fills the gaps.
- **One shape per job.** Builds, tests, scripts and agent tasks are all Runs.
  Every pane header ends the same way. Every setting row looks the same.
- **Calm by default.** Hints, guides and fold chevrons stay quiet until they
  are needed; notifications separate what needs you from what is only news.
- **The file is the truth.** Settings show their key in settings.toml; the
  diff's right side is the live, editable working copy.

All examples below come from the canvas Tenmoku visual directions, row B3.

## Colour

Two themes share one set of roles: **Yohen** (dark, warm iron-brown, the
signature; named for the kiln-transformed tenmoku bowls) and **Hakuji** (light,
white porcelain and cool greys, rust kept as the one brand thread). The closest
pair of roles in Yohen is 5.4 apart in OKLab ΔE, up from 1.2 before the retune;
every text colour clears 4.5:1 on every surface it sits on, and code text
clears 6:1 in the dark theme. The rust accent keeps the name hare's fur, after
the glaze it comes from.

T1512 (rev 2) moved five values by OKLab lightness only, keeping hue and
chroma, so the rules above hold: Yohen faint `#978777` → `#a09080` and comment
`#839c86` → `#88a18b` (4.5:1 on selected rows, 6:1 on the cursor line), Hakuji
comment `#6a737d` → `#69727c` (4.5:1 on the cursor line), and Hakuji shinsha
`#c0362c` → `#a1100e` and oil spot `#3f6587` → `#3a5f81` (0.05 OKLab L from
celadon, kiseto and hare's fur; kiseto can't darken in the sRGB gamut, so
shinsha moved for both of its pairs).

Yohen's comment moved from `#88a18b` to the neutral grey `#9e9b97`: chroma dropped
and lightness stayed at the 6:1 floor on the cursor line.

### Signals

| Role | Yohen | Hakuji | Means | Never |
| --- | --- | --- | --- | --- |
| Hare's fur (accent) | `#e0874c` | `#a84a16` | You: caret, focus ring, NORMAL mode, active tab, selected row, primary button | Decoration, a second accent |
| Oil spot | `#b4c2cc` | `#3a5f81` | Agents: their lines, chips, terminals, toasts, review | Anything a person did |
| Celadon | `#6fd49a` | `#17773f` | Added, passed, INSERT mode | Modified |
| Shinsha | `#f2716f` | `#a1100e` | Removed, failed, errors | Warnings |
| Kiseto | `#e8b440` | `#776a00` | Warnings, flaky tests | Modified |
| Ruri | `#4f95f5` | `#1f68cf` | Modified files and lines | Agent work |
| Jun | `#c7a5e6` | `#7b36a8` | VISUAL mode (shares the keyword hue) | Status |

Each signal has a soft ground for rows and badges (for example celadon-soft
`#18291f` / `#e3f3e8` behind added lines). A colour-blind mode swaps green and
red for blue and orange as its own setting, rather than weakening the default
for everyone.

### Syntax

| Token | Yohen | Hakuji | Glaze | Style |
| --- | --- | --- | --- | --- |
| Keyword | `#c7a5e6` | `#7b36a8` | Jun splash | |
| Function | `#f2c77a` | `#925a00` | Ame | Bold at its definition |
| Type | `#7fc9be` | `#0d6e66` | Seiji | |
| String | `#b5cf86` | `#2f6f1f` | Oribe | |
| Number, constant | `#f0a3c2` | `#a32f6b` | Peach bloom | |
| Macro, attribute | `#8db1f2` | `#2457c5` | Lapis | |
| Property | `#8db1f2` | `#2457c5` | Lapis | |
| Key | `#8db1f2` | `#2457c5` | Lapis | |
| Variable | `#ede4d6` | `#1f2328` | ink | |
| Punctuation | `#a89a8c` | `#596069` | Iron | |
| Comment | `#9e9b97` | `#69727c` | Wood ash | Italic |

A name that labels a value is Property: member access, field declarations,
named arguments and the keys written in object, struct and table literals.
Key (`string.special.key`) is only for the keys of data files. In Yohen and
Hakuji both are Lapis.

Brackets pair by depth in function, keyword and type colours. Mutable bindings
get a quiet solid underline in their own colour; warnings and errors use wavy
underlines in ochre and red.

### Surfaces

| Surface | Yohen | Hakuji | Used for |
| --- | --- | --- | --- |
| Chrome | `#120e0b` | `#f2f2f4` | Title bar, rail, tab strip, status bar |
| Tool window | `#17120f` | `#f7f7f9` | Sidebars, bottom panels |
| Editor | `#1c1613` | `#ffffff` | Code, terminal, diff panes |
| Cursor line | `#231c18` | `#f6f7f9` | Current line band |
| Hover | `#2b221d` | `#ebecef` | Rows and ghost buttons |
| Selected | `#2f2621` | `#e8eaee` | Selected row without focus, pressed segment |
| Focused selection | `#3e2717` | `#fbe7da` | Selected row with focus (rust tint + 2px rust edge) |
| Overlay | `#2a211c` | `#ffffff` + shadow | Popups, hover cards, menus, toasts |
| Line | `#2e251e` | `#e1e3e7` | Hairlines between regions |
| Control border | `#4a3a2e` | `#c6cad0` | Inputs, secondary buttons |

Text is three steps on every surface: ink `#ede4d6` / `#1f2328`, muted
`#b8a99a` / `#4b525b`, faint `#a09080` / `#5f6670`.

## Type, spacing and sizes

Three families: **Instrument Sans** for the interface, **Spline Sans Mono** for
code and anything you could type, **Instrument Serif** only for page titles
(Settings, welcome). Interface text is sentence case, never all caps.

| Style | Family | Size / line | Weight | Used for |
| --- | --- | --- | --- | --- |
| Page title | Serif | 30 / 36 px | 400 | Settings, welcome, empty window |
| Section heading | Sans | 15 / 20 px | 600 | Settings sections |
| UI heading | Sans | 13 / 18 px | 600 | Panel and dialog headings |
| UI body | Sans | 13 / 18 px | 400 | Tree rows, tabs, menus, settings help |
| UI label | Sans | 12 / 16 px | 500 | Buttons, badges, status bar |
| UI small | Sans | 11 / 14 px | 500 | Timestamps, counts, key hints |
| Code | Mono | 13 / 20 px | 400 | Editor; line height fixed so splits and diffs align |
| Code small | Mono | 12 / 18–20 px | 400 | Terminal, diff panes, paths, setting keys |

| Measure | Value | Notes |
| --- | --- | --- |
| Title bar | 38 px | Project and branch side by side, no slash |
| Tab strip | 34 px | Panel tab strips match |
| Row | 24 px | Trees, lists, menus (28 px in menus and settings index) |
| Status bar | 24 px | |
| Rail | 42 px wide | 32 px tall items |
| Sidebar | 232–248 px | Resizable |
| Bottom panel | 250 px | Resizable |
| Controls | 26 px, 22 px small | |
| Spacing | 2, 4, 6, 8, 12, 16, 24, 40 px | 2 px base for dense chrome, 4 px grid for layout |
| Radius | 2 / 4 / 6 / 10 px | Find fills; buttons, tabs, rows; menus, cards; palette, toasts |

Only floating things cast a shadow (overlay: 0 12px 32px at 55% black in dark,
14% in light, plus a 1 px hairline). Panels are separated by hairlines, never
shadows.

## Workbench anatomy

The window is a title bar, a rail, a sidebar, the editor and one bottom panel,
framed by a status bar. Each region has one job and one place for its controls.

| Region | What it holds | Rules |
| --- | --- | --- |
| Title bar | Back/forward, project ▾, branch ▾ (side by side, no slash), agent chip, the run widget (a task or debug configuration ▾, run or stop, debug; while debugging, the session's controls: continue or pause, the three steps, restart, stop), search, settings gear | The agent chip is the global "agents working" indicator; without a title bar the session's controls float (see Debugger) |
| Rail | Top: Project, Search, Changes, Agents. Bottom: Problems, Runs, Debug, Git log, Terminal | Left or right (`workbench.rail`); panels open on the rail's side. Debug joins once the root has a debug configuration or a session starts |
| Rail selection | A tinted tab behind the icon that runs to the rail's panel-facing edge, with a 2 px rust bar on that edge | Flips with the rail side |
| Rail badges | Bottom-right corner, 2 px cut-out ring; colour = meaning (blue changes, silver-blue agents, red dot errors) | Never on top of the glyph |
| Sidebar | Project tree with Structure collapsed at its foot; or Changes; or Review | Structure, collapsed, still shows the symbol under the caret |
| Editor tab strip | Tabs (rust top line on the active tab, unsaved dot, pinned tab as an icon), then tab list ▾, split right, ⋯ | |
| Breadcrumbs | `fe-display › src › minimap.rs › MinimapTile › render` | Symbols in their syntax colours; clicking opens the outline |
| Bottom panel | Problems, Runs, Debug (Variables, Breakpoints, Console), Terminal, Git log | Own header, own tabs where it has them |
| Status bar | Left: Vim mode pill, the operation in progress (spinner, label, bar, count, +N more), error and warning counts. Right: language server, position, indent, encoding, language, notification bell | No branch here: it is in the title bar |

**Pane controls**, the same on every pane. Right-aligned in this order: the
pane's own actions, then ⋯ (pane actions), then — (hide). The ⋯ menu offers
split right ⌃w v, split down ⌃w s, maximize ⌃w z, move to new window, move to
the other split ⌃w x, pin tab, close other tabs ␣b o, reopen closed tab ␣b u.

**Icons.** 16 px line icons on a 24 grid, 1.7–1.8 stroke, in the text colour of
their state. Each language has its own glyph (Lucide's, with Tabler's marks
where Lucide has none), tinted by family: file-type icons sit in the palette
(Rust in muted kaki, the compiled languages jun, TOML grey, Markdown lapis); folders tint by role (source, tests, docs) and go neutral grey
in Hakuji. Agents use a small robot head; Runs a play triangle with list lines;
Changes a page with +/−.

## Editor

The editor carries most of the information density, so its layers are ranked:
code first, then problems and changes, then language-server annotations, then
guides that only appear on demand.

**Gutter**, left to right (81 px at 13 px code). Line numbers 38 px
right-aligned; problem dot 10 px (ochre warning, red error); fold column 16 px,
always reserved so code never shifts; change bar 3 px (blue modified, green
added, a red notch for removed, a dotted silver-blue lane for agent-written
lines); 10 px to the code.

**Folding.** Chevrons appear on every block opener only while the pointer is in
the gutter. The hovered one is drawn at full strength on a hover tile, its
closing line shows a small up-chevron, and its indent guide brightens. No
traced range lines. Collapsed blocks (to design) show a `{ … 9 lines }` pill
that takes the colour of any error or agent edit inside.

**Indent guides.** 1 px at every 4 columns inside leading whitespace only,
`#2c241e` / `#eceef1`. The guide of the block holding the caret, or of a
hovered fold, is one step brighter.

**Language-server formatting.**

- Inlay type hints as small grey pills: `budget: Budget`, `row: usize`.
- Closing-brace hints in faint italic after a block's last brace: `} fn render`.
- Mutable bindings underlined solid, thin, in their own colour.
- Warnings and errors: wavy underline, gutter dot, and an inline lens at the
  end of the line (ochre at 70%, red at full).
- Code lenses above tests in sans: `▷ Run test · Debug · passed in the last 8 runs`.
- Identifier highlight: the symbol under the caret and its other uses get an
  ochre-soft fill.

**Hover card** (K, or a resting pointer). Module path in faint mono, the
signature syntax-coloured with the name bold, prose docs, then a footer:
definition location, caller count, `gd definition · K pin`. The hovered token
gets a soft fill.

**Find and replace.** A row under the breadcrumbs, not a floating box. The find
field has a rust focus ring; match case, whole word and regex toggle inside it
(on = rust-soft tint, not a solid fill); "2 of 3", previous, next, in
selection. Replace adds Replace, Replace all, and Rename symbol instead when the
match is a symbol. The equivalent Vim command shows faintly on the right
(`/dirty`, `:%s/dirty/needs_paint/gI`): find and Vim search are one feature.
Current match uses find-match `#5a4314`; others ochre-soft.

## Panels and screens

Every example lives on the canvas as an artboard, most in both themes. Each
artboard is one component with switches (panel, sidebar, find, hover, rail
side, Vim mode), so any combination can be shown.

| Example | Artboard on the canvas | Also shows |
| --- | --- | --- |
| Problems | B3 · Problems panel, find and replace, agent toast | Find and replace, fold hover, inlay hints, toast |
| Problems in Hakuji | B3 · Hakuji (light): the same screen | The light theme |
| Runs: tests | B3 · Runs: a test run in progress | Structure expanded under Project |
| Runs: build | B3 · Runs: build timeline, notification inbox open | Inbox, INSERT mode |
| Terminal | B3 · Terminal as tabs, command blocks, LSP hover | Hover card |
| Changes and commit | B3 · Changes sidebar | Pane ⋯ menu open |
| Review diff | B3 · Reviewing an agent's change, and the same diff in Hakuji | Agent comment |
| Settings | B3 · Settings, and Settings after picking Hakuji | Rail on the right |
| Palette | Yohen, retuned | B2 and B3 colours side by side |
| Debugger: gutter | B3 · Debugger gutter, in Yohen and Hakuji | Every breakpoint state, the later conditional and logpoint glyphs, both frame arrows and bands, a breakpoint beside a problem dot, the hover tile |
| Debugger: run widget | B3 · Run widget with a session, in Yohen and Hakuji | The picker with tasks and debug configurations; the stopped session's controls; the floating toolbar without a title bar |
| Debugger: stopped | B3 · Debug tool window while stopped, in Yohen and Hakuji | Call Stack, Watch and Variables; the current line in the editor; the session's row in Runs and in the status bar |
| Debugger: breakpoints | B3 · Debug tool window: Breakpoints, in Yohen and Hakuji | Exception filters, a disabled and an unverified breakpoint, the empty state |
| Debugger: console | B3 · Debug Console, in Yohen and Hakuji | Mixed output, an expanded result, an error |
| Debugger: hover value | B3 · Hover value, in Yohen and Hakuji | A value with children above the language server's hover |

### Problems

Grouped by file, with a scope switch (This file, Project, From agents) and
severity filters that double as counts. The selected problem opens a detail
pane: code and source, a three-line snippet with the suggested fix as a green
line, then Apply fix ⌥↵, Ask an agent and Explain. Files an agent wrote carry a
"written by lane 2" chip.

### Runs: builds, tests and agent tasks in one list

Everything that runs has the same shape: a status icon, a mono title, a
one-line summary, and an age. A running item shows a thin rust progress line.
The detail depends on the kind:

- **Tests:** a split bar (green passed, red failed, track remaining), a test
  tree, and each test's last 8 results as small bars, so flaky tests stand out.
  A failure shows the assertion with left in red and right in green, a "failed
  3 of its last 8 runs, likely flaky" note, and Debug this test or Ask an agent.
- **Build:** a crate waterfall instead of a wall of log: fresh crates as thin
  lines, rebuilt ones as bars, the crate you edited in rust; messages beside
  it; Timeline or Output.

### Terminal

Terminals are tabs in the panel's own strip: active tab with a rust top line; a
spinner for one still running; a red mark for one that exited badly; an agent's
terminal in silver-blue, read-only. Each command and its output is a block with
a left edge in green or red, the exit code and duration on its prompt line, and
Send to an agent on failures. Paths in output are links; the live prompt shows a
faint history suggestion.

### Changes and commit

Your changes have checkboxes; an agent's changes sit in their own silver-blue
group with no checkboxes and a Review link, so unreviewed agent work cannot slip
into your commit. Above the message: live pre-commit checks from Runs (rustfmt,
clippy, tests). The message box has a 72-character counter and Draft from diff;
then Amend, Commit 1 file ⌘↵ and "and push".

### Review diff

Reviewing an agent's change. The left panel lists the task, files with viewed
checkboxes and a progress bar, and Accept all or Request changes. The diff is
side by side: red and green row tints, stronger tints on the exact words that
changed, a clear hatch where a line does not exist, collapsed unchanged runs.
Hunks are named by symbol (`impl MinimapTile › render · 4 changes`) with
Comment, Revert, Accept. The right side is the live working copy with the
language server running, so review catches errors; comments go to the agent as
its next instruction.

### Settings

One page in an editor tab (␣,), not a dialog. A serif title, a You / This
project switch, one search that also takes keys like `editor.font`. A section
index on the left marks sections you changed. Common comes first: theme cards
drawn in each theme's real colours (Yohen, Hakuji, Match system) with the
colour-blind switch under them, then rail position, editor font and size, Vim
mode, agent command permissions and notification level. Every row: label, its
key in settings.toml, one line of help, one control; a rust dot and Reset when
changed from default. Choices apply live: the example shows the rail already
moved to the right.

### Debugger

The debugger (`docs/decisions/r1b9dw3g0g-debugger.md`) fits into the parts that are
already here: its breakpoints sit over the line numbers, its controls are in the
title bar's run widget, its views are one more bottom tool window, and a session
is one more Run. There is no right dock and no debug layout. The calls behind
this section, and the script that checks its colours, are in
`docs/decisions/skgc7fe5z9-debugger-design.md`.

**Breakpoints over the line numbers.** The gutter stays 81 px (Editor, Gutter);
breakpoints get no lane. They draw in the line-number column: x 0–42 at 13 px
code, the 4 px inset and the 38 px of numbers. The column has two slots, and a
row with a glyph in either draws no line number; the glyph's tooltip and the
status bar's position give it, and the numbers above and below bracket it.
The problem dot, the fold column and the change bar don't move, so a breakpoint
and a problem dot sit side by side. Sizes are at 13 px code, on a 20 px row, and
scale with the code as the gutter does.

| State | Glyph | Slot | Colour |
| --- | --- | --- | --- |
| Enabled, verified (or no session) | Filled dot, 12 px | Breakpoint, x 14–26 | Breakpoint |
| Enabled, not yet verified | Ring, 12 px, 2 px stroke; tooltip is the adapter's message | Breakpoint | Breakpoint |
| Disabled | Filled grey dot, 12 px | Breakpoint | Disabled breakpoint |
| Current frame (the top frame, stopped) | Filled arrow pointing right, 12 × 10 px: a 6 px body and a 6 px point | Frame, x 29–41 | Hare's fur |
| Caller frame (selected in Call Stack) | The same arrow as an outline, 2 px stroke | Frame | Hare's fur |
| Conditional or hit count (later) | The state's dot or ring with a 6 × 2 px bar across its middle: cut out of a dot, drawn in the ring | Breakpoint | The state's |
| Logpoint (later) | A diamond 12 px corner to corner, filled, outlined or grey as the dot | Breakpoint | The state's |

Every state differs from the others in shape or in lightness, not only in hue:
filled against outlined, a dot against an arrow, and the disabled grey is at
least 0.13 OKLab L from the breakpoint red in both themes. A breakpoint the
adapter moved draws on the row the adapter reports for the session's lifetime.

- **Click target:** the whole line-number column of a row, 42 × 20 px at 13 px
  code, every display row of a wrapped line. A click toggles a breakpoint on
  that buffer row and doesn't move the caret; a press that moves before release
  is a drag, as in the rest of the gutter. Right-click opens the menu: Add
  Breakpoint, or Disable or Enable Breakpoint and Remove Breakpoint, and Run to
  Line while stopped; later Edit Condition…, Edit Hit Count… and Edit Log
  Message…. The keys (F9, ␣db) do the same at the caret, so the mouse is never
  the only way.
- **Hover:** the pointer over the column draws the fold chevrons' hover tile
  (the hover surface, 38 × 16 px, 4 px radius) behind the number, and after a rest
  the tooltip: "Add breakpoint", or the glyph's own text, with the key.
- **The current line:** while stopped, the top frame's row gets the current-line
  band and the selected caller frame's row the caller-frame band. Each spans the
  code area's width on every display row of the buffer row, and has a 2 px edge
  at its left, in the last 2 px of the gutter's gap to the code: hare's fur on
  the current line, muted text on a caller frame. A band is opaque and draws
  instead of the cursor line on its row; selections, find matches and the
  identifier highlight draw over it. The editor scrolls the row to the middle
  third on each stop and each frame selection. Without a gutter the bands and
  their edges still draw, the edge at the code's left.

**The run widget.** The title bar's run widget (`run_widget.rs`) chooses one
thing to run: a task or a debug configuration.

- **The picker** (Choose Task or Configuration…) has two groups under small
  faint headings: Tasks, as today, then Debug configurations, each row with the
  `bug` icon and its source as a faint suffix (launch.toml, detected,
  launch.json). Typing filters both.
- **Idle,** the widget is the label, ▷ and 🐞: with a task chosen, ▷ Run Task (■
  Stop Task while it runs) and 🐞 Debug…, which opens the picker at the debug
  configurations; with a debug configuration chosen, its label carries the
  `bug` icon and only 🐞 Debug shows, which starts it. With no debug
  configuration 🐞 isn't drawn.
- **While a session runs** the widget becomes the session's controls: the label
  names the configuration with the session's state icon (a spinner while
  running, `pause` while stopped), and its click opens the session menu (Show
  Debugger, Show Console, Show Adapter Log, Choose Task or Configuration…).
  Then, 22 px icon buttons 2 px apart: Continue or Pause in one place (both,
  side by side, while the session is uncertain), Step Over, Step Into, Step
  Out, 6 px of space, Restart, Stop. A task still running keeps its tab in the
  terminal and its row in Runs. Disabled states follow the session: no
  stepping while running, resuming or uncertain, and nothing but Stop while
  resuming.
- **Without a title bar** (Linux, and macOS without the unified title bar) the
  same controls float: a toolbar centred over the editor area, 4 px below the
  tab strip, on the overlay surface with the floating shadow and hairline, 6 px
  radius, 30 px tall (the 22 px buttons and 4 px padding), the configuration's
  label first. It shows while a session exists, and a title bar never has both.

**The Debug tool window.** Variables, Watch, Call Stack, Breakpoints and the
Debug Console are one tool window in the bottom panel, its rail icon (Lucide
`bug`) in the bottom group after Runs. The rail icon carries a hare's fur dot
while a session is stopped: it needs you. Its header is a 34 px panel tab strip:
Variables, Breakpoints and Console, then the session's label ▾, then the pane
controls. A session opens the tool window on Console once; its first stop
switches it to Variables unless you picked a tab since. Ending a session leaves
it where it is.

- **The Console is a tab here, not in the terminal panel.** It evaluates in the
  selected frame, so it sits beside the frames, and the terminal panel keeps to
  real terminals. The debuggee's own terminal stays a terminal tab (it has a
  pty).
- **Variables** is two columns, split by a hairline: Call Stack on the left (280
  px, resizable), Watch and Variables on the right. All rows are 24 px.
  - Call Stack: threads, each with its frames; a thread row reads `main · stopped:
    breakpoint` or `worker 2 · running`. A frame row is the function in small
    code with `minimap.rs:42` faint at its right; the top frame carries the filled
    arrow and the selected frame the focused selection, with the outlined arrow
    if it isn't the top. Frames with no source, or that the adapter calls
    subtle, are faint.
  - Watch: a group heading at the top of the right column, its expressions as
    tree rows, then an Add watch row (a small input, "Add a watch expression").
    A watch that fails shows its error in shinsha with ✕.
  - Variables: one root per scope, the first expanded, as a tree. A row is the
    name in property colour, `=`, the value in ink, both small code, and the
    type faint at the right; a long value ends in an ellipsis and its tooltip
    holds it all. Children page by 100, with a "Show 100 more" row.
- **Breakpoints**: Exception filters first, as checkboxes with the adapter's
  labels and descriptions as tooltips; then breakpoints grouped by file as
  Problems groups (file icon, path, count). A row is a checkbox (enabled), the
  state's glyph, `line 42`, and the line's code in faint small code; later its
  condition in mono after it. Selecting a row reveals the line.
- **Console**: output rows in small code, 18 px (its line height, not a 24 px
  row), by category: program output in
  ink, its error stream in shinsha with a 2 px shinsha left edge (as a failed
  command block), the adapter's own messages faint, important ones kiseto with
  ▲. Under them, an input (26 px, `›` prompt, history on ↑ ↓). An evaluation
  echoes `› expr` in muted, then its result as an expandable value row; an
  error in shinsha with ✕.
- **Empty states**, centred, in muted body text, at most one button:
  - no session: "Not debugging." and Debug ‹configuration›, or "No debug
    configurations." and Add launch.toml;
  - running: "Running. Pause to see variables and the call stack." and Pause;
  - a frame with no scopes: "No variables in this frame.";
  - no breakpoints: "No breakpoints. Click a line number or press F9.", and with
    no session "Exception filters show when a session starts.";
  - Console with no session: "Start a session to evaluate expressions."

**Everywhere else.**

- **Runs:** a session is a run of its own kind, with the `bug` icon, the
  configuration as its mono title and a one-line summary: `stopped at
  minimap.rs:42 · breakpoint`, `running`, `exited 0 in 4.2 s`. Running or
  stopped it has the thin rust progress line, without a fraction; stopped, its
  icon is `pause` in hare's fur. Its detail offers Show Debugger, Restart and
  Stop, and once it has ended, Debug Again and Show Output. A pre-launch task is
  its own run, before it.
- **Operation in progress:** a session is listed from launch to end: spinner and
  "Debugging ‹configuration›" while running, `pause` and "Paused in
  ‹configuration›" while stopped, no bar. Clicking its entry shows the Debug tool
  window.
- **Test lens:** `▷ Run test · Debug` (Editor, language-server formatting): Debug
  is a second item, after a faint `·`, that debugs that test (D13). It shows only
  where the test's framework can be debugged (Cargo and pytest first, Go once
  delve is supported); a module's `▷ Run tests` has no Debug.
- **Debug this test** in a test failure (Runs: tests) debugs the failed test as
  the lens does, with a one-shot breakpoint on the failure's line when it is in
  the project, so the session stops at the assertion.
- **Hover value:** while stopped, the hover card gets a section above the
  language server's: a faint `value · frame render` line, then `name: Type =
  value` in syntax colours, then up to 20 children as `field = value` rows in
  small code and "+N more in Variables" (it opens Variables at that value), a
  hairline, then the language server's hover as today. Values don't expand in
  the card.

**Colour roles.** Four new editor roles (theme keys in `[editor]`, D6). Bands are
checked against code text and glyphs against the gutter and the cursor line;
the check and its output are in the decision file.

| Role | Key | Yohen | Hakuji | Falls back to | Used for |
| --- | --- | --- | --- | --- | --- |
| Current line | `debug_current_line` | `#351501` | `#fff4ed` | `active_line` | The top frame's band, with a 2 px hare's fur edge |
| Caller frame | `debug_frame_line` | `#2a1a11` | `#f6f7f9` (the cursor line's) | `active_line` | A selected caller frame's band, with a 2 px muted edge |
| Breakpoint | `debug_breakpoint` | `#f2716f` | `#a1100e` | `error` | Enabled and unverified breakpoints (shinsha's value, its own role) |
| Disabled breakpoint | `debug_breakpoint_disabled` | `#827468` | `#7c828a` | `gutter_foreground` | Disabled breakpoints |

- Code text on both bands keeps its floor: comment, the lowest, is 6.03:1 and
  6.05:1 in Yohen, 4.52:1 and 4.56:1 in Hakuji. Each band replaces the cursor
  line, so these are the cursor line's numbers too.
- Every glyph is 3:1 or better wherever it sits: the gutter, the cursor line and
  the hover tile, and in the tool window's rows bare, hovered, selected and
  selected with focus. The lowest is the disabled breakpoint, 3.08:1 on a
  focused row in Yohen and 3.22:1 on a selected row in Hakuji. The edges clear
  3:1 on their bands.
- The comment's floor leaves the bands little room: on the cursor line it sits at
  6:1 in Yohen and 4.5:1 in Hakuji, so no band can be lighter in Yohen or darker
  in Hakuji. Hakuji's current line is 3.0 ΔE from the editor and its caller frame
  is the cursor line itself; the edge and the arrow carry them, not the fill.
- A breakpoint is the one shared value: it is red because every debugger's is,
  and it never sits in the problem column, so it is shinsha's colour under its
  own role, which a theme may set apart.

**Accessibility.** Every string is in `debug.ftl`; every control has a name.

| Control | Accessible name |
| --- | --- |
| Line-number column (tooltip and name) | "Add breakpoint, line 42"; "Breakpoint, line 42"; "Breakpoint, line 42, not bound: ‹adapter message›"; "Disabled breakpoint, line 42"; "Stopped here, line 42"; "Caller frame, line 42" |
| Run widget label | "Task: ‹label›", "Debug configuration: ‹name›", "Debug session: ‹name›, running" or ", stopped at line 42" |
| Run widget and toolbar buttons | Run Task, Stop Task, Debug, Debug…, Continue, Pause, Step Over, Step Into, Step Out, Restart, Stop Debugging; tooltips add the key |
| Floating toolbar | A toolbar named "Debug" |
| Rail | "Debug", and "Debug, stopped" while stopped |
| Tool window | Tabs Variables, Breakpoints, Console; trees "Call stack", "Watch", "Variables", "Breakpoints"; "Add a watch expression"; "Evaluate in ‹frame›"; the list "Console output" |
| Rows | Frame: "render, minimap.rs line 42, current frame"; variable: "budget, Budget, ‹value›"; breakpoint checkbox: "Breakpoint, minimap.rs line 42"; exception filter checkbox: the adapter's label |

Announcements, polite, one per change:

- "Debugging ‹configuration›" when a session starts;
- "Stopped at line N in ‹file›: ‹reason›" (breakpoint, step, pause, exception
  ‹text›, entry) on each stop, and "Running" on continue;
- "Frame K of M: ‹function›, line N" on a frame selection;
- "Breakpoint added at line N", "removed", "disabled", "enabled", and
  "Breakpoint at line N moved to line M" when the adapter moves one;
- "‹configuration› ended: exited with code N", or "Debugging stopped";
- a console result, read as its value.

**Sizes.** Every size above is a kiln-ui role (`roles.rs`) or one to add:

| Size | Role |
| --- | --- |
| Rows in every view but the Console's output | `height::ROW` (24) |
| Console output rows | `CODE_SMALL`'s line height (18) |
| Tool window header | `height::TAB_STRIP` (34) |
| Console input, Add watch | `height::CONTROL` (26) |
| Toolbar and run widget buttons | `height::CONTROL_SMALL` (22), 2 px apart (`space::S2`), 6 px between groups (`space::S6`) |
| Floating toolbar | New `height::TOOLBAR` (30): the buttons and `space::S4` padding; `radius::MENU`, `SHADOW_FLOAT`; 4 px (`space::S4`) below the tab strip |
| Run widget label | New `width::TITLE_LABEL_MAX` (200), today `run_widget.rs`'s `LABEL_MAX_WIDTH` |
| Call Stack column | New `width::CALL_STACK_DEFAULT` (280) |
| Icons | New `glyph::ICON` (16), the design's line icons |
| Dot, ring and diamond; the arrow's width; the same glyphs in rows | New `glyph::BREAKPOINT` (12) at 13 px code; the gutter scales the glyphs as its columns |
| The arrow's height | New `glyph::ARROW_HEIGHT` (10) |
| The arrow's point and body; the conditional bar's width | New `glyph::HALF` (6), half of `glyph::BREAKPOINT` |
| Rings, outlined arrows, the conditional bar's height, band and console edges | New `glyph::STROKE` (2) |
| The two slots' left edges in the number column | New fe-gpui gutter constants beside `GUTTER_INSET`: `GUTTER_BREAKPOINT_X` (14) and `GUTTER_FRAME_X` (29), so the frame slot ends 1 px before the problem column |
| Click target | `GUTTER_INSET` + `GUTTER_NUMBERS` (42) wide, `CODE`'s line height (20) tall, per display row |
| Hover tile | `GUTTER_NUMBERS` (38) wide, `CODE`'s line height less 2 × `space::S2` (16) tall, `radius::CONTROL` (4) |
| Type | `CODE_SMALL` for frames, values, console and the hover's children; `UI_BODY` for empty states; `UI_SMALL` for group headings and suffixes; `UI_LABEL` for the session label |

The gutter's columns are fe-gpui's (`GUTTER_INSET`, `GUTTER_NUMBERS` in
`element.rs`, pinned by `gutter_b3.rs`), not kiln-ui roles, since they scale
with the code. New icons from Lucide: `pause`, `redo-dot` (Step Over),
`arrow-down-to-dot` (Step Into), `arrow-up-from-dot` (Step Out); `bug`, `play`,
`square` and `rotate-ccw` (Restart) are in kiln-ui.

**Keys.** F5 start or continue, ⇧F5 stop, ⇧⌘F5 restart, F6 pause, F9 toggle a
breakpoint, F10 and F11 step over and into and ⇧F11 step out while stopped
(only then, so Linux keeps F10 and F11), ⌘F10 run to cursor. With the LazyVim
preset: ␣db breakpoint, ␣dc continue, ␣dO over, ␣di into, ␣do out, ␣dP pause,
␣dt stop, ␣du the Debug tool window, ␣dr the Console, ␣dk ␣dj up and down the
stack, ␣td debug the nearest test.

## Notifications and agents

Notifications come from Runs and agents, and say what they need from you.

- **Toast:** lower right above the panel, 340 px, overlay surface with the
  toast shadow. An icon in the source's colour, a title, one line, at most two
  actions (agent toasts: a silver-blue Review changes and Later), and a
  dismiss. No coloured left borders.
- **Inbox:** opened from the bell at the right end of the status bar (a rust
  dot when something needs you). Two groups: Needs you (tinted rows, each with
  one action) and Earlier. A Focus mode holds non-urgent items until you are
  idle.
- **Operation in progress:** the status bar's left side, right after the Vim
  mode: spinner, label, a 72 px bar, a count, and +N for other background
  tasks. Clicking lists them all.

### Agent provenance

Everywhere it appears:

| Where | How agent work shows |
| --- | --- |
| Title bar | "2 agents working" chip |
| Rail | Robot icon with a silver-blue count |
| Editor | Dotted silver-blue change lane, a faint row tint, an inline "lane 2 · pending" chip |
| Problems, tests | "written by lane 2" and "new, lane 2" chips; a From agents scope |
| Terminal | The agent's own tab, silver-blue, read-only |
| Changes | Its own group, no checkboxes, Review link |
| Diff | "After · lane 2" header; comments become its next instruction |

Silver-blue is never used for anything a person did, and nothing an agent did
is shown in rust.

## Keyboard, accessibility and open questions

**Keys the design shows.** ␣ opens which-key; ␣p problems, ␣r runs, ␣tf rerun
failed, ␣v mark viewed, ]f next file in review, ␣\` new terminal, ␣1–9 switch
terminals, ␣, settings, ␣o quick outline, K hover, gd definition, / find, n N
next and previous.

### Accessibility rules

- Text 4.5:1 or better on every surface it sits on, including selected rows;
  checked per theme.
- Colours that must be told apart also differ in lightness, and every signal
  carries a glyph (+, −, ✓, ✕, ▲) so meaning never rests on hue alone.
- Real buttons, inputs, tabs, trees and switches with labels; icon-only buttons
  have names; the focus ring is 2 px rust, offset 1 px.
- A colour-blind mode for diffs and signals, offered next to the theme picker.

### Still to design

- Collapsed fold pill
- Command palette and the shared popup list
- Completion popup with agent ghost text
- Inline agent proposal in the editor
- Split editors with a VISUAL selection
- Three-way merge
- Welcome and empty window
- Move these tokens into the Tenmoku design system artifact, and render the
  canvas from tokens rather than generating Hakuji by colour mapping
