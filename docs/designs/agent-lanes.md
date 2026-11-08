# Agent lanes and provenance

Written 2026-11-08 for T322 (Mac T1567). The should is
`docs/design/design-system.md` (revision 18, on the Mac trunk as of
c3bab527): the Principles ("Agents are authors, not a feature: their work
reaches your tree only through review") and § Notifications and agents ›
Agent provenance. This file says how Tenmoku models more than one agent's
pending work, keeps it out of the user's tree until it is reviewed, and feeds
every row of the provenance table. It writes no code and files no cards; the
lead files them from § Card breakdown.

Out of scope here: the review panel itself (`docs/designs/agent-review.md`),
command permissions (`docs/designs/agent-command-permissions.md`), and the
choice of an agent protocol or provider. Where the lane model needs something
from a protocol, § Lane model › What a protocol must give says what, not which.

**What there is today.**

- **One chat agent.** `ChatPanel` (`crates/fe-workbench/src/chat_panel.rs`),
  one per window, shows one chat at a time. Apply on a code block dispatches
  `chat.propose_edit` as `Actor::Ai { agent: "fastedit.chat", session: <chat
  id> }` (`chat_apply.rs`, `AGENT`), which maps to
  `Origin::Ai { agent, session }` (`chat_apply.rs:origin_of`).
- **Proposals over buffers.** `fe_buffer::ProposedChange`
  (`crates/fe-buffer/src/proposed.rs`) holds hunks anchored in one buffer,
  each `Pending`, `Accepted`, `Rejected` or `Stale`. Nothing touches the
  buffer until a hunk is accepted; a hunk whose old text the user changed goes
  `Stale` and can't be accepted. `EditorView::set_proposals`
  (`crates/fe-gpui/src/view/proposal.rs`) draws them as diff blocks with
  Accept and Reject, and tells `ProposalEvent`s.
- **A turn's changeset.** Review Changes (`chat_changeset.rs`,
  `chat.review_changes`) loads a turn's file edits into one
  `fe_editor::Changeset` (`crates/fe-editor/src/changeset.rs`: per-file
  proposals, new files, accept/reject per hunk, file and all, and a
  checkpoint that reverts the turn). The workbench holds one at a time
  (`Workbench::set_changeset`, `AiChanges` in `ai_changes_panel.rs`), shown
  in the AI Changes sidebar panel.
- **Provenance.** Accepting goes through `ProposedChange::accept_as` with
  `Origin::User` for undo, but the text stays authored by the proposal's
  origin: `fe_buffer::Author::Ai { agent }`
  (`crates/fe-buffer/src/provenance.rs`). `editor.toggle_provenance`
  (`provenance_column.rs`) labels those rows `AI · fastedit.chat`. `Author`
  drops the session, so two chats' lines can't be told apart.
- **A second in-process agent.** Inline Edit (`inline_edit.rs`) proposes as
  `Origin::Ai { agent: "fastedit.inline_edit", session }`
  (`crates/fe-chat/src/inline.rs:INLINE_EDIT_AGENT`), in the editor at the
  selection.
- **Plugins, not agents.** A WASM plugin (preview,
  `extensions.plugins_preview`; `plugin_host.rs`, `plugin_bridge.rs`) can
  edit or propose as `Origin::Host(<plugin id>)`, which provenance shows by
  name and doesn't treat as AI. There is no MCP server or other
  external-agent seam.
- **Nothing else.** No second agent, no lane, no record of agent work that is
  pending outside the open editors, no agent terminal, no Runs panel, and no
  agent colour token yet: the `agent.*` tokens are Mac T1509, on neither
  trunk.

## Lane model

A **lane** is one agent's line of work in one workspace: who the agent is,
what it is doing, and the changes it has made that the user has not yet
decided. Everything the provenance table shows is read from lanes.

### Who creates a lane

| Creator | When | Kind |
| --- | --- | --- |
| The built-in chat | Made first, when the window's lanes are | `LaneKind::Chat`, lane 1 (§ Today's chat agent) |
| An external agent over the plugin or MCP seam | It asks for a lane before its first edit (`lane.open`) | `LaneKind::External { host }` |
| The user, for a CLI agent | **New agent terminal** (`agent.new_terminal`): a lane, a worktree and a terminal in it | `LaneKind::Terminal` |
| An agent task in Runs (later) | Starting the task | `LaneKind::Task`, backed as a terminal lane |

Not lanes:

