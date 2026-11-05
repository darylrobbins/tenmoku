---
task_uid: "01m6p36wpbhzx8gke1q3z7nktv"
task: "T452"
rev: 1
type: "docs"
complexity: "xs"
criteria:
  - id: "C1"
    text: "§ 8 of docs/decisions/5at38c8v89-ai-chat.md states the limit: what is checked at lookup, the swap it doesn't survive, why it stays and what closing it would take, as the spec lists; nothing else in the file changes"
    required: true
    manual: false
---

# Record the chat Apply and OpenPath symlink race as a known limit in the AI chat decision record (T451's F3)

Record a known limit in the AI chat decision record, `docs/decisions/5at38c8v89-ai-chat.md`, at the end
of § 8 ("Code blocks become proposals"). Docs only: no code or test changes.

T451 tried to close this race and was abandoned on the Mac lead's decision of 2026-11-05. It began as
medium finding F3 in the review of T251.

Add a short paragraph that says:

- **What is checked.** Chat Apply and OpenPath check that a target stays inside the root when they look
  it up (`locate` in `crates/fe-workbench/src/chat_apply.rs`). What they carry on to loading is a
  pathname, not a file identity.
- **The limit.** If the file or one of its ancestors is swapped for a symlink pointing outside the root
  between that lookup and the load, the load follows the symlink. An open buffer found through an alias
  has the same gap if the buffer closes before the open and the alias has been retargeted.
- **Why it stays.** The swap needs something already able to write inside the project at that moment. A
  same-user process that can do that already has full access to the user's files. What remains is mainly
  a folder shared with another user.
- **What closing it would take.** Every opener would have to carry file identity: `fe-buffer` and the
  editor model recording it at load, `fe-fileview` and the image viewer taking a verified handle instead
  of reopening the path, root-anchored resolution in place of `locate` and `real_path`, and real file ids
  on non-Unix. That is a separately sized piece of work, and no card is open for it.

Keep it to the facts above, in the record's existing style. Don't change the rest of § 8.
