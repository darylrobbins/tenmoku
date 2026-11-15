---
task_uid: "01m7b0mqw0kwxqyc7jr8shp5sk"
task: "T353"
rev: 1
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "Test in crates/fe-terminal-view/tests/read_only.rs: (own; the design gives no Tests line) a read-only view's keys don't reach the pty, while selection, copy, scroll, vi copy mode and find still work"
    required: true
    manual: false
  - id: "C2"
    text: "Test in crates/fe-workbench/src/terminal_lane_tests.rs: (own) a `Session { lane: Some(_), read_only: true }` tab is named `lane 2 · \u003ccommand\u003e` with silver-blue text and spinner and takes no typing; a terminal lane's own tab (read_only false) is silver-blue and writable"
    required: true
    manual: false
  - id: "C3"
    text: "Test in crates/fe-workbench/src/terminal_lane_tests.rs: § Colour rule: A surface card's test reads its colour from the theme's `agent.*` token and asserts it is not the accent's, in both themes"
    required: true
    manual: false
---

# Read-only agent terminals: TerminalView::set_read_only drops key input, and terminal sessions carry a lane with a silver-blue lane N tab (agent lanes card 13) (Mac T1808)

# Read-only agent terminals: TerminalView::set_read_only drops key input, and terminal sessions carry a lane with a silver-blue lane N tab (agent lanes card 13)

Source: `docs/designs/agent-lanes.md` § Card breakdown, card **13** (the agent lanes design, Mac T1567, on main as e553a094a). The design files no cards itself; this is its card 13. Read the sections it cites before starting (quoted below or named).

Linux-OK: nothing in the card needs macOS; it builds and tests on Linux.

## The card, verbatim from § Card breakdown

**Wave C, after 6 (all ∥ with each other unless a dependency says so):**

13. **Read-only agent terminals.** Size **s**. Deps: 0, 6. ∥. Files:
    `crates/fe-terminal-view/src/view.rs` (`set_read_only`),
    `crates/fe-workbench/src/terminal_panel.rs` (`Session { lane,
    read_only }`, the tab's colour and name).

### The breakdown's preamble, verbatim

Ordered; each card is at most m (about 2k lines with tests). **∥** marks
cards that can run in parallel with the others in the same wave once their
dependencies have landed. Files are where the card's change is; tests sit
beside them in each crate's `*_tests.rs` convention. Wave C's cards each
register commands or a panel through `workbench.rs` and the command
catalog, so parallel lanes there should expect small rebase conflicts in
those lists, not in each other's files.

## Since the design (Mac main at fe5d87fd7)

- **Card 0 has landed** as Mac T1509 (c79a9ec40), so this card's dependency on it is met. The tokens it gave differ in name from card 0's text: kiln's theme has `agent.base` and `agent.soft` (`kiln/crates/kiln-core/src/theme/tokens.rs`, `kiln/crates/kiln-core/themes/{dark,light}.toml`; no `agent.fg`), and fe-gpui's `EditorTheme` has only `agent_lane` (`crates/fe-config/themes/*.toml` `[editor] agent_lane`; no `agent`/`agent_soft`). Read the Colour rule's `agent.*` as these.
- Nothing opens a lane terminal until card 19 (`agent.new_terminal`); the tests make such a session directly.

## Design sections it cites, verbatim

### Terminal: the agent's own tab, silver-blue, read-only

- **Shows**: a terminal an agent runs commands in has its own tab, named
  `lane 2 · <command>`, its tab text and spinner silver-blue, and it takes
  no typing: keys don't reach the pty; selection, copy, scroll, vi copy mode
  and find still work. A terminal lane's own tab (the CLI agent the user
  talks to) is silver-blue and writable (open question 5).
