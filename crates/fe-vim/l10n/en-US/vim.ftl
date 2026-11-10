# fe-vim's messages. en-US is fe-vim's text byte for byte, which for a
# message nvim has too is nvim's English (the oracle tests compare them). An
# E-code such as `E492:` is prepended in code, outside the catalogue, as nvim
# keeps it in every language.

## Ex lines (`ExError`): the text after each E-code.

vim-e492 = Not an editor command: { $command }
vim-e477 = No ! allowed
vim-e481 = No range allowed
vim-e488 = Trailing characters: { $characters }
vim-e939 = Positive count required
vim-e134 = Cannot move a range of lines into itself
vim-e475-argument = Invalid argument: { $argument }
vim-e474 = Invalid argument
vim-e654 = Missing delimiter after search pattern: { $pattern }
vim-e16 = Invalid range
vim-e1247 = Line number out of range
vim-e493 = Backwards range given
vim-e20 = Mark not set
vim-e35 = No previous regular expression
vim-e33 = No previous substitute regular expression
vim-e486 = Pattern not found: { $pattern }
vim-e1510 = Value too large: { $value }
vim-e146 = Regular expressions can't be delimited by letters
vim-e10 = \ should be followed by /, ? or &
vim-e148 = Regular expression missing from global
vim-e147 = Cannot do :global recursive with a range
vim-e471 = Argument required
vim-e192 = Recursive use of :normal too deep
vim-e34 = No previous command
vim-e19 = Mark has invalid line number
vim-nul-in-replacement = \n in a replacement (a NUL): not supported
vim-shell-refused = Shell commands run only from your own keys
vim-shell-without-range = :!{"{"}cmd{"}"} without a range isn't run here: use the terminal panel

## What fe-vim doesn't do yet. `{ $what }` is a command, flag or option as
## typed; the vim-unsupported-* are fe-vim's own.

vim-not-supported-yet = { $what }: not supported yet
vim-unsupported-quickfix-in-global = a quickfix command in :g or :normal: not supported yet
vim-unsupported-buffer-in-global = a buffer command in :g or :normal: not supported yet
vim-unsupported-window-in-global = a window command in :g or :normal: not supported yet
vim-unsupported-bar-after-buffer-command = | after a buffer command: not supported yet
vim-unsupported-ls-flags = :ls flags: not supported yet
vim-unsupported-preview-time-limit = the preview's time limit
vim-unsupported-bd-several = :bd of several buffers: not supported yet
vim-unsupported-buffer-count = a count other than a number: not supported yet
vim-unsupported-buffer-mark = a mark as a buffer number: not supported yet
vim-unsupported-buffer-offset = an offset in a buffer range: not supported yet
vim-unsupported-bar-after-tab-command = | after a tab command: not supported yet
vim-unsupported-tab-number = a tab number: not supported yet
vim-unsupported-tab-offset = an offset in a tab number: not supported yet
vim-unsupported-tab-non-number = a tab number other than a number: not supported yet
vim-unsupported-cmd-and-opt = +cmd and ++opt: not supported yet
vim-unsupported-bar-after-window-command = | after a window command: not supported yet
vim-unsupported-window-number = a window number: not supported yet
vim-unsupported-bar-after-quickfix-command = | after a quickfix command: not supported yet
vim-unsupported-clist-range = :clist's range: not supported yet
vim-unsupported-vimgrep-f = :vimgrep's f flag: not supported yet
vim-unsupported-expression-replacement = \= in a replacement: not supported yet
vim-unsupported-replacement-past-preview = a replacement past the preview's size: not supported yet
vim-unsupported-confirm-under-global = the c flag (confirm) under :g: not supported yet

## Marks, the jumplist and the changelist.

vim-e662 = At start of changelist
vim-e663 = At end of changelist
vim-e664 = Changelist is empty
# `{ $name }` is the character, quoted as Rust's Debug quotes it: `'1'`.
vim-mark-not-settable = m doesn't set mark { $name }

