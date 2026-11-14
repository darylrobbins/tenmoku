---
task_uid: "01m7azse4hkc12xpeen0t97ad7"
task: "T620"
rev: 3
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "Test in crates/fe-workbench/src/agent_tools_tests.rs: caps and truncation lines"
    required: true
    manual: false
  - id: "C2"
    text: "Test in crates/fe-workbench/src/agent_tools_tests.rs: deny list"
    required: true
    manual: false
  - id: "C3"
    text: "Test in crates/fe-workbench/src/agent_tools_tests.rs: outside-root refusal"
    required: true
    manual: false
  - id: "C4"
    text: "Test in crates/fe-workbench/src/agent_tools_tests.rs: pending-proposal view in `read_file`"
    required: true
    manual: false
  - id: "C5"
    text: "Test in crates/fe-workbench/src/agent_tools_tests.rs: overlapping edits refused"
    required: true
    manual: false
  - id: "C6"
    text: "Test in crates/fe-workbench/src/agent_tools_tests.rs: supersession across turns (§ 12): a→b then b→c, a partly decided earlier proposal, a stale hunk, a pending new file edited again"
    required: true
    manual: false
---

# Workbench tool executors for the API chat: read_file, list_files, search and propose_edit, with ChatProposals supersession across turns (AI agents AG7, T1779) (Mac T1788)

# Workbench tool executors for the API chat: read_file, list_files, search and propose_edit, with ChatProposals supersession across turns (AI agents AG7, T1779)

Source: `docs/decisions/50t8wt9we2-ai-agents.md` § 13, card **AG7** — T1779's decision record (AI agents: Claude Agent over ACP, a tool loop for the API chat, permissions and review), approved by the lead with round-4 findings F6–F11 carried into card criteria (§ 14). Read the record's sections the card cites before starting.

Linux-OK: the card can be built and tested on Linux (§ 13; no card in the plan is Mac-only).

## The card, verbatim from § 13

**AG7. Workbench tool executors.** Size **m**. Deps: AG1, AG3, T604.
Linux-OK. **[chat_panel.rs]**

- Files:
  - `crates/fe-workbench/src/agent_tools.rs` (new: `read_file`,
    `list_files`, `search` and `propose_edit`, and `ChatProposals` with
    supersession (§ 6). Built over `chat_apply::locate` and `propose`,
    `chat_context::refusal`, `fe_project::search_filtered`, and a turn's
    `Changeset`);
  - `crates/fe-workbench/src/chat_panel.rs`, visibility only:
    `mod chat_apply` and `mod chat_context` become `pub(crate) mod`
    (`chat_panel.rs:82-85`), so a sibling module can reach them;
  - `crates/fe-workbench/src/chat_apply.rs`, visibility only: `locate`
    (`:424`) and `propose` (`:537`) become `pub(crate)`;
  - `crates/fe-workbench/src/lib.rs`.
- New test file: `crates/fe-workbench/src/agent_tools_tests.rs`.
- Tests:
  - caps and truncation lines;
  - deny list;
  - outside-root refusal;
  - pending-proposal view in `read_file`;
  - overlapping edits refused;
  - supersession across turns (§ 12): a→b then b→c, a partly decided
    earlier proposal, a stale hunk, a pending new file edited again.

## § 12 workbench loop tests: cross-turn proposals, verbatim

- **Cross-turn proposals** (AG7): turn 1's a→b, then turn 2's b→c, leaves
  one pending a→c proposal in turn 2, and turn 1's hunk reads as superseded.
  The tests also cover a partly decided earlier proposal, a stale hunk, and
  a pending new file edited again.

## § 12 Workbench tests, verbatim

### Workbench tests

Workbench tests use GPUI's test platform, and Linux temp folders (`.txt` and
`.go` files, not `.rs`, so no language server starts).

## Plan-wide rules, verbatim from § 13

Cards are size **s** or **m**. Scopes are exact files, and tests are in new
files. "Linux-OK" means the card can be built and tested on Linux. Marks:

- **[chat_panel.rs]** touches `crates/fe-workbench/src/chat_panel.rs`;
- **[settings.rs]** touches `crates/fe-config/src/settings.rs`;
- **[Cargo deps]** adds Cargo dependencies, and needs the `deny.toml`
  licence check.

Cards that touch T604's files depend on **T604** (the picker card).

New fe-workbench modules are declared from `crates/fe-workbench/src/lib.rs`,
not from `chat_panel.rs`. Each card's tests are declared from its own new
module.

Order: wave 1 in parallel; then AG7, AG8 and AG9; then AG10. AG11 waits for
its lanes cards, then AG12 and AG13 follow.

- **Mac-only cards:** none. Every card is Linux-OK.
- **Mac check:** the sandbox's macOS path (Seatbelt) is checked in the
  manual smoke run on a Mac.

T604 in the record is Mac T1780 (the model and effort picker), already on main.

## Scope beyond the card's file list

- `crates/fe-workbench/l10n-allow.toml`: truncation lines and tool-result text are model-facing literals under fe-workbench's `all = true` lint.

## Notes

