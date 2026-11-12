---
task_uid: "01m77720v01mnkccfz84b43zsv"
task: "T328"
rev: 2
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "`Capability` gains `jobs`, `language`, `panels`, `tasks` and `lsp:\u003cserver\u003e` (named like `process.spawn:\u003cprogram\u003e`, no wildcard), with parsing, Display, serde, `Grants::allows`, and escalation in `grants::evaluate`."
    required: true
    manual: false
  - id: "C2"
    text: "New tests in kiln/crates/kiln-command/tests/capability_stage3.rs: `stage3_each_capability_parses_and_round_trips`, `stage3_lsp_wildcard_is_refused`, `stage3_lsp_name_refuses_separators`."
    required: true
    manual: false
  - id: "C3"
    text: "New test in kiln/crates/kiln-plugin/tests/grants.rs `stage3_adding_a_capability_to_an_installed_plugin_is_an_escalation`."
    required: true
    manual: false
  - id: "C4"
    text: "The grant prompt (crates/fe-workbench/src/plugin_prompt.rs) words each new capability; `lsp:` names the server and says whether it is built in; tests in plugin_prompt_tests.rs."
    required: true
    manual: false
---

# Stage-3 capabilities in kiln-command: jobs, language, panels, tasks, lsp:<server> (K1) (Mac T1697)

# Stage-3 capabilities in kiln-command: jobs, language, panels, tasks, lsp:<server> (K1)

Draft id K1 of the plugins stage 3 plan (docs/decisions/dqsjfernkt-plugins-stage-3.md, section 6). The design is approved by the user (2026-11-09); questions Q1 to Q7 follow the design's proposals.

Section 3.6 and 4. These capabilities gate APIs added by later cards; this card adds only the enum, parsing, grants and prompt wording. Root context rule: a capability names what it grants, never a wildcard.

Read the design sections named above before starting. Do not widen --scope; if a file outside it is needed, stop and say so.
## Decision (neon lead)

Scope widened to kiln/crates/kiln-plugin/src/warnings.rs, kiln/crates/kiln-plugin/l10n/en-US/kiln-plugin.ftl, kiln/crates/kiln-plugin/l10n-allow.toml and crates/fe-workbench/l10n/en-US/extensions.ftl: the new Capability variants make warnings.rs's describe() non-exhaustive, and it is where each capability's prompt wording comes from, so word each new variant there (catalogue entries, no wildcard arm) and have plugin_prompt.rs use it. First step: this lane started before neon main took mac/main 0b3dd960 (T1689 changed the grant prompt and warnings); run `git merge main` in the lane before editing.
