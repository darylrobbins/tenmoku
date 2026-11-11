---
task_uid: "01m71fq2tx9c0rwyd3yt58se9f"
task: "T1676"
rev: 2
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "kiln-platform's system_a11y reads Reduce Motion and Reduce Transparency on macOS and notifies on change, returning defaults off macOS, with the only lib.rs change the forced pub mod system_a11y line; OS cfgs stay inside kiln-platform/src"
    required: true
    manual: false
  - id: "C2"
    text: "Reduce Motion feeds the motion setting section 8 specifies, so the system value applies unless the user's settings override it; tests in crates/fe-config/src/tests/settings.rs cover the system value, the override and a change notification; all pass"
    required: true
    manual: false
  - id: "C3"
    text: "The finish notes list this card's section 12 human-check lines (H-numbers and text), for the A11Y-11 VoiceOver pass"
    required: true
    manual: false
---

# A11Y-10 System a11y observer: read Reduce Motion and Reduce Transparency on macOS and follow Reduce Motion (T294 design)

# A11Y-10 System a11y observer and Reduce Motion

Spec: `docs/decisions/qenw9xxdzk-accessibility-design.md`, the **A11Y-10 System a11y observer and Reduce Motion** row of section 11 "Proposed card breakdown", and section 8 "System settings to respect".
The user approved the design's platform path (section 3, option (a): gpui-pre 0.3.9's
AccessKit, no vendoring) and its first slice (section 10) as written on 2026-11-09.

Build what that row and those sections specify, with the tests section 9 "Testing strategy"
assigns (shape 1 for text elements, shape 2 for chrome). If the design's text differs from
this card's criteria, the design wins.

Platform: **Mac-only reader; the API builds and returns defaults on Linux**.

## Implementation notes

- `workbench.reduce_motion` (`system` | `on` | `off`) needed its schema golden, settings and settings-template ftl, template toml and the qps-ploc fixture updated (tests pin them), and `crates/fe-workbench/l10n/en-US/settings.ftl` gained the Settings page label (`every_row_has_label_key_help_and_control` pins it), all beyond the listed scope.
- The macOS reader uses objc2 0.6 `msg_send!` and block2 0.6 (already in the lock) instead of objc2-app-kit 0.2, whose feature flags and objc2 0.5 types would not mix with kiln-platform's objc2 0.6.
- Section 12 human-check lines for A11Y-11: H9 (Increase Contrast with the system in Dark mode, `theme.mode = system`; does Tenmoku switch to light, does the log print "unknown appearance"; restore afterwards), H10 (Reduce Motion on, then off; expect no visible change, the baseline for the polish audit), H10′ (Reduce Motion on: `cx.reduce_motion()` true in the dump-hook log line, an animated image stops).
