---
task_uid: "01m5jjm1zf5d70ktj7xfewhcm8"
task: "T285"
rev: 1
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "decorations::a_set_at_the_current_version_is_provided: PluginDecorations returns the pushed decorations for the r"
    required: true
    manual: false
  - id: "C2"
    text: "decorations::a_set_for_an_older_version_is_mapped_through_edits: after an insert before a range, the range is shi"
    required: true
    manual: false
  - id: "C3"
    text: "decorations::the_provider_never_calls_the_plugin: with the plugin stuck in a 100 ms call or unloaded, decorations"
    required: true
    manual: false
  - id: "C4"
    text: "decorations::pushing_needs_the_decorations_capability: without it set fails naming decorations"
    required: true
    manual: false
  - id: "C5"
    text: "decorations::entries_are_capped_per_buffer: the 10,001st decoration is refused"
    required: true
    manual: false
  - id: "C6"
    text: "decorations::unloading_clears_the_plugins_sets: after an unload the provider returns nothing for that plugin"
    required: true
    manual: false
  - id: "C7"
    text: "status::items_hold_text_tooltip_and_command: set then read returns all three; needs the status capability"
    required: true
    manual: false
  - id: "C8"
    text: "status::activating_an_item_dispatches_as_the_plugin: the item command is dispatched as Host(id) through P8 author"
    required: true
    manual: false
  - id: "C9"
    text: "cargo nextest run -p fe-plugin and cargo clippy -p fe-plugin --all-targets -- -D warnings pass"
    required: true
    manual: false
---

# Keep plugin decoration sets, gutter entries and status items in host-held stores mapped through edits and read by the host, never by calling the plugin (Mac T1059)

## Problem

Plugins push decoration sets and gutter-column entries for a buffer at a version, anchored; the host keeps them and m
them through edits, and `DecorationProvider` (`crates/fe-editor/src/decorations.rs:118`, called every painted frame)
reads the host's copy, never the plugin (§7). Status items (text, tooltip, command) are the same kind of host-held st

## Change

`crates/fe-plugin/src/decorations.rs`, `status.rs`, `wit/decorations.wit`, `wit/status.wit`:

- `DecorationStore` keyed by (plugin, buffer): `set(version, decorations)` and `set_gutter(version, entries)` need the
  `decorations` capability; a set for an older version is mapped through the edits since (the buffer's anchors or the
  edit log) and anything inside a deleted range is dropped; a count cap per buffer (a named constant, 10,000) refuses
  more. `PluginDecorations` implements `fe_editor::DecorationProvider` by reading the store only, so it returns at on
  even when the plugin is stuck or unloaded. Unloading a plugin clears its sets.
- `StatusItems`: `set(id, text, tooltip, command)` needs the `status` capability; a `command` is a command id which,
  when activated, is dispatched as `Host(<id>)` through P8's authorization. Items are cleared on unload.
- If `fe-editor` or `fe-gpui` has no seam for gutter-column entries, the store exposes
  `gutter_entries(buffer, range)` and painting them is left to the wiring card (P19), which says so.

## Notes

No painting and no workbench edits here. Run `rg -n gutter crates/fe-editor/src/decorations.rs` first to see what sea

Project rules: keep tests lean and fast (hostile and process fixtures bounded to under 2 s each); new tests go in new

Lend note (helium lead): the lend file cut long lines (title, scope, some criteria and spec sentences); this card's title, scope and the ends of cut criteria were completed from the spec. The Mac copy is authoritative: if it differs, respec to it.

## Implementation notes

- **Versions.** Plugins name versions by the `u64` they were shown, which only the app can turn into a buffer state.
  The decorations slice takes a `BufferVersions` from the app: `now(buffer)` and `at(buffer, version)`, the
  snapshot plugins were shown at that version. Ranges and lines are checked against the text at `version`, then
  mapped with `BufferSnapshot::map_offset` (start `Right`, end `Left`, as `DecorationSet` does), which maps as
  anchors resolve: an edit undone right after is skipped (review F2). A version `at` doesn't know, a newer one, or
  one below the log's floor is `conflict`.
- **What deletion takes.** Each entry has a span: its own range, or for an empty range the characters either side of
  it (none at either end of the text). An entry is gone once all its span is deleted, so an empty one goes with the
  text around it. A gutter entry's range is its line's bytes, line break included, so it goes with its line. The
  same rule holds for a set made late, whose entries are dropped before they are kept, and for a set held while the
  edits come, whose gone entries are hidden from reads at once and dropped when it is refreshed (review F1).
- **Storage.** Each set is a `fe_editor::DecorationSet` holding the ranges only, each entry's `priority` the index of
  the plugin's record, which keeps the style, colour, hover text and the span's anchors. So the sets follow edits and
  are queried as the editor's own are. `PluginDecorations::refresh` rebases them and drops gone entries, and so does
  every set before the cap is counted, so what edits deleted no longer counts toward it.
- **Cap.** `MAX_ENTRIES_PER_BUFFER` (10,000) counts one plugin's decorations and gutter entries on one buffer over
  all its sets; another plugin, or another buffer, has its own. Text lengths are not capped.
- **Painting.** `PluginDecorations` gives highlight → `Highlight`, underline → `Underline`, squiggle → wavy
  `Underline`, faded → `Foreground("comment")`. The colour picks the `Tint` by its last segment
  (`diagnostic.warning` → `Warning`), else the style's default. fe-editor has no strikethrough and no gutter-column
  entry with a symbol or tooltip (its `GutterMark`s are bars), so strikethroughs are not provided and gutter entries
  are read with `DecorationStore::gutter_entries(buffer, snapshot, lines)`; `DecorationStore::decorations` returns
  every decoration with its hover text. Painting those, and registering the provider on each editor, is P19's.
- **Loads.** `State::new` attaches a new load of the plugin to the store (and to `StatusItems`), dropping any sets or
  items an earlier load left. `unload(plugin)` clears them and revokes the load, so a store kept past an unload sets
  nothing more (`denied`), as T283's commands slice does.
- **Status.** `StatusItems` keeps each plugin's items (at most `MAX_ITEMS_PER_PLUGIN`, 64) in the order set.
  `StatusItem::invocation` is the item's command as `Actor::Host(<id>)` from `Source::Mouse`, and
  `StatusItems::activate` dispatches it on the app's `Registry`, whose `PluginPolicy` authorizes it against the
  plugin's grants (P8). A `HostState::default()` answers both slices' imports with `unimplemented`.
- **C3.** `the_provider_never_calls_the_plugin` runs kiln-plugin's `strings.wat`, whose `init` logs; the log sink
  sleeps 100 ms and then waits for the test (1.5 s at most, review F3), so the plugin is inside a call for at least 100 ms while the provider is
  read, and the test checks that the call is still running after the read returns. It then drops the instance (the
  sets stay) and unloads the plugin (they go).
