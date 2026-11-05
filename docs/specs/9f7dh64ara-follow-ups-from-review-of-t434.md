---
task_uid: "01m6pw51ts991ezc9f7dh64ara"
task: "T459"
rev: 1
type: "chore"
complexity: "s"
criteria: []
---

# Follow-ups from review of T434

Findings of the review of T434 that don't block the merge (below merge.block_severity), filed by the lead at the Mac lead's request (2026-11-05):

- F1 medium crates/fe-vim/tests/corpus/ex_search_case.toml:213: Two new repeat regressions do not exercise the intended case-policy distinction. star_address_repeat executes :/\/d, searching for literal /d and failing with E486; its recorded expectation confirms that it replaces the search register instead of repeating Foo. star_amp_repeat replaces the first Foo under either case policy, so it passes even if :&& loses the smartcase exemption.
  fix: Use the actual :\/d address-repeat syntax and record the deletion of lowercase foo. Make the :&& target line start with lowercase foo so losing the exemption changes the result, then regenerate both expectations.
  instances: crates/fe-vim/tests/corpus/ex_search_case.toml:213, crates/fe-vim/tests/corpus/ex_search_case.expected.json:748, crates/fe-vim/tests/corpus/ex_search_case.toml:218, crates/fe-vim/tests/corpus/ex_search_case.expected.json:767
