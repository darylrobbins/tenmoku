# The editor view's strings (crates/fe-gpui).

## The status bar's items (pane_item.rs, file_viewer)

gpui-status-cursor = Ln { $line }, Col { $column }
# The cursor with several carets: `$count` of them, always more than one.
gpui-status-cursor-selections =
    { $count ->
        [one] Ln { $line }, Col { $column } ({ $count } selection)
       *[other] Ln { $line }, Col { $column } ({ $count } selections)
    }
# The cursor with one caret and `$count` characters selected.
gpui-status-cursor-selected =
    { $count ->
        [one] Ln { $line }, Col { $column } ({ $count } selected)
       *[other] Ln { $line }, Col { $column } ({ $count } selected)
    }
gpui-status-cursor-tooltip = Go to Line/Column
gpui-status-tab-size = Tab Size: { $size }
gpui-status-spaces = Spaces: { $size }
gpui-status-line-ending-tooltip = Select End of Line Sequence
gpui-status-language-tooltip = Change Language Mode
# The language item with no language.
gpui-status-plain-text = Plain Text
gpui-status-read-only = Read-only

## A screen reader's names

# The editor's group, around its text area (named for the file) and popovers.
gpui-a11y-editor = Editor
# Vim's command line, by what it's for: `:`, `/` and `?`.
gpui-a11y-command-line-ex = Command
gpui-a11y-command-line-search-forward = Search forward
gpui-a11y-command-line-search-backward = Search backward

## What the editor tells a screen reader (its announcements)

# The vim mode it changed to.
gpui-a11y-vim-mode-normal = Normal
gpui-a11y-vim-mode-insert = Insert
gpui-a11y-vim-mode-replace = Replace
gpui-a11y-vim-mode-visual = Visual
gpui-a11y-vim-mode-visual-line = Visual Line
gpui-a11y-vim-mode-visual-block = Visual Block
# Lines a fold command hid.
gpui-a11y-folded = { $count ->
        [one] Folded 1 line
       *[other] Folded { $count } lines
    }
gpui-a11y-unfolded = Unfolded
# More than one cursor, then one again.
gpui-a11y-cursors = { $count } cursors
gpui-a11y-one-cursor = One cursor
# A selection bigger than the text a screen reader is given: how many lines
# it really spans.
gpui-a11y-lines-selected = { $count ->
        [one] 1 line selected
       *[other] { $count } lines selected
    }
# An inline suggestion (ghost text) appeared.
gpui-a11y-suggestion-available = Suggestion available
# The completion menu opened: how many items, then the selected one's label.
gpui-a11y-suggestions = { $count ->
        [one] 1 suggestion, { $label }
       *[other] { $count } suggestions, { $label }
    }
# Editor: Announce Position. The caret's line and column, and the lines in the file.
gpui-a11y-position = line { $line }, column { $column } of { $total }
# A diagnostic the caret rests in, by severity.
gpui-a11y-diagnostic-error = error: { $message }
gpui-a11y-diagnostic-warning = warning: { $message }
gpui-a11y-diagnostic-information = info: { $message }
gpui-a11y-diagnostic-hint = hint: { $message }

## The vim mode item, as nvim's modes say it

gpui-vim-mode-normal = NORMAL
gpui-vim-mode-insert = INSERT
gpui-vim-mode-replace = REPLACE
gpui-vim-mode-visual = VISUAL
gpui-vim-mode-visual-line = VISUAL LINE
gpui-vim-mode-visual-block = VISUAL BLOCK
gpui-vim-mode-cmdline = CMDLINE
gpui-vim-mode-jump = JUMP
# A jump pending, with what's typed of its query.
gpui-vim-mode-jump-query = JUMP { $query }

## Vim's messages that fe-gpui says

# After `E21: `, which stays as nvim has it.
gpui-vim-e21 = Cannot make changes, 'modifiable' is off
gpui-vim-map-keys-not-run = a map's keys after { $what } aren't run
gpui-vim-map-keys-after-host-command = a map's keys after a file or window command aren't run
# `$command` is `fe:` and a command's id.
gpui-vim-command-failed = { $command }: { $error }
gpui-vim-command-not-here = { $command } doesn't apply here

## Tabs and windows

gpui-untitled = untitled
# A window's title: the tab's, then the app's name.
gpui-window-title = { $title } — { $app }
gpui-window-title-dirty = { $title } ● — { $app }

## Saving and closing

gpui-save-unsavable = this is a copy to review, not saved
gpui-save-not-saved-changed = not saved: { $name } changed on disk
gpui-save-replaced = { $name } was replaced while it was being saved
gpui-save-dropped = the save was dropped
gpui-save-failed = “{ $name }” couldn’t be saved.
gpui-save-failed-ok = OK
gpui-save-changed-on-disk = “{ $name }” changed on disk since it was opened.
gpui-save-changed-on-disk-detail = Overwrite it with your version?
gpui-save-overwrite = Overwrite
gpui-save-cancel = Cancel
gpui-save-encoding-kept = the file changed on disk after it was written in { $encoding }; the buffer keeps its encoding
gpui-close-discard-question = Discard unsaved changes to { $name }?
gpui-close-discard-detail = Your changes will be lost if you close it without saving.
gpui-close-cancel = Cancel
gpui-close-discard = Discard