- On main after T1780, `mod chat_apply` and `mod chat_context` are at `chat_panel.rs:89` and `:91` (the record cites `:82-85` at its base).
## Round-2 decision (approver, Mac lead): F2 option (a)

Carry every live, undecided proposal into the newest turn's changeset when `set_changeset` replaces it. Section 8 says the panel shows the newest turn, and section 6 supersession already moves a file's pending work into the current turn; (a) applies that to all pending work. Wherever the card or the record's § 6 says one proposal lives in one turn, read it as: **pending work moves to the latest turn's review**. Scope unchanged. Then rgt finish once.
## Round-3 scope widening (lead)

Scope now also covers `crates/fe-workbench/src/ai_changes_panel.rs` (F2: `set_changeset`/`AiChanges` hands the replaced Changeset back to its chat, or emits its per-file states and hunk memo, so `ChatProposals` can absorb decisions and rebind) and `crates/fe-buffer/src/buffer.rs` (F12: add `Buffer::from_reader_with(reader, FileEncoding)` mirroring `from_reader_detecting`, decode the bounded bytes in the chosen encoding). F13 (`editorconfig_for` reading a FIFO) is pre-existing and shared with the pane's ordinary loader: dispute it as out of scope, to be a follow-up card. Then `rgt finish` once.

## Implementation notes

