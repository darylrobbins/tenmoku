---
task_uid: "01m6fdjrytg62r60pxq67p0bk4"
task: "T1337"
rev: 1
type: "chore"
complexity: "s"
criteria:
  - id: "C1"
    text: "Codex review of T1319 (F1, medium): the T1319 fix changed the shared VimPicker's row width and minimum-width rule, so :marks and :registers pickers no longer lay out as they did before T1319 (crates/fe-workbench/src/vim_pick.rs:93, :126). Compare the :marks and :registers popovers at the T1319 parent and after T1319. If they changed, either limit the full-width rows and minimum-width rule to the :ls buffers picker, or keep them for all vim pickers and say why in the commit message."
    required: true
    manual: false
  - id: "C2"
    text: "tests::vim_pick_layout (new crates/fe-workbench/src/tests/vim_pick_layout.rs, declared from src/tests/mod.rs) checks :marks and :registers popover and row widths (Picker::list_bounds, Picker::row_bounds) against the chosen behaviour"
    required: true
    manual: false
  - id: "C3"
    text: "Existing tests still pass: fe-workbench vim_ls_tests and tests::vim_ls_layout, fe-vim --test ex_ls"
    required: true
    manual: false
  - id: "C4"
    text: "cargo clippy -p fe-workbench -p fe-vim --all-targets -- -D warnings is clean"
    required: true
    manual: false
---

# Follow-ups from review of T1319: keep the :marks and :registers pickers sized as before T1319, limiting its full-width rows and minimum-width rule to :ls unless they suit all vim pickers
