---
task_uid: "01m785w6npqgkj9xrt8txzhy31"
task: "T1743"
rev: 1
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "tests::toast_overflow passes: a choice-only toast with three or more choices shows at most two buttons plus a More button whose menu lists the rest, and picking from the menu replies with that choice"
    required: true
    manual: false
  - id: "C2"
    text: "tests::toast_b3 passes with the debug assertion now covering choice-only toasts too (more than two visible actions asserts); the grammar prompt (three choices) and an LSP showMessageRequest with four actions render without asserting"
    required: true
    manual: false
  - id: "C3"
    text: "cargo nextest run -p fe-workbench --test l10n passes (the More label comes from toast.ftl)"
    required: true
    manual: false
---

# Toasts: choices beyond two overflow into a More menu, so choice-only toasts keep the B3 two-action limit (T1556 F3 follow-up)

T1556 (Toasts in B3) caps a toast at two actions with a debug assertion, but exempts choice-only toasts (`Toast::choices`), because real callers carry more: the grammar prompt (crates/fe-workbench/src/grammar_prompt.rs:438) offers three choices, and LSP window/showMessageRequest (crates/fe-workbench/src/lsp.rs:1867) shows however many actions the server sends. Codex kept that exemption open as F3 through two review rounds; T1556's C1 was respecced to a known limit naming this card.

Do: in toast.rs, render at most two choice buttons; when there are more, the second slot becomes a "More" button (label from toast.ftl) opening a menu with the remaining choices, keyboard-reachable in the toast's tab order, and replying with the picked choice through the same callback. Then extend the debug assertion to choice-only toasts (visible actions <= 2). Callers stay unchanged.

Tests: new crates/fe-workbench/src/tests/toast_overflow.rs (declared from tests/mod.rs): three and four choices, menu contents and order, reply on pick, keyboard path, accessible name of More. Adjust toast_b3's assertion test to cover the choice-only case.

## Implementation notes

- The More menu is an inline column of buttons under the toast's buttons (not an anchored popup), so its items join the tab order after More and nothing clips.
- `crates/fe-workbench/l10n-allow.toml` needed three literal entries for the new debug selectors (outside the listed scope; required by the l10n lint).
- T1556 F3: the no-area fallback now carries an explicit max height (viewport less bottom offset and top margin) via `ToastLayer::fallback_height`. No setup-sheet small-window test was added.
- `tests/a11y_reach_chrome.rs` (out of scope) pinned the grammar offer's three buttons; updated to Install, More (menu: Not now, Never), Dismiss.
