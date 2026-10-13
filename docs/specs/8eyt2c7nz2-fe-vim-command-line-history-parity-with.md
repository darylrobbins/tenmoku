---
task_uid: "01m4rv3dbr9jnjyh8eyt2c7nz2"
task: "T566"
rev: 2
type: "bugfix"
complexity: "s"
criteria:
  - id: "C1"
    text: "fe-vim test history_matches_neovim: tests/corpus/history.toml gains at least 8 cases covering an \u003cEsc\u003e-cancelled : and / line recalled by \u003cUp\u003e, * and # patterns, :s and :g patterns and an ex-address search pattern recalled by / \u003cUp\u003e; every case matches nvim 0.12.4"
    required: true
    manual: false
  - id: "C2"
    text: "The fe-vim lib.rs Known-limits lines for these history gaps are removed, and every earlier corpus expected file other than history.expected.json is byte-identical and every earlier fe-vim test passes"
    required: true
    manual: false
---

# fe-vim: command-line history parity with nvim: keep <Esc>-cancelled lines, and add * # :s :g and ex-address search patterns to the search history

Follow-up from T422 (vim command-line history). T422 matched its card but left three nvim 0.12.4 behaviours out, documented in fe-vim lib.rs Known limits:

1. A line cancelled with <Esc> stays in its history (probed: `:let b=2<Esc>:<Up>` recalls `let b=2`). T422's C1 leaves cancelled lines out; change that to match nvim (check <C-c> and an empty line too, against nvim).
2. The patterns of `*` and `#` (and g* g#) are added to the search history.
3. The patterns of `:s`, `:g`/`:v` and ex-address searches (`:/pat/d`) are added to the search history.

Extend tests/corpus/history.toml with cases for each, checked against nvim with the existing history oracle, and remove the matching Known-limits lines.
