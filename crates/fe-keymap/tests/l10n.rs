//! fe-keymap's catalogue: the literal lint (kiln-l10n), no hard-coded
//! user-facing text in this crate's non-test source outside
//! `l10n-allow.toml`, and every key it uses exists; then the built-in menu
//! bar, whose every title is a `title_key`, in the table and in the
//! per-platform copies the build script writes, and resolves in en-US to
//! the English each title was before it moved into the catalogue.

use std::{fmt::Write, path::Path};

use fe_keymap::{
    DEFAULT_LINUX_MENUS, DEFAULT_MACOS_MENUS, ItemKind, MENUS, MenuBar,
    menu::{DEFAULT_MENUS_PATH, set_label_arg},
};
use kiln_core::Os;

#[test]
fn ui_text_is_in_the_catalogue() {
    kiln_l10n::lint::literals(env!("CARGO_MANIFEST_DIR"))
        .allow_list("l10n-allow.toml")
        .check();
}

#[test]
fn keys_used_here_exist() {
    kiln_l10n::lint::keys(env!("CARGO_MANIFEST_DIR"), &fe_keymap::l10n::RESOURCES).check();
}

/// `source` parsed as on `os`, with no diagnostics.
fn parse(source: &str, os: Os) -> MenuBar {
    set_label_arg("app", "Tenmoku");
    let (bar, diagnostics) = MenuBar::parse_for(Path::new(DEFAULT_MENUS_PATH), source, None, os);
    assert_eq!(diagnostics, [], "on {os:?}");
    bar.unwrap()
}

/// The menus and items of `bar` whose title is literal text rather than a
/// catalogue key, as `menu` or `menu > item id`.
fn literal_titles(bar: &MenuBar) -> Vec<String> {
    let mut literal = Vec::new();
    for menu in &bar.menus {
        if menu.title_key.is_none() {
            literal.push(menu.name.clone());
        }
        for item in &menu.items {
            let (id, key) = match &item.kind {
                ItemKind::Command(command) => (&command.id, &command.title_key),
                ItemKind::System(system) => (&system.id, &system.title_key),
                ItemKind::Separator | ItemKind::Remove(_) => continue,
            };
            if key.is_none() {
                literal.push(format!("{} > {id}", menu.name));
            }
        }
    }
    literal
}

/// Each menu's title, then its items' `id = title`, one a line, as the
/// labels resolve; each label checked against the en-US title parsing
/// filled in.
fn titles(bar: &MenuBar) -> String {
    let mut titles = String::new();
    for menu in &bar.menus {
        assert_eq!(menu.label().resolve(), menu.name);
        writeln!(titles, "{}", menu.name).unwrap();
        for item in &menu.items {
            let (id, title, label) = match &item.kind {
                ItemKind::Command(command) => (&command.id, &command.title, command.label()),
                ItemKind::System(system) => (&system.id, &system.title, system.label()),
                ItemKind::Separator | ItemKind::Remove(_) => continue,
            };
            assert_eq!(label.resolve(), *title, "{id}");
            writeln!(titles, "    {id} = {title}").unwrap();
        }
    }
    titles
}

#[test]
fn every_bundled_title_resolves_to_todays_english() {
    let tables = [
        ("menus/fastedit.toml", MENUS, Os::MacOs, MACOS_TITLES),
        ("menus/fastedit.toml", MENUS, Os::Linux, LINUX_TITLES),
        ("menus/fastedit.toml", MENUS, Os::Windows, LINUX_TITLES),
        (
            "DEFAULT_MACOS_MENUS",
            DEFAULT_MACOS_MENUS,
            Os::MacOs,
            MACOS_TITLES,
        ),
        (
            "DEFAULT_LINUX_MENUS",
            DEFAULT_LINUX_MENUS,
            Os::Linux,
            LINUX_TITLES,
        ),
        (
            "DEFAULT_LINUX_MENUS",
            DEFAULT_LINUX_MENUS,
            Os::Windows,
            LINUX_TITLES,
        ),
    ];
    for (what, source, os, english) in tables {
        let bar = parse(source, os);
        assert_eq!(literal_titles(&bar), [""; 0], "{what} on {os:?}");
        assert_eq!(titles(&bar), english, "{what} on {os:?}");
    }

    // As the app's helpers parse them, which name the app themselves.
    let (bar, diagnostics) = fe_keymap::default_menus_macos(None);
    assert_eq!(diagnostics, []);
    assert_eq!(titles(&bar.unwrap()), MACOS_TITLES);
    let (bar, diagnostics) = fe_keymap::default_menus_linux(None);
    assert_eq!(diagnostics, []);
    assert_eq!(titles(&bar.unwrap()), LINUX_TITLES);
}

