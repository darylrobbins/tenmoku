---
task_uid: "01m5f6t7gn3aq8a41wvxvae9vt"
task: "T249"
rev: 3
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "fe-chat test budget_fills_in_order_and_truncates_at_rows (tests/context.rs): items are kept in the order selection, files, diagnostics, active file, open editors; the first that doesn't fit is cut at a row boundary ending in a [truncated: N more rows] line when 10 or more rows fit, else Dropped; estimate is ceil(bytes / 3)"
    required: true
    manual: false
  - id: "C2"
    text: "fe-workbench test chat_panel::chat_panel_tests::context::trusted_roots_attach_the_workspace_context: in a trusted temp root with two open editors, a selection and diagnostics set on the active editor's model, the recorded request's new turn holds the selection (relative path, 1-based rows), the active file's unsaved text, its diagnostics and the open editors' paths, then the question"
    required: true
    manual: false
  - id: "C3"
    text: "fe-workbench test chat_panel::chat_panel_tests::context::untrusted_roots_attach_only_what_the_user_chose: in an untrusted root only the selection and a picked @file are sent; with grants lacking buffer.read nothing is gathered"
    required: true
    manual: false
  - id: "C4"
    text: "fe-workbench test chat_panel::chat_panel_tests::context::at_file_refuses_secrets_and_large_files: picking .env, a .pem, a file under .git, a 2 MiB file, a binary file or a path outside the root attaches nothing and toasts the reason; an open file's unsaved text is what's attached"
    required: true
    manual: false
  - id: "C5"
    text: "fe-chat-view test chips_show_estimate_and_budget (tests/chips.rs): chips show the estimate and truncated or won't fit; removing a chip drops it from the next request; typing @ at a word start emits AttachFileRequested and removes the @"
    required: true
    manual: false
---

# Attach @files, the selection, the active file, its diagnostics and the open editors to chat messages within chat.context_budget, through the buffer.read grant (Mac T969)

# Attach @files, the selection, diagnostics and open editors to chat messages, within a token budget

## Problem

The AI chat decision record (`docs/decisions/*-ai-chat.md`; signed off by the user) has each
message carry context from the workspace: files picked with @, the selection, the active
file's diagnostics and the open editors, shown as chips before Send and kept within
`chat.context_budget` (record §13), read through the chat's `buffer.read` grant (§9), and in
an untrusted folder only what the user chose (§10). The chat panel (the card this depends on)
sends the composer's text alone.

## Change

**Headless, in `crates/fe-chat/src/context.rs`** (re-exported from `lib.rs`):

- `ContextKind` (`File`, `Selection`, `Diagnostics`, `ActiveFile`, `OpenEditors`) on
  `ContextItem` (from the core card), plus `estimate()`: `ceil(text.len() / 3)` tokens.
- `budget(items, limit) -> Vec<Budgeted>`: fills in the record's order (selection, files in the
  order attached, diagnostics, the active file, the open-editor list); an item that doesn't fit
  is cut at a row boundary with a last line `[truncated: N more rows]` if at least 10 rows fit,
  else dropped. Each `Budgeted` says `Whole`, `Truncated { rows_kept, rows_total }` or
  `Dropped`, so the chip can say so. The rendered tags (`render_user_content`) carry the rows
  kept.

**In the workbench**, new `crates/fe-workbench/src/chat_context.rs`, declared in
`chat_panel.rs` with `#[path = "chat_context.rs"] mod chat_context;` (so `lib.rs` stays
untouched):

- `gather(workbench, grants, cx) -> Vec<ContextItem>`, nothing at all unless
  `grants.allows(&Capability::BufferRead)`:
  - **Selection**: the active editor's primary selection, if not empty: path relative to the
    root, rows (1-based in the tag), text.
  - In a **trusted** root only (`Workbench::root_trusted`): the **active file**'s text (its
    buffer, unsaved edits included), its **diagnostics** (`EditorModel::diagnostics`:
    severity, row, message, source; omitted when there are none), and the **open editors**'
    relative paths (`Workbench::open_editors`), one per line.
