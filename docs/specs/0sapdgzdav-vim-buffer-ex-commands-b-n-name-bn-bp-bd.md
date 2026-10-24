---
task_uid: "01m5qphwyhmj0hz80sapdgzdav"
task: "T326"
rev: 4
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "fe-vim tests/ex_buffers.rs: ex::parse gives HostCommand::Buffer(..) for :b, :b 3, :3b, :b name, :b#, :bn[ext], :bN, :bp[revious], :bd[elete][!] [n], :bf[irst], :bl[ast], :br[ewind] with nvim's shortest abbreviations (the expected exists(':bne')-style values are stated in the test from nvim 0.12.4), count forms, and E471 for a missing argument where nvim gives it; the other :b-prefixed names (:bufdo, :bm) keep their current E492/User parse"
    required: true
    manual: false
  - id: "C2"
    text: "vim_buffers_tests::numbering: each file opened in the window gets the next number, never reused after :bd, shown by :b N; :b N activates the tab if the current pane has it, else opens the file in the current pane; :b# goes to the previously active tab and E23 when there is none; :b with no argument changes nothing Limit (Known limits, review round 2): the registry observes the workspace at effect time, and gpui coalesces notifications per update, so several raw pane activations, additions or removals within one update are seen only as the final state (the commands record their own switches); follow-up card F2 hooks Pane::activated and Pane::items_changed."
    required: true
    manual: false
  - id: "C3"
    text: "vim_buffers_tests::name_and_cycle: :b part matches a unique path substring (case-insensitive when lower-case), E93 on several matches, E94 on none; :bn and :bp (with counts) cycle by number and wrap, :bf/:bl go to the ends Limit (Known limits, review round 3): (a) ranges and counts are not fully validated (`:0bd 2`, `:9bd 2`, `:0,2b` run where nvim errors, a backwards range runs where nvim gives E16, an overflowing prefix count becomes 1) [F8]; (b) `$` resolves to the highest listed buffer, not the last known one [F9]; (c) escaped filenames are matched literally [F12]. Follow-up T334 fixes these."
    required: true
    manual: false
  - id: "C4"
    text: "vim_buffers_tests::delete: :bd closes the buffer's tab(s) in this window and leaves the window open, E89 for a dirty buffer unless !, the active tab moves to the alternate buffer as nvim does; :bd on the last file leaves an empty pane, not a closed window Limit (Known limits, review round 3): (d) deletion assumes its replacement tab has already opened, so an asynchronous open with intermediate tab activations can cancel the deletion [F10]; (e) a synchronously completed path switch is not recorded in the history [F11]. Follow-up T334 fixes these."
    required: true
    manual: false
  - id: "C5"
    text: "cargo test -p fe-vim -p fe-workbench and cargo clippy -p fe-vim -p fe-workbench --all-targets -- -D warnings pass; the oracle-based suites are unaffected (the oracle compares buffer text, cursor, mode, registers and marks only, so it cannot see window or buffer lists: parse tests carry the nvim facts instead)"
    required: true
    manual: false
---

# Vim buffer ex commands: :b {n|name|#}, :bn, :bp, :bd and :bfirst/:blast over the open files, numbered as nvim numbers buffers (Mac T1136)

## Problem

User 2026-10-24: "Do we offer good vim control of the explorer/outline?" and, with it, switching files: a vim user expects :b, :bn, :bp and :bd. fe-vim's host commands are only :w :q :wq :x :e :qa :wa :sav :r :Ex :oldfiles :copen :cclose :Replace and the capital-letter user commands (crates/fe-vim/src/ex.rs:267-310, :835-853).

## Current state

- `ex::parse` returns `HostCommand` for what the host must do; vim_host.rs `run_vim_host_command` (:90-190) and `run_window_command` (:194-215) run them on the editor's tab and window. `:Pick` lists exist (ex/pick.rs) for the next card (:ls).
- Tabs per pane are the workbench's buffers: pane.rs has `position(path)` (:1131) and the tab strip; there is no buffer number anywhere.
- Day-one pattern to copy: T522 (`:Ex`, `:oldfiles`, `:copen`) added a HostCommand variant, a completion-list entry (ex/complete.rs) and a vim_host arm.

## Change