/// What [`every_bundled_title_resolves_to_todays_english`] rejects: a
/// menu or an item that names its title literally.
#[test]
fn a_literal_title_in_a_bundled_file_is_rejected() {
    let source = r#"
[[menus]]
title_key = "keymap-menu-file"

[[menus.items]]
title_key = "keymap-menu-save"
command = "editor.save"

[[menus.items]]
title = "Save All"
command = "workbench.save_all"

[[menus.items]]
title = "Services"
system = "services"

[[menus]]
name = "Edit"

[[menus.items]]
title_key = "keymap-menu-undo"
command = "editor.undo"
"#;
    assert_eq!(
        literal_titles(&parse(source, Os::MacOs)),
        ["File > workbench.save_all", "File > services", "Edit"]
    );
}

/// The menus as on macOS, as `menus/fastedit.toml` titled them literally
/// before its titles were keys, `Tenmoku` for the app.
const MACOS_TITLES: &str = "\
Tenmoku
    app.hide = Hide Tenmoku
    app.hide_others = Hide Others
    app.show_all = Show All
    app.check_for_updates = Check for Updates…
    app.set_up = Set Up Tenmoku…
    workbench.open_settings_ui = Settings…
    workbench.open_keyboard_shortcuts = Keyboard Shortcuts
    workbench.open_keyboard_shortcuts_json = Keyboard Shortcuts (JSON)
    app.install_shell_command = Install 'tenmoku' Command in PATH
    app.uninstall_shell_command = Uninstall 'tenmoku' Command from PATH
    services = Services
    app.quit = Quit Tenmoku
File
    workbench.new_file = New File
    workbench.open_folder = Open Folder…
    workbench.open_file = Open File…
    workbench.open_recent = Open Recent…
    workbench.trust_folder = Trust This Folder
    workbench.untrust_folder = Untrust This Folder
    editor.save = Save
    workbench.save_as = Save As…
    workbench.save_all = Save All
    workbench.save_without_formatting = Save without Formatting
    diff.compare_file = Compare Active File With…
    workbench.close_tab = Close Tab
    workbench.reopen_closed_editor = Reopen Closed Editor
    pane.close = Close Pane
    window.close = Close Window
Edit
    editor.undo = Undo
    editor.redo = Redo
    editor.cut = Cut
    editor.copy = Copy
    editor.paste = Paste
    editor.select_all = Select All
    editor.indent = Indent
    editor.outdent = Outdent
    editor.move_rows_up = Move Line Up
    editor.move_rows_down = Move Line Down
    editor.duplicate_rows = Duplicate Line
    editor.delete_rows = Delete Line
    editor.toggle_comment = Toggle Line Comment
    lsp.rename = Rename Symbol…
    lsp.code_actions = Code Actions…
    lsp.organize_imports = Organize Imports
    lsp.fix_all = Fix All
    lsp.refactor = Refactor…
    lsp.source_action = Source Action…
    lsp.format_document = Format Document
    lsp.format_selection = Format Selection
    spell.suggest = Spelling: Suggest Corrections…
    spell.next = Spelling: Next Misspelling
    spell.previous = Spelling: Previous Misspelling
    spell.add_word = Spelling: Add Word to Dictionary
    spell.ignore_word = Spelling: Ignore Word
    spell.toggle = Spelling: Toggle Spell Check
    find.show = Find
    find.show_replace = Replace
    search.show = Find in Files
    search.show_replace = Replace in Files