- **Inline Edit.** Its proposal is under the selection the user just typed an
  instruction for, and is decided there. Its rows are still `Author::Ai`, so
  the editor marks them as agent-written, with no lane number (the
  provenance column's `AI · fastedit.inline_edit`).
- **A CLI agent the user starts in an ordinary terminal.** It writes the
  user's files directly, so Tenmoku sees disk changes, not proposals, and
  can't hold them back. The lane-backed way to run one is New agent terminal.
  Open question 3 asks whether to detect and warn.

### Identity

```rust
// crates/fe-editor/src/lane.rs (new, headless)
pub struct LaneId(NonZeroU32);           // the number: "lane 2"
pub struct Lane {
    pub id: LaneId,
    pub name: Arc<str>,                  // "Chat", "claude-code", the task's title
    pub agent: Arc<str>,                 // Origin::Ai's agent: "fastedit.chat", a plugin id
    pub sessions: Vec<Arc<str>>,         // Origin::Ai sessions it owns
    pub kind: LaneKind,
    pub backing: Backing,                // § Isolation
    pub state: LaneState,
    pub started: SystemTime,
}
pub struct Lanes { /* by id; next number; claims */ }
```

- **The number** is given by the workspace's `Lanes` in order of creation and
  is never reused while the workspace's lane record lasts, so "written by lane
  2" means the same lane in Problems today and in a commit message next week.
  Lane 1 is the chat, made first.
- **The name** is what the agent calls itself (the plugin's display name, the
  CLI's command, the task's title), shown after the number where there is
  room: `lane 2 · claude-code`.
- **Claims.** An edit's `Origin::Ai { agent, session }` finds its lane through
  `Lanes::lane_for(agent, session)`: the lane whose `agent` matches and whose
  `sessions` hold the session. The chat lane adds each chat's id to its
  sessions as that chat proposes, so every chat is lane 1. An origin no lane
  claims (Inline Edit, scripted hosts) has no lane.
- **Provenance carries the session.** `Author::Ai { agent }` becomes
  `Author::Ai { agent, session }`, copied from the `Origin` as it is today
  minus the drop, so a row's lane is `lane_for` of its author. Runs stay
  merged per author, so two chats' lines become two runs; nothing else in
  `Provenance` changes.

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

### Where the state lives

- **One owner per root.** A root's lanes belong to one window at a time:
  the first window to open the root takes its lanes by an exclusive lock,
  `<state_dir>/lanes/<layout_key(root)>/lanes.lock` (an advisory file lock
  for other processes, and a `LaneOwners` global keyed by canonical root
  for other windows of this one). The owner holds a `LaneStore` (an entity
  on its `Workbench`) that reads and writes the record, gives numbers, and
  makes and clears worktrees; only the owner does any of these.
- **Other windows on the root** get a window-local `LaneStore`: lane 1
  only (their chat), memory-only, never written, numbered apart from the
  record and shown as `lane 1 (this window)`. `agent.new_terminal` and
  `lane.open` are refused there with a toast naming the owning window. When
  the owner closes, the lock goes, and the next window that opens the root
  takes it and restores the record; a window already open on the root keeps
  its local store until reopened (open question 2 asks whether to share).
- **Persisted across restarts** as `lanes.json` beside the lock (the chat
  store's layout, `docs/decisions/5at38c8v89-ai-chat.md` § 11): each lane's
  id, name, agent, sessions, kind, backing, state and start time, and for a
  worktree lane its revisions' refs, plus the next number. Written atomically by the
  owner after each change, 0600.
- **Pending work persists with it**: an overlay lane's undecided hunks as
  detached hunks and its whole-file operations with their texts and
  preconditions (§ Isolation › Detached hunks), a worktree lane's as its
  frozen revisions and decision ledger (§ Isolation › Worktree lanes).
- **Untrusted folders: memory-only**, as their chats are: no `lanes.json`, no
  lock file (the in-process `LaneOwners` still picks one owner), no
  worktrees, and a lane's work is gone with the window. Terminal lanes are
  refused there (they need a worktree on disk).

### What a protocol must give

Whatever carries an external agent (MCP, the plugin host, a CLI's stdio), the
lane model needs these from it and nothing else:

1. **A stable agent id and a display name**, granted to the plugin or server
   by the app, which become `Lane::agent` and `Lane::name`.
2. **A session id** per conversation, which the lane claims.
3. **Edits as proposals**, file by file: whole text or ranges against a text
   the agent read, through `lane.propose`, which proposes under
   `Origin::Ai { agent, session }`. Never a direct write. Today a plugin can
   change the user's files three ways: `buffer.edit` with `buffer.write`
   (straight into a buffer as `Origin::Host(<plugin id>)`,
   `crates/fe-plugin/wit/buffer.wit`), files through the read-write WASI
   preopen `fs.write:workspace` gives (`kiln/crates/kiln-plugin/src/fs.rs`),
   and any program `process.spawn:<program>` lets it run. So an agent's
   grant, `agent.lane`, is **exclusive of all three**: a manifest asking for
   `agent.lane` with `buffer.write`, `fs.write:workspace` or any
   `process.spawn:*` is refused when the plugin is installed or enabled,
   and `lane.open` checks the grants again and refuses if they have changed.
   It may hold `buffer.read` and `fs.read:workspace`. Running commands is
   `lane.run`, later, inside its worktree (§ Card breakdown › Later). The
   built-in chat has no write path either (`5at38c8v89-ai-chat.md` § 9).
4. **Turn boundaries**: `lane.finish` when the agent stops and waits, so the
   lane can go to `PendingReview`.
5. **A way back**: review comments and Request changes are delivered as the
   agent's next instruction (`lane.instruction` events).
6. **Liveness**: the seam tells the lane when the agent disconnects, which
   abandons it.

A terminal lane needs none of these: its edits are the worktree's diff, and
its turns are the user's.

## Isolation

The rule is the Principle's: nothing a lane writes is in the user's buffers,
on the user's disk, in the language server's view of the user's files, or in
a commit, until the user accepts it. A lane is backed one of two ways, and
both are reviewed the same way, as proposals over the user's buffers.

```rust
pub enum Backing {
    /// Proposals over the user's buffers: the chat, and external agents that
    /// only edit.
    Overlay,
    /// A git worktree of its own, at `base`: terminal lanes and agents that
    /// run commands (builds, tests) on their own changes.
    Worktree { path: PathBuf, base: String }, // base: the commit's full hex id
}
```

### What a lane owns

Undecided work belongs to the lane, not to whatever shows it, so no panel,
tab or newer turn can drop it.

- **`LaneWork`**, one per lane in the `LaneStore`: every proposal the lane
  has made that still has undecided hunks, by file, each an
  `Rc<RefCell<ProposedChange>>` (the same one editors and changesets hold),
  whether in a turn's changeset or **loose** (Apply's, made outside one);
  every new file and deletion it proposes, with its precondition (§ New and
  deleted files); the detached hunks of files no buffer holds now
  (§ Detached hunks); and every turn's `AiChanges` whose changeset isn't
  resolved, in turn order. A proposal leaves `LaneWork` when its last hunk
  is decided; a turn's `AiChanges` when its changeset is resolved, except
  the newest, which stays for Revert Turn as today.
- **The AI Changes panel is a view of the newest turn**, as today: a lane's
  newer turn takes the panel, as a newer Review does now, and the earlier
  turn's `AiChanges` stays in `LaneWork`, its pending new files with it
  (text in its `Changeset`; a new file gets a buffer only when accepted,
  and a tab at once, as `land_ai_changes` does today), still pending and
  still counted.
- **One user buffer per path.** `LaneBuffers` (in the owner's `LaneStore`)
  maps each canonical real path (resolved as `chat_apply::locate` resolves
  a block's file) to the one `Buffer` every lane and every editor uses for
  it. A lane that needs a file that is open takes the editor's buffer; one
  that isn't open is read into a buffer kept there with no editor; an
  editor opened on the path later (`open.rs`) takes that buffer and every
  lane's proposals on it. So two lanes editing one unopened file propose
  against one `BufferId`, and accepting one's hunk stales the other's by
  the existing rule. A background lane never opens a tab (accepting, the
  user's act, does: below); today's Apply,
  which opens the file in the active pane, still does for lane 1.
- **An editorless buffer is always clean**, so no unsaved text ever lives
  where hot exit and the close prompt can't see it:
  - A buffer a lane reads with no editor is the file's text from disk, and
    follows disk changes as a clean open buffer does (`disk_sync.rs`), its
    hunks going stale by the usual rules.
  - **Accepting** a hunk, file or turn on a buffer with no editor first opens
    the file in a tab in the active pane, unfocused, so the dirty buffer has
    an editor from the moment it is dirty, and saving, hot exit and the
    close prompt work as today. Rejecting doesn't dirty, so opens nothing.
  - **The last editor closing** runs the close prompt as today. Then the
    lane's undecided hunks on the buffer are detached (saved as § Detached
    hunks says, with the buffer's text as their base) and the buffer goes as
    today. Opening the file again, or a lane needing it, restores them onto
    the text then (what the user saved, or the disk's if they discarded),
    so hunks over discarded edits come back stale.
  - Chat's block header still clears when its file's editor closes, as
    today; the lane keeps the hunks, and opening the file shows them again.
- **Nothing saved.** A pending hunk isn't in the buffer, so saving, the
  language server, auto-save, `git status` and commit never see it.

### Overlay lanes: `ProposedChange` over the user's buffers

What the chat does today. Each file the lane edits has a
`ProposedChange` against its `LaneBuffers` buffer, made against the buffer
as it is now (unsaved edits included), and each turn's files are a
`fe_editor::Changeset`, so accept, reject, accept all, checkpoint and revert
are the code that exists.

### Detached hunks: persisting and restoring without guessing

A restart, or the last editor on a file closing (§ What a lane owns),
needs an overlay lane's undecided work to outlive its anchors. It is never
found again by searching for its old text alone, nor by trusting whatever
alignment a diff happened to choose: an equal text elsewhere is not the same
place. Only undecided work is saved; the owner rewrites the file after each
proposal and decision.

- **What is saved**, in `<lane>.patch.json`, per file:
  - for hunks: the file's whole **base text** (the buffer's text the hunks'
    ranges are in: when the proposal was made, or when it was detached),
    each hunk's range in that base, its old and new text, and its status,
    `Stale` kept as stale;
  - for whole-file operations (§ New and deleted files): the operation
    (`New` or `Deleted`), a new file's whole text, a deletion's expected
    text, and whether it is already stale.
- **Restoring hunks** is the projection with the saved base as base, the
  base with the saved hunks applied as the lane's text, and the text as it
  is now (the open buffer, or the file on disk) as current, under the
  placement rule below; a hunk saved stale stays stale.
- **Restoring whole-file operations** checks each precondition again
  against the text as it is now: a new file whose path now exists, or a
  deletion whose expected text no longer equals the file's (or whose file is
  gone, which decides it as no change), comes back stale or decided as
  § New and deleted files says; one saved stale stays stale.
- **The placement rule**, used by restore and by every projection
  (`lane_project`): a hunk's **window** is the full lines its base range
  touches plus one whole line on each side (or the start or end of the
  text); for an insertion, the line it is in plus one on each side. If
  current equals base, every hunk is placed at its base range (there is no
  alignment to choose). Otherwise a hunk is placed only if both hold:
  1. its window lies inside a single unchanged run of the base→current
     diff and touches no changed region (the start or end of the text is
     not a change); and
  2. the window's text occurs exactly once in base and exactly once in
     current, at the position the diff maps it to.
  Otherwise it is stale. So with base `x\nx\n` and current `x\n`, a hunk on
  either `x` is stale (neither window is in current), not moved onto the
  survivor; text the user moved is a deletion and an insertion in the diff,
  so a hunk on it is stale; repeated text with the same neighbours is never
  unique, so is stale whenever anything changed around it.
- **`ProposedChange` can't express this today**: its constructors read old
  text from the snapshot they are given and have no stale hunk. Card 3 adds
  `ProposedChange::from_detached(current, hunks, origin)`, taking per hunk
  `DetachedHunk { at: Option<Range<usize>>, old_text, new_text, stale:
  bool }`: `at: None` or `stale: true` makes a hunk that is `Stale` for good
  (shown, rejectable, never acceptable), and a placed hunk whose `old_text`
  doesn't match the text at `at` is refused, not silently re-anchored.
- **Insertions** have no old text to mismatch, so only the placement rule
  decides them: an insertion beside or inside anything the user changed is
  stale, as is one whose surrounding lines aren't unique.

### Worktree lanes: a git worktree, frozen into revisions

For an agent that must run its own code. `agent.new_terminal` (or, later,
an external agent asking for `backing: worktree`) has the owner run
`git worktree add --detach <state_dir>/lanes/<key>/<n> <HEAD>` and starts
the agent there: a terminal's shell cwd, a task's process cwd. The agent
writes files there freely; review never reads the live worktree.

- **Revisions.** Review works on a **frozen revision**: a commit of the
  worktree's whole state (tracked and untracked, through a temporary index)
  on a private ref, `refs/tenmoku/lanes/<layout_key(root)>/<n>/r<k>`.
  Refs are shared by every linked worktree of a repository, and two roots
  (the main checkout and a linked worktree, say) can each own a lane 2, so
  the root's persistent layout key is part of every ref, and clearing a
  lane deletes only refs under its own root's key and number. A revision is
  frozen when a turn ends: the user's **Review lane**
  (`agent.review_lane`), the terminal's process exiting, a task ending, and
  once on restore. Between revisions the worktree's live diff feeds the
  Agents panel's `changed since review` count, and starts the next turn
  (§ Lifecycle).
- **Each revision is reviewed against the one before.** Review of `r<k>`
  projects the diff `r<k-1>`→`r<k>` (`r0` is `base`) onto the user's
  buffers with `lane_project(prev_text, rev_text, current, origin)`, so a
  hunk the user already accepted or rejected is not in the next diff and
  never comes back, and an accepted hunk is already in `current`, where the
  mapping places the next revision's hunks around it.
- **The decision ledger**, `<lane>.ledger.json`, records per revision and
  file each hunk's revision ranges and texts and its decision. Rebuilding a
  revision's projection (after a restart, or a buffer dropped) applies the
  ledger, so decided hunks stay decided; a revision whose hunks are all
  decided is done, and the next is reviewed against it.
- **Rejected work stays in the worktree**, where the agent's later work may
  build on it. Rejecting sends it to the agent as an instruction (Request
  changes); the next revision's diff shows only what changed since.
- **Update lane** (`agent.update_lane`) brings the user's tree into the
  worktree. It runs only when the lane's newest revision is fully decided
  and the worktree has nothing since it (else it is refused, saying why);
  it merges the user's saved tree into the worktree and freezes the result
  as an **imported** revision, which is never reviewed: it becomes the base
  of the next revision's diff, so the user's own changes never show as the
  agent's.
- **The user's tree** is never written by the agent. Accepting writes the
  user's buffers (dirty, unsaved), as an overlay accept does.
- **Cost.** A worktree per lane shares the object store but has its own
  checkout and its own `target/`. Lanes are worktrees only when they run
  code; open question 4 asks for a cap.

### New and deleted files

A whole-file change has no hunk to go stale, so each carries a precondition
checked when it is proposed and again when it is accepted.

- **New file**: the path must not exist, on disk or as a buffer. If it does
  at accept (the user made it meanwhile: add/add), the change is stale; the
  lane diff shows the user's file beside the lane's, and the user can copy
  from it. `Changeset::add_new_file` gains this check for every lane; today
  accepting over a path the user has since made makes a second buffer.
- **Deleted file**: the user's current text must equal the text the
  deletion was made against (the revision before, for a worktree lane). If
  the user edited it (delete/modify), the deletion is stale. If the user
  already deleted it, accepting changes nothing (the `AcceptedNoChange` of a
  hunk) and the deletion is decided.
- **Renames** are a deletion and a new file, each with its precondition.

### The user's own edits to the same file

- **Overlay.** The buffer stays the user's. Hunk anchors follow their edits
  (`ProposedChange`'s anchor biases): an edit beside a hunk leaves it pending,
  an edit inside the text it replaces makes it `Stale`.
- **Worktree.** The user's edits are invisible to the agent until review,
  where `lane_project` places each hunk against the user's current text; the
  agent's worktree isn't rebased behind its back (Update lane, above, is
  the user's choice).
- **Saving, committing, switching branch** with hunks pending is allowed and
  carries no agent text. After a branch switch a worktree lane's base may not
  be on the new branch; its projection still works, from its revisions, and
  the Agents panel says `based on <short sha>`.

### Conflicts

- **One hunk, one place.** A stale hunk shows in its editor block as stale
  (today's drawing), counts in its lane's chip (`lane 2 · 1 conflict`), its
  file's row in the review panel and the Changes group, and Request changes
  sends it back to the agent with the user's text as the next instruction.
- **Two lanes, one text.** Both lanes' hunks are on the one buffer
  `LaneBuffers` gives, each with its own lane chip. Whichever is accepted
  first goes in; the other's old text is gone, so it goes `Stale` by the
  existing rule (an insertion of the other lane at that point is stale by
  the position rule). The hunk header says `also changed by lane 3` while
  both are pending, read from the editor's other proposals' ranges.
- **The agent's base moved.** For a worktree lane, every hunk is checked at
  projection time, not when the agent wrote it, so a lane started on an old
  base shows conflicts only where the user's text actually differs.

## Surfaces

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

### Editor: dotted silver-blue change lane, faint row tint, "lane 2 · pending" chip

- **Shows**, per editor:
  - the **dotted lane** in the 3 px change-bar column beside rows an agent
    wrote: accepted agent text (provenance) and the rows of its pending
    hunks. It takes the column's place on those rows; the git bar shows on
    every other row.
  - the **faint row tint** (`agent.*`'s soft ground) behind the rows a
    pending hunk replaces, under the proposal's diff block, whose own added
    and removed bands keep celadon and shinsha.
  - the **chip**, `lane 2 · pending` (or `lane 2 · stale`, `lane 2 · 1
    conflict`), in the proposal block's header row before Accept and Reject.
- **Data**: `fe_editor::AgentRows` (new), `Vec<(Range<Anchor>, LaneMark)>`
  with `LaneMark { lane: Option<LaneId>, pending: bool }`, built from
  `Provenance::row_authors` (accepted: `Author::Ai { agent, session }`
  through `Lanes::lane_for`) and from each shown `ProposedChange`'s
  `hunks(snapshot)` (pending: `HunkView` with `HunkStatus::Pending` or
  `Stale`, the proposal's `origin()` through `lane_for`). The chip's text is
  a `fe_gpui::ProposalBadge { text: SharedString, stale: bool }` per
  proposal, handed with `EditorView::set_proposal_badges`.
- **Reads it**: a decoration provider in
  `crates/fe-workbench/src/lane_marks.rs` (new), installed with
  `EditorModel::add_decoration_provider`, rebuilt after edits rest (as
  `provenance_column.rs` is) and on `ProposalEvent`s; it emits
  `DecorationKind::Gutter(GutterMark::Dotted)` and
  `DecorationKind::RowBackground` with a new `Tint::Agent`
  (`crates/fe-editor/src/decorations.rs`). Painted by
  `crates/fe-gpui/src/element/decorations.rs` (the change lane, colour from
  `tint_color`, `Tint::Agent` from `EditorTheme`'s agent colour) and the chip
  by `crates/fe-gpui/src/element/proposal.rs`.
- **Gap**: no agent `Tint`, no dotted `GutterMark`, no agent colour in
  `EditorTheme`. The provenance column (`editor.toggle_provenance`) is a
  separate, opt-in annotation and stays as it is.

### Problems and tests: "written by lane 2", "new, lane 2", From agents scope

- **Shows**: a `written by lane 2` chip on a file row whose text the lane
  wrote and the user accepted; `new, lane 2` on a problem on a row the lane
  wrote; a scope switch, This file / Project / From agents, where From
  agents keeps only files with a lane chip. A file with only pending hunks
  gets no chip: its problems are the language server's view of the user's
  text, which holds none of the lane's yet.
- **Data**: `LaneStore::written_files() -> HashMap<PathBuf, Vec<LaneId>>`
  (files with accepted hunks in each lane's changeset or loose proposals,
  and open buffers with rows the lane is author of) for file rows; for a
  problem, its row's `Author` from the open buffer's
  `Provenance::row_authors` through `Lanes::lane_for` (closed files have no
  provenance, so only the file chip). The rows the panel lists stay
  `fe_editor::diagnostics::FileProblems` and `Problem`; the panel's own
  `FileProblems` listing gains `lanes: Vec<LaneId>` and each listed problem
  `lane: Option<LaneId>`, and a new `ProblemScope { ThisFile, Project,
  FromAgents }` filters `rows`.
- **Reads it**: `crates/fe-workbench/src/problems_panel.rs`
  (`ProblemsPanel`, `Row::File` and `Row::Problem`).
- **Tests**: there is no Runs panel or test explorer yet; tasks run in
  terminal tabs (`task_run.rs`, `TaskTab`) and feed Problems. The tests
  row's `new, lane 2` waits for the Runs design, but the data contract is
  fixed here, because a result can arrive after the lane has moved on:
  - Each run captures **what it tested** when it starts: `Tested { lane:
    LaneId, tree: String }`, the git tree id of the worktree's whole state
    (tracked and untracked, written through a temporary index, as a frozen
    revision is), or for a run in the user's tree no lane.
  - Results and the problems a task's output gives carry that `Tested`
    (`TaskTab { tested: Option<Tested> }`, and through `task_problems.rs`'s
    `TaskProblem` to the panel's `SourceProblem`).
  - A result belongs to a frozen revision only if that revision's tree id
    equals `tested.tree`; otherwise it is shown as `stale result, lane 2`,
    never as the current revision's. Overlay lanes run nothing, so have no
    test results.
- **Gap**: no scope switch today; the panel always lists every open buffer,
  unopened files the server reported and task output.

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

### Changes: its own group, no checkboxes, Review link

- **Shows**: under the user's groups, one group per lane with pending work,
  `lane 2 · claude-code` in silver-blue with `Review` at its right; its rows
  are the lane's files with `+a −r` and the pending count, and offer no
  Stage, Unstage or Discard, so they can't reach a commit. Accepted text is
  in the user's buffers and shows in the user's groups as any change.
- **Data**: `LaneSummary` per lane (`LaneStore::summaries()`): its files and
  `Counts` from the lane's `Changeset::files`, plus its loose proposals.
- **Reads it**: `crates/fe-workbench/src/git_panel.rs`: `enum Group { Merge,
  Staged, Changes }` gains `Lane(LaneId)`; lane rows are a new
  `Row::LaneFile(LaneId, usize)` beside `Row::File`, since `Entry` carries a
  `fe_git::ChangeKind` and lane files carry `Counts`. `Action` gets
  `ReviewLane`, which runs `agent.review_lane`.
- **Gap**: the panel has no checkboxes at all today (staging is buttons and
  keys); "no checkboxes" here means no staging actions on lane rows.

### Diff: "After · lane 2" header

- **Shows**: reviewing one of a lane's files side by side, the left header
  `Before` and the right `After · lane 2`, the right header's text
  silver-blue. Comments becoming the lane's next instruction is the review
  panel's (`docs/designs/agent-review.md`); the lane end of it is
  `LaneStore::instruct(lane, text)`, which delivers a `lane.instruction`.
- **Data**: `fe_gpui::DiffSide { title, model, read_only }`
  (`crates/fe-gpui/src/diff_view.rs`) with the title `After · lane 2` and a
  new `DiffSide::agent(bool)` for the header's colour; the right text is
  the buffer with the lane's pending hunks applied,
  `ProposedChange::dry_run(snapshot).text()` (`DryRun`).
- **Reads it**: `crates/fe-workbench/src/lane_diff.rs` (new), opened from
  the Agents panel, the Changes group's rows and the AI Changes panel, as
  `git_diff.rs` opens `DiffView`s with `fixed_side()`.
- **Gap**: headers today are free-form (`HEAD`, `Index`, `Working Tree`,
  `Saved`); there is no Before/After pair or agent header.

### Also: toasts and the inbox

Not a row of the table, but the design system names agent toasts. When a
lane goes `PendingReview`, `toast::show` (`crates/fe-workbench/src/toast.rs`,
`Toast { level, message, detail, action, choices }`) says `lane 2 ·
claude-code is done: 3 files to review`, with Review changes
(`agent.review_lane`) and Later. `Toast` gains a source (`Agent`) so its icon
and action are silver-blue. The inbox is not built yet.

## Today's chat agent

Lane 1 is the built-in chat, as the chat works today. The change is where
its state is kept and what it is called, not what it does.

- **Made first.** `LaneStore::new` makes lane 1 before anything else:
  `LaneKind::Chat`, `Backing::Overlay`, agent `fastedit.chat`
  (`chat_apply::AGENT`), name `Chat`, state `Accepted` (nothing pending) until
  the first send makes it `Working`. Its number is fixed at 1 in the lane
  record too, so a restored record never gives 1 to anything else.
- **Every chat is lane 1.** Each `chat.propose_edit` and
  `chat.review_changes` adds its `Actor::Ai` session (the chat id) to lane
  1's sessions before it proposes. Switching chats, starting a new one or
  restoring History doesn't change the lane.
- **What moves, and where to.**

  | Today | Lane 1 |
  | --- | --- |
  | `Workbench::ai_changes: Option<AiChanges>` | Lane 1's newest turn in its `LaneWork`, the same `AiChanges`, which the AI Changes panel shows |
  | `Workbench::set_changeset(changeset, ..)` replacing the old one | `LaneStore::set_changeset(LaneId(1), changeset, ..)`, with `set_changeset` kept as a one-line call to it; the old turn stays in `LaneWork` while it has pending hunks |
  | Apply's proposal, held only by its editor and `Applies` | Also registered as one of lane 1's loose proposals (`LaneStore::adopt(lane, buffer, proposal)`), so it counts as pending |
  | The chat reply finishing, failing or stopped | The turn's end: lane 1 to `PendingReview` if it has pending hunks |
  | Apply or Review Changes on a finished reply | Hunks added outside a turn: lane 1 back to `PendingReview` |
  | `ProposalEvent`s the AI Changes panel follows | The same, also recounting the lane's state |

- **Behaviour that doesn't change** (the lane-1 card's test is that these
  suites pass unedited): `chat_apply_tests.rs`, `chat_apply_state_tests.rs`,
  `chat_changeset_tests.rs`, `chat_commands_tests.rs`,
  `ai_changes_panel_tests.rs` (including the refactor preview, which shows a
  language server's edit in the same panel and is not a lane),
  `provenance_column_tests.rs`, and fe-editor's `changeset.rs` and
  `scripted_ai*.rs` tests. In particular:
  - Apply still opens the file in the active pane and reveals the first
    hunk; block headers still follow `ProposalEvent`s.
  - The AI Changes panel still shows one changeset, lane 1's, with the same
    header text, rows, buttons, commands and `No pending changes`. Showing
    other lanes there is the review panel's
    (`docs/designs/agent-review.md`), not lane 1's.
  - Accept still lands as `Origin::User` with the text authored by the
    agent; undo still reverts it.
  - The provenance column's labels stay `AI · <agent>` and `Written by
    <agent>`. The `session` added to `Author::Ai` is not shown by it; the
    card that adds the lane to its label (`lane 1 · Chat`) changes those
    tests on purpose and says so.
- **Inline Edit** stays outside lane 1 (§ Lane model › Who creates a lane).
  Its proposals keep `fastedit.inline_edit` and no lane claims it.

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

## Card breakdown

Ordered; each card is at most m (about 2k lines with tests). **∥** marks
cards that can run in parallel with the others in the same wave once their
dependencies have landed. Files are where the card's change is; tests sit
beside them in each crate's `*_tests.rs` convention. Wave C's cards each
register commands or a panel through `workbench.rs` and the command
catalog, so parallel lanes there should expect small rebase conflicts in
those lists, not in each other's files.

**Wave A, no dependencies (all ∥):**

0. **Agent colour tokens** (port of Mac T1509). Size **s**. Deps: none. ∥.
   Files: `kiln/crates/kiln-core/src/theme/tokens.rs` (`agent` group:
   `agent.fg`, `agent.soft`), `kiln/crates/kiln-core/themes/{dark,light}.toml`,
   `crates/fe-config/themes/*.toml` (`[editor]` `agent`, `agent_soft`),
   `crates/fe-gpui/src/lib.rs` (`EditorTheme::agent`, `agent_soft`). If the
   Mac card lands first by sync, this card is dropped.
1. **Headless lane model.** Size **s**. Deps: none. ∥. Files:
   `crates/fe-editor/src/lane.rs` (new: `LaneId`, `Lane`, `LaneKind`,
   `Backing`, `LaneState` with every transition of § Lifecycle and its
   restart reconciliation, `Lanes` with numbering, claims and `lane_for`,
   `LaneCounts`), `crates/fe-editor/src/lib.rs`. Tests: each row of the
   transition table, the restart rules, counts.
2. **`Author::Ai` carries the session.** Size **s**. Deps: none. ∥. Files:
   `crates/fe-buffer/src/provenance.rs` (`Author::Ai { agent, session }`,
   `From<&Origin>`), `crates/fe-buffer/tests/provenance.rs`,
   `crates/fe-workbench/src/provenance_column.rs` (the match arms only;
   labels unchanged, `provenance_column_tests.rs` passes unedited).
3. **Detached hunks.** Size **s**. Deps: none. ∥. Files:
   `crates/fe-buffer/src/proposed.rs` (`DetachedHunk`,
   `ProposedChange::from_detached`: placed hunks checked against their old
   text and refused on a mismatch, unplaced or stale ones stale for good),
   `crates/fe-buffer/src/tests/` (a sticky stale hunk can be rejected,
   never accepted, and survives `refresh`; a mismatched placement is
   refused).
4. **Git worktrees and lane revisions in fe-git.** Size **m**. Deps: none.
   ∥. Files: `crates/fe-git/src/worktree.rs` (new: add detached at a
   commit, remove, list; freeze a worktree's whole state, untracked
   included, as a commit on a given private ref through a temporary index;
   its tree id; the files changed between two commits),
   `crates/fe-git/src/lib.rs`. Tests: freezing in two linked worktrees of
   one repository under different ref namespaces leaves each other's refs
   alone; removing one namespace's refs removes no other's.

**Wave B:**

5. **Three-way projection.** Size **m**. Deps: 3. ∥ with 6. Files:
   `crates/fe-editor/src/lane_project.rs` (new: `lane_project(base, lane,
   current, origin) -> ProposedChange` through `from_detached`: hunks
   placed by § Detached hunks' placement rule, everything else stale; the
   whole-file preconditions of § New and deleted files as
   `FilePrecondition`). Tests: an unchanged current places every hunk,
   repeated lines included; base `x\nx\n` to current `x\n` leaves a
   hunk on either line stale; a target deleted while an identical line
   survives elsewhere is stale; moved text is stale; insertions beside an
   edit are stale; a property test that a placed hunk's window is unique in
   both texts and lands at the diff's image.
6. **Chat as lane 1.** Size **m**. Deps: 1. ∥ with 5. Files:
   `crates/fe-workbench/src/lanes.rs` (new: `LaneStore`, an entity on the
   `Workbench`, lane 1 made first; `LaneWork` with each unresolved turn's
   `AiChanges` and loose proposals; `counts`, `summaries`, `written_files`;
   state from turn ends and `ProposalEvent`s),
   `crates/fe-workbench/src/workbench.rs` (the field),
   `ai_changes_panel.rs` (`ai_changes` becomes lane 1's newest turn, the
   panel a view of it; `set_changeset` delegates and keeps the older turn in
   `LaneWork`), `chat_apply.rs` and `chat_changeset.rs` (claim the session,
   `LaneStore::adopt`), `chat_panel.rs` (a reply finishing, failing or
   stopped ends the turn). Tests: the suites in § Today's chat agent pass
   unedited, plus `lanes_tests.rs`: lane 1's transition rows, Apply after a
   finished reply going back to PendingReview, an older turn still pending
   after a newer Review.

**Wave C, after 6 (all ∥ with each other unless a dependency says so):**

7. **Lane ownership and the record.** Size **m**. Deps: 5, 6. ∥. Files:
   `crates/fe-workbench/src/lane_record.rs` (new: `LaneOwners` and
   `lanes.lock`; the window-local store for a non-owner; `lanes.json` and
   `<lane>.patch.json` with base texts, written through a worker in
   `chat_store_worker.rs`'s pattern, trust-gated, memory-only when
   untrusted; whole-file operations saved with their texts and
   preconditions; restore through `lane_project` and the restart
   reconciliation), `crates/fe-workbench/src/lanes.rs` (owner or local).
   Tests: two windows on one root (one owner; the other refused
   `lane.open`; the lock passed on when the owner closes), a restored hunk
   whose text the user edited coming back stale, a repeated text not
   re-anchored, a pending new file and a pending deletion surviving a
   restart, and each coming back stale when its precondition broke
   meanwhile.
8. **One buffer per path for every lane.** Size **m**. Deps: 5, 6. ∥.
   Files: `crates/fe-workbench/src/lane_buffers.rs` (new: `LaneBuffers`,
   clean buffers with no editor following the disk; accepting on one opens
   its tab first; detaching a closed file's hunks and restoring them),
   `crates/fe-editor/src/changeset.rs` (`BufferSet::load`),
   `crates/fe-workbench/src/ai_changes_buffers.rs` (lends `LaneBuffers`
   buffers), `crates/fe-workbench/src/open.rs` (an editor opened on the path
   takes its buffer and every lane's proposals). Tests: two lanes proposing
   on one unopened file share a `BufferId`, and accepting one stales the
   other; accepting the last pending hunk of a background file opens it
   unfocused, dirty, in a tab, and hot exit then backs it up; closing the
   last editor with hunks pending after discarding edits brings back the
   hunks over the discarded text stale and the rest pending;
   `ai_changes_panel_tests.rs`'s reopen tests still pass (they follow the
   file, and don't assert a new buffer).
9. **Title bar chip and `agents` status item.** Size **s**. Deps: 0, 6. ∥.
   Files: `kiln/crates/kiln-shell/src/title_bar.rs` (trailing slot),
   `crates/fe-workbench/src/title_bar.rs`, `crates/fe-workbench/src/status_bar.rs`,
   `crates/fe-workbench/src/tests/title_bar.rs`.
10. **Agents panel and rail badge.** Size **m**. Deps: 0, 6. ∥. Files:
    `kiln/crates/kiln-ui/src/activity_rail.rs` (`RailBadge`),
    `kiln/crates/kiln-shell/src/dock.rs` (`DockItem::badge`),
    `kiln/crates/kiln-ui/src/file_icons.rs` and `assets/icons/bot.svg`
    (`Icon::Bot`), `crates/fe-workbench/src/dock.rs` (`DockPanel::Agents`),
    `crates/fe-workbench/src/agents_panel.rs` (new). A new rail item and
    commands change rail and command counts in fe-workbench tests outside
    this scope (`chat_rail_tests.rs`, layout and palette tests): the card's
    scope lists them.
11. **Editor lane marks.** Size **m**. Deps: 0, 2, 6. ∥. Files:
    `crates/fe-editor/src/decorations.rs` (`Tint::Agent`,
    `GutterMark::Dotted`), `crates/fe-editor/src/agent_rows.rs` (new:
    `AgentRows`, `LaneMark`), `crates/fe-gpui/src/element/decorations.rs`,
    `crates/fe-gpui/src/element/proposal.rs` and `view/proposal.rs`
    (`ProposalBadge`, `set_proposal_badges`; the `also changed by lane N`
    note), `crates/fe-workbench/src/lane_marks.rs` (new).
12. **Problems: lane chips and the scope switch.** Size **m**. Deps: 0, 2,
    6. ∥. Files: `crates/fe-workbench/src/problems_panel.rs`
    (`ProblemScope`, lane chips), `problems_panel_tests.rs`.
13. **Read-only agent terminals.** Size **s**. Deps: 0, 6. ∥. Files:
    `crates/fe-terminal-view/src/view.rs` (`set_read_only`),
    `crates/fe-workbench/src/terminal_panel.rs` (`Session { lane,
    read_only }`, the tab's colour and name).
14. **Changes: lane groups.** Size **m**. Deps: 0, 6. ∥. Files:
    `crates/fe-workbench/src/git_panel.rs` (`Group::Lane`, `Row::LaneFile`,
    `Action::ReviewLane`), `git_panel_groups_tests.rs`.
15. **Lane diff with the "After · lane N" header.** Size **s**. Deps: 0, 6.
    ∥. Files: `crates/fe-gpui/src/diff_view.rs` (`DiffSide::agent`),
    `crates/fe-workbench/src/lane_diff.rs` (new: `agent.diff_lane_file`,
    right side from `DryRun::text`).
16. **Agent toasts.** Size **s**. Deps: 0, 6. ∥. Files:
    `crates/fe-workbench/src/toast.rs` (`Toast` source, silver-blue
    action), `crates/fe-workbench/src/lanes.rs` (toast on `PendingReview`;
    its only edit there, a call at the state change).
17. **Lanes in the provenance column.** Size **s**. Deps: 2, 6. ∥. Files:
    `crates/fe-workbench/src/provenance_column.rs` (`lane 1 · Chat` where a
    lane claims the row, `AI · <agent>` otherwise),
    `provenance_column_tests.rs` (changed on purpose: the one card that
    edits them).

**Wave D:**

18. **Worktree lane revisions and the ledger.** Size **m**. Deps: 4, 5, 7,
    8. Files: `crates/fe-workbench/src/lane_worktree.rs` (new: make and
    clear a lane's worktree as owner; freeze revisions; project
    `r<k-1>`→`r<k>`; `<lane>.ledger.json`; rebuild with decisions applied),
    `crates/fe-editor/src/changeset.rs` (`Kind::Deleted`, and the new-file
    and deletion preconditions for every lane), `crates/fe-workbench/src/lanes.rs`
    (`Backing::Worktree`; the tree-id check that starts the next turn).
    Tests: a rejected hunk not resurrected by the next revision or a
    restart; an accepted hunk not turned into a conflict; add/add,
    delete/modify and already-deleted outcomes; refs made under the root's
    layout key and only those cleared.
19. **New agent terminal, Review lane and Update lane.** Size **s**. Deps:
    13, 18. ∥ with 20. Files: `crates/fe-workbench/src/terminal_panel.rs`
    (`agent.new_terminal`: a session with the worktree as cwd; its exit
    freezes a revision), `crates/fe-workbench/src/lane_worktree.rs`
    (`agent.review_lane`, `agent.update_lane` and its imported revision).
    Tests: Update lane refused with an undecided revision; after it, the
    user's changes are not in the next review; a running terminal lane
    reviewed, decided, then written to again goes back to Working and to
    PendingReview at its next Review lane.
20. **The external agent seam.** Size **m**. Deps: 6, 7, 8. ∥ with 19.
    Files: `crates/fe-plugin/wit/lane.wit` (new: `open` (with a session to
    reopen), `propose`, `finish`, an `instruction` event, liveness),
    `crates/fe-plugin/src/lane.rs`, `kiln/crates/kiln-plugin/src/warnings.rs`
    and the manifest check (`agent.lane` refused with `buffer.write`,
    `fs.write:workspace` or `process.spawn:*`),
    `crates/fe-workbench/src/plugin_bridge.rs` and `plugin_host.rs` (the
    grant re-checked at `lane.open`; `Origin::Ai` for the plugin's lane),
    plus the same calls as kiln commands (`lane.open`, `lane.propose`,
    `lane.finish`) so a later MCP or CLI adapter reaches them without WIT.
    Tests: each refused grant combination; a disconnect abandoning the
    lane and a reopen of its session resuming it.

**Later, not filed from this design:**

- **Tests' `new, lane N` chip**, with `Tested` carried through task
  results: after the Runs design and its panel.
- **Agent tasks in Runs** (`LaneKind::Task`): after Runs and card 18.
- **`lane.run`**, an external agent running a command in its worktree's
  read-only terminal: after cards 19 and 20, with the command permissions
  design (`docs/designs/agent-command-permissions.md`).
- **The review panel** (`docs/designs/agent-review.md`), which builds on
  cards 6, 14 and 15 and owns comments as the next instruction.

## Open questions

1. **Lane numbers.** Numbered per workspace for as long as its lane record
   lasts (so `lane 7` stays `lane 7` in history), or restarted at 2 each
   session so the numbers stay small?
2. **Two windows on one root.** Here one window owns a root's lanes and
   another on the same root has only its own memory-only chat lane, and is
   refused new lanes. Should a second window instead see and review the
   owner's lanes (a shared `LaneStore` per root, with buffers shared across
   windows, which they aren't today)?
3. **CLI agents in an ordinary terminal** write the user's tree directly and
   aren't lanes. Detect known agent commands (`claude`, `codex`, `aider`)
   and offer to restart them in an agent terminal, or leave them alone?
4. **Worktrees.** Where (the state directory, as designed, or `.worktrees/`
   in the repo as regatta does), how many at once (a cap such as 4), and
   does each get its own `target/` or share one (`CARGO_TARGET_DIR`)?
5. **The CLI agent's own terminal.** The design system says an agent's
   terminal is read-only. Here, terminals an agent runs commands in are
   read-only, but a terminal lane's tab, where the user talks to a CLI
   agent, is writable. Right, or should those be split into a read-only
   output tab and an input elsewhere?
6. **Inline Edit** is not a lane here (decided in place, right away).
   Should it be lane 1's, so its rows read `lane 1` too?
7. **How long the dotted lane stays.** Accepted agent text keeps the mark
   as long as provenance does (until the row is rewritten). Should it fade
   at save, at commit, or never?
8. **Untrusted folders** keep lanes in memory only and refuse terminal
   lanes. Acceptable, or should terminal lanes be allowed there with the
   worktree deleted on close?
9. **Lane 1 for a window that never chats.** The chat holds lane 1 whether
   or not it is used, so the first external agent is lane 2. Fine, or
   should numbers go in order of first work?
10. **Agent plugins and `buffer.write`.** This design gives an agent
    `agent.lane` and never `buffer.write`. Should a plugin that holds both
    (a formatter that is also an agent) be refused, or have its direct edits
    marked as the agent's anyway?
