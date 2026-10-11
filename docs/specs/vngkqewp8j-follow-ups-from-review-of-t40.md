---
task_uid: "01m4nz4f9qwfn7jjvngkqewp8j"
task: "T45"
rev: 1
type: "chore"
complexity: "m"
criteria: []
---

# Follow-ups from review of T40

Findings of the review of T40 that don't block the merge (below merge.block_severity):

- F1 medium crates/fe-workbench/src/vim_host.rs:259: Parent traversal still bypasses filesystem errors. For `:e missing/../file.txt`, failed canonicalization is ignored and `missing` is popped; for `:e a.txt/../file.txt`, canonicalization succeeds on the regular file and it is also popped. Both commands open root/file.txt despite invalid intermediate components, allowing a subsequent save to overwrite an unintended file.
  fix: Require every component traversed before `..` to resolve successfully to a directory, preserving resolution errors rather than falling back to lexical removal. Continue allowing a missing final file, and add regressions for missing, regular-file, and dangling-symlink intermediate components.
  instances: crates/fe-workbench/src/vim_host.rs:259, crates/fe-workbench/src/vim_host.rs:262

- F2 medium crates/fe-workbench/src/vim_host.rs:268: Appending an empty component does not enforce the directory requirement before tab matching: Rust Path equality ignores trailing separators, so `:e a.txt/` activates an already-open a.txt through Pane::position without checking the filesystem. An unopened missing `new.txt/` also becomes an empty file buffer because the loader treats NotFound as a new file. Additionally, rebuilding components drops the directory requirement in `a.txt/.`. The regression only covers an existing file that is not already open.
  fix: Preserve and validate directory requirements before existing-tab or peer-buffer matching and before treating NotFound as a missing final file. Add tests for an already-open file with a trailing slash, a missing path with a trailing slash, and a regular file followed by `/.`.
  instances: crates/fe-workbench/src/vim_host.rs:257, crates/fe-workbench/src/vim_host.rs:268

- F3 medium crates/fe-workbench/src/vim_host.rs:260: Canonicalizing the entire prefix changes unrelated symlink spellings and breaks existing-tab matching. With a workspace root accessed through a directory symlink, an open alias/a.txt is not matched by `:e sub/../a.txt`, which resolves to real/a.txt. Pane matching compares paths directly, so this creates a separate buffer for the same file and can hide unsaved edits or allow competing saves. This also affects symlinked path prefixes common on macOS.
  fix: Separate filesystem traversal from buffer identity, ensuring a resolved path reuses an existing buffer for the same destination despite canonicalized directory prefixes. Add a regression with a symlinked workspace root and a dirty existing tab, including a missing final file.
  instances: crates/fe-workbench/src/vim_host.rs:260

- F4 medium crates/fe-workbench/src/vim_host_tests.rs:458: The `:wa` test does not establish that the first failure in pane order is reported. Both b.txt and c.txt fail through the same directory replacement and produce indistinguishable filesystem errors; asserting only that the message lacks E141 accepts either failure. An implementation that selects the second named-file failure would pass, leaving the ordering gap from the task unresolved.
  fix: Create distinguishable deterministic failures and assert the expected first failure's diagnostic. Arrange failures across panes to exercise pane ordering while retaining the later-successful-save and untitled-buffer checks.
  instances: crates/fe-workbench/src/vim_host_tests.rs:451, crates/fe-workbench/src/vim_host_tests.rs:452, crates/fe-workbench/src/vim_host_tests.rs:458
