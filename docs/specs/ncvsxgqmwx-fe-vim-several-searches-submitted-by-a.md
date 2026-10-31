---
task_uid: "01m608cy4qabxskrncvsxgqmwx"
task: "T1209"
rev: 2
type: "bugfix"
complexity: ""
criteria:
  - id: "C1"
    text: "recorded oracle cases global_macro_with_several_searches and address_macro_with_several_searches in crates/fe-vim/tests/corpus/history.toml match pinned Neovim's / history order"
    required: true
    manual: false
  - id: "C2"
    text: "searches_submitted_inside_an_ex_line_wait_for_it_to_end in crates/fe-vim/tests/history_oracle.rs asserts nvim's order ([new, zed, zip] after the ex line)"
    required: true
    manual: false
  - id: "C3"
    text: "cargo test -p fe-vim --test history_oracle passes"
    required: true
    manual: false
---

# fe-vim: several searches submitted by a macro inside an ex line (:g/…/normal @a) reach the / history in the wrong order (T1201 defers them and keeps only the last)

Found by T1205 (oracle coverage for T1201's nested search history).

When a macro run from an ex line submits several searches (macro `/zed<CR>?zip<CR>/<CR>`, then `:g/foo/normal @a` or `:/foo/normal @a`), pinned Neovim adds each search to the `/` history as it runs: after the ex line the history is [new, zed, zip], so `/<Up><Up>` recalls `zed`. fe-vim defers the adds until the ex line ends and keeps only the last, giving [zed, new, zip], so the same keys recall `new`.

T1201 introduced the deferral (crates/fe-vim/src/register.rs, crates/fe-vim/src/cmdline.rs), and its direct test `searches_submitted_inside_an_ex_line_wait_for_it_to_end` (crates/fe-vim/tests/history_oracle.rs:218) asserts the fe behaviour, which nvim does not show.

Fix: add each search submitted inside an ex line to the history in nvim's order, keeping T1201's cases (empty-only macros keep the pending search; delimiter/offset recall) passing. Record the several-searches cases (global_macro_with_several_searches, address_macro_with_several_searches) in the history corpus from the pinned Neovim, and correct the T1201 direct test to nvim's order.
