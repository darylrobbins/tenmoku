---
task_uid: "01m5qydt75rgsv0wxyvasj3wa2"
task: "T333"
rev: 2
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "The vim buffer registry records each pane activation, addition and removal from Pane::activated and Pane::items_changed, so transitions coalesced into one gpui update keep distinct buffer numbers in order. Tests several_tabs_in_one_update_keep_distinct_numbers, activations_in_one_update_are_recorded_in_order. Known limit, documented in a comment at vim_buffers.rs where pending transitions are replayed: a pane added and removed within one update before any registry read, in a session with several windows, may replay into the wrong window; the follow-up T337 carries the fix (review F2)"
    required: true
    manual: false
  - id: "C2"
    text: "Headless, Linux-OK"
    required: true
    manual: false
---

# Record every pane activation, addition and removal in the vim buffer registry at the mutation site (T326 F2)

## Problem
T326 (Mac T1136, vim buffer ex commands) review F2: the vim buffer registry observes the workspace through gpui notifications, which gpui coalesces per update, so several raw pane activations, additions or removals inside one update are seen only as the final state; a buffer number can then be missed or reused.

## Change
- `crates/fe-workbench/src/pane.rs`: call into the vim buffer registry (or emit a pane event the workbench subscribes to) from `Pane::activated` and `Pane::items_changed`, with the tab's item, so every transition is recorded at the mutation site.
- `crates/fe-workbench/src/vim_buffers.rs`: take those transitions instead of sampling the final state.

## Tests
`crates/fe-workbench/src/vim_buffers_transition_tests.rs` (`#[path]`): two tabs added and one removed in a single update keep distinct buffer numbers; an activation sequence in one update is recorded in order.
