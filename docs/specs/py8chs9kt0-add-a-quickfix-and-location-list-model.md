---
task_uid: "01m6rmmkmpj19dcspy8chs9kt0"
task: "T481"
rev: 2
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "ex_quickfix::cnext_and_friends_parse_to_quickfix_commands: :cn, :cnext 3, :3cnext, :cp, :cN, :cfir, :cla, :cc 4, :cnf, :col 2, :cnew, :cl and every :l twin parse to the matching QuickfixCommand with location set only for the :l forms"
    required: true
    manual: false
  - id: "C2"
    text: "ex_quickfix::quickfix_commands_refuse_other_ranges: a range other than the count forms is E481 and :cnext 1 2 is a trailing-characters error"
    required: true
    manual: false
  - id: "C3"
    text: "quickfix_tests::next_and_previous_do_not_wrap_and_report_e553_and_e42: past the end is E553, an empty list E42, and :cc N above the count selects the last item"
    required: true
    manual: false
  - id: "C4"
    text: "quickfix_tests::stack_keeps_ten_lists_and_colder_cnewer_walk_it: the eleventh push drops the oldest"
    required: true
    manual: false
  - id: "C5"
    text: "quickfix_tests::location_lists_belong_to_their_pane: two panes have separate location lists, dropped with the pane"
    required: true
    manual: false
  - id: "C6"
    text: "quickfix_commands_tests::cnext_opens_the_item_in_the_active_pane_at_its_row_and_column: the typed command quickfix.next and the vim line :cnext both land there and the status line reads (2 of 3) with the item text"
    required: true
    manual: false
  - id: "C7"
    text: "cargo clippy --workspace --all-targets -- -D warnings and cargo nextest run -p fe-vim -p fe-workbench pass"
    required: true
    manual: false
---

# Add a quickfix and location list model with :cnext, :cprev, :cfirst, :clast, :cc, :colder, :cnewer and the :l variants (Mac T1404)

Lent from Mac T1404 (lend y50, 2026-11-06). Cut it back as-is with no Mac numbers in a helium batch.

# Add a quickfix and location list model with :cnext, :cprev, :cfirst, :clast, :cc, :colder, :cnewer and the :l variants

## Problem
`:copen` and `:lopen` only show the Problems panel (crates/fe-vim/src/ex.rs, `HostCommand::OpenProblems`), and `:cnext`, `:cprev`, `:cc` do not exist (E492). A nvim user's quickfix loop (`:grep`, `:make`, then `]q`) has nothing to walk. This card is the model and the navigation; the fillers are the next card (`:grep`, `:vimgrep`, `:make`) and the list window is the one after (`:copen`, `<leader>xq`, `]q`).