## The diff view

gpui-diff-wrapped-to-first = Wrapped to first change
gpui-diff-wrapped-to-last = Wrapped to last change
gpui-diff-no-changes = No changes
gpui-diff-too-large = Too large to compare
gpui-diff-change-of =
    { $count ->
        [one] Change { $index } of { $count }
       *[other] Change { $index } of { $count }
    }
gpui-diff-changes =
    { $count ->
        [one] { $count } changes
       *[other] { $count } changes
    }
gpui-diff-previous-change = Previous Change
gpui-diff-next-change = Next Change
# A diff tab: the file, then the two sides' titles.
gpui-diff-tab-title = { $name } ({ $left } ↔ { $right })
gpui-diff-hidden-lines =
    { $count ->
        [one] { $count } hidden line
       *[other] { $count } hidden lines
    }

## The file viewer

gpui-file-viewer-changed = { $name } changed on disk.
gpui-file-viewer-changed-reload = { $name } changed on disk. Reload it before editing.
gpui-file-viewer-cannot-read = Cannot read { $name }: { $error }
gpui-file-viewer-reload = Reload
gpui-file-viewer-edit = Edit This File
gpui-file-viewer-too-large = Read only: too large to edit

## The image viewer

gpui-image-blocked = Blocked: links a file outside the workspace
gpui-image-zoom-fit = Fit
gpui-image-zoom-percent = { $percent }%
gpui-image-too-large = Too large to preview
gpui-image-not-found = File not found
gpui-image-cannot-display = Can't display this image: { $reason }
gpui-image-open-as-text = Text
gpui-image-not-a-file = not a regular file
gpui-image-unsupported = not a supported image format
gpui-image-bad-gzip = malformed gzip data
gpui-image-svg-not-utf8 = the SVG isn't UTF-8
gpui-image-svg-unparsed = can't parse the SVG
gpui-image-dimensions = { $width } × { $height }
gpui-image-actual-size-tooltip = Actual Size
gpui-image-fit-tooltip = Fit to Window
gpui-size-gib = { $size } GiB
gpui-size-mib = { $size } MiB
gpui-size-kib = { $size } KiB
gpui-size-bytes =
    { $count ->
        [one] { $count } bytes
       *[other] { $count } bytes
    }

## Hover popovers

# A related location in the diagnostic's own file.
gpui-hover-related-line = line { $line }
gpui-hover-related = ↳ { $at }: { $message }
gpui-hover-code = { $code }: { $href }
# The hover card's footer: vim's keys (gd) to go to the definition, then what
# they do.
gpui-hover-go-to-definition = { $keys } definition

## Proposed changes

gpui-proposal-label = Proposed change
gpui-proposal-accept = Accept
gpui-proposal-reject = Reject

## The which-key popup

gpui-which-key-footer = esc close
# A command whose keys go on to longer maps.
gpui-which-key-command-group = { $title } +

## The substitute preview

# A replaced row's text, with the row it lands on.
gpui-substitute-preview-row = → { $row }

## Command titles: `command-<id>` with the id's `.` as `-`

command-editor-show_hover = Show Hover
command-editor-show_signature_help = Show Signature Help
command-editor-go_to_bracket = Go to Bracket
command-editor-announce_position = Editor: Announce Position
command-editor-trigger_completion = Trigger Completion
command-completion-select_prev = Select Previous Completion
command-completion-select_next = Select Next Completion
command-completion-accept = Accept Completion
command-completion-dismiss = Dismiss Completion
command-ghost_text-accept = Accept Suggestion
command-ghost_text-accept_word = Accept Next Word of Suggestion
command-ghost_text-accept_line = Accept Next Line of Suggestion
command-ghost_text-dismiss = Dismiss Suggestion
command-vim-toggle = Toggle Vim Mode
command-vim-key = Vim Key
command-image-zoom_in = Zoom In
command-image-zoom_out = Zoom Out
command-image-zoom_fit = Fit to Window
command-image-zoom_actual = Actual Size
command-diff-next_change = Go to Next Difference
command-diff-previous_change = Go to Previous Difference
command-diff-toggle_inline = Toggle Inline Diff
command-diff-toggle_folding = Toggle Unchanged Regions
command-proposal-next_hunk = Next Proposed Change
command-proposal-previous_hunk = Previous Proposed Change
command-proposal-accept_hunk = Accept Proposed Change
command-proposal-reject_hunk = Reject Proposed Change
command-proposal-accept_all = Accept All Proposed Changes
command-proposal-reject_all = Reject All Proposed Changes
