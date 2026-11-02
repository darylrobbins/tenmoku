---
task_uid: "01m69fhz3wwgfeamv0vntnxcwq"
task: "T387"
rev: 1
type: "chore"
complexity: "s"
criteria: []
---

# Follow-ups from review of T155

Findings of the review of T155 that don't block the merge (below merge.block_severity):

- F1 medium crates/fe-vim/src/edit.rs:283: The newline replacement always copies the entire original indentation, including indentation after the replacement point, and retains it on empty new rows. Neovim 0.12.4 produces " \n foo" for r<CR> at column 1 of "    foo", but this implementation produces " \n    foo". At column 6 it should produce "    fo\n", but instead leaves four trailing spaces and the wrong cursor column.
  fix: Calculate indentation from the surviving prefix before the replacement, and omit generated indentation when no nonblank suffix remains. Add oracle cases for replacements within indentation, whitespace-only rows, and indented rows whose suffix is exhausted.
  instances: crates/fe-vim/src/edit.rs:283, crates/fe-vim/src/edit.rs:293

- F2 medium crates/fe-vim/src/edit.rs:276: Removing the newline/carriage-return guard also permits '\r' through the ordinary replacement branch. Dispatching vim.replace_char with {"char":"\r"}, or feeding a literal carriage return after r, now passes CR into fe-text, triggering its normalization assertion in debug builds and violating its LF-only invariant in release builds.
  fix: Retain rejection of '\r' while allowing the new '\n' path, and cover carriage-return arguments through the public command API.
  instances: crates/fe-vim/src/edit.rs:276
