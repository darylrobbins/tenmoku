---
task_uid: "01m7b0n33haan27e1ejhpffaf0"
task: "T374"
rev: 2
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "Test in crates/fe-git/tests/worktree.rs: freezing in two linked worktrees of one repository under different ref namespaces leaves each other's refs alone. Known limit: in a sparse checkout, a modified tracked file outside the sparse cone is not captured by the freeze (git filters those paths itself); that is follow-up T376"
    required: true
    manual: false
  - id: "C2"
    text: "Test in crates/fe-git/tests/worktree.rs: removing one namespace's refs removes no other's"
    required: true
    manual: false
---

# Git worktrees and lane revisions in fe-git: add detached, remove, list, freeze a worktree whole state as a commit on a private ref through a temporary index, tree ids, changed files (agent lanes card 4) (Mac T1799)

# Git worktrees and lane revisions in fe-git: add detached, remove, list, freeze a worktree whole state as a commit on a private ref through a temporary index, tree ids, changed files (agent lanes card 4)

Source: `docs/designs/agent-lanes.md` § Card breakdown, card **4** (the agent lanes design, Mac T1567, on main as e553a094a). The design files no cards itself; this is its card 4. Read the sections it cites before starting (quoted below or named).

Linux-OK: nothing in the card needs macOS; it builds and tests on Linux.

## The card, verbatim from § Card breakdown

**Wave A, no dependencies (all ∥):**

4. **Git worktrees and lane revisions in fe-git.** Size **m**. Deps: none.
   ∥. Files: `crates/fe-git/src/worktree.rs` (new: add detached at a
   commit, remove, list; freeze a worktree's whole state, untracked
   included, as a commit on a given private ref through a temporary index;
   its tree id; the files changed between two commits),
   `crates/fe-git/src/lib.rs`. Tests: freezing in two linked worktrees of
   one repository under different ref namespaces leaves each other's refs
   alone; removing one namespace's refs removes no other's.

### The breakdown's preamble, verbatim

Ordered; each card is at most m (about 2k lines with tests). **∥** marks
cards that can run in parallel with the others in the same wave once their
dependencies have landed. Files are where the card's change is; tests sit
beside them in each crate's `*_tests.rs` convention. Wave C's cards each
register commands or a panel through `workbench.rs` and the command
catalog, so parallel lanes there should expect small rebase conflicts in
those lists, not in each other's files.

## The AI agents record (`docs/decisions/50t8wt9we2-ai-agents.md`, T1779, helium 629e37c1)

**AI agents dependency.** This card is one of the agent-lanes cards the AI agents record (`docs/decisions/50t8wt9we2-ai-agents.md`, T1779, § 10 › Needed first) lists before **AG11** (Mac T1792, the Claude Agent session host): cards 1, 3, 4, 5, 6, 7, 8 and 18. The record makes Claude Agent `LaneKind::External { host: "acp" }` with `Backing::Worktree`, made by the workbench when a Claude Agent chat starts (not through `lane.open`), agent `acp.claude-agent`, name "Claude Agent", its `sessions` holding the ACP session id.

The record's § 3 › Claude Agent runs in a worktree lane: "The worktree starts from the user's saved tree, not from `HEAD`. It includes tracked changes and untracked files that are not ignored, frozen as an imported revision (agent-lanes' Update lane)." So freezing a worktree's whole state "untracked included" means untracked files **not ignored** (respect `.gitignore` and `info/exclude`), and freezing must work on the main checkout's tree as well as on a lane worktree (the imported revision is frozen from the user's tree). Following the record. Resolving the git common and admin dirs for the sandbox is AG6's (T1787, fe-acp), not this card's.

## Design sections it cites, verbatim

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

## Scope

- `crates/fe-git/src/worktree.rs`
- `crates/fe-git/src/lib.rs`
- `crates/fe-git/tests/worktree.rs`: new test file, fe-git's `tests/` convention (beyond the card's file list).
- `crates/fe-git/l10n-allow.toml`: fe-git's literal lint is `all = true`; git arguments and ref names are allowed literals there, as `src/branch.rs`'s are (beyond the card's file list).

## Criteria

Each Tests item of the card, verbatim, as a criterion.

1. Test in `crates/fe-git/tests/worktree.rs`: freezing in two linked worktrees of one repository under different ref namespaces leaves each other's refs alone
2. Test in `crates/fe-git/tests/worktree.rs`: removing one namespace's refs removes no other's

## Notes

- Ref layout is `refs/tenmoku/lanes/<layout_key(root)>/<n>/r<k>` (§ Worktree lanes); fe-git takes the namespace prefix as a parameter, the workbench (card 18) builds it from `kiln_core::layout::layout_key`.
- Use the git CLI as the rest of fe-git does (`crates/fe-git/src/cli.rs`); the temporary index is `GIT_INDEX_FILE` in a temp dir, never the worktree's own index.

## Implementation notes

- API (`crates/fe-git/src/worktree.rs`): `Repo::add_worktree_detached`, `remove_worktree`, `worktrees`; `RefNamespace` (validated `refs/…` prefix, revision `k` is `<namespace>/r<k>`); `Repo::worktree_tree` (the live tree id through a temporary index), `freeze` (commit on the revision ref, fixed `tenmoku` identity, unsigned), `revisions`, `delete_namespace` (one `update-ref --stdin` transaction, `--no-deref`), `tree_id`, `changed_files`.
- The temporary index starts as a copy of the worktree's own index, keeping its mtime so git's racy-clean check still applies; assume-unchanged and present skip-worktree flags are cleared in the copy, and every command on it runs with `core.splitIndex=false` and `core.ignoreStat=false`.
- Known limit (review round 2, F5, follow-up): in a sparse checkout, files on disk outside the sparse cone are not staged by `add --all`, so their changes are not frozen. The fix (`add --sparse`, git 2.34+, keeping absent sparse entries) is follow-up T376.