View
    dock.show_project = Project
    search.show = Search
    outline.show = Outline
    problems.show = Problems
    dock.toggle = Toggle Dock
    terminal.toggle = Toggle Terminal
    chat.open = Chat
    view.toggle_breadcrumbs = Toggle Breadcrumbs
    view.toggle_status_bar = Status Bar
    editor.toggle_soft_wrap = Soft Wrap
    vim.toggle = Toggle Vim Mode
    editor.fold = Fold
    editor.unfold = Unfold
    editor.fold_all = Fold All
    editor.unfold_all = Unfold All
    lsp.restart = Restart Language Server
    lsp.show_log = Language Server Log
    tasks.run = Run Task…
    tasks.run_build = Run Build Task
    git.toggle_line_blame = Toggle Line Blame
    git.toggle_blame_annotations = Toggle Blame Annotations
    diff.toggle_inline = Toggle Inline Diff
    window.toggle_fullscreen = Toggle Full Screen
    workbench.toggle_zen_mode = Toggle Zen Mode
Go
    nav.back = Back
    nav.forward = Forward
    quick_open.toggle = Go to File…
    go_to_line.toggle = Go to Line/Column…
    editor.go_to_bracket = Go to Bracket
    nav.document_symbols = Go to Symbol in File…
    nav.workspace_symbols = Go to Symbol in Workspace…
    nav.goto_definition = Go to Definition
    nav.goto_declaration = Go to Declaration
    nav.goto_type_definition = Go to Type Definition
    nav.goto_implementation = Go to Implementations
    editor.peek_references = Peek References
    nav.show_call_hierarchy = Show Call Hierarchy
    nav.show_type_hierarchy = Show Type Hierarchy
    nav.find_references = Find All References
    workbench.next_tab = Next Tab
    workbench.previous_tab = Previous Tab
    workbench.tab_switcher_next = Switch Tab
    workbench.tab_switcher_previous = Switch Tab Backward
    pane.focus_left = Focus Left Pane
    pane.focus_right = Focus Right Pane
    pane.focus_up = Focus Above Pane
    pane.focus_down = Focus Below Pane
    project_panel.reveal_active = Reveal Active File in Project
    diagnostics.next = Next Problem
    diagnostics.previous = Previous Problem
    git.next_change = Next Change
    git.previous_change = Previous Change
Window
    window.minimize = Minimize
    window.zoom = Zoom
    pane.split_right = Split Right
    pane.split_down = Split Down
    pane.move_tab_left = Move Tab Left
    pane.move_tab_right = Move Tab Right
    pane.move_tab_up = Move Tab Up
    pane.move_tab_down = Move Tab Down
";

/// The menus as on Linux and Windows, likewise.
const LINUX_TITLES: &str = "\
File
    workbench.new_file = New File
    workbench.open_folder = Open Folder…
    workbench.open_file = Open File…
    workbench.open_recent = Open Recent…
    workbench.trust_folder = Trust This Folder
    workbench.untrust_folder = Untrust This Folder
    editor.save = Save
    workbench.save_as = Save As…
    workbench.save_all = Save All
    workbench.save_without_formatting = Save without Formatting
    diff.compare_file = Compare Active File With…
    workbench.open_settings_ui = Settings…
    workbench.open_keyboard_shortcuts = Keyboard Shortcuts
    workbench.open_keyboard_shortcuts_json = Keyboard Shortcuts (JSON)
    app.install_shell_command = Install 'tenmoku' Command in PATH
    app.uninstall_shell_command = Uninstall 'tenmoku' Command from PATH
    workbench.close_tab = Close Tab
    workbench.reopen_closed_editor = Reopen Closed Editor
    pane.close = Close Pane
    window.close = Close Window
    app.quit = Quit
