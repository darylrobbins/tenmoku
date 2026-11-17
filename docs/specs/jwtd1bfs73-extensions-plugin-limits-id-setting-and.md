---
task_uid: "01m7bky3285rcdpvjwtd1bfs73"
task: "T357"
rev: 3
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "Setting `[extensions.plugin_limits.\"\u003cid\u003e\"]` exists in the schema, template and parser, user layer only (project layer ignored with a diagnostic), values clamped to the host ceilings with a clamping diagnostic."
    required: true
    manual: false
  - id: "C2"
    text: "New tests in crates/fe-config/tests/plugin_limits_setting.rs: `plugin_limits_user_layer_applies`, `plugin_limits_project_layer_is_ignored`, `plugin_limits_clamped_with_diagnostic`, `plugin_limits_unknown_plugin_id_is_kept_not_error`."
    required: true
    manual: false
  - id: "C3"
    text: "The grant prompt shows a background-limits line when the manifest declares them; test in plugin_prompt_tests.rs."
    required: true
    manual: false
  - id: "C4"
    text: "plugin_host.rs applies the effective limits (manifest, then user setting, clamped) when loading a plugin; test that a lowered user value wins."
    required: true
    manual: false
---

# [extensions.plugin_limits."<id>"] setting and prompt line for background limits (B2) (Mac T1699)

# [extensions.plugin_limits."<id>"] setting and prompt line for background limits (B2)

Draft id B2 of the plugins stage 3 plan (docs/decisions/dqsjfernkt-plugins-stage-3.md, section 6). The design is approved by the user (2026-11-09); questions Q1 to Q7 follow the design's proposals.

Section 3.2. Settings are user-only so a project cannot raise a plugin's limits.

Read the design sections named above before starting. Do not widen --scope; if a file outside it is needed, stop and say so.

## Implementation notes

- The setting is validated in fe-config's `clamp_plugin_limits`: invalid values are dropped and out-of-range ones lowered, each with a warning. Background memory is held to min(2 GiB, RAM / 4), as the host's `Ceilings` hold it; `effective_limits` in plugin_host.rs holds the result to `Ceilings` again.
- The grant prompt always shows the background-limits line: `BackgroundLimits` can't tell a declared default from an undeclared one.
- The host loads with the limits of the artifact it loads, not the newest manifest found.
- A manifest that raises the background memory or wall time over what the user accepted asks again, as a new capability does (`with_background`, over kiln-plugin's `evaluate_background`). Allow records the limits with `accept_background`, Don't Allow with `decline_with_background`.
