---
task_uid: "01m4m9qxn0a4dpax8eab6gc6mz"
task: "T341"
rev: 1
type: "chore"
complexity: "m"
criteria: []
---

# Follow-ups from review of T324

Findings of the review of T324 that don't block the merge (below merge.block_severity):

- F2 medium crates/fe-vim/src/search.rs:1541: Alternative-dependent endings still collapse into one boolean. For \%(a\|.\). on "x\u{301}", the bare ending of the unused `a` alternative enables the following dot's mark-only path after the `.` alternative. Its mark suffix can backtrack, producing a match where nvim 0.12.4 finds nothing. Capturing alternatives have the same defect.
  fix: Preserve the connection between each alternative and its outgoing character context instead of OR-ing their endings; add positive and negative oracle cases for both capturing and noncapturing alternatives.
  instances: crates/fe-vim/src/search.rs:1541, crates/fe-vim/src/search.rs:1544, crates/fe-vim/src/search.rs:1549

- F3 medium crates/fe-vim/src/search.rs:1517: Appending greedy MARKS does not ensure that the remaining marks form one atom: regex matching can partition them between repetitions. On "a\u{301}\u{323}", a.\{2}, a\S\{2}, a[^x]\{2}, and a\(.\)\{2} still match, whereas nvim 0.12.4 finds nothing. The new positive counted-dot case passes because another base follows the marks and does not detect this failure.
  fix: Prevent a mark-consuming path from ending inside the remaining mark sequence, including between repetition iterations; add negative counted-multi cases without enough following characters and cases with captures.
  instances: crates/fe-vim/src/search.rs:1517, crates/fe-vim/src/search.rs:1624, crates/fe-vim/src/search.rs:1626, crates/fe-vim/src/search.rs:1635, crates/fe-vim/src/search.rs:1700

- F4 medium crates/fe-vim/src/search.rs:1450: Order-independent matching is implemented only for at most three written marks. nvim 0.12.4 matches "a\u{301}\u{323}\u{302}\u{303}" against "a\u{323}\u{301}\u{302}\u{303}b", and likewise matches the leading-mark version; carrying deliberately falls back to pattern order and rejects both.
  fix: Implement order-independent mark requirements without silently changing semantics after three marks, and record reversed-order cases with four or more marks for literal-base and leading-mark patterns.
  instances: crates/fe-vim/src/search.rs:1450

- F5 medium crates/fe-vim/src/search.rs:1445: carrying requires a separate occurrence for every written mark, including duplicates. nvim 0.12.4 matches both "a\u{301}\u{301}" and "\u{301}\u{301}" against "a\u{301}b", but this translation requires two acute marks and finds nothing.
  fix: Treat duplicate written marks as the same carrying requirement, and add oracle cases for duplicate marks after a literal base and in a leading-mark pattern.
  instances: crates/fe-vim/src/search.rs:1445
