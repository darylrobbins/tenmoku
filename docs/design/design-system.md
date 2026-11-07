# Tenmoku design system

Written 2026-11-07 by Daryl Robbins (revision 18, direction B3). This is the
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

### Signals

| Role | Yohen | Hakuji | Means | Never |
| --- | --- | --- | --- | --- |
| Hare's fur (accent) | `#e0874c` | `#a84a16` | You: caret, focus ring, NORMAL mode, active tab, selected row, primary button | Decoration, a second accent |
| Oil spot | `#b4c2cc` | `#3f6587` | Agents: their lines, chips, terminals, toasts, review | Anything a person did |
| Celadon | `#6fd49a` | `#17773f` | Added, passed, INSERT mode | Modified |
| Shinsha | `#f2716f` | `#c0362c` | Removed, failed, errors | Warnings |
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
| Property | `#dccbb8` | `#3d444d` | Shino | |
| Variable | `#ede4d6` | `#1f2328` | ink | |
| Punctuation | `#a89a8c` | `#596069` | Iron | |
| Comment | `#839c86` | `#6a737d` | Wood ash | Italic |

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
`#b8a99a` / `#4b525b`, faint `#978777` / `#5f6670`.

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
| Title bar | Back/forward, project ▾, branch ▾ (side by side, no slash), agent chip, run configuration with run/stop/debug, search, settings gear | The agent chip is the global "agents working" indicator |
| Rail | Top: Project, Search, Changes, Agents. Bottom: Problems, Runs, Git log, Terminal | Left or right (`workbench.rail`); panels open on the rail's side |
| Rail selection | A tinted tab behind the icon that runs to the rail's panel-facing edge, with a 2 px rust bar on that edge | Flips with the rail side |
| Rail badges | Bottom-right corner, 2 px cut-out ring; colour = meaning (blue changes, silver-blue agents, red dot errors) | Never on top of the glyph |
| Sidebar | Project tree with Structure collapsed at its foot; or Changes; or Review | Structure, collapsed, still shows the symbol under the caret |
| Editor tab strip | Tabs (rust top line on the active tab, unsaved dot, pinned tab as an icon), then tab list ▾, split right, ⋯ | |
| Breadcrumbs | `fe-display › src › minimap.rs › MinimapTile › render` | Symbols in their syntax colours; clicking opens the outline |
| Bottom panel | Problems, Runs, Terminal, Git log | Own header, own tabs where it has them |
| Status bar | Left: Vim mode pill, the operation in progress (spinner, label, bar, count, +N more), error and warning counts. Right: language server, position, indent, encoding, language, notification bell | No branch here: it is in the title bar |

**Pane controls**, the same on every pane. Right-aligned in this order: the
pane's own actions, then ⋯ (pane actions), then — (hide). The ⋯ menu offers
split right ⌃w v, split down ⌃w s, maximize ⌃w z, move to new window, move to
the other split ⌃w x, pin tab, close other tabs ␣b o, reopen closed tab ␣b u.

**Icons.** 16 px line icons on a 24 grid, 1.7–1.8 stroke, in the text colour of
their state. File-type icons sit in the palette (Rust in muted kaki, TOML grey,
Markdown lapis); folders tint by role (source, tests, docs) and go neutral grey
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
- Debugger: breakpoints, variables, call stack
- Split editors with a VISUAL selection
- Three-way merge
- Welcome and empty window
- Move these tokens into the Tenmoku design system artifact, and render the
  canvas from tokens rather than generating Hakuji by colour mapping
