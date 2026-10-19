---
task_uid: "01m55mj5yg0j5j7v5n1739cske"
task: "T154"
rev: 1
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "The integration test argument_objects (top level, in crates/fe-vim/tests/argument_object.rs) passes: cia and daa on the first, a middle, the last and an only argument, inside a nested call, with a string holding a comma, over a multi-row list, with a trailing comma, in brackets and braces, with a count, and via repeated via in visual mode."
    required: true
    manual: false
  - id: "C2"
    text: "The integration test argument_object_unclosed_list_is_bounded (top level, same file) passes: dia on an unclosed ( near the start of a 100,000-row buffer ends within the command deadline, changing nothing."
    required: true
    manual: false
---

# fe-vim: ia and aa select a function argument, as VSCodeVim's argument text object does (Mac T892)

Filed on argon from Mac T892 (card ~/coordination/argon-cards/zzp/T892.json rev 1; note zzp, the Vim parity track part 2). Mac task uid 01m55mechzt21fdmbf5gktwr8j. Base: argon trunk (mac/main synced). Corpus cases are recorded with nvim 0.12.x; unit tests are named by full module path, integration tests by bare fn name. Argon deps: T153 (Mac T891). Title the host commit "... (Mac T892)".

## Problem

At Mac main 6f032b12 fe-vim has no argument text object (`ia`/`aa`, as targets.vim and VSCodeVim's
argument object give): `cia` cancels. The user approved it in the "Vim parity" track on 2026-10-17.

## Change

New feature module `crates/fe-vim/src/feature/argument.rs`, declared in `crates/fe-vim/src/feature.rs` and
listed in `FEATURES`, its `object` hook mapping `a` to new `TextObject` variants (`crates/fe-vim/src/args.rs`)
dispatched from `crates/fe-vim/src/text_object.rs`, with an arm in `crates/fe-vim/src/visual/object.rs`.

- The list is the nearest enclosing `( )`, `[ ]` or `{ }` around the cursor; arguments are split at commas
  at that list's own depth, skipping nested brackets and quoted strings (`"`, `'`, `` ` ``, with backslash
  escapes). The list may span rows.
- `ia`: the argument under the cursor (on a comma or the white space after it, the next argument) without
  its surrounding white space. `aa`: with one separator: the comma and white space after it, or for the
  last argument the comma and white space before it; a single argument takes just itself.
- A count takes that many arguments from the cursor's. Charwise; in a visual mode, again extends to the next
  argument.

Behaviour reference: VSCodeVim's argument text object documentation (no source); no nvim oracle. Tests in a
new integration file `crates/fe-vim/tests/argument_object.rs`, with nested calls, strings holding commas,
multi-row lists, a trailing comma, and visual mode. Bound the work by the list's extent, and test an
unclosed `(` near the start of a 100,000-row buffer stays under the command deadline.

## Implementation notes

- A bracket under the cursor belongs to the argument it sits in (the nested call), not to the list it
  opens or closes: `cia` on the `(` or `)` of `g(b)` in `f(a, g(b))` changes `g(b)`.
- Strings: a `"` or `` ` `` string may span rows (Rust and Python strings, JS templates, Go raw
  strings); a `'` string ends at its row's end unless a backslash escapes the line break, so an
  apostrophe in prose or a comment (`don't`) can't swallow every row after it. Bracket depth counts
  every kind; the list's closer must match its opener, or the object fails.
- Because a string can span rows, the string state at the list's opener depends on all the text
  before it: the scan reads the text once from its start to the list's closer (not past it), keeping
  each open bracket's commas and argument extents as it goes, and checks the command deadline
  (`search::Deadline`) every 4 KiB. An unclosed list reads to the buffer's end once and fails.
- A trailing comma (followed only by white space before the closer) ends the last argument. On it the
  object is the last argument; `aa` on the last of several takes the comma before it, so
  `f(a, b,)` becomes `f(a,)`; an only argument with a trailing comma takes that comma too (`f(a,)`
  becomes `f()`).
- In a visual mode from a fresh single position, the object is the argument there (from a comma or
  white space, the next one). Repeating it grows by `count` arguments when the selection is that
  object; a one-position selection counts only if `ia`/`aa` made it, which a flag on the visual state
  (`Visual::by_object`, in `visual.rs`, cleared by any other selection change) records. A wider
  selection the object at the cursor doesn't hold fails rather than shrinking. Matching the selection
  to the arguments is a binary search, not a walk per argument.
