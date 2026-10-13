---
task_uid: "01m4sv0yszkmgec4xjcm4ce210"
task: "T622"
rev: 1
type: "chore"
complexity: "xs"
criteria:
  - id: "C1"
    text: "delimiters_matches_neovim sets fe_vim::search::set_time_limit to 60 s before it runs, with a comment naming T582 as the real fix"
    required: true
    manual: false
  - id: "C2"
    text: "The test passes 10 runs in a row while every core is loaded, with the command and result in the decision file"
    required: true
    manual: false
  - id: "C3"
    text: "Only crates/fe-vim/tests/delimiters_oracle.rs changes"
    required: true
    manual: false
---

# Flake: run fe-vim delimiters_oracle delimiters_matches_neovim with a raised search time limit so load can't fail it (interim until T582)

fe-vim::delimiters_oracle delimiters_matches_neovim fails under load: on the case yit_count_2_over_pattern_names, the delimiter object runs past fe-vim's process-wide search time limit (search::DEFAULT_TIME_LIMIT, 2 s), so the object fails while nvim's recorded result succeeds. It turned Mac main's post-merge check red on 62470f56 (run 01m4st46bnhqfyc838z45jpvys) and failed T564's gate earlier. The real fix (making that case fast) is T582, which waits on T564, parked overnight.

Interim, test-only: the oracle compares results with nvim; it doesn't test the time limit, which delimiters' own work tests cover. tests/delimiters_oracle.rs is its own test binary, and nextest runs each test in its own process, so the test can raise the process-wide limit for itself. At the top of delimiters_matches_neovim, call fe_vim::search::set_time_limit with a generous limit (60 s), with a one-line comment that names T582 as the real fix and says that the oracle checks results, not the time limit.

Prove it: run the test 10 times while every core is loaded (for example several `yes > /dev/null`), and record the command and the result in the decision file. This changes only a test, so it is a chore with no regression test. Don't touch crates/fe-vim/src/ or the corpus: T564 holds them.
