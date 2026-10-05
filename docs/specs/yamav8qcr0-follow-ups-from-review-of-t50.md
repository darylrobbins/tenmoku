---
task_uid: "01m44scpeny5bpcfyamav8qcr0"
task: "T55"
rev: 3
type: "chore"
complexity: "s"
criteria:
  - id: "C1"
    text: "The self dev-dependency in crates/fe-syntax/Cargo.toml sets default-features = false and keeps features = [\"no-query-budget\"]"
    required: true
    manual: false
  - id: "C2"
    text: "scripts/check-syntax-features.sh passes, now building --all-targets with no language features and with each feature alone"
    required: true
    manual: false
  - id: "C3"
    text: "cargo test -p fe-syntax passes with default features (the bash/markdown-gated upstream_mismatches tests still compile)"
    required: true
    manual: false
---

# Follow-ups from review of T50

Findings of the review of T50 that don't block the merge (below merge.block_severity):

- F1 medium crates/fe-syntax/Cargo.toml:106: The self dev-dependency enables default features. Cargo unifies these into the --all-targets builds used by scripts/check-syntax-features.sh, so its no-feature and single-feature checks now compile all default grammars. This silently removes coverage for incorrect feature gating.
  fix: Add default-features = false to the self dev-dependency while retaining features = ["no-query-budget"], then rerun the syntax_features gate.
  instances: crates/fe-syntax/Cargo.toml:106
