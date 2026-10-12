---
task_uid: "01m4rhftzwg1029s0dsjjgeqp0"
task: "T517"
rev: 1
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "toast_tests::queue: at most 3 shown with more queued, an identical shown toast counts up instead of adding one, info and warning expire after 6 s and errors do not, and a hovered toast does not expire (GPUI-free queue, fake clock)"
    required: true
    manual: false
  - id: "C2"
    text: "toast_tests::window: toast::show from App reaches the active window's overlay; the close button and toast.dismiss_all remove them; clicking the action runs its command and dismisses the toast"
    required: true
    manual: false
  - id: "C3"
    text: "toast_tests::focus_and_vim: with vim on and the editor in insert mode, and again in visual mode, showing, hovering, clicking the action of and dismissing a toast leave focus in the editor and its vim mode unchanged"
    required: true
    manual: false
  - id: "C4"
    text: "cargo test -p fe-workbench and cargo clippy -p fe-workbench --all-targets -- -D warnings pass; nothing outside the scope changes except this card's docs/specs file"
    required: true
    manual: false
---

# Show app-wide notifications as toasts: info, warning and error, with an optional action, auto-dismissed, never taking focus

## Problem

fastedit has no app-wide way to tell the user something: messages that matter only reach the log (for example a
language server's `window/showMessage`, `crates/fe-workbench/src/lsp.rs:1347`, or a file that can't be opened,
`crates/fe-workbench/src/pane.rs:432`). Day-one audit 2026-10-12: notifications/toasts are table stakes. This card
is the surface; the next routes those messages to it.

## Change

New `crates/fe-workbench/src/toast.rs` (declared from workbench.rs), rendered by each window's `Workbench` as an
overlay at the bottom right, above the status bar:

- `toast::show(cx: &mut App, Toast)` from anywhere; it goes to the active window (else the first). `Toast { level:
  Info | Warning | Error, message, detail: Option, action: Option<(label, command id)> }`.
- At most 3 shown, newest at the bottom; more queue. An identical toast still shown gets a count (`×3`) instead of
  another. Info and warning dismiss after 6 s, errors stay until dismissed; hovering pauses the timer.
- A close button on each; `toast.dismiss_all` "Dismiss Notifications" (palette; key in the keys card). Clicking the
  action runs its command (as the palette would) and dismisses the toast.
- Toasts never take focus: showing, hovering, dismissing or clicking one leaves focus, and the editor's vim mode
  (normal, insert, visual, pending), exactly as it was.
- The queue and timers live in a GPUI-free struct in toast.rs, unit-tested without a window.

## Tests

New file `crates/fe-workbench/src/toast_tests.rs`, declared from toast.rs.
