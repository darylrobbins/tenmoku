---
task_uid: "01m7ap0wde1pq9nb3dk530gq4m"
task: "T346"
rev: 3
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "Tests in cargo nextest run -p fe-runlog --test cargo_json pass: parses_a_recorded_build_into_spans (fresh crates are zero-length, local crates flagged by their path+file package id), timing_info_wins_when_present, warning_and_error_counts, malformed_lines_are_skipped"
    required: true
    manual: false
  - id: "C2"
    text: "The fixtures were recorded from this repo's cargo build --message-format=json-render-diagnostics on the Mac (on the lane's host, Linux: the JSON is platform-independent; the Mac may re-record later)"
    required: true
    manual: false
---

# fe-runlog: parse cargo's JSON build messages into a per-crate timeline with fresh, rebuilt and local crates (Mac T1578)

# fe-runlog: cargo's JSON build messages and per-crate timing

Spec: Runs: build (`docs/design/design-system.md#runs-builds-tests-and-agent-tasks-in-one-list`):
"a crate waterfall instead of a wall of log: fresh crates as thin lines,
rebuilt ones as bars, the crate you edited in rust; messages beside it".

## Change (in `crates/fe-runlog`)

- `CargoParser` (`src/cargo.rs`): fed stdout lines of `cargo build
  --message-format=json-render-diagnostics` and stderr lines, each with its
  arrival offset. `compiler-artifact` (package id, target, `fresh`),
  `compiler-message` (level, file, line, column, rendered text),
  `build-script-executed`, `build-finished`. A crate's span starts at its
  `Compiling` stderr line (or its first message) and ends at its artifact;
  `fresh` crates have a zero-length span. When the stream also carries cargo's
  `timing-info` records (`-Zunstable-options --timings=json`), their
  durations win.
- `BuildTimeline { spans: Vec<CrateSpan { name, version, start, end, fresh,
  local }>, warnings, errors, success }`; `local` is a `path+file://` package
  id, which is what the design means by "the crate you edited".
- Fixtures are recorded from this repo's build on the Mac and checked in.

## Not in this card

Running cargo (a later card), drawing the waterfall (a later card).

Platform: **Linux-OK**.

## Implementation notes

- The fixtures were recorded on Linux (cargo 1.98.1), not the Mac: the same repo's `cargo build -p fe-runlog --message-format=json-render-diagnostics`. Stable cargo can't emit `timing-info`, so `cargo_timing.jsonl` is the recorded stdout with `timing-info` records inserted in cargo's documented shape.
- `json-render-diagnostics` prints diagnostics on stderr, not as `compiler-message`s, so the parser counts both: stderr's `warning:`/`error:` headers (with their `-->` location) and JSON `compiler-message`s. Don't feed it a mode that carries both.
- A path package id of `path+file:///dir#0.1.5` gives the name from the directory; a package whose directory differs from its name gets a separate span from its `Compiling` line.
- Freshness comes from artifacts only (cargo replays a cached crate's warnings); package-id-bound entries stay distinct, and `Compiling` lines are matched to ids by name, version and source. Stderr diagnostics keep every line up to the next diagnostic or cargo status line. Timing durations that don't fit a `Duration` or overflow a sum are skipped.