- **@file**: `@` typed in the composer at the start of a word (`ChatViewEvent::AttachFileRequested`
  from fe-chat-view, which removes the `@`) or the composer's Attach button opens quick open in
  pick mode (`QuickOpen::set_pick`, shown with `Workbench::show_palette`, as `git_diff.rs`'s
  compare-with does; use public accessors rather than widening anything in `workbench.rs`).
  The picked file attaches as a `File` item: its open buffer's text, else the file read from
  disk off the main thread. Refused with a toast naming the reason, never attached: outside the
  root, under `.git/`, named `.env*`, `*.pem`, `*.key` or `id_*`, over 1 MiB, or binary (a NUL
  in the first 8 KiB).
- The panel refreshes the automatic chips when the active editor or its selection changes and
  when the panel gains focus; the user's removals stick until the message is sent; @files stay
  until sent or removed. On `chat.send` the panel passes the budgeted items to the view.

**In `crates/fe-chat-view/src/view.rs`**: chips show the item's label, its estimate ("~1.2k")
and "truncated" or "won't fit" from `Budgeted`; the `@` handling and the Attach button above.

Tests: `crates/fe-chat/tests/context.rs` (top-level fns) for the budget; the gathering in
`crates/fe-workbench/src/chat_context_tests.rs`, declared from `chat_panel_tests.rs` as
`#[path = "chat_context_tests.rs"] mod context;`, on GPUI's test platform with temp folders
(trusted and untrusted) and `fe_chat::ScriptedProvider`, asserting on the request it records.
## Round 4 design (Mac y34: reject + respec, no override)

Review rounds 1-3 kept finding new holes in one design: approvals cached per lexical path and retained across validation. The criteria stay as they are (C3, C4 and C5 are the trust and chip guarantees; none is narrowed). Rework the trust path to fail closed:

- **No retained approvals.** An approval is bound to the root generation and the buffer/file identity it was made for, and is dropped the moment either changes (replace, reload, root change, trust revocation). A validation completing for an older root generation or identity is discarded, never cached.
- **Withhold, don't retain, while validating.** While a revalidation is pending (including a changed version of the same file), no automatic context from that source is sent: the previous text is not kept "until validation finishes". Trust and capability restrictions apply to retained items synchronously, before awaiting validation (review F12).
- **Resolve every path through its existing ancestors.** A missing component followed by `..` or a further link is resolved again from the last existing ancestor; a path whose allowed resolved location cannot be established is rejected (review F8), for automatic context and for disk and open-buffer attachments.
- **Dismissals persist until Send.** Removing an explicit file chip records a dismissal that applies to items gathered later, including while the automatic copy is absent during validation (review F9).
- Add the controlled tests the findings ask for: replacement, reload and root change during validation; trust revoked while same-file validation is pending, then Send before completion; `alias.txt -> gone/../public.txt` with `public.txt` linking outside the root; removal during validation followed by gather and Send.

## Implementation notes

- `ContextItem` and `ContextKind` moved to `fe-chat/src/context.rs`. `ContextKind::ActiveFile`
  carries `path` and `rows: Option<Rows>` (set only once the budget cuts it), so the active file
  tags as `<file>` with the rows kept, as a cut @file does. `Budgeted` holds the item as sent, its
  index among the items budgeted, the whole item's estimate and a `Fit` (`Whole`,
  `Truncated { rows_kept, rows_total }`, `Dropped`). An item that doesn't fit takes what's left
  of the budget; the items after it are still tried, so one that estimates to nothing still goes.
- The view, not the panel, budgets: the panel gives it the gathered items (`set_context`) and
  `chat.context_budget` (`set_context_budget`; `ChatOptions` is unchanged, as other crates build
  it literally), and `chat.send` sends `chips()` as budgeted. The user's @files stay in
  `attachments()`; removing one of the host's chips hides its kind until the next send.
- Gathering runs at most once per update, deferred, when the workbench or the active editor
  notifies, and only while the dock shows the Chat panel; also when the composer gains focus.
