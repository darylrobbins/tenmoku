---
task_uid: "01m4sv9j3950dnh08373jz0d66"
task: "T80"
rev: 2
type: "bugfix"
complexity: "xs"
criteria:
  - id: "C1"
    text: "After Next Prompt on the alternate screen, a hidden resize and a return to a scrolled primary view, Previous Prompt goes to the newest eligible mark; a regression test for that sequence fails on 621015a"
    required: true
    manual: false
  - id: "C2"
    text: "Every existing fe-terminal shell_integration test still passes, unchanged"
    required: true
    manual: false
---

# Clear the frozen remembered prompt when Next Prompt runs on the terminal's alternate screen, so Previous Prompt doesn't skip the newest mark after a hidden resize (T76 review F11, Mac T613 follow-up)

Follow-up from the final review of argon T76 (Mac T613, shell integration, 621015a): finding F11 (medium, crates/fe-terminal/src/shell.rs:481). Filed by the argon lead under note d (a real bug). It goes back to the Mac with Mac T613 in an argon batch.

Problem: on the alternate screen, Next Prompt clears the live remembered prompt but leaves `frozen.remembered` as it was. A later hidden resize, followed by a return to a scrolled primary view, restores that stale remembered prompt, so Previous Prompt then skips the newest eligible mark. It's a sibling of review F4, which T76 fixed, in code outside T76's changed lines.

Fix: clear the navigation state with `forget()` here, so the frozen copy goes too. Or have prompt navigation return straight away while the alternate screen is active. Choose whichever keeps the existing tests' behaviour.

Test (crates/fe-terminal/tests/shell_integration.rs): a regression for exactly this sequence, failing on 621015a. Prompt marks on the primary screen, scroll back, enter the alternate screen, Next Prompt, resize while hidden, leave the alternate screen: Previous Prompt must go to the newest eligible mark.
