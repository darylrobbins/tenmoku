---
task_uid: "01m55ce5x8txntann1a1x5g0ws"
task: "T148"
rev: 1
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "fe-workbench test status_bar::vim_line::history_tests::complete_tests::tab_completes_and_cycles_the_command_name (new crates/fe-workbench/src/vim_line/complete_tests.rs, declared by #[path = \"complete_tests.rs\"] mod complete_tests; in the existing crates/fe-workbench/src/vim_line/history_tests.rs) passes: in a real workbench, :noh then tab gives :nohlsearch; a prefix with several completions cycles through them in order with tab, back with shift-tab, and returns to the typed text after the last; a range ('\u003c,'\u003enoh) is kept."
    required: true
    manual: false
  - id: "C2"
    text: "fe-workbench test status_bar::vim_line::history_tests::complete_tests::typing_ends_the_cycle_and_search_lines_keep_tab (same file) passes: a key typed after tab ends the cycle so the next tab completes the new text, and tab on a / line does what it did before this card."
    required: true
    manual: false
---

# Tab on the vim command line completes the ex command's name and cycles through the others, shift-Tab back, as nvim's wildmenu does (Mac T832)

Filed on argon from Mac T832 (card ~/coordination/argon-cards/zzk/T832.json rev 2; note zzk, the Vim breadth track). Mac task uid 01m55bpge5amwynjv17m5dc86m. Base: argon trunk (mac/main 76125d0 synced). Behaviour probed with nvim --clean --headless 0.12.6. Held in the inbox: waits until helium's Mac T831 (Ex::completions) is on argon's trunk; the Mac lead clears it. Title the host commit "... (Mac T832)".

## Problem

The status bar's vim command line (`crates/fe-workbench/src/vim_line.rs`) takes Up and Down for history but
nothing for completion at Mac main 92c57c57: `<Tab>` doesn't complete a command's name as nvim's does
(`:help c_<Tab>`). The user approved the line's completion on 2026-10-17 in the "Vim breadth" track; Mac
T831 adds `fe_vim::Ex::completions(line)`, the byte offset of the command name being typed and the full
names fe-vim runs that it abbreviates, in nvim's order.

## Change

In `crates/fe-workbench/src/vim_line.rs` `on_line_key`, for an ex (`:`) line only: `tab` replaces the name
being typed (from the offset `Ex::completions` gives to the end) with the first completion and puts the
caret at the end; each further `tab` the next, and after the last the text as typed, as nvim's wildmenu
cycles (`wildmode=full`); `shift-tab` goes the other way. No completion leaves the text as it is. Any other
key (typing, a backspace, Up/Down recall) ends the cycle, so the next `tab` completes what is there then.
On a `/` or `?` line `tab` does what it does today. Recording a macro keeps what it keeps today (the line's
final text). If the input's `TextInput` takes `tab` before `on_line_key` sees it (focus traversal), stop
that in the line's own key handling rather than in the keymap TOMLs.

Test: a new `crates/fe-workbench/src/vim_line/complete_tests.rs`, declared in the existing
`crates/fe-workbench/src/vim_line/history_tests.rs` by `#[path = "complete_tests.rs"] mod complete_tests;`
(reusing its `opened` workbench), driving the real line: `:noh` then `tab` gives `nohlsearch`; a prefix with
several completions cycles through them with `tab`, back with `shift-tab`, and back to what was typed; a
range kept (`:'<,'>noh`); typing after a `tab` ends the cycle; `tab` on a `/` line unchanged.
