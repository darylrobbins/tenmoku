---
task_uid: "01m5xbqfv9cv2azbqc9bm0z6y5"
task: "T1201"
rev: 1
type: "chore"
complexity: "s"
criteria: []
---

# Follow-ups from review of T566

Findings of the review of T566 that don't block the merge (below merge.block_severity):

- F2 medium crates/fe-vim/src/cmdline.rs:105: Nested submitted searches still bypass deferred history recording. A macro containing /bar followed by /baz inside :g/foo/normal @a or :/foo/normal @a adds both entries instead of only baz. Conversely, a macro containing an empty submitted search clears pending foo without recording anything. The new single-search cases detect the stale-pattern symptom but miss these transitions.
  fix: Defer nested submitted search entries, including their delimiter, until the outer Ex execution ends; replace the pending entry only for a nonempty history contribution. Add oracle and direct-history tests for multiple submitted searches and empty searches under both global and address commands.
  instances: crates/fe-vim/src/cmdline.rs:105, crates/fe-vim/src/cmdline.rs:108, crates/fe-vim/src/register.rs:286, crates/fe-vim/src/register.rs:307, crates/fe-vim/src/register.rs:325

## Implementation notes

Nested submitted searches now go through `Registers::submit_search`, which inside an ex line keeps only the last nonempty one (with its delimiter) as the pending entry. Oracle corpus cases were not added: the local nvim is 0.12.6 and the recorder is pinned to 0.12.4. The direct-history test `searches_submitted_inside_an_ex_line_wait_for_it_to_end` covers both `:g` and address forms, with multiple and empty searches.
