---
task_uid: "01m5v86j7vhwk7w5m47ha5zypf"
task: "T356"
rev: 2
type: "chore"
complexity: "xs"
criteria:
  - id: "C1"
    text: "vim_ls_tests::without_a_root and every other typed-filter test in vim_ls_tests.rs no longer depend on which characters the random tempdir name holds: the filter cannot match the other row through the directory part of its path"
    required: true
    manual: false
  - id: "C2"
    text: "Headless, Linux-OK"
    required: true
    manual: false
---

# Make vim_ls_tests::without_a_root independent of the random tempdir name (flaky fuzzy filter)

## Problem
`workbench::vim_host::vim_ls_tests::without_a_root` (T328) is flaky: it types the filter "qz.t" into the :ls picker and expects only qz.txt, but the picker fuzzy-matches the whole path, and the random tempdir name (`/tmp/.tmpXXXXXX/…`) can supply the q and z so "wv.txt" matches as a subsequence too. It failed the T335 gate and T348's finish (extra row `3 %a …/wv.txt`).

## Change
Test only: make the filter unable to match the other row whatever the tempdir name is, e.g. give the files names sharing no characters with each other and filter on a full distinctive name, and build the root under a fixed-name directory inside the tempdir so only fixed characters precede the file names; or assert on the selected row set after filtering with a query the other path cannot contain. Check the other tests in vim_ls_tests.rs that filter by typing for the same pattern.

## Tests
`crates/fe-workbench/src/vim_ls_tests.rs`: the test passes for 200 consecutive runs with random tempdirs (loop in a scratch run).
