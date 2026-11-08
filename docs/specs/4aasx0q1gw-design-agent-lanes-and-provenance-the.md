---
task_uid: "01m6w2wf9twhgmjp4aasx0q1gw"
task: "T322"
rev: 1
type: "docs"
complexity: "m"
criteria:
  - id: "C1"
    text: "docs/designs/agent-lanes.md exists with level-2 sections Lane model, Isolation, Surfaces, Today's chat agent, Colour rule, Card breakdown and Open questions (rg '^## ' lists them in that order)"
    required: true
    manual: false
  - id: "C2"
    text: "Surfaces has one entry for each row of the provenance table in docs/design/design-system.md (title bar, rail, editor, problems and tests, terminal, changes, diff), each naming the type that carries its data and the file that reads it"
    required: true
    manual: false
  - id: "C3"
    text: "Card breakdown lists implementation cards each with files, dependencies and a size of at most m, and marks which can run in parallel"
    required: true
    manual: false
  - id: "C4"
    text: "Only docs/designs/agent-lanes.md changes"
    required: true
    manual: false
---

# Design agent lanes and provenance: the lane model, isolation from the tree, every surface's data, today's chat agent as lane 1, and the card breakdown (Mac T1567)

# Design agent lanes and provenance

Spec: `docs/design/design-system.md#notifications-and-agents` (Agent
provenance table) and the Principles ("Agents are authors, not a feature:
their work reaches your tree only through review"). Today there is one chat
agent: edits it proposes carry `fe_buffer::Author::Ai { agent }`
(`crates/fe-buffer/src/provenance.rs`), the AI Changes panel
(`crates/fe-workbench/src/ai_changes_panel.rs`) reviews a proposal, and
`provenance_column.rs` shows "AI · agent" beside lines. Nothing models a
second agent, a lane, or agent work that is still pending.

This card writes the design and stops. It writes **no code**, and it does not
file cards; the lead files them from the breakdown.

## Output: `docs/designs/agent-lanes.md`

Sections, in this order (each a level-2 heading):

1. **Lane model**: what a lane is, who creates one (a second chat? an
   external agent over the plugin/MCP seam? a CLI agent in a terminal?), its
   identity (number, name), lifecycle states (working, pending review,
   accepted, rejected, abandoned) and where the state lives (per window?
   persisted across restarts?).
2. **Isolation**: how a lane's edits stay out of the user's tree until
   reviewed (overlay buffers over the open files? a git worktree? the existing
   `ProposedChange`?), what the user's own edits to the same file do, and how
   conflicts surface.
3. **Surfaces**: for each row of the provenance table (title bar chip, rail
   count, editor lane + "lane 2 · pending" chip + faint row tint, "written
   by lane 2" in Problems and tests, From agents scope, agent terminal
   read-only, Changes group, "After · lane 2" diff header) the data it reads
   and the type that carries it.
4. **Today's chat agent**: how it becomes lane 1 without a regression in
   `chat_apply*`, `ai_changes_panel` and `provenance_column` behaviour.
5. **Colour rule**: confirm silver-blue (`agent.*` tokens, T1509) is only ever
   an agent signal and never rust.
6. **Card breakdown**: an ordered list of implementation cards, each with the
   files it touches, its dependencies, and a size of at most m (about 2k
   lines). Mark which ones can run in parallel.
7. **Open questions** for the user.

## Not in this card

- Any Rust, any setting, any token. The review panel is
  `docs/designs/agent-review.md` (a later card), command permissions
  `docs/designs/agent-command-permissions.md`.
- Choosing an agent protocol or provider. State what the lane model needs
  from one, not which to use.

Platform: **Linux-OK** (docs only).
