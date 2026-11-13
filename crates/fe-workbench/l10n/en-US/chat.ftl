# fe-workbench's chat and AI strings (i18n design, W7): `workbench-chat-…`.

## The Chat panel (chat_panel.rs)

workbench-chat-history = History
workbench-chat-new = New Chat
workbench-chat-save = Save
workbench-chat-cancel = Cancel
workbench-chat-delete = Delete
workbench-chat-no-chats = No chats yet.
workbench-chat-untitled = Untitled chat
# A history row: $title is the chat's, $when how long ago.
workbench-chat-history-label = { $title }  ·  { $when }
workbench-chat-ago-now = just now
workbench-chat-ago-minutes = { $count }m ago
workbench-chat-ago-hours = { $count }h ago
workbench-chat-ago-days = { $count }d ago
# The saved chat, as a new one of its own, after another writer changed it.
workbench-chat-copy-title = { $title } (copy)
workbench-chat-changed-elsewhere = The chat was changed elsewhere; your turns were saved as a copy.
workbench-chat-cant-save = Couldn't save the chat: { $error }
workbench-chat-cant-open = Couldn't open the chat: { $error }
workbench-chat-cant-delete = Couldn't delete the chat: { $failure }
workbench-chat-untrusted = the folder isn't trusted any more
workbench-chat-delete-title = Delete this chat?
workbench-chat-delete-detail = “{ $title }” will be deleted for good.

## The model picker in the chat's header. A model's name, such as
## "Opus 5.5", is a product name and isn't translated.

# The picker button: $model is the chat's model, $effort its effort.
workbench-chat-model-effort = { $model } · { $effort }
# The picker button's name for a screen reader: $current is its label.
workbench-chat-picker-name = Model and effort: { $current }
workbench-chat-picker-model = Model
workbench-chat-picker-effort = Effort
# A picker row that is the chat's now: $name is its label.
workbench-chat-picker-current = { $name }, current
workbench-chat-effort-low = Low
workbench-chat-effort-medium = Medium
workbench-chat-effort-high = High
workbench-chat-effort-xhigh = Extra high
workbench-chat-effort-max = Max

## The API key form

workbench-chat-key-placeholder = Anthropic API key
workbench-chat-key-note = The key is kept in the system keychain and only sent to api.anthropic.com.
# $variable is the environment variable's name.
workbench-chat-key-env-wins = { $variable } is set, and is used instead of the stored key.
workbench-chat-key-from-env = Key from { $variable }
workbench-chat-cant-save-key = Couldn't save the API key: { $error }
workbench-chat-cant-clear-key = Couldn't clear the API key: { $error }

## Why a worker's save or delete didn't happen (chat_store_worker.rs)

workbench-chat-why-trust-changed = the folder's trust changed
workbench-chat-why-quitting = the app is quitting
workbench-chat-why-changed-elsewhere = the chat was changed elsewhere

## Applying a reply's code block (chat_apply.rs, chat_changeset.rs). A
## reason ends "Can't apply the block to { $file }: …".

workbench-chat-propose-edit-title = Propose a Chat Edit
workbench-chat-propose-edit-description = Proposes a chat reply's code block as a change in its file.
workbench-chat-review-title = Review a Chat Reply's Changes
workbench-chat-review-description = Loads every file edit a chat reply proposes into one AI changeset.
workbench-chat-cant-apply = Can't apply the block to { $file }: { $why }
workbench-chat-no-change = The block changes nothing in { $file }
workbench-chat-no-file = This block doesn't say which file it's for
workbench-chat-the-selection = the selection
# A SEARCH/REPLACE section: $number is its place, $first its first line.
workbench-chat-search-which = SEARCH { $number } ("{ $first }")
workbench-chat-search-empty = { $which } is empty
workbench-chat-search-absent = { $which } isn't in it
workbench-chat-search-repeated = { $which } is in it more than once
workbench-chat-sections-overlap = two of its sections change the same text
workbench-chat-why-outside = it's outside the folder
workbench-chat-why-folder-unreadable = the folder can't be read
workbench-chat-why-unresolvable = its path can't be resolved
workbench-chat-why-read-only = it's read-only
workbench-chat-why-binary = it's binary
workbench-chat-why-selection-unknown = the selection it was sent with was cut to fit, or isn't known
workbench-chat-why-selection-lost = the selection it was sent with is lost
workbench-chat-why-no-folder = no folder is open
workbench-chat-why-file-closed = its file is closed
workbench-chat-why-not-opened = it couldn't be opened in an editor
workbench-chat-why-another-block = the reply has another block for it
workbench-chat-why-no-parent = its folder isn't there
workbench-chat-why-nothing-to-search = it isn't there to search

