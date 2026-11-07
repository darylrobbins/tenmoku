---
task_uid: "01m6n84wvc313gkeate0eakbym"
task: "T288"
rev: 2
type: "bugfix"
complexity: "s"
criteria:
  - id: "C1"
    text: "A save started at a version and finishing after more than 256 further changes to its buffer reaches a subscribed plugin as BufferSaved with the plugin version of the text it wrote"
    required: true
    manual: false
  - id: "C2"
    text: "cargo nextest run -p fe-workbench and cargo clippy -p fe-workbench --all-targets -- -D warnings pass"
    required: true
    manual: false
---

# Plugin events: a save that finishes after more than 256 later changes still reaches plugins with the version it wrote (follow-up from T278 review)

## Problem

T278 sends plugins `BufferSaved` with the plugin version of the text the save wrote: the bridge
(`plugin_bridge.rs`) keeps each buffer's last 256 versions with their plugin versions
(`HISTORY`), and `advance` looks a save's version up there. A save that finishes after more than
256 further changes to its buffer finds its version evicted, and `plugin_events::drain` drops the
`BufferSaved` (it logs at debug). Found in T278's review round 2 (F1); T278 documents it as a limit.

## Change

- Keep the correspondence between a buffer's versions and its plugin versions for as long as a
  save in flight may name one, with no edit-count cutoff: e.g. count the batches since the save's
  version (`BufferSnapshot::edits_since`), since every batch of a followed buffer is one plugin
  version, or keep the history back to the buffer's edit-log floor.
- Memory stays bounded by what the buffer itself keeps.

## Acceptance

- A test starting a save, then making more than 256 edits before it completes: the plugin hears
  `BufferSaved` with the version the save wrote.

## Implementation notes

`plugin_bridge::advance` no longer keeps a 256-version history. A version older than the latest seen is mapped by counting the buffer's edit batches since it (`edits_since`), minus those since the latest seen. Memory is bounded by the buffer's own edit log. A version older than the buffer's oldest version, or one plugins never heard of, still gives none.
