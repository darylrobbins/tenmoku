---
task_uid: "01m516qsgc3dmh6admmm3hv1k3"
task: "T101"
rev: 1
type: "docs"
complexity: "s"
criteria:
  - id: "C1"
    text: "kiln/README.md opens its usage with the notes example and keeps the manual kiln::init path as the layer underneath"
    required: true
    manual: false
  - id: "C2"
    text: "kiln/docs/framework.md describes each convention with its override, each macro with its expansion and handwritten equivalent, the Module rule, Cx and cx.watch, dropping to GPUI, and what kiln deliberately does not do, without naming fastedit"
    required: true
    manual: false
---

# Document kiln's app layer in kiln's README and framework.md: the notes example first, the manual path underneath, and dropping to GPUI (Mac T689)

Filed on argon from Mac T689 (card ~/coordination/argon-cards/T689.json rev 1; note zj). Mac task uid 01m4vbjh877ydh7y37nm3hxskd. Base: argon trunk 6a499d8 = mac/main b39ea29 synced (Mac T675 landed). Mac deps already on the trunk: Mac T665. Argon deps: T100 (Mac T688). Title the host commit "... (Mac T689)".

## Problem

Part of the Kiln app layer the owner approved on 2026-10-13 (design record `docs/decisions/kiln-app-layer.md`).
Once phase 2 lands, kiln's README still opens with the manual `kiln::init` path, and nothing in kiln's own docs
walks an author from `kiln::app` down to raw GPUI.

## Change

Update `kiln/README.md` and `kiln/docs/framework.md` (kiln's docs, written without naming fastedit):

- README "Using it" opens with the notes example (`examples/notes`), then "underneath": the manual path
  (`kiln::init`, `Config`, `Root`/`Shell`/`Workspace`) as today's text has it.
- framework.md gains a section on the app layer: the conventions table with each override, the macros with what
  each expands to and its handwritten equivalent, the `Module` rule for library crates, `Cx` and `cx.watch`, and
  a "dropping to GPUI" part (`Cx`'s `window`/`cx` fields, any `Render + Focusable` accepted, `Workspace::shell()`/
  `group()`/`dock()`, `window_root`), and what kiln deliberately doesn't do (no element macro, no signals, no DSL,
  nothing on the paint path).
