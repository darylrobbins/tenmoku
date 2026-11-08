---
task_uid: "01m6tg30bgftc9x3t7vs59w490"
task: "T1497"
rev: 2
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "cargo test -p fe-workbench todo_decorations passes, including PERF tint differs from the cursor colour"
    required: true
    manual: false
  - id: "C2"
    text: "The decision file records bold as out of scope and why"
    required: true
    manual: false
---

# Style the TODO comment badge: kind foreground colours and a PERF tint of its own (T1413)

Follow-up of T1413. The TODO/FIXME/HACK/NOTE/PERF/WARN comment badge (editor.todo_comments) is a tinted background only. Modest improvement, in crates/fe-editor/src/decorations.rs (Tint, DecorationKind::Foreground), the theme highlight names and the todo decorations code in fe-workbench:
1. Give the badge keywords a foreground in the kind's colour, via new theme highlight names (e.g. comment.todo, comment.fixme, comment.hack, comment.note, comment.perf, comment.warn) resolved through DecorationKind::Foreground, with a fallback to the existing tints in themes that lack them.
2. Give PERF its own tint: Tint::Accent resolves to the cursor colour, so add a distinct Tint (or highlight name) for it.
3. Bold is OUT OF SCOPE: fe-gpui spans carry only a colour (StyledSpan<Hsla>, one font per shaped line). Record in the decision file why, and what would be needed.
Update crates/fe-workbench/src/todo_decorations_tests.rs and add a screenshot or resolver test showing PERF differs from the cursor colour.
