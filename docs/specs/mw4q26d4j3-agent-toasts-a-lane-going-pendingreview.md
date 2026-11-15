---
task_uid: "01m7b0mrcb38x7namw4q26d4j3"
task: "T356"
rev: 1
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "Test in crates/fe-workbench/src/toast_agent_tests.rs: (own; the design gives no Tests line) a lane going `PendingReview` shows `lane 2 · claude-code is done: 3 files to review` with Review changes (`agent.review_lane`) and Later, from a single call at the state change in `lanes.rs`"
    required: true
    manual: false
  - id: "C2"
    text: "Test in crates/fe-workbench/src/toast_agent_tests.rs: § Colour rule: A surface card's test reads its colour from the theme's `agent.*` token and asserts it is not the accent's, in both themes"
    required: true
    manual: false
---

# Agent toasts: a lane going PendingReview says lane N is done with files to review, with a silver-blue Review changes action (agent lanes card 16) (Mac T1811)

# Agent toasts: a lane going PendingReview says lane N is done with files to review, with a silver-blue Review changes action (agent lanes card 16)

Source: `docs/designs/agent-lanes.md` § Card breakdown, card **16** (the agent lanes design, Mac T1567, on main as e553a094a). The design files no cards itself; this is its card 16. Read the sections it cites before starting (quoted below or named).

Linux-OK: nothing in the card needs macOS; it builds and tests on Linux.

## The card, verbatim from § Card breakdown

**Wave C, after 6 (all ∥ with each other unless a dependency says so):**

16. **Agent toasts.** Size **s**. Deps: 0, 6. ∥. Files:
    `crates/fe-workbench/src/toast.rs` (`Toast` source, silver-blue
    action), `crates/fe-workbench/src/lanes.rs` (toast on `PendingReview`;
    its only edit there, a call at the state change).

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
- Since the design, `Toast` already has `source: Option<NotificationSource>` and `NotificationSource::Agent` exists (`toast.rs:168`, `notification_inbox.rs:30`), so "`Toast` gains a source" is done; what is left is drawing an Agent-source toast's icon and action silver-blue, and the call in `lanes.rs`.

## Design sections it cites, verbatim

### Also: toasts and the inbox

Not a row of the table, but the design system names agent toasts. When a
lane goes `PendingReview`, `toast::show` (`crates/fe-workbench/src/toast.rs`,
`Toast { level, message, detail, action, choices }`) says `lane 2 ·
claude-code is done: 3 files to review`, with Review changes
(`agent.review_lane`) and Later. `Toast` gains a source (`Agent`) so its icon
and action are silver-blue. The inbox is not built yet.

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

- `crates/fe-workbench/src/toast.rs`
- `crates/fe-workbench/src/lanes.rs`
- `crates/fe-workbench/src/toast_agent_tests.rs`: new test file, declared from `toast.rs` (beside `toast_tests.rs`) (beyond the card's file list).
- `crates/fe-workbench/l10n/en-US/toast.ftl`: the toast's message and its Review changes and Later (beyond the card's file list).
- `crates/fe-workbench/l10n-allow.toml`: fe-workbench's literal lint is `all = true` (beyond the card's file list).

## Criteria

Each Tests item of the card, verbatim, as a criterion; items marked "(own)" are mine where the design gives none.

1. Test in `crates/fe-workbench/src/toast_agent_tests.rs`: (own; the design gives no Tests line) a lane going `PendingReview` shows `lane 2 · claude-code is done: 3 files to review` with Review changes (`agent.review_lane`) and Later, from a single call at the state change in `lanes.rs`
2. Test in `crates/fe-workbench/src/toast_agent_tests.rs`: § Colour rule: A surface card's test reads its colour from the theme's `agent.*` token and asserts it is not the accent's, in both themes

## Notes

- Ready task T1584 (focus mode) also touches `toast.rs`.