Edit
    editor.undo = Undo
    editor.redo = Redo
    editor.cut = Cut
    editor.copy = Copy
    editor.paste = Paste
    editor.select_all = Select All
    editor.indent = Indent
    editor.outdent = Outdent
    editor.move_rows_up = Move Line Up
    editor.move_rows_down = Move Line Down
    editor.duplicate_rows = Duplicate Line
    editor.delete_rows = Delete Line
    editor.toggle_comment = Toggle Line Comment
    lsp.rename = Rename Symbol…
    lsp.code_actions = Code Actions…
    lsp.organize_imports = Organize Imports
    lsp.fix_all = Fix All
    lsp.refactor = Refactor…
    lsp.source_action = Source Action…
    lsp.format_document = Format Document
    lsp.format_selection = Format Selection
    spell.suggest = Spelling: Suggest Corrections…
    spell.next = Spelling: Next Misspelling
    spell.previous = Spelling: Previous Misspelling
    spell.add_word = Spelling: Add Word to Dictionary
    spell.ignore_word = Spelling: Ignore Word
    spell.toggle = Spelling: Toggle Spell Check
    find.show = Find
    find.show_replace = Replace
    search.show = Find in Files
    search.show_replace = Replace in Files
View
    dock.show_project = Project
    search.show = Search
    outline.show = Outline
    problems.show = Problems
    dock.toggle = Toggle Dock
    terminal.toggle = Toggle Terminal
    chat.open = Chat
    view.toggle_breadcrumbs = Toggle Breadcrumbs
    view.toggle_status_bar = Status Bar
    editor.toggle_soft_wrap = Soft Wrap
    vim.toggle = Toggle Vim Mode
    editor.fold = Fold
    editor.unfold = Unfold
    editor.fold_all = Fold All
    editor.unfold_all = Unfold All
    lsp.restart = Restart Language Server
    lsp.show_log = Language Server Log
    tasks.run = Run Task…
    tasks.run_build = Run Build Task
    git.toggle_line_blame = Toggle Line Blame
    git.toggle_blame_annotations = Toggle Blame Annotations
    diff.toggle_inline = Toggle Inline Diff
    window.toggle_fullscreen = Toggle Full Screen
    workbench.toggle_zen_mode = Toggle Zen Mode
Go
    nav.back = Back
    nav.forward = Forward
    quick_open.toggle = Go to File…
    go_to_line.toggle = Go to Line/Column…
    editor.go_to_bracket = Go to Bracket
    nav.document_symbols = Go to Symbol in File…
    nav.workspace_symbols = Go to Symbol in Workspace…
    nav.goto_definition = Go to Definition
    nav.goto_declaration = Go to Declaration
    nav.goto_type_definition = Go to Type Definition
    nav.goto_implementation = Go to Implementations
    editor.peek_references = Peek References
    nav.show_call_hierarchy = Show Call Hierarchy
    nav.show_type_hierarchy = Show Type Hierarchy
    nav.find_references = Find All References
    workbench.next_tab = Next Tab
    workbench.previous_tab = Previous Tab
    workbench.tab_switcher_next = Switch Tab
    workbench.tab_switcher_previous = Switch Tab Backward
    pane.focus_left = Focus Left Pane
    pane.focus_right = Focus Right Pane
    pane.focus_up = Focus Above Pane
    pane.focus_down = Focus Below Pane
    project_panel.reveal_active = Reveal Active File in Project
    diagnostics.next = Next Problem
    diagnostics.previous = Previous Problem
    git.next_change = Next Change
    git.previous_change = Previous Change
Window
    pane.split_right = Split Right
    pane.split_down = Split Down
    pane.move_tab_left = Move Tab Left
    pane.move_tab_right = Move Tab Right
    pane.move_tab_up = Move Tab Up
    pane.move_tab_down = Move Tab Down
";
