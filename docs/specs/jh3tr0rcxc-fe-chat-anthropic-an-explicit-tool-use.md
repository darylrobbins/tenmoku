---
task_uid: "01m7b7w7t0qv5hr9jh3tr0rcxc"
task: "T625"
rev: 3
type: "bugfix"
complexity: "s"
criteria:
  - id: "C1"
    text: "Regression test in crates/fe-chat/tests/tool_input_parse_status.rs: a reply ending at max_tokens whose finished last call has a JSON string input is resent once as the identical request and the call never enters history"
    required: true
    manual: false
  - id: "C2"
    text: "Test in crates/fe-chat/tests/tool_input_parse_status.rs: an input cut short mid-stream at max_tokens still keeps finished text only, no tool_use and no results"
    required: true
    manual: false
---

# fe-chat-anthropic: an explicit tool_use input parse status, so a finished non-object input at max_tokens is resent as invalid input, not taken as cut short (T1784 F2 follow-up) (Mac T1815)

Follow-up to T1784 (AI agents AG3, helium T616), carried finding F2 (medium) from its review, accepted there as a known limit on criterion C10.

Problem: in `crates/fe-chat-anthropic/src/events.rs` (`Block::finish`), a `tool_use` input that is not valid JSON is kept as `Value::String(raw)`. A finished input that is a JSON string (e.g. `"a.rs"`) is also a `Value::String`, so the two cannot be told apart. `ended_mid_input` in `crates/fe-chat/src/conversation.rs` tests `input.is_string()` when a reply stops at `max_tokens` with a kept last call, so a finished non-object input is taken as cut short: the turn keeps finished text only and ends Done, with no resend. Record rule (F9): a reply with invalid tool input is retried once as the identical request and never enters history.

Fix: give a `tool_use` block an explicit input parse status from the provider (parsed object / parsed non-object / unparsed or cut short) instead of the raw-string flag, carry it through `crates/fe-chat/src/provider.rs`, and have fe-chat treat a finished non-object input at `max_tokens` as invalid tool input (one identical resend, never in history), while a truly cut-short input keeps the cut-short handling.
