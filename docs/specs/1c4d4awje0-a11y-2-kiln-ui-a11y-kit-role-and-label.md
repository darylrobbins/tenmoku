---
task_uid: "01m743s5kgttsm9c1c4d4awje0"
task: "T572"
rev: 3
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "kiln_ui::a11y has the role and label helpers, the recorder of (element id, Role, label, states) under cfg(any(test, feature = \"test-support\")) read back by nodes(cx), the Announcer live region and Politeness, as sections 5 and 9 specify; kiln-ui gains [features] test-support = [] and its lib.rs only the forced mod and pub use lines. Known limits, fixed by follow-up T576 and not required here: the recorder's snapshot is refreshed only when a helper records a node, so empty draws and cached subtrees are not tracked (review rd71ft F1); the recorder keys nodes by (view, local element id) without namespace (F4); an announcement submitted while its window is mid-update falls back to the Announcer's previous listening flag (F3). The implementation notes in the spec name each of these"
    required: true
    manual: false
  - id: "C2"
    text: "Tests in kiln/crates/kiln-ui/src/tests/a11y.rs cover the recorder and the Announcer's politeness, and crates/fe-workbench/tests/a11y_announcer.rs shows an announcement made in a Workbench reaching the Status node; both pass"
    required: true
    manual: false
  - id: "C3"
    text: "The Announcer is mounted in Workbench::render and in kiln-shell's Root; the A11Y-1 dump or AT-SPI probe of a running Tenmoku window shows the Status node, quoted in the finish notes"
    required: true
    manual: false
  - id: "C4"
    text: "The finish notes list this card's section 12 human-check lines (H-numbers and text), for the A11Y-11 VoiceOver pass"
    required: true
    manual: false
---

# A11Y-2 kiln-ui a11y kit: role and label helpers with a test recorder, and the Announcer live region mounted in Tenmoku and kiln Root (T294 design) (Mac T1668)

Spec: `docs/decisions/qenw9xxdzk-accessibility-design.md`, the **A11Y-2 kiln-ui a11y kit** row of section 11 "Proposed card breakdown", and section 5 "Announcements" (the Announcer) and section 9 shape 2 (the recorder).
The user approved the design's platform path (section 3, option (a): gpui-pre 0.3.9's
AccessKit, no vendoring) and its first slice (section 10) as written on 2026-11-09.

Build what that row and those sections specify, with the tests section 9 "Testing strategy"
assigns (shape 1 for text elements, shape 2 for chrome). If the design's text differs from
this card's criteria, the design wins.

Platform: **Linux-OK**.

## Known limits (respec after the round-3 review cap, 2026-11-11)

Review rd71ft's three mediums move to follow-up T576 and are not required here: F1, the recorder's snapshot refreshes only when a helper records a node (empty draws and cached subtrees are not tracked); F3, an announcement submitted while its window is mid-update falls back to the Announcer's previous listening flag; F4, the recorder keys nodes by (view, local element id) without namespace. Dispute each of F1, F3 and F4 with `rgt review dispute`, citing T576 and C1's known limits, then run finish once.

## Implementation notes

- The recorder lists the nodes of the last draw, keyed by (drawing view, local element id). The list restarts at the first `node` call after a draw's end (a `window.defer` armed by the previous record). Known limits, for a follow-up card: a draw that records nothing leaves the previous list in place; a cached subtree that doesn't re-render keeps its nodes only if its view draws again; two namespaces inside one view that share a local id merge, because GPUI exposes no namespace-qualified id to an element's builder.
- `node_with` applies `States.value` and `States.live` (the Status node's value, and `set_live` through a synthetic-children closure). The Announcer gives each message a fresh element id so a repeat is a node the platform sees added.
- `announce` asks the receiving window whether a screen reader is listening, falling back to its last draw when the window is mid-draw; tests always listen.
- Section 12 lines for A11Y-11: H1′ (the announcer is one `AXGroup` with subrole `AXApplicationStatus`) and H7 (announcements are spoken: toast, vim mode, find count, fold, Announce Position; whether a later focus change cuts them off).
- Running-app evidence: `driver.sh a11y-tree` on Tenmoku shows `element_id Name("kiln-a11y-announcer")`, role `Status`, in the app's own dump.
