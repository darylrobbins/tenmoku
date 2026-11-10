# fe-keymap's messages.

# The built-in menu bar's titles (menus/fastedit.toml names them by
# `title_key`), menu by menu: each menu's name, then its items' titles, in
# the order they first appear. `$app` is the app's name, which kiln gives
# every menu title (kiln-core's `menu::set_label_arg`).

## The app menu (macOS)

keymap-menu-app = { $app }
keymap-menu-hide-app = Hide { $app }
keymap-menu-hide-others = Hide Others
keymap-menu-show-all = Show All
keymap-menu-check-for-updates = Check for Updates…
keymap-menu-set-up-app = Set Up { $app }…
keymap-menu-settings = Settings…
keymap-menu-keyboard-shortcuts = Keyboard Shortcuts
keymap-menu-keyboard-shortcuts-json = Keyboard Shortcuts (JSON)
keymap-menu-install-shell-command-in-path = Install 'tenmoku' Command in PATH
keymap-menu-uninstall-shell-command-from-path = Uninstall 'tenmoku' Command from PATH
keymap-menu-quit-app = Quit { $app }

## File

keymap-menu-file = File
keymap-menu-new-file = New File
keymap-menu-open-folder = Open Folder…
keymap-menu-open-file = Open File…
keymap-menu-open-recent = Open Recent…
keymap-menu-trust-this-folder = Trust This Folder
keymap-menu-untrust-this-folder = Untrust This Folder
keymap-menu-save = Save
keymap-menu-save-as = Save As…
keymap-menu-save-all = Save All
keymap-menu-save-without-formatting = Save without Formatting
keymap-menu-compare-active-file-with = Compare Active File With…
keymap-menu-close-tab = Close Tab
keymap-menu-reopen-closed-editor = Reopen Closed Editor
keymap-menu-close-pane = Close Pane
keymap-menu-close-window = Close Window
keymap-menu-quit = Quit

## Edit

keymap-menu-edit = Edit
keymap-menu-undo = Undo
keymap-menu-redo = Redo
keymap-menu-cut = Cut
keymap-menu-copy = Copy
keymap-menu-paste = Paste
keymap-menu-select-all = Select All
keymap-menu-indent = Indent
keymap-menu-outdent = Outdent
keymap-menu-move-line-up = Move Line Up
keymap-menu-move-line-down = Move Line Down
keymap-menu-duplicate-line = Duplicate Line
keymap-menu-delete-line = Delete Line
keymap-menu-toggle-line-comment = Toggle Line Comment
keymap-menu-rename-symbol = Rename Symbol…
keymap-menu-code-actions = Code Actions…
keymap-menu-organize-imports = Organize Imports
keymap-menu-fix-all = Fix All
keymap-menu-refactor = Refactor…
keymap-menu-source-action = Source Action…
keymap-menu-format-document = Format Document
keymap-menu-format-selection = Format Selection
keymap-menu-spelling-suggest-corrections = Spelling: Suggest Corrections…
keymap-menu-spelling-next-misspelling = Spelling: Next Misspelling
keymap-menu-spelling-previous-misspelling = Spelling: Previous Misspelling
keymap-menu-spelling-add-word-to-dictionary = Spelling: Add Word to Dictionary
keymap-menu-spelling-ignore-word = Spelling: Ignore Word
keymap-menu-spelling-toggle-spell-check = Spelling: Toggle Spell Check
keymap-menu-find = Find
keymap-menu-replace = Replace
keymap-menu-find-in-files = Find in Files
keymap-menu-replace-in-files = Replace in Files

## View

keymap-menu-view = View
keymap-menu-project = Project
keymap-menu-search = Search
keymap-menu-outline = Outline
keymap-menu-problems = Problems
keymap-menu-toggle-dock = Toggle Dock
keymap-menu-toggle-terminal = Toggle Terminal
keymap-menu-chat = Chat
keymap-menu-toggle-breadcrumbs = Toggle Breadcrumbs
keymap-menu-status-bar = Status Bar
keymap-menu-soft-wrap = Soft Wrap
keymap-menu-toggle-vim-mode = Toggle Vim Mode
keymap-menu-fold = Fold
keymap-menu-unfold = Unfold
keymap-menu-fold-all = Fold All
keymap-menu-unfold-all = Unfold All
keymap-menu-restart-language-server = Restart Language Server
keymap-menu-language-server-log = Language Server Log
keymap-menu-run-task = Run Task…
keymap-menu-run-build-task = Run Build Task
keymap-menu-toggle-line-blame = Toggle Line Blame
keymap-menu-toggle-blame-annotations = Toggle Blame Annotations
keymap-menu-toggle-inline-diff = Toggle Inline Diff
keymap-menu-toggle-full-screen = Toggle Full Screen
keymap-menu-toggle-zen-mode = Toggle Zen Mode

## Go

keymap-menu-go = Go
keymap-menu-back = Back
keymap-menu-forward = Forward
keymap-menu-go-to-file = Go to File…
keymap-menu-go-to-line-column = Go to Line/Column…
keymap-menu-go-to-bracket = Go to Bracket
keymap-menu-go-to-symbol-in-file = Go to Symbol in File…
keymap-menu-go-to-symbol-in-workspace = Go to Symbol in Workspace…
keymap-menu-go-to-definition = Go to Definition
keymap-menu-go-to-declaration = Go to Declaration
keymap-menu-go-to-type-definition = Go to Type Definition
keymap-menu-go-to-implementations = Go to Implementations
keymap-menu-peek-references = Peek References
keymap-menu-show-call-hierarchy = Show Call Hierarchy
keymap-menu-show-type-hierarchy = Show Type Hierarchy
keymap-menu-find-all-references = Find All References
keymap-menu-next-tab = Next Tab
keymap-menu-previous-tab = Previous Tab
keymap-menu-switch-tab = Switch Tab
keymap-menu-switch-tab-backward = Switch Tab Backward
keymap-menu-focus-left-pane = Focus Left Pane
keymap-menu-focus-right-pane = Focus Right Pane
keymap-menu-focus-above-pane = Focus Above Pane
keymap-menu-focus-below-pane = Focus Below Pane
keymap-menu-reveal-active-file-in-project = Reveal Active File in Project
keymap-menu-next-problem = Next Problem
keymap-menu-previous-problem = Previous Problem
keymap-menu-next-change = Next Change
keymap-menu-previous-change = Previous Change

## Window

keymap-menu-window = Window
keymap-menu-minimize = Minimize
keymap-menu-zoom = Zoom
keymap-menu-split-right = Split Right
keymap-menu-split-down = Split Down
keymap-menu-move-tab-left = Move Tab Left
keymap-menu-move-tab-right = Move Tab Right
keymap-menu-move-tab-up = Move Tab Up
keymap-menu-move-tab-down = Move Tab Down
