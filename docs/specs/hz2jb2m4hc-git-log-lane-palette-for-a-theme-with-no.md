---
task_uid: "01m75pycwcrdxwmzhz2jb2m4hc"
task: "T350"
rev: 1
type: "feature"
complexity: "xs"
criteria:
  - id: "C1"
    text: "Tests in tests::git_log_b3 (cargo nextest run -p fe-workbench) pass: a theme where every signal role equals accent.base or agent.base gets a defined lane palette that is signal-only and never equals accent.base or agent.base, the policy is named in a doc comment, and a test builds such a theme"
    required: true
    manual: false
  - id: "C2"
    text: "cargo nextest run -p fe-workbench --test l10n passes (ui_text_is_in_the_catalogue, keys_used_here_exist)"
    required: true
    manual: false
---

# Git log: lane palette for a theme with no eligible signal colour (Mac T1551 follow-up F1)

# Git log lane palette for a theme with no eligible signal colour (follow-up of Mac T1551, review F1)

Mac T1551 (argon T348) draws the Git log graph's lane colours from the theme's signal roles, excluding `accent.base` and `agent.base` (which mean you and agents). Its C1 is narrowed to "whenever any signal role is eligible"; a theme where every signal role equals the accent or the agent colour is a known limit that this card closes.

## Change (`crates/fe-workbench/src/git_log.rs`, `crates/fe-workbench/src/tests/git_log_b3.rs`)

Define the policy for a theme with no eligible signal colour and test it. The policy must keep both rules, signal-only lane colours and never the accent or agent colours: for example derive a distinct palette from the signal roles by shifting them off the reserved colours, or validate and normalise the conflicting theme and report it, rather than falling back to a non-signal colour. Record the chosen policy in a doc comment at the palette function.

Platform: **Linux-OK**.
