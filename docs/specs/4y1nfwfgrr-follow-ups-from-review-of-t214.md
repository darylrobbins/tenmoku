---
task_uid: "01m4h05dsbg44ayt4y1nfwfgrr"
task: "T298"
rev: 1
type: "chore"
complexity: "m"
criteria: []
---

# Follow-ups from review of T214

Findings of the review of T214 that don't block the merge (below merge.block_severity):

- F8 medium crates/fe-vim/src/cmdline.rs:139: Invalid patterns overwrite the last search and its direction, contrary to the specification's explicit requirement that invalid patterns preserve the / register. For example, /foo<CR>?a\(<CR> replaces foo with the invalid expression; subsequent empty searches can no longer reuse foo. This affects command-line submission and direct vim.move/vim.operate dispatch through the shared checker. The new tests and crate documentation encode the conflicting behavior.
  fix: Preserve the complete last-search state on PatternError::Invalid, as on unsupported patterns, and test both submission and direct dispatch. Reconcile the optional invalid-pattern oracle case with that contract without hand-editing recorder output; explicitly amend the specification if the intended contract instead follows Neovim here.
  instances: crates/fe-vim/src/cmdline.rs:139, crates/fe-vim/src/tests/search.rs:157, crates/fe-vim/tests/corpus/search.expected.json:991, crates/fe-vim/src/lib.rs:118

- F9 medium crates/fe-vim/src/cmdline.rs:28: Opening the line through the public command leaves VimState.pending intact. After feeding "2", directly dispatching vim.command_line with count 2, cancelling, and feeding "x", the stale count deletes two characters. A pending register similarly leaks into the next command. The keyed opening path conceals this because VimState::feed takes pending before dispatch.
  fix: Clear the live pending input when opening the command line, after its carried arguments have been supplied, so only CommandLine.args owns the prefix. Add direct-opening tests with pending counts and registers, covering both cancellation and submission followed by another command.
  instances: crates/fe-vim/src/cmdline.rs:28

- F10 low crates/fe-vim/src/cmdline.rs:34: Entering cmdline from visual mode makes the retained Vim cursor and anchor inaccessible through the public getters because visual::ends only accepts Mode::Visual. On "abc xyz" starting at (0,1), feeding "vl/" changes HeadlessVim::cursor() from (0,2) to (0,3) and visual_anchor() becomes None, although the selection is retained. A host querying these positions cannot preserve the buffer cursor while the line is being edited.
  fix: Expose the saved visual positions while a command line suspends a visual mode, without permitting visual commands or changing cmdline mode during synchronization. Use those positions in HeadlessVim's cursor reporting and test cursor and anchor visibility while the line remains open.
  instances: crates/fe-vim/src/cmdline.rs:34, crates/fe-vim/src/visual.rs:250, crates/fe-vim/src/headless.rs:186, crates/fe-vim/src/headless.rs:192