- fe-vim: new ex/buffers.rs with `BufferCommand { Switch(Target), Next(u32), Previous(u32), Delete{target, force}, First, Last }` and `HostCommand::Buffer(BufferCommand)` (one variant, edited in ex.rs); add names to ex/complete.rs.
- fastedit: new vim_buffers.rs on Workbench: a registry mapping path to a number assigned on first open in the window (untitled tabs get one too), consulted by the commands; arms in vim_host.rs call it. Semantics are nvim's: :b N shows the buffer in the current window; :bn/:bp wrap; :bd keeps the window.
- Decisions made here, so they need not be asked: numbers are per window session (not persisted), and a buffer in another pane is opened as a tab in this pane rather than moving focus there.

## Tests

New files fe-vim tests/ex_buffers.rs (parse and abbreviation facts from nvim) and fe-workbench vim_buffers_tests.rs (headless, declared from vim_buffers.rs). Serialise after the other cards touching ex.rs/vim_host.rs: this is the first of the ex-command chain.

Lend note (helium lead): spec, scope and criteria are the Mac's full JSON (y57), not truncated.

## Implementation notes

- Scope was widened (respec) to `crates/fe-vim/src/lib.rs` (re-export `BufferCommand`, `BufferTarget`, `BufferAddress`) and `crates/fe-workbench/src/git_diff.rs` / `git_commit.rs`, whose exhaustive `HostCommand` matches need the new arm (both hand it to `run_window_command`, which runs it on the active pane).
- `Target` is `BufferTarget` (the crate has a `Target` already): `Current`, `Alternate`, `Number(n)` (an argument: E86 past the last), `Name`, and `Range { first, last }` for a range of buffer addresses (`N`, `.`, `$`, `%`: E16 past the last). `:b` takes its range's last; `:bn`/`:bp` read a number range as their count.
- The registry is a gpui `Global` keyed by the workbench's entity id (pruned as workbenches go), as pane.rs keeps `.editorconfig` results, since workbench.rs is out of scope. It observes the window's workspace (installed as the workbench first follows its active editor, held in the global), so buffers are numbered and each pane's shown history noted as tabs open, show or close, not at render; the commands also note what they show at once. A buffer is identified by its editor's text (`BufferId`) and its path, so a rename or Save As keeps its number and a closed file opened again gets its old one. Each pane's history gives `#` and what `:bd` shows next.
- Name matching follows `buflist_findpat`'s four attempts (anywhere, start, end, whole) over listed buffers, then closed ones (`:b` only), on both the path relative to the folder and the full path, a buffer counted once. Case is ignored for a lower-case name, as the criterion says (nvim on Linux is case-sensitive; macOS's 'fileignorecase' isn't).
- `:bd` restores focus to the line's pane after closing tabs in other panes (closing another pane's shown tab focuses its neighbour).
- A `|` after a buffer command is refused before the line runs (review F1): the host switches once the line has run, so `:b 2 | d` would edit the buffer left. A command before one (`:2 | bn`) runs, as in nvim.
- Not supported (a "not supported yet" error): `+cmd`, `:bd` of several arguments, offsets or marks in a buffer range, a command after a buffer command.
- A buffer command run by `:g` or `:normal` is refused as it runs, stopping them (review round 2, F7), for the same reason as a `|` after one.
- A buffer that takes a closed buffer's file by Save As takes the file: the closed one keeps its number but no file (review round 2, F6).
- Known limit (review round 2, F2): the registry learns of tab changes from the workspace's notifications, which gpui coalesces per update, so within one update several shown-tab changes made by anything but these commands are seen as the last, a tab opened and closed is never numbered, and tabs added together are numbered in pane order. Recording every transition needs hooks at the mutation sites in pane.rs (`Pane::activated`, `items_changed`), outside this card's scope: a follow-up card.
- Known limits (review round 3, review nhj7cs; disputed, follow-up T334 fixes them):
  - (a) [F8, C3] ranges and counts are not fully validated: `:0bd 2`, `:9bd 2` and `:0,2b` run where nvim errors, a backwards range (`:2,1bn`, `:2,1bf`) runs where nvim gives E16, and an overflowing prefix count becomes 1.
  - (b) [F9, C3] `$` resolves to the highest listed buffer, not the last known one, so after the highest-numbered buffer is deleted `:$b`, `:%b` and `:$bd` act on the one below it.
  - (c) [F12, C3] escaped filenames are matched literally: `:b foo\ bar` gives E94, and `:bd foo\ bar` is refused as several buffers.
  - (d) [F10, C4] deletion assumes its replacement tab has already opened: when the replacement is still being read into the line's pane (a viewer's file), removing the deleted tabs activates another and cancels that open, and the pane can close.
  - (e) [F11, C4] a path switch that `Pane::open` completes at once, sharing another pane's editor, returns before the switch is noted in the pane's history, so `#` can miss it.
