---
task_uid: "01m6n0tfhshcy22ga52ctjshs0"
task: "T287"
rev: 1
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "Held platform text and pastes that arrive behind an unresolved ambiguous map prefix go in after the prefix's resolution, in order and none lost, across a \u003cCmd\u003e suspension and a map that closes the line"
    required: true
    manual: false
  - id: "C2"
    text: "Existing tests pass unedited; clippy passes"
    required: true
    manual: false
---

# Command-line maps: held platform text and pastes keep their order behind an ambiguous prefix across a <Cmd> suspension or a closing map (T261 review F10)

Follow-up to T261's review (F10 and its "held platform text ordering" variant; T261's C2 narrowed to a documented limit).

Held line edits (platform text commits, a paste) can still overtake an earlier ambiguous map prefix, or be lost when that prefix's resolution suspends for a `<Cmd>` or closes the line. Fixing it needs the held-edit queue in `crates/fe-workbench/src/vim_line.rs` (`release_line_keys`, `make_held`) to resolve prefixes and then wait on any resulting continuation per edit, in order.

Tests: hold an ambiguous prefix, commit platform text and paste, resolve the prefix to a `<Cmd>` map and to a line-closing map; assert order, nothing lost. Also the other documented T261 limits if cheap (F1 platform ranges, F2 queued actions on a closed line, F12 caret off the end).

## Implementation notes

- `make_held` now resolves held `c` map prefixes before held text, pastes and ops; if that leaves a `<Cmd>` waiting, the edit goes back to the front of the queue, and if it closes the line the text goes to the editor. `resolve_line_prefix` stops once a continuation is pending.
- Not done: T261 F1 (platform ranges), F2 (queued actions on a closed line), F12 remain documented limits.