- `crate::agent_tools` holds the four executors and `ChatProposals`.
  `list_files` and `search` are blocking (run them off the main thread, with
  the turn's cancel flag); `read_file` and `propose_edit` take the
  workbench and an `Rc<RefCell<ChatProposals>>` and return a
  `Task<Result<String, String>>` (the error side is the `is_error` text).
- The loop (AG10) calls `ChatProposals::begin_turn(turn_id, origin)` at a
  turn's start and `end_turn()` at its end, which builds the turn's
  `Changeset` (one entry per file, in the order first proposed; `None` if
  the turn left no live proposal) for `Workbench::set_changeset`. Building
  it at the end keeps a file re-proposed within one turn to one entry.
- An earlier turn's changeset is reached as the one the workbench shows
  (`Workbench::ai_changes`), when its turn id matches: an existing file's
  superseded hunks are rejected through the shared proposal, a new file's
  entry through `Changeset::reject_file`. A turn no longer shown has nothing
  left to update.
- "Replaced by a later change" rows read `ChatProposals::is_superseded(hunk)`
  and `is_new_file_superseded(turn, path)`. A stale hunk isn't superseded:
  it stays stale for the user to reject, and is left out of the new
  proposal.
- `chat_apply::Located` became `pub(crate)` with `locate`, since `locate`
  returns it.
- The model's text in `agent_tools.rs` is allowed as a whole file in
  `l10n-allow.toml`, as fe-chat's `tools.rs` is.

### Review round 1 (review 32zw4w)

- F1: a call's file is vetted once, off the main thread (`vet`): `resolve`,
  then `chat_context::open_vetted`, which walks and opens the file and
  refuses FIFOs and anything but regular files and folders. Its real path
  keys the proposals; an open buffer, or one opened for the call, stands for
  the file only if its stamp has the walked file's device and inode. Files
  over 1 MiB are refused before they're opened.
- F2: proposals are never dropped for being out of sight; only a later one
  replaces them. A file whose proposal's editor closed is opened again and
  `sync_ai_changes` lets the shown changeset `rebind` it; the running turn's
  own proposal (in no changeset yet) is made again on the reopened buffer
  when its text is the one proposed against. A pending new file keeps its
  last-seen state while another turn's changeset is shown.
- F3: a proposal reaches the editors only with its turn's changeset, through
  `set_changeset`, so every accept is armed and adopted and Revert Turn
  covers it; `propose_edit` no longer calls `set_proposals`.
- F4: a pending new file's view is its proposed text, whatever editor
  previews it; the read-only preview is never the base.
- F5: a first line over 64 KiB is cut and the truncation line says the rest
  of that line was left out.
- F6: files are opened with `Pane::open_as_text`, so an SVG gets an editor.
- F7: open buffers are looked for in every window, the asking one's first.
- F8: a view over 1 MiB, or binary, is refused before it's copied out of the
  rope; a proposal that would make one isn't made.

### Review round 2 (review xyy99x)

- F1: no call reads a file by its path any more. A file no editor has is
  read through the handle the vetting walk opened (at most 1 MiB, decoded
  as `Buffer::from_file` decodes, stamped before the read) and given a tab
  with `Pane::add`, so a file, parent or link swapped after the walk, a
  FIFO, or a file grown past 1 MiB is never read. `Pane::open_as_text` is
  no longer used. Known difference: a file opened this way isn't decoded in
  a `.editorconfig` charset (`loader::read_in_charset` is private to
  loader.rs, outside the card); its bytes are detected as any file's are.
- F8: a pending view is refused before the dry run when the buffer is over
  1 MiB, so no conflicting text is copied out of it.
- F9: `propose_edit` proposes only on a buffer this window's AI Changes
  panel reaches: a file open only in another window gets a tab here sharing
  that buffer (`EditorModel::split`), unsaved edits kept.
- F2 is open: see the block note. It asks who owns a turn's proposals once
  the workbench shows another changeset, which § 6 and § 8 leave to the
  single shown changeset.

### Round-2 decision: F2 option (a)

- `ChatProposals::end_turn(shown)` builds the turn's changeset from its own
  files, in the order first proposed, then every earlier turn's proposal
  still undecided (a hunk pending or stale, or a new file pending as the
  shown changeset last says), by path; those become the new turn's. A turn
  that proposed nothing returns a changeset only if some undecided work is
  in no changeset the workbench shows (another review replaced it).
- `agent_tools::end_turn(workbench, proposals, window, cx)` is the loop's
  turn end: it gives each undecided proposal whose editor closed a tab
  again (from the vetted handle), so the shown changeset rebinds it or it's
  made again there, then hands the changeset to `set_changeset`.
- A proposal no changeset follows (its turn's review replaced) is made again
  on a reopened buffer holding the text it was made against, from any turn,
  unless a hunk was accepted; hunks rejected as last seen are rejected again.

### Review round 1 of revision 2 (review y7jg1a)

- F2: the chat no longer rebinds a proposal a changeset holds: once handed
  in, `Changeset::rebind` follows it, and a stale hunk it refuses leaves the
  proposal reading as no longer applying, its buffer binding untouched. The
  chat makes again only a proposal never handed in, which no review showed,
  and not if a hunk was found stale. `end_turn` reads the shown changeset
  before replacing it, so decisions made there just before the next turn
  are kept; a new file accepted while another review replaced the chat's is
  known by the editable buffer the accept made.
- F9: whether the receiving workbench reaches a proposal's buffer is checked
  against its own editors; `read_file` and the turn's end give it a tab
  sharing another window's buffer when only that window has it.
- F10: a partly decided proposal is carried as a new proposal of its
  pending changes (its pending hunks superseded in the old one), so each
  review keeps its own decisions and Revert Turn checkpoint.
- F11: the bounded loader applies the pane loader's `.editorconfig` charset
  rules (charset encoding unless another BOM, BOM-less UTF-16 or
  undecodable bytes say otherwise); a test compares it with `loader::read`.

### Review round 2 of revision 2 (review wrggwj), revision 3 scope

- F2: the AI Changes panel now hands a replaced changeset back to the chat
  that handed it in: `Workbench::set_changeset_returning` stores a
  `Returned` hook on `AiChanges`, which its `Drop` calls with the changeset
  as it then stood, however the review is replaced or cleared (a later
  turn, Review Changes, a refactor preview). `ChatProposals` keeps each one
  while a live proposal is still its turn's: a new file's state is read from
  it (a rejection or accept made just before the replacement, and an accept
  whose tab then closed, stay decided), and a pending proposal whose file is
  reopened is rebound through it (`Changeset::rebind`, its memo). The earlier
  Known limits are gone.
- F12: `Buffer::from_reader_with(reader, FileEncoding)` (fe-buffer) decodes
  the bounded bytes already read in the charset's encoding; nothing is read
  again, and the buffer keeps the stamp of the read.
- F13 (the `.editorconfig` lookup reads a FIFO named `.editorconfig` and
  blocks) is disputed as pre-existing: `fe_config::editorconfig_for` reads
  ancestor files by path for every open through the pane's own loader
  (`loader::read`) on main. Its fix is in fe-config's `editorconfig.rs`, a
  follow-up card.

### Review round 1 of revision 3 (review 3245xz)

- F14: a changeset given back rebinds from its memo, its hunks as last
  seen before the replacement, while the editors can still decide them. So
  `rebind_kept` checks the rebuilt proposal against the one the editors
  hold: decisions only go one way, so equal status counts mean the same
  statuses; otherwise the proposal is left as it was, on its old buffer,
  reading as no longer applying, rather than have a rejection, an accept or
  a staleness recorded after the replacement undone. An accept made then
  isn't under any review's checkpoint, as no review shows that turn.
- F15: whether this window's review reaches a buffer is judged by its tabs
  (`open_editors`, as `AiChanges` reaches them), not by diff views' working
  sides; a file only a diff shows gets a tab sharing its buffer before the
  proposal is made or handed in.

### Review round 2 of revision 3 (review f0jc92)

- F14: the rollback is gone. A changeset given back keeps observing until
  its proposals move to a newer review: `agent_tools::watch` follows every
  tab editor's `ProposalEvent`s and every pane (for new tabs), and files a
  decision on a given-back review's proposal as the AI Changes panel files
  one on the review it shows: an accept by `Changeset::adopt_accept`, under
  its checkpoint, then `observe` and `arm`. Each tool call and turn end
  takes those reviews' memos again too. So `rebind_kept` rebinds from a
  current memo: a hunk rejected after the replacement stays rejected, the
  others stay pending and are carried on, and the proposal, the
  changeset's buffer and its checkpoint stay consistent. A hunk the editor
  found stale refuses the rebind, as `Changeset::rebind` refuses one. The
  rebound proposal is shown in its editor again.

