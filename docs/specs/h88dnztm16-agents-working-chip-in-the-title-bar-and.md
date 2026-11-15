---
task_uid: "01m7g0gdztb7g64wh88dnztm16"
task: "T395"
rev: 1
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "Test in crates/fe-workbench/src/tests/title_bar.rs: (own; the design gives no Tests line) the chip shows `1 agent working` / `N agents working` from `LaneCounts::working`, after the branch widget, hidden at zero, and clicking it shows the Agents panel"
    required: true
    manual: false
  - id: "C2"
    text: "Test in crates/fe-workbench/src/tests/title_bar.rs: (own) where the platform draws the title bar, the same count is the `agents` status item, on the left after the operation in progress, hidden at zero"
    required: true
    manual: false
  - id: "C3"
    text: "Test in crates/fe-workbench/src/tests/title_bar.rs: § Colour rule: A surface card's test reads its colour from the theme's `agent.*` token and asserts it is not the accent's, in both themes"
    required: true
    manual: false
---

# Agents working chip in the title bar and the agents status item where the platform draws the title bar (agent lanes card 9) (Mac T1805)

# Agents working chip in the title bar and the agents status item where the platform draws the title bar (agent lanes card 9)

Source: `docs/designs/agent-lanes.md` § Card breakdown, card **9** (the agent lanes design, Mac T1567, on main as e553a094a). The design files no cards itself; this is its card 9. Read the sections it cites before starting (quoted below or named).

Linux-OK: nothing in the card needs macOS; it builds and tests on Linux.

## The card, verbatim from § Card breakdown

**Wave C, after 6 (all ∥ with each other unless a dependency says so):**

9. **Title bar chip and `agents` status item.** Size **s**. Deps: 0, 6. ∥.
   Files: `kiln/crates/kiln-shell/src/title_bar.rs` (trailing slot),
   `crates/fe-workbench/src/title_bar.rs`, `crates/fe-workbench/src/status_bar.rs`,
   `crates/fe-workbench/src/tests/title_bar.rs`.

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
- Since the design, T1538 gave kiln-shell's `TitleBar` slots: `TitleBarItems { left, right }` (`kiln/crates/kiln-shell/src/title_bar.rs`), and the workbench fills `left` with back/forward, the project widget and the branch widget. So the card's `TitleBar::trailing` is not needed and `kiln/crates/kiln-shell/src/title_bar.rs` is dropped from the scope: the chip goes in the left slot after the branch widget. If a kiln change turns out to be needed, respec.
- **Dependency added: card 10.** "Clicking shows the Agents panel" needs card 10's `DockPanel::Agents`, which the design runs in parallel; this card waits for it rather than leave the click unwired. (Deviation from the design's Deps line, said here.)

## Design sections it cites, verbatim

### Title bar: "2 agents working" chip

- **Shows** `1 agent working` / `N agents working`, silver-blue, after the
  branch; hidden at zero. Clicking shows the Agents panel.
- **Data**: `LaneCounts::working`, from `LaneStore::counts()`.
- **Reads it**: `crates/fe-workbench/src/title_bar.rs`, which today makes
  only the root's name (`title_bar::name`) for kiln-shell's `TitleBar`
  (`kiln/crates/kiln-shell/src/title_bar.rs`, `TitleBar { title }`).
  `TitleBar` gains a trailing slot (`TitleBar::trailing(AnyElement)`), and
  the workbench puts the chip in it.
- **Gap**: Tenmoku draws its own title bar only in the macOS unified
  window. Where the platform draws the title bar (Linux today), the same
  chip is a status bar item, `agents` (`crates/fe-workbench/src/status_bar.rs`,
  a `kiln_shell::StatusItem`), on the left after the operation in progress.
  Neither the branch nor any chip is in the title bar yet; the branch is the
  `git.branch` status item. Moving it is not this design's.

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

- `crates/fe-workbench/src/title_bar.rs`
- `crates/fe-workbench/src/status_bar.rs`
- `crates/fe-workbench/src/tests/title_bar.rs`
- `crates/fe-workbench/l10n/en-US/chrome.ftl`: the chip's `1 agent working` / `N agents working` (title bar strings live there) (beyond the card's file list).
- `crates/fe-workbench/l10n/en-US/status.ftl`: the `agents` status item's text (beyond the card's file list).
- `crates/fe-workbench/l10n-allow.toml`: fe-workbench's literal lint is `all = true` (beyond the card's file list).

## Criteria

Each Tests item of the card, verbatim, as a criterion; items marked "(own)" are mine where the design gives none.

1. Test in `crates/fe-workbench/src/tests/title_bar.rs`: (own; the design gives no Tests line) the chip shows `1 agent working` / `N agents working` from `LaneCounts::working`, after the branch widget, hidden at zero, and clicking it shows the Agents panel
2. Test in `crates/fe-workbench/src/tests/title_bar.rs`: (own) where the platform draws the title bar, the same count is the `agents` status item, on the left after the operation in progress, hidden at zero
3. Test in `crates/fe-workbench/src/tests/title_bar.rs`: § Colour rule: A surface card's test reads its colour from the theme's `agent.*` token and asserts it is not the accent's, in both themes

## Notes

- Ready tasks touching the same files: T1583 and T1586 (`status_bar.rs`); blocked T1766 and T1768 (`title_bar.rs`).
