---
task_uid: "01m7b0h4xpd0td95x2mhy2czny"
task: "T1804"
rev: 5
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "Test in crates/fe-workbench/src/agents_panel_tests.rs: (own; the design gives no Tests line) the rail shows the Agents item (`Icon::Bot`) in the top group after Changes, with a count badge of `LaneCounts::working + LaneCounts::pending`, bottom-right, none at zero"
    required: true
    manual: false
  - id: "C2"
    text: "Test in crates/fe-workbench/src/agents_panel_tests.rs: (own) the panel lists `LaneSummary` rows (number, name, state, files, pending hunks, conflicts); an Accepted, Rejected or Abandoned lane stays listed, dimmed, until cleared; clearing an Abandoned lane with hunks pending asks first and rejects them; lane 1 is never cleared (§ Lifecycle › Closing)"
    required: true
    manual: false
  - id: "C3"
    text: "Test in crates/fe-workbench/src/agents_panel_tests.rs: § Colour rule: A surface card's test reads its colour from the theme's `agent.*` token and asserts it is not the accent's, in both themes"
    required: true
    manual: false
---

# Agents panel and its rail item: robot icon with a silver-blue count badge of working and pending lanes, LaneSummary rows with Review, Stop and Clear (agent lanes card 10)

# Agents panel and its rail item: robot icon with a silver-blue count badge of working and pending lanes, LaneSummary rows with Review, Stop and Clear (agent lanes card 10)

Source: `docs/designs/agent-lanes.md` § Card breakdown, card **10** (the agent lanes design, Mac T1567, on main as e553a094a). The design files no cards itself; this is its card 10. Read the sections it cites before starting (quoted below or named).

Mac-only: the card changes `crates/fe-workbench/examples/screenshots/golden/workbench.png` (the rail gains an item), which is rendered on Metal; the code itself is portable.

## The card, verbatim from § Card breakdown

**Wave C, after 6 (all ∥ with each other unless a dependency says so):**

10. **Agents panel and rail badge.** Size **m**. Deps: 0, 6. ∥. Files:
    `kiln/crates/kiln-ui/src/activity_rail.rs` (`RailBadge`),
    `kiln/crates/kiln-shell/src/dock.rs` (`DockItem::badge`),
    `kiln/crates/kiln-ui/src/file_icons.rs` and `assets/icons/bot.svg`
    (`Icon::Bot`), `crates/fe-workbench/src/dock.rs` (`DockPanel::Agents`),
    `crates/fe-workbench/src/agents_panel.rs` (new). A new rail item and
    commands change rail and command counts in fe-workbench tests outside
    this scope (`chat_rail_tests.rs`, layout and palette tests): the card's
    scope lists them.

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
- Since the design, T1540 (Rail B3) landed rail badges: `RailBadge` with `BadgeTone` in kiln-shell (`kiln/crates/kiln-shell/src/dock.rs:372`), set through the workbench dock's `set_badge` (`crates/fe-workbench/src/dock.rs:749`). kiln-ui already has `Icon::Bot` (`kiln/crates/kiln-ui/src/file_icons.rs:175`, `kiln/crates/kiln-ui/assets/icons/bot.svg`). So `kiln-ui/src/activity_rail.rs` (`RailBadge`), `kiln-shell/src/dock.rs` (`DockItem::badge`), `file_icons.rs` and `bot.svg` are dropped from the scope. `BadgeTone` (`kiln/crates/kiln-ui/src/badge.rs`) has no agent tone: add `BadgeTone::Agent` from `agent.base`.
- Review, Stop and Clear: `agent.review_lane` is card 19's and clearing a worktree is card 18's; this card wires them through `LaneStore` as far as those exist and says in the finish what waits for 18 and 19.

## Design sections it cites, verbatim

### Rail: robot icon with a silver-blue count

