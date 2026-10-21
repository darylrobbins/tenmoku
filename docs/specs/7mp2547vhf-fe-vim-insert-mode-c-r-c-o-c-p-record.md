---
task_uid: "01m5eaj9006f86y27mp2547vhf"
task: "T245"
rev: 1
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "the integration test insert_registers_match_neovim passes on the extended crates/fe-vim/tests/corpus/insert_registers.toml, including new nvim 0.12.x-recorded cases (asserted by name): dot_repeats_ctrl_r_ctrl_o_reads_register_again, dot_repeats_ctrl_r_ctrl_p_reads_register_again, linewise_ctrl_r_ctrl_o_dot_repeat, counted_ctrl_r_insertion"
    required: true
    manual: false
  - id: "C2"
    text: "a top-level test in the same file, stale_register_prefix_does_not_outlive_its_session, passes: through HeadlessVim, exiting insert mode with a pending \u003cC-r\u003e and starting a new session types that session's first character normally; cargo nextest run -p fe-vim and clippy pass"
    required: true
    manual: false
---

# fe-vim: insert-mode <C-r><C-o>/<C-p> record their register and kind for dot repeat and counts, and a pending <C-r> prefix is cleared when its session ends (T241 review F2 and F4 follow-up)

## Problem

T241 (= Mac T880) landed with two open MEDIUM review findings (round 4, approved because they are below the merge block severity):

- **F2** (`crates/fe-vim/src/insert/keys.rs`): `<C-r><C-o>{reg}` and `<C-r><C-p>{reg}` insert text without recording the register identity and the insertion kind for replay. `.` therefore repeats the captured text instead of reading the register again (nvim, probed: `A<C-r>a<Esc>`, change register `a`, `.` inserts the NEW text), and a linewise `<C-r><C-o>` that inserts above the session's anchor escapes the captured insertion, breaking `.` and counted insertion (`3A...`).
- **F4**: the pending `<C-r>` prefix (`VimState::register_key`) is keyed by the insert session's ledger allocation address, and ending a session does not clear it. A later session that reuses the address has its first character consumed as the stale prefix's register name.

## Change

Fix both inside fe-vim:

- F2: record the register identity and insertion kind (`<C-r>`, `<C-r><C-r>`, `<C-r><C-o>`, `<C-r><C-p>`) in the session so `.` and counted insertion re-read the register and repeat linewise insertion as nvim does; add oracle cases to `crates/fe-vim/tests/corpus/insert_registers.toml` (changed register then `.`, linewise `<C-o>` then `.`, counted insertion), recorded into `insert_registers.expected.json` with the ignored `record_corpus_with_neovim` in `tests/oracle.rs` (nvim 0.12.4 is on the host), asserted by `insert_registers_oracle.rs`.
- F4: clear the pending prefix when its session ends (or key it by a non-reusable session identity); add a regression test that exits insert mode through dispatch with a pending `<C-r>` and starts another session, whose first character must type normally.

Land in the same batch as T241 and T244 (the app wiring): the stale prefix is the swallowed-key risk T244 guards against from the app side.