- **Data**: `Session { lane: Option<LaneId>, read_only: bool, .. }`
  (`crates/fe-workbench/src/terminal_panel.rs`, today `Session { view,
  number, name, task }`), set when `LaneStore` opens the terminal (a terminal
  lane; later an agent task, or an external agent's `lane.run`); cwd the
  lane's worktree.
- **Reads it**: `terminal_panel.rs` (`fn tab_list`, the tab's colour and
  name) and `fe_terminal_view::TerminalView::set_read_only`
  (`crates/fe-terminal-view/src/view.rs`, new), which drops key input.
- **Gap**: no read-only mode in fe-terminal or fe-terminal-view today.

## Colour rule

Confirmed: **silver-blue (oil spot, `#b4c2cc` Yohen / `#3f6587` Hakuji)
is only ever an agent signal, and nothing an agent did is drawn in rust.**
Every lane surface takes its colour from the `agent.*` tokens (Mac T1509:
the agent signal and its soft ground, by theme), and no other surface does.

- **Silver-blue marks the agent's authorship or state**: the dotted change
  lane, the row tint, the `lane 2 · pending` chip, the title bar chip, the
  rail badge, the Problems and test chips, the agent terminal's tab, the
  Changes group's header, the diff's `After · lane 2` header, an agent
  toast's icon and its Review changes button.
- **Rust stays the user's**, also on agent things: the caret on an agent row,
  the focus ring on a lane chip or the Changes group, a selected row in the
  Agents panel (focused selection), the active tab line when the agent
  terminal is the active tab, the user's Accept all in review. These are
  where the user is, not who wrote it.
- **After accept** the text is still the agent's in provenance, so its rows
  keep the dotted silver-blue lane (no chip, no tint: those are for pending
  work). A row stays the agent's while any of its text is
  (`Author::row_rank` ranks AI above the user); one the user rewrites whole
  is theirs and loses the mark.
- **Added, removed, modified, warning** keep their hues on agent work: a
  pending hunk's new rows are celadon and its old rows shinsha, as any diff;
  silver-blue is the lane mark beside them, never a replacement for them.
- **No fallbacks to accent.** Until T1509's tokens land on this trunk, the
  surface cards don't start (§ Card breakdown, card 0). A surface card's
  test reads its colour from the theme's `agent.*` token and asserts it is
  not the accent's, in both themes.
- **Colour-blind mode** doesn't touch silver-blue: it swaps green and red,
  and the agent mark already differs from both in lightness and carries a
  glyph (the robot head, the dotted lane).

## Scope

- `crates/fe-terminal-view/src/view.rs`
- `crates/fe-terminal-view/tests/read_only.rs`: new test file, fe-terminal-view's `tests/` convention (beyond the card's file list).
- `crates/fe-workbench/src/terminal_panel.rs`
- `crates/fe-workbench/src/terminal_lane_tests.rs`: new test file, declared from `terminal_panel.rs` (beyond the card's file list).
- `crates/fe-workbench/l10n/en-US/terminal.ftl`: the `lane 2 · <command>` tab name (beyond the card's file list).
- `crates/fe-workbench/l10n-allow.toml`: fe-workbench's literal lint is `all = true` (beyond the card's file list).

## Criteria

Each Tests item of the card, verbatim, as a criterion; items marked "(own)" are mine where the design gives none.

1. Test in `crates/fe-terminal-view/tests/read_only.rs`: (own; the design gives no Tests line) a read-only view's keys don't reach the pty, while selection, copy, scroll, vi copy mode and find still work
2. Test in `crates/fe-workbench/src/terminal_lane_tests.rs`: (own) a `Session { lane: Some(_), read_only: true }` tab is named `lane 2 · <command>` with silver-blue text and spinner and takes no typing; a terminal lane's own tab (read_only false) is silver-blue and writable
3. Test in `crates/fe-workbench/src/terminal_lane_tests.rs`: § Colour rule: A surface card's test reads its colour from the theme's `agent.*` token and asserts it is not the accent's, in both themes

## Notes

- Ready tasks T1595, T1596 (`fe-terminal-view/src/view.rs`, `terminal_panel.rs`) and T1774 (`terminal_panel.rs`) touch the same files.
