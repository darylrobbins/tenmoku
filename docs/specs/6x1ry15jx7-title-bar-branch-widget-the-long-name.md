---
task_uid: "01m7agfz7zwxq3x76x1ry15jx7"
task: "T370"
rev: 2
type: "chore"
complexity: "xs"
criteria:
  - id: "C1"
    text: "tests::git_sync_ui passes (nextest -p fe-workbench), including a long-branch-name test that at a wide and a narrow window compares the counts bounds with the shaped count-text width, compares the name's room with its full shaped width (so truncation is established) and checks containment and order of name, counts and chevron from the computed bounds. Known limit: the rendered ellipsis text and the chevron's real bounds are not observed, because kiln_ui::Button draws its label and chevron without a test hook; that is follow-up T372"
    required: true
    manual: false
  - id: "C2"
    text: "cargo nextest run -p fe-workbench --test l10n passes"
    required: true
    manual: false
---

# Title bar branch widget: the long-name regression test inspects rendered text and chevron bounds (follow-up from review of T367, Mac T1766)

# Title bar branch widget: the long-name regression test inspects rendered text and chevron bounds (follow-up from review of T367, Mac T1766)

T367's review (codex, round 2, finding F2, medium) found that the regression test for "a long branch name ellipsizes while the counts stay" in crates/fe-workbench/src/tests/git_sync_ui.rs (around lines 186, 535, 553, 573) only partly establishes it: DRAWN records the count string before layout; the width assertions never compare against the full measured count-text width; neither window size checks name truncation; and an unchanged gap to the button edge does not verify ordering against the real chevron bounds.

## Change
Strengthen the test, at both window sizes (wide and the narrow one added in T367): inspect the rendered text and assert the name is truncated (ellipsis) while the complete count text is present; compare the count element's bounds with the full measured count-text width; assert name and counts are contained in the widget and ordered (name, counts, chevron) against the chevron's actual bounds.

Test only; no product change expected. If the test shows the layout really does clip the counts or fail to ellipsize, fix title_bar.rs the minimum needed and say so.
