---
task_uid: "01m5fg3216p4pt61yf2ad3qt8s"
task: "T209"
rev: 1
type: "chore"
complexity: "xs"
criteria:
  - id: "C1"
    text: "rg -n '^\\*\\*Status: accepted\\*\\*' docs/decisions/vmtnmkay62-extensions-and-plugins.md matches line 3 and rg -c 'Status: proposed' on the file is 0"
    required: true
    manual: false
  - id: "C2"
    text: "in §12 Q5, Q6 and Q7 are struck through and say decided/accepted/approved 2026-10-21 (rg -c '2026-10-21' on §12 is at least 5), Q8 stays an open bold question with the T845 measurements sentence, Q9 and Q10 say 'Deferred 2026-10-21'"
    required: true
    manual: false
  - id: "C3"
    text: "§10 no longer says 'drafted after sign-off' (rg -c 'drafted after sign-off' is 0) and §9 no longer says fastedit has no snippet engine"
    required: true
    manual: false
  - id: "C4"
    text: "git diff --stat shows only docs/decisions/vmtnmkay62-extensions-and-plugins.md"
    required: true
    manual: false
---

# Record the user's 2026-10-21 decisions in the extensions decision record: accept it, close Q5-Q7, defer Q9-Q10 (Mac T1043)

## Problem

The user answered the extensions record's open questions on 2026-10-21 ("finish stage 1" and approved the
lead's sign-off recommendations). The record still says "Status: proposed" and §12 lists all of
Q5-Q10 as open.

## Change

Only `docs/decisions/vmtnmkay62-extensions-and-plugins.md`:

- Line 3: `**Status: accepted** (user, 2026-10-21; §12). Task T839.` (drop "Waiting for the user's sign-off").
- §12, in the style of Q1-Q4 (`~~Title~~: decided 2026-10-21, ... (§n).`):
  - Q5: `~~Stage 1 isn't code-free~~: accepted 2026-10-21 (§1, §6). Any change to an lsp server's effective
    invocation asks again; the grammar card's execution deadline is T845's.`
  - Q6: `~~Sign-off~~: approved 2026-10-21; this record is accepted and the stage-1 cards are drafted.`
  - Q7: `~~WASM or mlua~~: accepted 2026-10-21: WASM components for stage 2; mlua only as a possible later
    user-scripting layer (§7).`
  - Q8: stays open (bold, unchanged question) with one added sentence: "Left open 2026-10-21 pending the
    grammar card's (T845) size and first-parse measurements."
  - Q9: `**Deferred 2026-10-21:** decided with the registry, not before (§8).`
  - Q10: `**Deferred 2026-10-21:** revisited with managed deployments.`
- §10: replace "Stage 2: drafted after sign-off." with "Stage 2: drafted after stage 1 is finished (user,
  2026-10-21: finish stage 1 first)."; and the stage-1 "Later" list notes that the snippet engine has
  landed (T856, T857), workspace extensions, disabling by setting, snippets as a contribution kind,
  and the consent prompt and extensions list are being carded.
- §9, first sentence: "fastedit has no snippet engine, so stage 1 doesn't take `contributes.snippets`"
  becomes past tense: the engine landed (T856, T857) and `snippets` is now a stage-1 kind.

## Out of scope

Any other section, `app-framework.md`, code. (T1005 edits §6 of the same file: it is inbox and its
edit is in a different section.)
