---
task_uid: "01m5qphyk640vr5hp3adwk2em6"
task: "T329"
rev: 4
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "fe-vim tests/ex_tabs.rs: ex::parse gives HostCommand::Tab(..) for :tabnew [file], :tabn[ext] [N], :tabp[revious]/:tabN[ext] [N], :tabc[lose][!], :tabo[nly][!], :tabfir[st], :tabl[ast], :tabe[dit] [file] with nvim's shortest abbreviations (expected values stated from nvim 0.12.4); :tabmove and :tabdo stay E492"
    required: true
    manual: false
  - id: "C2"
    text: "vim_tabs_tests::cycle: :tabn and :tabp (with counts; :tabn N goes to tab N, 1-based) move over the active pane's tabs in strip order and wrap, as workbench.next_tab / previous_tab do; :tabfirst and :tablast go to the ends"
    required: true
    manual: false
  - id: "C3"
    text: "vim_tabs_tests::new_close: :tabnew opens an untitled tab (or [file] like :e) after the active tab and activates it; :tabc closes the active tab by workbench.close_tab's rules (a dirty one asks as the command does, without ! ) and :tabc! discards; :tabo closes the pane's other tabs, leaving dirty ones in place with a message"
    required: true
    manual: false
  - id: "C4"
    text: "vim_tabs_tests::pane_scope: tabs of other panes are untouched, and the host answers a message, not a panic, when the pane has one tab"
    required: true
    manual: false
  - id: "C5"
    text: "cargo test -p fe-vim -p fe-workbench and cargo clippy -p fe-vim -p fe-workbench --all-targets -- -D warnings pass; no existing test is edited"
    required: true
    manual: false
---

# Vim tab ex commands as VSCodeVim maps them: :tabnew, :tabn, :tabp, :tabc, :tabo and :tabfirst/:tablast run on the editor tabs (Mac T1143)

## Problem

User 2026-10-24: "Do we offer good vim control of the explorer/outline?" Vim users move between files with :tabn/:tabp/:tabnew, and in an editor whose tabs are files those are the natural commands even though nvim's tab pages hold windows, not buffers.

## Current state

- No :tab* parse in ex.rs. The tab commands exist: workbench.next_tab / previous_tab / close_tab / keep_editor (workbench_commands.rs:37-60), the pane's tabs in pane.rs.
- Mapping choice (the one real design decision of the card): fastedit tabs stand in for nvim buffers-in-a-tab, as VSCodeVim maps :tabn/:tabp/:tabnew/:tabc to editor tabs. Real tab pages (one window layout per tab) are not modelled and :tabmove, :tabdo, :windo stay unsupported.
- Serialise after the :ls card (T1142): all four ex cards edit ex.rs and vim_host.rs.

## Change

- fe-vim: ex/tabs.rs with `TabCommand { New{file}, Next(Option<u32>), Previous(u32), Close{force}, Only{force}, First, Last }`, `HostCommand::Tab`.
- fastedit: vim_tabs.rs running them through the existing tab commands, no new tab model.

## Tests

New files fe-vim tests/ex_tabs.rs and fe-workbench vim_tabs_tests.rs (declared from vim_tabs.rs).

Lend note (helium lead): spec, scope and criteria are the Mac's full JSON (y57), not truncated.