- **Shows** the Agents item (a robot head, new `Icon::Bot` with
  `kiln/crates/kiln-ui/assets/icons/bot.svg`) in the rail's top group after
  Changes, with a badge: the count of lanes `Working` or `PendingReview`,
  silver-blue, bottom-right with the cut-out ring; none at zero.
- **Data**: `LaneCounts::working + LaneCounts::pending`. The panel it opens
  lists `LaneSummary` rows (number, name, state, files, pending hunks,
  conflicts), with Review, Stop and Clear.
- **Reads it**: a new sidebar panel, `crates/fe-workbench/src/agents_panel.rs`
  (`DockPanel::Agents`, a `SidebarPanel` with `always_on_rail: true`,
  registered in `crates/fe-workbench/src/dock.rs` `PANELS`), whose
  `DockItem` gains `fn badge(&self, cx) -> Option<RailBadge>`
  (`kiln/crates/kiln-shell/src/dock.rs`, trait `DockItem`), drawn by
  `RailButton::badge(RailBadge { text, color })`
  (`kiln/crates/kiln-ui/src/activity_rail.rs`).
- **Gap**: `RailButton` has no badge today (`id`, `icon`, `highlighted`,
  `on_click`, `tooltip`); badges exist only on tree rows
  (`TreeRow::badge`).

### § Surfaces, shared data

One entry per row of the provenance table. Each names the data the surface
reads, the type that carries it, and the file that reads it. "Lanes" is the
window's `LaneStore` (`crates/fe-workbench/src/lanes.rs`, new) over the
headless `fe_editor::Lanes`.

Shared data, read by more than one surface:

```rust
// crates/fe-editor/src/lane.rs
pub struct LaneCounts { pub working: usize, pub pending: usize }   // Lanes::counts()
pub struct LaneSummary {                                           // LaneStore::summaries()
    pub id: LaneId, pub name: Arc<str>, pub state: LaneState,
    pub files: Vec<(PathBuf, fe_editor::Counts)>,                  // from the lane's Changeset
    pub conflicts: usize,                                          // stale hunks not yet rejected
}
```

### Lifecycle

```rust
pub enum LaneState {
    Working,        // the agent is producing; it may already have pending hunks
    PendingReview,  // its turn is over and it has undecided hunks
    Accepted,       // every hunk decided, at least one accepted
    Rejected,       // every hunk decided, none accepted
    Abandoned,      // ended (closed, disconnected, exited) with hunks undecided
}
```

A lane's **pending hunks** are every undecided hunk it owns, wherever it is
shown or not (§ Isolation › What a lane owns): `Pending` hunks, and `Stale`
ones until the user rejects them (`ProposedChange::reject` takes a stale
hunk). A turn's **end** is the agent's signal for an external lane
(`lane.finish`), the reply finishing, failing or being stopped for the chat,
and a frozen revision for a worktree lane (§ Isolation › Worktree lanes),
whose next turn starts when its worktree changes again; a terminal still
running with nothing written leaves the lane as it is.

| From | To | On |
| --- | --- | --- |
| (new) | Working | `lane.open`, New agent terminal, the chat's first send |
| Working | PendingReview | The turn ends with pending hunks |
| Working | Accepted / Rejected | The turn ends with none pending (Rejected also when it made nothing) |
| PendingReview | Working | Request changes or a review comment sent as the next instruction; the chat's next send; the agent starting a turn by itself |
| PendingReview | Accepted / Rejected | The last pending hunk decided |
| Accepted / Rejected | PendingReview | Hunks added outside a turn: Apply or Review Changes on a finished reply, a worktree lane frozen again |
| Accepted / Rejected | Working | A persistent lane (the chat) or a reconnected agent starts another turn |
| PendingReview / Accepted / Rejected | Working | A worktree lane's live tree no longer equals its newest revision's: the agent wrote again (checked on the worktree watcher's debounced events, by tree id) |
| Working / PendingReview | Abandoned | Closed by the user, the agent disconnects, or its terminal exits, with hunks pending |
| Working / PendingReview | Accepted / Rejected | The same, with none pending |
| Abandoned | Accepted / Rejected | Its last pending hunk decided: an ended lane's hunks stay decidable |
| Abandoned | Working | The same agent reopens the same session (`lane.open` with it) before the lane is cleared |

- **Restart.** A restored lane is reconciled before anything reads it:
  lane 1 is PendingReview with pending hunks, else Accepted or Rejected; an
  external lane restored as Working or PendingReview is Abandoned (its agent
  is gone) until `lane.open` reopens its session; a worktree lane's terminal
  isn't restored, so it is frozen once (its worktree is still there) and
  goes to PendingReview or stays as decided.
- **Closing.** An Accepted, Rejected or Abandoned lane stays listed in the
  Agents panel, dimmed, until the user clears it; clearing an Abandoned
  lane with hunks pending asks first and rejects them. Clearing removes its
  worktree (if any) but not its number from the record. Lane 1 never closes.
- **Counts.** "Agents working" in the title bar counts lanes in `Working`;
  the rail's count is lanes with pending hunks (PendingReview, and Abandoned
  ones not yet decided) plus those `Working`.
