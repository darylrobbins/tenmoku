# fe-buffer's messages.

## Edits and undo

buffer-edit-foreign-snapshot = the snapshot belongs to a different buffer, a newer version, or one below the edit log's floor
buffer-edit-conflict = edit { $index } conflicts with changes made since the snapshot
buffer-edit-no-such-transaction = the transaction was undone, or is no longer in the history
buffer-undo-conflict = transaction { $transaction } was changed by later edits

## Saving

buffer-save-replaced-on-read = the file had bytes that aren't valid { $encoding }, shown as U+FFFD; saving would replace them
buffer-save-unmappable = { $ch } (U+{ $code }) on line { $line }, column { $column } can't be saved in { $encoding }

## Markers

buffer-marker-compacted = the edit log was compacted past the marker set's version
buffer-marker-other-buffer = the snapshot is of another buffer
buffer-marker-older = the snapshot is older than the marker set

## Proposed edits

buffer-proposal-stale = the text under the hunk has changed since it was proposed
buffer-proposal-not-pending = the hunk isn't pending
buffer-proposal-no-change = the hunk changes nothing
buffer-conflict-changed = the text under this change was edited
buffer-conflict-was-stale = this change went stale earlier

## Streaming edits

buffer-stream-conflict = the target text changed before the first chunk landed
buffer-stream-undone = the stream's transaction was undone, or is no longer in the history

## Undo records and local history

buffer-undo-import-stale = the text isn't the one the undo history ends at
buffer-undo-import-edited = the buffer already has undo history
buffer-undo-import-invalid = the undo history doesn't apply to its text
buffer-local-history-none = no local history for { $path }