## Change
- fe-vim, pure parsing in a new `crates/fe-vim/src/ex/quickfix.rs` (module docs list the forms, as ex/buffers.rs does): `QuickfixCommand` carried as `HostCommand::Quickfix(QuickfixCommand)` and `location: bool` for the `:l` forms. Forms: `:cn[ext]`/`:cp[revious]`/`:cN[ext]` with a count (`:cnext 3`, `:3cnext`), `:cfir[st]`, `:cla[st]`, `:cc [N]`, `:cnf[ile]`/`:cNf[ile]` (next/previous file's first item), `:col[der]`/`:cnew[er]` with a count, `:cl[ist]` (items printed in the status line, capped), plus `:lnext :lprevious :lNext :lfirst :llast :ll :lnfile :lolder :lnewer :llist`. Also parse, and carry as variants the host answers "not available yet" until the next card: `:grep`, `:lgrep`, `:vimgrep`, `:lvimgrep`, `:make`, `:lmake` with their argument text, and `:copen`/`:cclose`/`:lopen`/`:lclose` stay `OpenProblems`/`CloseProblems` here (the window card retargets them). No range except the count forms; `!` is accepted and ignored as nvim does for these.
- ex.rs: the `HostCommand::Quickfix` variant, its dispatch beside `buffers::kind`, the bang/range checks; lib.rs re-exports the new types. fe-workbench's two exhaustive host-command matches (`git_diff.rs`, `git_commit.rs`) route it to `run_window_command`, as `Buffer(_)` is.
- fe-workbench, new `crates/fe-workbench/src/quickfix.rs`, plain Rust, no gpui: `QuickfixItem { path: PathBuf, row: u32, col: u32 (0-based char column), text: String, kind: Error | Warning | Info | Note }`, `QuickfixList { title, items, current }`, and `QuickfixStack` holding up to 10 lists (a new list pushes; `:colder`/`:cnewer` move through them, as nvim does; the 11th push drops the oldest). One quickfix stack per workbench; one location stack per editor pane (keyed by the pane's id, dropped with the pane). `next(count)` and `previous(count)` do not wrap: past the end is nvim's `E553: No more items`, with no list `E42: No Errors`, and `:cc N` with N above the count selects the last item, as nvim does. `:cnfile` moves to the first item of the next file.
- Landing: a new `crates/fe-workbench/src/quickfix_commands.rs` holds the typed commands `quickfix.next`, `quickfix.previous`, `quickfix.first`, `quickfix.last` (and the `location.*` pair) over the same code, and the vim arm in `vim_host.rs::run_window_command` calls it. Landing opens the item's file at its row and column in the active pane with the existing open-at path (`open_at.rs`, as peek references does), selecting the item, and sets the status line to nvim's `(3 of 12) text` message.
- Item paths are absolute; the model never touches the disk.

## Tests (all new files)
- `crates/fe-vim/tests/ex_quickfix.rs` (flat, bare ids): the forms above parse to the right `QuickfixCommand` with counts and abbreviations; `:cnext` with a range other than a count is refused; `:colder` and `:cnewer` take counts.
- `crates/fe-workbench/src/quickfix_tests.rs` (declared `#[cfg(test)] mod quickfix_tests;` in lib.rs; ids `quickfix_tests::...`): navigation, E553, E42, `:cc N`, `:cnfile`, stack depth 10, per-pane location lists.
- `crates/fe-workbench/src/quickfix_commands_tests.rs` (`quickfix_commands_tests::...`): gpui `TestAppContext` test that `quickfix.next` opens the second item's file at its row and column in the active pane and that a vim line `:cnext` does the same.

## Not in scope
Filling lists (next card), the list window and key bindings (the card after), `:cexpr`/`:cfile`/`:cbuffer`/`:cdo`/`:cfdo`, errorformat parsing, `:vimgrepadd`. Vim regex is not translated (see the fillers card).

## Implementation notes

- Oracled against nvim 0.12.4 headless (`setqflist`, `fullcommand()`).
  Abbreviations follow nvim: `:cne` is `:cnext`, `:cnew` `:cnewer`, `:cl`
  `:clist`, `:cr`/`:lr` are `:crewind`/`:lrewind` (as `:cfirst`), and
  `:cpf`/`:lpf` are `:cNfile`/`:lNfile`; `:ln` stays `:lnoremap` and `:cf`
  `:cfile` (not ours).
- `:cNfile` lands on the *last* item of the previous file, as nvim's does
  (`:cnfile` on the next file's first). `:cfirst N` and `:clast N` are
  `:cc N`, as in nvim.
- A count that overshoots stops at the end when it moves at all (nvim's
  `:cnext 5` from item 2 of 3 lands on 3); E553 only when it can't move.
  `:colder`/`:cnewer` likewise. `:3cnext 2` adds as nvim does (count 4);
  `:0cnext` moves one; `:0cc` is E16; `:cnext 0` E939.
- Ranges other than one plain number are E481 per C2, though nvim gives
  E16 for `:.cn`/`:%cn` and accepts `:1,2cn`.
- The landing message is nvim's exact `(2 of 3) error: text` (kind word
  from `QuickfixKind`, text on one line); `:colder` says nvim's
  `error list 1 of 2; 3 errors ` padded to 34 columns and the title;
  `:clist` puts up to 10 items on the status line joined by ` | `, paths
  relative to the root.
- Answers go to the vim message of the editor the line was typed in (the
  landing one to the editor landed in); with vim off, a typed command's
  answer is an info toast.
- `workbench.rs` is outside the scope, so the lists live in a gpui Global
  keyed by the workbench's entity id, removed on its release; a pane's
  location stack is removed on `WorkspaceEvent::PaneRemoved`. `:l`
  commands use the active pane's stack.
- `Workbench::push_quickfix_list(location, list, cx)` is the seam for the
  fillers card; `:grep`/`:vimgrep`/`:make` (and `:l` twins) carry their
  raw argument text, `|` included, and answer "Not supported yet: filling
  a quickfix list".
