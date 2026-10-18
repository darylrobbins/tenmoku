---
task_uid: "01m57ypbvx73j33pxqb7pmv0c4"
task: "T995"
rev: 1
type: "docs"
complexity: "xs"
criteria:
  - id: "C1"
    text: "docs/decisions/5at38c8v89-ai-chat.md says Status accepted, signed off 2026-10-18, has no 'Open questions for the user' section and no remaining 'open question' wording, and has a numbered Decisions section with the six answers"
    required: true
    manual: false
  - id: "C2"
    text: "rg 'dev.fastedit.fastedit.chat|anthropic-api-key' on the record finds nothing; § 7 names service dev.fastedit.fastedit and account chat.anthropic"
    required: true
    manual: false
  - id: "C3"
    text: "§ 6 says deny.toml gets an exceptions entry for webpki-root-certs only (CDLA-Permissive-2.0), made by card 4; § 10 and § 11 say untrusted-folder chats are memory-only; card 9 is stated to depend on T846 and T847"
    required: true
    manual: false
  - id: "C4"
    text: "git diff --stat shows only docs/decisions/5at38c8v89-ai-chat.md changed"
    required: true
    manual: false
---

# Record the user's sign-off of the AI chat decision record: status accepted and the six answers (licence exception, T839 accepted, keychain service, Markdown order, fallback, untrusted chats)

# Record the user's sign-off of the AI chat decision record, with its six answers

## Problem

`docs/decisions/5at38c8v89-ai-chat.md` (T961) is marked "proposed" and ends in six open
questions. The user answered all six on 2026-10-18, which signs the record off. The record must
say so and state each decision where it applies, so the build cards (T962-T973) follow one text.

## Change

Edit only `docs/decisions/5at38c8v89-ai-chat.md`:

- **Status line**: "Status: **accepted**, signed off by the user 2026-10-18." Drop the sentence
  saying the user signs it off before any build card is readied.
- **Replace "Open questions for the user"** with a "Decisions (user sign-off, 2026-10-18)"
  section, one numbered item per former question, stating the decision, not the options:
  1. **Licence.** `deny.toml` gets an `exceptions` entry for `webpki-root-certs` only
     (`{ allow = ["CDLA-Permissive-2.0"], crate = "webpki-root-certs" }`, with a comment that
     `rustls-platform-verifier` pulls it in for wasm32 only and it is not linked into any build
     we ship), as `dwrote` has for MPL-2.0. `CDLA-Permissive-2.0` is not added to the global allow
     list, and `[graph]` is unchanged. The HTTP client card (card 4) makes the edit.
  2. **T839 reconciliation.** Accepted: § 9 stands with its four marked departures from T839; the
     grants card (card 2) follows this record as written and does not wait for T839's sign-off.
  3. **Keychain.** The service is the bare app id, `dev.fastedit.fastedit`, not `...chat`; each
     key has its own account name, the chat's being `chat.anthropic`. This is the general secret
     store's convention (T963), so extension secrets share the service with their own accounts.
  4. **Markdown.** Cards 6-8 (view, panel, context) ship with plain-text replies; card 9
     (Markdown rendering) depends on T846 and T847 and is held until both land.
  5. **Server-side fallback** is on by default; the first-open notice (§ 10) is enough disclosure.
  6. **Untrusted folders.** Chats in a folder the user marked untrusted are memory-only: never
     written to disk, gone when the window closes (and not listed from disk). Trusted folders keep
     chats on disk (files 0600, directories 0700) until deleted.
- **Make the sections agree**: in § 6 change "That is not decided here; it's open question 1" and
  the "No CDLA allowance is added by this record" sentences to the decision above (the exception
  is added by card 4); in § 7 replace service `dev.fastedit.fastedit.chat` / account
  `anthropic-api-key` with service `dev.fastedit.fastedit` / account `chat.anthropic`, and remove
  the "app id plus `.chat`" explanation (also in the "Corrections" list at the top); in § 10
  (Untrusted folders) and § 11 (Persistence) state the memory-only rule for untrusted folders and
  that the History list then shows only that window's in-memory chats; in the card list/table
  (wherever the record lists cards), note card 9's dependency on T846 and T847 and card 4's
  `deny.toml` exception. Fix any other mention of "open question" or "proposed".

No code, no other file. Use `rg` for "open question", "proposed", `.chat` and `anthropic-api-key`
over the record to find every spot.