## The command line and searches.

vim-e348 = No string under cursor
vim-ex-line-takes-no-operator = an ex command line takes no operator or register
vim-no-command-line = no command line is open
vim-search-offset-not-supported = search offsets not supported: { $offset }

## Listings (`:ls`, `:jumps`, `:changes`, `:marks`, `:registers`): each one's
## picker title, and its column heads as nvim prints them.

vim-ls-title = Buffers
vim-ls-header = nr  flags name  line
vim-ls-line = line { $line }
vim-jumps-title = Jumps
vim-jumps-header = jump line  col file/text
vim-changes-title = Changes
vim-changes-header = change line  col text
vim-marks-title = Marks
vim-marks-header = mark line  col file/text
vim-e283 = No marks matching "{ $marks }"
vim-registers-title = Registers
vim-registers-header = Type Name Content

## Other ex commands.

vim-e682 = Invalid search pattern or delimiter
vim-e353 = Nothing in register { $register }
vim-put-expression-register = the = register: not supported
# `{ $number }` is the change number as fe-vim read it (saturating at 2^64 - 1).
vim-undo-number-not-supported = :undo { $number }: not supported
vim-global-not-found = Pattern not found: { $pattern }
vim-global-every-line = Pattern found in every line: { $pattern }
vim-normal-fe-refused = fe: commands don't run inside :normal
vim-lines-filtered = { $count ->
    [one] { $count } line filtered
   *[other] { $count } lines filtered
}

## `:set`.

vim-set-needs-options = :set { $arguments }: not supported, name the options
vim-set-not-supported = { $argument }: not supported
vim-set-noexpandtab = noexpandtab: not supported unless shiftwidth is tabstop
vim-set-tabstop-under-noexpandtab = { $argument }: not supported under noexpandtab, set expandtab first
vim-set-shiftwidth-under-noexpandtab = { $argument }: not supported under noexpandtab unless it is tabstop
vim-e521 = Number required after =: { $argument }
vim-e487 = Argument must be positive: { $argument }

## Search patterns (`PatternError`). `{ $atom }` is the atom as typed, and
## `{ $message }` the `regex` crate's own (English) message.

