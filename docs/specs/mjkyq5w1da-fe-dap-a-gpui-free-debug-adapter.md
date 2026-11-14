---
task_uid: "01m7cafdpz3q6cqemjkyq5w1da"
task: "T643"
rev: 2
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "Tests in tests::transport (cargo nextest run -p fe-dap transport) pass: a_header_split_across_reads, two_messages_in_one_read, an_oversize_body_fails_with_a_protocol_error, an_unknown_header_is_ignored, content_length_is_case_insensitive"
    required: true
    manual: false
  - id: "C2"
    text: "Tests in tests::protocol (cargo nextest run -p fe-dap protocol) pass: every message type round-trips its specification example, and unknown_fields_survive_a_round_trip"
    required: true
    manual: false
  - id: "C3"
    text: "Tests in tests::fake (cargo nextest run -p fe-dap fake) pass: the_fake_answers_initialize_and_launch_over_a_pipe, plus one test per §2 fake knob, named after it, showing the knob changes what the fake sends"
    required: true
    manual: false
  - id: "C4"
    text: "Tests in tests::launcher (cargo nextest run -p fe-dap launcher) pass: local_launcher_substitutes_port_and_connects_to_fe_dap_fake, connecting_gives_up_after_5_s_on_the_clock, cancel_stops_a_pending_launch, the_adapter_runs_in_its_own_process_group"
    required: true
    manual: false
  - id: "C5"
    text: "scripts/check-no-gpui.sh prints `ok: fe-dap`, and cargo deny check licenses bans sources passes"
    required: true
    manual: false
  - id: "C6"
    text: "cargo nextest run -p fe-dap --test l10n passes (ui_text_is_in_the_catalogue, keys_used_here_exist)"
    required: true
    manual: false
---

# fe-dap: a GPUI-free Debug Adapter Protocol crate with framing over stdio and TCP, protocol types, the adapter launcher and a scripted fake adapter (debugger D1) (Mac T1840)

# fe-dap: a GPUI-free Debug Adapter Protocol crate with framing over stdio and TCP, protocol types, the adapter launcher and a scripted fake adapter (debugger D1)

Source: `docs/decisions/r1b9dw3g0g-debugger.md` (the debugger record, approved with amendments 2026-11-13; §13 wins over §§1-12). Size **m**.

## Problem

Tenmoku has no debugger. The approved record `docs/decisions/r1b9dw3g0g-debugger.md` adds a Debug Adapter Protocol
client as a new core crate, `fe-dap`. This card is its foundation: everything the
session model (D2, D2b) and the workbench need to talk to an adapter process.

## What to build (§2, §11 D1)

A new workspace member `crates/fe-dap` with no GPUI (fe-lsp's runtime shape: a
thread per session, `async-io`, `async-process`, no tokio; no public item names a
dependency's type):

- **`transport`**: Content-Length framing over stdio and over a TCP connection to
  127.0.0.1; case-insensitive `Content-Length`, other headers ignored, one
  `write_all` and flush per message, a 64 MB body limit that fails the session with
  a protocol error. Not async-lsp: DAP's envelope (`seq`, `type`) isn't JSON-RPC (§2).
- **`protocol`**: hand-written serde types for §2's ~30 requests, events and reverse
  requests, `i64` integers, `#[serde(flatten)] extra` maps so nothing an adapter
  sends is lost. `launch`/`attach` arguments are a `serde_json::Map`. No DAP crate (§1).
- **`AdapterLauncher`** (`launch(spec, cwd, cancel) -> io::Result<AdapterIo>`),
  `LocalLauncher` (own process group, `${port}` substitution, connect retries up to
  5 s), `LaunchCancel`, and an injected clock.
- **`fake`** behind a `test-support` feature: the scripted in-process adapter over an
  in-memory pipe, with **every knob §2 lists**, serving any byte stream including an
  accepted TCP connection.
- **`fe-dap-fake`**, a test-support binary target running the fake over TCP on the
  port it is given, so `LocalLauncher`'s TCP mode is tested end to end.
- The crate's l10n plumbing (`src/l10n.rs`, `l10n/en-US/debug.ftl`, `l10n-allow.toml`,
  `tests/l10n.rs`, as fe-lsp has).

Suggested layout, which later cards' scopes name: `src/{lib,transport,protocol,launcher,clock,fake,l10n}.rs`
(or directories of those names), `src/bin/fe-dap-fake.rs`, `src/tests/`.

## Not in this card

The session state machine (D2, D2b), adapter specs and settings (D3), any workbench
code. `scripts/check-no-gpui.sh` needs no edit (every member not in `UI_CRATES` is
core); it is in scope only in case its output format needs it.

## Conventions for every debugger card (§13)

- **Strings.** Every user-visible string goes through the catalogue
  (`docs/decisions/xte5e7hqsw-i18n-design.md` §§ 4-5): `tr!` / `text!`, whole
  messages, plural selectors for counts. Workbench keys are `workbench-debug-…` in
  `crates/fe-workbench/l10n/en-US/debug.ftl` (the first card that needs the file
  adds it to `resources!` in `src/l10n.rs`); fe-dap's are in
  `crates/fe-dap/l10n/en-US/debug.ftl`; fe-gpui's in `crates/fe-gpui/l10n/en-US/debug.ftl`.
  A new command's title is the key `command-<id>` in the same file. Tooltips and
  accessible names count. Tests build expected text through the catalogue.
- **Accessibility.** Every control this card adds has an accessible name (an
  icon-only button names itself), and session changes are announced through
  `kiln_ui::a11y::announce` ("Stopped at line N", "Running", "Session ended").
- **Tests.** New tests go in new files, following today's placement:
  `src/tests/<name>.rs` declared from `src/tests/mod.rs` (ids `tests::<name>::<test>`),
  or an integration test under `tests/` for public API (ids bare, no binary prefix).
- **Design calls** go in your decision file `docs/decisions/<lane>-*.md` (exempt
  from scope). Where §13 and §§1-12 of the record disagree, §13 wins.

Platform: **Linux-OK** (headless tests only; no screenshot goldens move).

## Implementation notes

- Design calls are in `docs/decisions/mjkyq5w1da-fe-dap-foundation.md`, including
  the known limit of TCP mode's free-port race.
- C3's knob tests, one per §2 knob, in `tests::fake`:
  `startup_orderings_with_configuration_done`,
  `startup_orderings_without_configuration_done`,
  `resumes_answered_with_no_continued_event`,
  `a_stopped_event_sent_before_a_step_response`, `a_resume_that_fails`,
  `a_resume_that_is_never_answered`,
  `a_resume_failure_delayed_until_after_another_resume`,
  `a_resume_failure_delayed_until_after_a_terminate`,
  `a_variables_reply_delayed_past_a_step`,
  `a_program_that_runs_with_no_thread_events`,
  `unverified_then_verified_breakpoints`, `the_adapter_exiting_mid_request`.
- C2: one `<message>_round_trips` test per request, event and reverse request in
  `tests::protocol`, plus `unknown_fields_survive_a_round_trip`.
- `scripts/check-no-gpui.sh` and `deny.toml` needed no change: fe-dap adds no
  dependency the lock didn't already have.