- The deny list also keeps a secret's or an outside file's selection, text and diagnostics out of
  the automatic chips, by its path and its real path (`chat_context::resolve`: every component
  checked from the last that exists, every link followed, a dangling one's too; an unreadable
  link, a link loop or a `..` after a missing folder refuses, as where it leads can't be known).
  Round 4: an approval is the active file as it was vetted (root, root generation, editor, path,
  buffer version, disk stamp); any change drops it, and a vetting that lands for another is
  discarded. While the active file is being vetted, including a new version of the same file,
  nothing from it is in the view's context: it's withheld, not kept. A root or trust change
  (`RootChanged`, `TrustChanged`) bumps the generation and cuts the view's context to what the
  root's trust allows at once, whether the panel shows or not; so does a change of grants. The active file over 1 MiB or binary is left out, as an @file would be. A
  selection over 1 MiB is left out too. `chat.exclude` (record § 12) isn't applied yet.
- An @file's real path must pass the deny list as well as the path picked. Its text is an open
  buffer's if one has the same real path (a tab's or a diff's working side's, any window's
  `file_editors`, held while it's matched so a closing editor doesn't lose it), and the disk is
  read only otherwise; size and binary checks apply to the text chosen. All the file system work
  is off the main thread. Its chip holds its place while it's read
  (`ChatView::reserve`/`fill`), Send waits for it, and a read ending after its chat was replaced
  lands nowhere.
- `@` is recognised from the composer buffer's own edit events: a typed (not undone or redone)
  batch inserting exactly `@` at a word's start, over a selection or not, and it's taken out with
  `Buffer::edit_in` in the transaction that typed it, so undo neither restores it nor asks again.
- A turn's Edit doesn't bring back items the host's current context covers (the active file's
  file, a selection of the same path and rows, the diagnostics, the open editors), and a user's
  @file of the active file stands in for the automatic one, so neither goes twice; taking the
  user's chip off records it, and keeps the host's copies it stands for off until the send,
  gathered then or later (say while the active file is being vetted).
- Round 4's documented limits (F10, F11 and the restored selection's newline) are handled
  anyway: an @file's open-buffer candidates are held strongly while matched, every window's are
  matched off the main thread, and a selection is reconciled by its path and rows.
- Carried T243 findings: the provider's request and stream now run on the background executor,
  events forwarded to the view over a channel, and dropping the view's task drops the stream
  (F1); the keychain wrapper keeps only a store that opened, so a failed open is retried (F2).
- Round 4, review 2: an approval also binds the text to its file. A buffer counts only if its
  load or save stamp names the vetted file (device and inode; no stamp only where there's no
  file), so a link retargeted, or a file replaced, under a buffer keeping older text refuses
  (`chat_context::text_from`); the approval records the buffer's id too, and an @file's matched
  buffer is checked again for its stamp as it lands. A disk @file is read through the handle
  opened after vetting, and only if the path, vetted again, still leads to that same file.
- Explicit attachments: a root change takes off everything the user attached, read or being
  read, at once, and so does losing `buffer.read`; a read landing after either (or under another
  root, or without the grant) is dropped. A reservation knows its file, so taking a chip off
  while it's read keeps the host's copy off too.
- Edit and Remove Attachments bring back only what the user attached: the view records, per turn
  it sent, which items were the user's, and none for a turn it didn't send. What the host
  gathered is gathered again, if it still may be.
- The chat panel is made with the workbench now (Mac T1022), before the workbench exists: it
  follows its window's workbench from its first draw.
- Round 4, review 3: paths are no longer checked by name after vetting. `chat_context::walk`
  opens the path from `/` a component at a time (`fstatat` without following, `openat` with
  `O_NOFOLLOW`, the opened handle checked to be what was looked up, links read and walked in
  place), so the deny list is checked on the path actually walked and the file's identity comes
  from the handle it opened (or, for a file it may not read, from the lookup in the folder it
  holds). A buffer's stamp is compared with that, and a disk @file is read from that handle: a
  swap after the walk changes neither. An @file's matched buffer must still be the same buffer
  (by id), at the same path, with the same stamp, when it lands.
- Losing authorization (a root change, or `buffer.read`) also forgets which items of earlier
  turns were the user's, so Edit and Remove Attachments don't bring them back.