- **Tests.** Card 1 tests every row of this table headlessly, with the
  pending count and the worktree's tree id as inputs; card 6 tests lane 1's
  rows through the chat; card 19 tests a terminal lane through review,
  decisions and the agent writing again (Working, PendingReview, Accepted,
  Working, PendingReview).

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

- `kiln/crates/kiln-ui/src/badge.rs`: `BadgeTone::Agent`, the silver-blue tone the badge needs (the rest of the card's kiln list has landed: see Since the design) (beyond the card's file list).
- `crates/fe-workbench/src/dock.rs`
- `crates/fe-workbench/src/agents_panel.rs`
- `crates/fe-workbench/src/agents_panel_tests.rs`: new test file, declared from `agents_panel.rs` (beyond the card's file list).
- `crates/fe-workbench/src/lib.rs`: declares the module; the panel's commands in the command catalog (§ Card breakdown preamble) (beyond the card's file list).
- `crates/fe-workbench/src/workbench.rs`: registers the panel (§ Card breakdown preamble: Wave C registers through `workbench.rs`) (beyond the card's file list).
- `crates/fe-workbench/l10n/en-US/dock.ftl`: the panel's title and strings (beyond the card's file list).
- `crates/fe-workbench/l10n-allow.toml`: fe-workbench's literal lint is `all = true` (the panel's const name, as the Explorer's has an entry) (beyond the card's file list).
- `crates/fe-workbench/src/chat_rail_tests.rs`: named by the card: a new rail item changes rail counts (beyond the card's file list).
- `crates/fe-workbench/src/sidebar_panels_tests.rs`: the card's "layout and palette tests": pins sidebar panels (beyond the card's file list).
- `crates/fe-workbench/src/palette_tests.rs`: the card's "layout and palette tests" (beyond the card's file list).
- `crates/fe-workbench/src/dock_position_tests.rs`: the card's "layout and palette tests": pins rail layout (beyond the card's file list).
- `crates/fe-workbench/src/tests/activity_rail.rs`: pins the rail's items (beyond the card's file list).
- `crates/fe-workbench/src/tests/rail_b3.rs`: pins the rail's top group (beyond the card's file list).
- `crates/fe-workbench/src/tests/sidebar_b3.rs`: pins sidebar panels (beyond the card's file list).
- `crates/fe-workbench/examples/screenshots/golden/workbench.png`: the golden shows the rail, which gains an item (beyond the card's file list).

## Criteria

Each Tests item of the card, verbatim, as a criterion; items marked "(own)" are mine where the design gives none.

1. Test in `crates/fe-workbench/src/agents_panel_tests.rs`: (own; the design gives no Tests line) the rail shows the Agents item (`Icon::Bot`) in the top group after Changes, with a count badge of `LaneCounts::working + LaneCounts::pending`, bottom-right, none at zero
2. Test in `crates/fe-workbench/src/agents_panel_tests.rs`: (own) the panel lists `LaneSummary` rows (number, name, state, files, pending hunks, conflicts); an Accepted, Rejected or Abandoned lane stays listed, dimmed, until cleared; clearing an Abandoned lane with hunks pending asks first and rejects them; lane 1 is never cleared (§ Lifecycle › Closing)
3. Test in `crates/fe-workbench/src/agents_panel_tests.rs`: § Colour rule: A surface card's test reads its colour from the theme's `agent.*` token and asserts it is not the accent's, in both themes

## Notes

- Ready task T1571 (Runs panel) also touches `crates/fe-workbench/src/dock.rs`.

## Implementation notes

- **Rail order.** Agents sits in the top group after Changes and before
  Chat (`SIDEBAR.order` 45, between Changes' 40 and Chat's 47), so
  `DockPanel::ALL` is Project, Search, Changes, Agents, Chat. The dock
  registers it in `Dock::new`, as the rail lists panels in the order they
  are added; the workbench now makes the `LaneStore` before the dock so the
  panel can be made with it.
- **The count.** Card 1's `LaneCounts::pending` already counts the lanes
  `Working` as well as those with hunks pending (PendingReview, and
  Abandoned not yet decided), so the design's
  `LaneCounts::working + LaneCounts::pending` is `LaneCounts::pending`
  (`LaneStore::rail_count`): adding `working` again would count a Working
  lane twice (`lanes_tests::a_working_lane_with_pending_hunks_counts_once_in_the_rail`).
  The badge is `RailBadge::count(n, BadgeTone::Agent, …)`, set by the dock
  (`Dock::follow_agents`) whenever the panel redraws for a change of its
  lanes; none at zero.
- **Chat's icon.** Chat already used `Icon::Bot`. The design gives the
  robot head to Agents, and one rail test asserts every rail icon is
  distinct, so Chat now draws `Icon::MessageSquare`, the speech bubble it
  had before T1540; the Agents panel lists the chat as lane 1.
- **Review, Stop and Clear, as far as they exist.** Review is offered on
  lane 1 while it has hunks pending and goes to them
  (`Workbench::review_lane`): the AI Changes panel if it shows lane 1's
  turn with hunks undecided; else the editor of the first kept turn's file
  or loose (Apply) proposal with one, brought forward and focused, the caret
  on that hunk. A refactor preview on the panel stays there. Another lane's
  review is `agent.review_lane`, card 19's. Stop is offered on lane 1 while
  Working and stops the turn as the chat's own Stop button does
  (`ChatView::stop_turn`: every permission prompt refused, the reply
  stopped); stopping another lane's agent is the agent seam's (card 20).
  Clear is offered on every lane but 1 once it has ended; an Abandoned lane
  with hunks pending asks (a prompt naming the count) and then rejects them
  (`Workbench::reject_lane_work`): through the open editors where they are,
  else as recorded (`ProposedChange::reject_undecided`, for a buffer no
  editor holds any more). Only once nothing is left undecided does
  `LaneStore::clear` remove the lane and its work, its number still spent.
  Removing a cleared lane's worktree is card 18's.
- **Scope added** by respec: `lanes.rs` (`LaneStore::clear`,
  `Workbench::reject_lane_work`, `LaneSummary::pending`), `chat_panel.rs`
  (Chat's icon), and four more tests that pin the rail's items
  (`explorer_views_tests.rs`, `source_control_views_tests.rs`,
  `git_panel_tests.rs`, `tests/references_panel.rs`, `chat_panel_tests.rs`);
  after review round 1, `fe-buffer/src/proposed.rs`
  (`ProposedChange::reject_undecided`) and `fe-chat-view/src/view.rs`
  (`ChatView::stop_turn`).
- **Not yet:** the panel's key context isn't in the dock-leave bindings
  (vim's `<C-w>` keys), which fe-gpui's context whitelists pin; Up/Down
  (and `j`/`k` with vim) move its selection.