vim-interrupted = Interrupted
vim-pattern-not-supported = pattern not supported: { $atom }
vim-pattern-invalid-regex = invalid regex: { $message }
vim-pattern-too-large-regex = pattern too large: { $message }
vim-pattern-empty = invalid pattern: empty pattern
vim-pattern-missing-bracket = invalid pattern: missing ] after \_[
vim-pattern-brace-syntax = invalid pattern: syntax error in \{"{"}...{"}"}
vim-pattern-reverse-range = invalid pattern: reverse range in a collection
vim-pattern-code-atom = invalid pattern: invalid character after \%[dxouU]
vim-pattern-unmatched-close = invalid pattern: unmatched \)
vim-pattern-unmatched-open = invalid pattern: unmatched \(
vim-pattern-unmatched-group = invalid pattern: unmatched \%(
vim-pattern-multi-after-multi = invalid pattern: a multi can't follow a multi
vim-pattern-misplaced-multi = invalid pattern: misplaced multi
vim-pattern-too-many-groups = invalid pattern: too many \(
vim-pattern-compiled-over = pattern too large: over { $limit ->
    [one] { $limit } bytes
   *[other] { $limit } bytes
} compiled
# `{ $digits }` is the count as typed.
vim-pattern-count = pattern too large: count { $digits }
vim-pattern-composing-marks = pattern not supported: more than { $limit ->
    [one] { $limit } different composing characters
   *[other] { $limit } different composing characters
} on one character
vim-pattern-alternatives = pattern too large: alternatives that end in different ways
vim-pattern-repeated-alternatives = pattern too large: a repeat of alternatives that end in different ways
vim-pattern-multi-after-varying-atom = pattern not supported: a multi after an atom that ends differently each time
vim-pattern-nested-too-deeply = pattern too large: groups nested too deeply

## A vimrc's issues (`VimrcIssue`).

vim-vimrc-select-mode = Select mode isn't in Tenmoku
vim-vimrc-language-remaps = language remaps aren't supported
vim-vimrc-terminal-remaps = terminal remaps aren't supported
vim-vimrc-visual-only = { $command } applies to Visual mode only: Select mode isn't in Tenmoku
vim-vimrc-line-continuation = line continuation isn't supported
vim-vimrc-fegroup-bang = not supported: fegroup!
vim-vimrc-not-supported = not supported: { $command }
vim-vimrc-bang-not-supported = not supported: { $command }!
vim-vimrc-listing-maps = listing maps isn't supported
vim-vimrc-fegroup-needs-keys = fegroup needs keys and a name
vim-vimrc-fegroup = fegroup: { $message }
vim-vimrc-unmap-needs-keys = unmap needs a left side
vim-vimrc-buffer-not-supported = <buffer> isn't supported
vim-vimrc-trailing = trailing characters: { $characters }
vim-vimrc-argument-not-supported = <{ $argument }> isn't supported
vim-vimrc-let-not-supported = not supported: let { $name }
vim-vimrc-mapleader-unquoted = mapleader must be a quoted string
vim-vimrc-mapleader-key-not-supported = mapleader: <{ $key }> isn't supported
vim-vimrc-mapleader-key-invalid = mapleader: <{ $key }> is invalid
vim-vimrc-mapleader-not-utf8 = mapleader isn't valid UTF-8

## `:s`: nvim's report, and its question for the `c` flag (whose answer
## keys y, n, a, q, l, ^E and ^Y are the same in every language).

vim-substitute-matches = { $lines ->
    [one] { $matches ->
        [one] { $matches } match on { $lines } line
       *[other] { $matches } matches on { $lines } line
    }
   *[other] { $matches ->
        [one] { $matches } match on { $lines } lines
       *[other] { $matches } matches on { $lines } lines
    }
}
vim-substitute-substitutions = { $lines ->
    [one] { $substitutions ->
        [one] { $substitutions } substitution on { $lines } line
       *[other] { $substitutions } substitutions on { $lines } line
    }
   *[other] { $substitutions ->
        [one] { $substitutions } substitution on { $lines } lines
       *[other] { $substitutions } substitutions on { $lines } lines
    }
}
vim-substitute-prompt = replace with { $replacement }? (y)es/(n)o/(a)ll/(q)uit/(l)ast/scroll up(^E)/down(^Y)
vim-substitute-not-asking = no :s is asking

## Macros, registers and keys. A quoted `{ $register }` is the name as
## Rust's Debug quotes a character: `'1'`.

vim-e748 = No previously used register
vim-e30 = No previous command line
vim-macro-register-cant-record = { $register } can't hold a recording
vim-macro-register-cant-play = { $register } can't be played
vim-macro-already-recording = already recording
vim-macro-not-recording = not recording
vim-macro-replay-capped = macro stopped: it would replay more than { $limit ->
    [one] { $limit } keys
   *[other] { $limit } keys
}
vim-not-a-register = { $register } isn't a register
vim-unknown-key = unknown key <{ $key }>
vim-jump-chars = a jump takes one or two chars, not { $chars }

## Inserting, putting and operating. `{ $limit }` is the most bytes one
## change may insert. Each count selects its own noun's plural; en-US keeps
## the plural ("1 copies") in every branch, as the text always has.

vim-e29 = No inserted text yet
vim-completion-not-found = Pattern not found
vim-register-read-only = register { $register } is read-only
vim-insert-too-large = inserting { $bytes ->
    [one] { $bytes } bytes
   *[other] { $bytes } bytes
} at { $carets ->
    [one] { $carets } carets
   *[other] { $carets } carets
} would pass { $limit ->
    [one] { $limit } bytes
   *[other] { $limit } bytes
}
vim-put-too-large = putting { $count ->
    [one] { $count } copies
   *[other] { $count } copies
} of { $bytes ->
    [one] { $bytes } bytes
   *[other] { $bytes } bytes
} would insert more than { $limit ->
    [one] { $limit } bytes
   *[other] { $limit } bytes
}
vim-put-block-too-large = putting { $count ->
    [one] { $count } copies
   *[other] { $count } copies
} of a { $width } column block would insert more than { $limit ->
    [one] { $limit } bytes
   *[other] { $limit } bytes
}
vim-put-indented-too-large = putting { $count ->
    [one] { $count } copies
   *[other] { $count } copies
} of { $bytes ->
    [one] { $bytes } bytes
   *[other] { $bytes } bytes
}, indented, would insert more than { $limit ->
    [one] { $limit } bytes
   *[other] { $limit } bytes
}
vim-blank-rows-too-large = putting { $count ->
    [one] { $count } blank rows
   *[other] { $count } blank rows
} would insert more than { $limit ->
    [one] { $limit } bytes
   *[other] { $limit } bytes
}
vim-replace-register-too-large = repeating { $bytes ->
    [one] { $bytes } bytes
   *[other] { $bytes } bytes
} on { $rows ->
    [one] { $rows } rows
   *[other] { $rows } rows
} would insert more than { $limit ->
    [one] { $limit } bytes
   *[other] { $limit } bytes
}
vim-block-too-large = a block of { $rows ->
    [one] { $rows } rows
   *[other] { $rows } rows
} would take more than { $limit ->
    [one] { $limit } bytes
   *[other] { $limit } bytes
}
vim-shift-too-large = shifting { $rows ->
    [one] { $rows } rows
   *[other] { $rows } rows
} { $count ->
    [one] { $count } times
   *[other] { $count } times
} would insert more than { $limit ->
    [one] { $limit } bytes
   *[other] { $limit } bytes
}
vim-surround-through-surround-add = the surround operator acts through vim.surround_add
# `{ $operator }` is the operator's Rust name, such as `Format`.
vim-operator-visual-only = { $operator } acts only on a visual selection
vim-increment-empty-extent = an increment's extent is empty
vim-jump-labels-stale = jump: the labels are stale
vim-coerce-unknown = cr{ $key } names no coercion
vim-coerce-text-object-only = the coerce operator acts only on a text object
vim-exchange-overlapping = Exchange aborted: overlapping text
# `{ $name }` is quoted as Rust's Debug quotes it.
vim-surround-no-pair = no pair is named { $name }
vim-surround-no-replacement = no replacement is named { $name }
vim-no-visual-selection = no visual selection
vim-no-visual-selection-to-comment = no visual selection to comment
vim-no-visual-selection-to-fold = no visual selection to fold
vim-no-visual-selection-to-format = no visual selection to format
vim-no-visual-selection-to-operate = no visual selection to operate on
vim-no-visual-selection-to-surround = no visual selection to surround

## Which-key: the title of each built-in key (`BUILTIN_KEYS`).

vim-key-g-hash = partial word backward
vim-key-g-dollar = end of screen line
vim-key-g-percent = match backward
vim-key-g-amp = repeat :s on all lines
vim-key-g-star = partial word forward
vim-key-g-plus = newer text state
vim-key-g-comma = newer change
vim-key-g-minus = older text state
vim-key-g-semicolon = older change
vim-key-g-question = rot13
vim-key-g-underscore = last non-blank
vim-key-g-tilde = toggle case
vim-key-g-c = comment
vim-key-g-e = end of previous word
vim-key-g-upper-e = end of previous WORD
vim-key-g-g = first line
vim-key-g-i = insert where insert stopped
vim-key-g-j = down a display line
vim-key-g-upper-j = join without spaces
vim-key-g-k = up a display line
vim-key-g-m = middle of screen line
vim-key-g-p = put after, cursor after it
vim-key-g-upper-p = put before, cursor after it
vim-key-g-q = format
vim-key-g-r = replace with register
vim-key-g-upper-r = virtual replace mode
vim-key-g-s = jump forward
vim-key-g-upper-s = jump backward
vim-key-g-u = lowercase
vim-key-g-upper-u = uppercase
vim-key-g-v = reselect
vim-key-g-w = format, keep cursor
vim-key-g-ctrl-a = increment progressively
vim-key-g-ctrl-x = decrement progressively
vim-key-z-enter = scroll top, first non-blank
vim-key-z-minus = scroll bottom, first non-blank
vim-key-z-dot = scroll center, first non-blank
vim-key-z-a = toggle fold
vim-key-z-upper-a = toggle folds recursively
vim-key-z-b = scroll bottom
vim-key-z-c = close fold
vim-key-z-upper-c = close folds recursively
vim-key-z-d = delete fold
vim-key-z-upper-d = delete folds recursively
vim-key-z-upper-e = delete all folds
vim-key-z-f = create fold
vim-key-z-upper-f = create fold of lines
vim-key-z-j = next fold start
vim-key-z-k = previous fold end
vim-key-z-upper-m = close all folds
vim-key-z-o = open fold
vim-key-z-upper-o = open folds recursively
vim-key-z-upper-r = open all folds
vim-key-z-t = scroll top
vim-key-z-v = open folds at cursor
vim-key-z-z = scroll center
vim-key-upper-z-upper-q = quit without writing
vim-key-upper-z-upper-z = write and quit
vim-key-prev-lparen = previous unmatched (
vim-key-prev-lbrace = previous unmatched { "{" }
vim-key-prev-lbracket = previous section start
vim-key-prev-rbracket = previous section end
vim-key-prev-m = previous function start
vim-key-prev-upper-m = previous function end
vim-key-prev-c = previous change
vim-key-prev-p = put before, indented
vim-key-prev-upper-p = put before, indented
vim-key-prev-space = blank line above
vim-key-prev-e = move line up
vim-key-next-rparen = next unmatched )
vim-key-next-rbrace = next unmatched { "}" }
vim-key-next-rbracket = next section start
vim-key-next-lbracket = next section end
vim-key-next-m = next function start
vim-key-next-upper-m = next function end
vim-key-next-c = next change
vim-key-next-p = put after, indented
vim-key-next-upper-p = put before, indented
vim-key-next-space = blank line below
vim-key-next-e = move line down

## The bundled LazyVim preset's `fegroup` names, by slug (`preset_group_name`).

vim-group-buffer = buffer
vim-group-code = code
vim-group-file-find = file/find
vim-group-git = git
vim-group-hunks = hunks
vim-group-quit-session = quit/session
vim-group-search = search
vim-group-test = test
vim-group-ui = ui
vim-group-windows = windows
vim-group-diagnostics-quickfix = diagnostics/quickfix

## Remap and group parse problems, and the map engine's stops.

vim-remap-icon-name = { $icon } isn't an icon's name: lowercase letters and digits joined by -
vim-remap-group-keys = a group needs keys
vim-remap-group-name = a group needs a name
vim-remap-empty-left = empty left side
vim-remap-cmd-no-cr = <Cmd> without a <CR> to end it
vim-remap-cmd-not-text = <Cmd> holds a key that isn't text before its <CR>
vim-remap-cmd-error = <Cmd>{ $line }<CR>: { $error }
vim-remap-nop-left = <Nop> can't be a left side
vim-remap-cmd-left = <Cmd> in a left side is not supported
vim-remap-unsupported = <{ $name }> is not supported
vim-remap-macro-cap = macro stopped: it would replay more than { $cap } keys
vim-e223 = recursive mapping