## Files attached to a message (chat_context.rs)

workbench-chat-attach-placeholder = Attach a file…
workbench-chat-context-problems = Problems ({ $count })
workbench-chat-context-open-editors = Open editors ({ $count })
workbench-chat-cant-attach = Can't attach { $path }: { $reason }
workbench-chat-why-in-git = it's in .git
workbench-chat-why-secrets = it may hold secrets
workbench-chat-why-too-big = it's over 1 MiB
workbench-chat-why-no-folder-there = the folder isn't there
workbench-chat-why-folder = it's a folder
workbench-chat-why-changed = it changed while it was read
workbench-chat-why-link-missing = a link leads through a folder that doesn't exist
workbench-chat-why-links = too many links
workbench-chat-why-not-regular = it isn't a regular file
workbench-chat-why-file-for-folder = a file where a folder should be
workbench-chat-why-unsupported = can't check it on this system
workbench-chat-why-unreadable = it can't be read
workbench-chat-why-not-its-text = its text isn't from this file
workbench-chat-why-no-file = no such file

## The AI Changes panel (ai_changes_panel.rs)

workbench-chat-changes-none = No pending changes
workbench-chat-changes-apply = Apply
workbench-chat-changes-cancel = Cancel
workbench-chat-changes-accept = Accept
workbench-chat-changes-reject = Reject
workbench-chat-changes-accept-all = Accept All
workbench-chat-changes-reject-all = Reject All
workbench-chat-changes-revert-turn = Revert Turn
workbench-chat-changes-revert = Revert
workbench-chat-changes-revert-title = Revert the turn's changes?
# $files is the files' names, a line each.
workbench-chat-changes-revert-detail =
    These files were edited since the turn accepted its changes:
    { $files }

    Restoring them loses those edits.
# The header over a language server's previewed edit: $title is its title.
workbench-chat-changes-preview-header =
    { $title }: { $files ->
        [one] 1 file
       *[other] { $files } files
    }, { $edits ->
        [one] 1 edit
       *[other] { $edits } edits
    }
workbench-chat-changes-header =
    { $files ->
        [one] 1 file
       *[other] { $files } files
    }, { $hunks ->
        [one] 1 hunk
       *[other] { $hunks } hunks
    } pending
# A file row's counts, such as 2 pending.
workbench-chat-changes-count-pending = { $count } pending
workbench-chat-changes-count-accepted = { $count } accepted
workbench-chat-changes-count-rejected = { $count } rejected
workbench-chat-changes-count-stale = { $count } stale

## The inline edit input (inline_edit.rs)

workbench-chat-inline-placeholder = Edit selected code, or describe code to insert
workbench-chat-inline-no-key = Set an API key in the chat
workbench-chat-inline-read-only = This editor is read-only
workbench-chat-inline-generating = Generating…  Cancel (Esc)
workbench-chat-inline-deleted-nothing = The selection was deleted: nothing edited
workbench-chat-inline-deleted-discard = The selection was deleted: the edit is discarded
workbench-chat-inline-changed = The selection changed: the edit is discarded
workbench-chat-inline-no-code = The reply had no code to apply
workbench-chat-inline-no-change = The reply changes nothing

## Asking the agent from elsewhere (agent_actions.rs)

workbench-chat-ask-no-key = The draft is in the chat: set an API key to send it.
workbench-chat-ask-untrusted = The draft is in the chat: this folder isn't trusted, so it isn't sent.
