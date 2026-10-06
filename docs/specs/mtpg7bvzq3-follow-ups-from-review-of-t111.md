---
task_uid: "01m497hkgje6v5cdmtpg7bvzq3"
task: "T132"
rev: 3
type: "chore"
complexity: "l"
criteria:
  - id: "C1"
    text: "F1: cancelling a search run (rerun or panel drop) during offset decoding stops its batch: the cancel flag is checked between files, between lines and at bounded intervals while reading and decoding a line, and a cancelled batch is not published; proven by tests::search_panel_cancel::cancelling_during_decoding_stops_the_batch (deterministic, no timing)."
    required: true
    manual: false
---

# Follow-ups from review of T111

Follow-up from the review of T111 (fe-workbench search panel).

## F1 (medium): offset decoding ignores cancellation

Still true on main: `SearchPanel::run`'s batch callback (search_panel.rs ~l.179) runs `decode_offsets` on every file of a batch and sends it, with no cancel check; `decode_offsets`, `read_line` and `decode_line` (~l.439–590) never look at the flag. Rerunning or dropping the panel sets the flag but the obsolete batch is still decoded and published. 64 files with matches near the ends of ~16 MiB lines can mean ~1 GiB of extra reads and scanning after cancellation.

Fix: keep a clone of the run's cancel flag in the batch callback and pass it through decoding. Check it between files, between lines, and at bounded intervals while reading and decoding a long line. A cancelled batch stops processing and is not sent.

Test: a deterministic regression in a NEW file, crates/fe-workbench/src/tests/search_panel_cancel.rs (registered in tests/mod.rs; don't add to search_panel.rs or search_panel_followups.rs), that sets the flag during conversion (e.g. from a pub(crate) seam or a flag already set mid-batch) and checks that the remaining files/lines are not decoded and the batch is not published.
