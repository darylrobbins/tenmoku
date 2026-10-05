---
task_uid: "01m46tyt3y0erm0yq7c5xbtpe8"
task: "T70"
rev: 1
type: "docs"
complexity: "m"
criteria:
  - id: "C1"
    text: "docs/decisions/vim-engine.md exists with the ten numbered sections named in the spec (Summary and recommendation, Licence and provenance, API shape, Vim model, Host buffer contract, Coverage against Neovim, Maintenance, Fit with our command model, Requirements vim puts on the command model, Unanswered)"
    required: true
    manual: false
  - id: "C2"
    text: "Summary and recommendation recommends exactly one of adopt, fork or write our own, names the hjkl crate versions and the git commit read, estimates each option in lines of code, and outlines the first fe-vim card"
    required: true
    manual: false
  - id: "C3"
    text: "Licence and provenance states the licence in each evaluated crate’s metadata and LICENSE file, lists any runtime dependency licence outside deny.toml’s allowlist (or says there is none), and answers whether any code is ported from Vim/Neovim C source, Zed or Helix, with file references"
    required: true
    manual: false
  - id: "C4"
    text: "Vim model gives API-level traces for d3w, \"ayy, ciw and .; Host buffer contract answers multi-cursor support, undo integration, index units (bytes, chars or graphemes) and whether it can run over EditorModel without copying the buffer; Coverage against Neovim gives the test count and a 20-command spot check"
    required: true
    manual: false
  - id: "C5"
    text: "Fit with our command model answers whether hjkl output maps onto editor.operate / editor.move invocations under vim.mode context predicates and how keys reach it; Requirements vim puts on the command model is a numbered list covering at least the eight items listed in the spec"
    required: true
    manual: false
  - id: "C6"
    text: "The candidate changes only docs/decisions/vim-engine.md (plus its own spec file); Cargo.toml and Cargo.lock are unchanged"
    required: true
    manual: false
---

# Evaluate the hjkl vim engine and decide: adopt, fork, or write our own

# Evaluate the hjkl vim engine and decide: adopt, fork, or write our own

Foundation card 2: the time-boxed M7 spike the plan calls for ("start with a time-boxed `hjkl`
evaluation … then adopt and fork it or write our own"). Treat this as a decision task. The output
is one decision document, and **no code merges**. It needs nothing else, and the command-model card
(1a) can use its findings.

## Problem

The plan's Architecture table says `fe-vim` will be an "`hjkl` fork or our own engine" that "emits
`EditorAction`s". The plan's reuse map notes hjkl is MIT, has about 1k tests and a Neovim oracle,
and that "its `View` trait is sealed, so we'd fork it if adopted". Those notes are second-hand. We
need evidence before M7, and before the command model (card 1a) and keymap (cards 4a and 4b) fix
shapes that vim will have to live with.

The engine is split across crates in the `kryptic-sh/hjkl` workspace. Evaluate the engine-level
crates, not the TUI:

- `hjkl-engine`: "Vim FSM, motion grammar, and ex commands. Pre-1.0 churn."
- `hjkl-buffer`
- `hjkl-keymap`
- `hjkl-editor`, the front door
- `hjkl-ex`

Use the latest release, 0.42.1 as of 2026-10-05, and say which version and commit you read.

## How to work

- Read the source from a checkout or crate tarballs **outside this repository**: a `git clone`, or
  `curl` of `https://static.crates.io/crates/<name>/<name>-<ver>.crate` into a temp dir. You may
  build it and run its tests there.
- Do **not** add hjkl to any `Cargo.toml` here, and don't touch `Cargo.lock`. The only file this
  task changes is `docs/decisions/vim-engine.md`.
- Clean room:
  - hjkl is MIT and fine to read.
  - Never open Zed's GPL crates (`vim`, `editor` …) to compare.
  - If hjkl contains code derived from Zed's `vim` crate, Helix (MPL) or Vim's own C source (the
    Vim licence), say so and stop reading that part.
- Time box: one lane session. Leave something unanswered rather than overrun, and say in the
  doc what is unanswered.

## Output: `docs/decisions/vim-engine.md`

It has these sections, each answering its named questions with links (file paths with line
numbers at the commit you read) or command output as evidence:

1. **Summary and recommendation.** One of **adopt** (depend on the crates unmodified), **fork**
   (copy into a `fe-vim` crate with headers, per `CLEAN_ROOM.md`), or **write our own**. Include a
   rough size of each option in lines of code, and the first `fe-vim` card's outline.
2. **Licence and provenance.**
   - Is the licence in each crate's metadata, and the `LICENSE` file in the repo and in each crate
     tarball, MIT?
   - What are the licences of its runtime dependency tree? List anything outside our `deny.toml`
     allowlist.
   - Is any code ported from Vim or Neovim C source, Zed or Helix?
3. **API shape.**
   - How is input fed: key events and their type?
   - What comes out: actions or edits, or does it mutate its own buffer?
   - Who owns the state?
   - Is the `View` trait (or its equivalent) sealed, and what would unsealing take?
4. **Vim model.** How are these represented?
   - operators, motions, text objects and counts
   - registers, including the unnamed and system clipboard registers
   - modes: normal, insert, visual, visual-line, visual-block and operator-pending
   - dot-repeat, macros (`q`/`@`), marks, search (`/`, `n`, `*`) and ex commands
   - the result of `d3w`, `"ayy`, `ciw` and `.`, shown as API-level traces
5. **Host buffer contract.**
   - What must the host provide: traits, rope type and version, cursor and selection model?
   - Does it support multiple cursors?
   - How does it hook into undo, and could undo groups map onto `fe_buffer` transactions?
   - Does it index by bytes, chars or graphemes?
   - Would it work over our `EditorModel` without copying the buffer?
6. **Coverage against Neovim.**
   - test count, and how the Neovim oracle runs
   - its own known gaps or TODO list
   - a spot check of 20 common normal-mode commands, listing which are present
7. **Maintenance.**
   - release cadence over the last six months, contributors, and open versus closed issues
   - MSRV (1.95) against our 1.98 pin
   - how many crates it would add to our lock
   - risk from "pre-1.0 churn"
8. **Fit with our command model.**
   - Can hjkl's output map onto a parameterised command such as
     `editor.operate { operator, target: Motion | TextObject, count, register }` and
     `editor.move { motion, extend, count }`, dispatched through a `fe-command` registry with
     `vim.mode == normal` context predicates?
   - Can our keymap hand keys to it?
   - Can its keymap trie (`hjkl-keymap`) coexist with ours, or would it replace ours in vim
     contexts?
9. **Requirements vim puts on the command model.** A numbered list, for card 1a or a follow-up.
   Cover at least:
   - pending-input routing from the keymap to a mode handler
   - mode as a context key
   - count and register as args
   - text objects as targets
   - dot-repeat of the last mutating invocation
   - macros as invocation lists
   - per-mode cursor shape and selection semantics
   - undo grouping of an insert session
10. **Unanswered.** What the time box didn't cover.

## Decided (lead defaults; the user may revisit)

- Docs only. Any prototype stays outside the repo and is described, not merged.
- Evaluate the engine crates at their latest release, not git main, unless main fixes a blocker.
