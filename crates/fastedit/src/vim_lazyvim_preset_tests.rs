//! The bundled `lazyvim` preset's content: LazyVim's core leader layout.

use std::sync::Arc;

use fe_vim::{HeadlessVim, RemapMode, RemapSet, VimKey, parse_vimrc};

use super::vim_preset_tests::unknown_commands;
use super::*;

/// Every (keys, command) the preset maps, in normal mode, with `<Bar>` for `|`.
const TABLE: &[(&str, &str)] = &[
    ("<leader>ff", "quick_open.toggle"),
    ("<leader><space>", "quick_open.toggle"),
    ("<leader>fr", "workbench.open_recent"),
    ("<leader>fn", "workbench.new_file"),
    ("<leader>fp", "workbench.open_folder"),
    ("<leader>ft", "terminal.toggle"),
    ("<leader>fT", "terminal.toggle"),
    ("<leader>/", "search.show"),
    ("<leader>sg", "search.show"),
    ("<leader>sr", "search.show_replace"),
    ("<leader>ss", "outline.show"),
    ("<leader>sS", "nav.workspace_symbols"),
    ("<leader>:", "command_palette.toggle"),
    ("<leader>sc", "command_palette.toggle"),
    ("<leader>e", "dock.show_project"),
    ("<leader>E", "dock.show_project"),
    ("<leader>bd", "workbench.close_tab"),
    ("<leader>bb", "workbench.next_tab"),
    ("<leader>,", "workbench.next_tab"),
    ("<leader>cr", "lsp.rename"),
    ("<leader>ca", "lsp.code_actions"),
    ("<leader>cf", "lsp.format_document"),
    ("<leader>cs", "outline.show"),
    ("<leader>co", "lsp.organize_imports"),
    ("<leader>cd", "diagnostics.next"),
    ("<leader>gg", "git.show_changes"),
    ("<leader>gs", "git.show_changes"),
    ("<leader>gb", "git.toggle_blame_annotations"),
    ("<leader>ghs", "git.stage_hunk"),
    ("<leader>ghr", "git.revert_hunk"),
    ("<leader>xx", "problems.show"),
    ("<leader>-", "pane.split_down"),
    ("<leader><Bar>", "pane.split_right"),
    ("<leader>wd", "pane.close"),
    ("<leader>ww", "pane.focus_next"),
    ("<leader>w=", "pane.equalize"),
    ("<C-h>", "pane.focus_left"),
    ("<C-j>", "pane.focus_down"),
    ("<C-k>", "pane.focus_up"),
    ("<C-l>", "pane.focus_right"),
    ("<leader>qq", "app.quit"),
    ("<leader>uw", "editor.toggle_soft_wrap"),
];

/// The visual-mode maps.
const VISUAL: &[(&str, &str)] = &[("<leader>cf", "lsp.format_selection")];

fn preset() -> Preset {
    bundled(VimPreset::Lazyvim).unwrap()
}

/// The (keys, command) pairs of the preset's maps in `mode`.
fn pairs(mode: RemapMode) -> Vec<(String, String)> {
    (parse_with_leader(preset().text, "é").remaps.into_iter())
        .filter(|remap| remap.modes == [mode])
        .map(|remap| {
            let command = remap
                .to
                .strip_prefix("<Cmd>fe:")
                .and_then(|to| to.strip_suffix("<CR>"));
            (
                remap.from.replace("é", "<leader>"),
                command.expect("a <Cmd>fe: map").to_owned(),
            )
        })
        .collect()
}

/// The keys the file's closing comment block lists as unmapped.
fn unmapped() -> Vec<String> {
    let text = preset().text;
    let (_, block) = text.split_once("\" Unmapped").expect("an unmapped block");
    (block.lines().skip(1))
        .filter_map(|line| line.strip_prefix("\"   "))
        .filter_map(|line| line.split_whitespace().next())
        .map(str::to_owned)
        .collect()
}

#[test]
fn lazyvim_preset_parses_cleanly() {
    let parsed = parse_vimrc(preset().text);
    assert_eq!(parsed.issues, []);
    assert!(!parsed.remaps.is_empty());
    assert_eq!(unknown_commands(preset().text), Vec::<String>::new());
}

#[test]
fn lazyvim_layout_is_the_table() {
    let expect = |table: &[(&str, &str)]| {
        (table.iter())
            .map(|(keys, command)| (keys.to_string(), command.to_string()))
            .collect::<Vec<_>>()
    };
    assert_eq!(pairs(RemapMode::Normal), expect(TABLE));
    assert_eq!(pairs(RemapMode::Visual), expect(VISUAL));
}

/// The keys of `text`'s maps as typed, with `<leader>` as `é`.
fn mapped_keys(text: &str) -> Vec<Vec<VimKey>> {
    (parse_with_leader(text, "é").remaps.into_iter())
        .map(|remap| parse_keys(&remap.from).unwrap())
        .collect()
}

/// The listed keys in `listed` that `text` maps, as typed with the same
/// leader.
fn listed_but_mapped(listed: &[String], text: &str) -> Vec<String> {
    let mapped = mapped_keys(text);
    (listed.iter())
        .filter(|key| mapped.contains(&parse_keys(&key.replace("<leader>", "é")).unwrap()))
        .cloned()
        .collect()
}

#[test]
fn unmapped_keys_are_listed_and_absent() {
    let listed = unmapped();
    assert_eq!(listed.len(), 21, "{listed:?}");
    assert_eq!(
        listed_but_mapped(&listed, preset().text),
        Vec::<String>::new()
    );

    // A listed key the file maps is caught.
    let both = format!(
        "{}nnoremap <leader>sk <Cmd>fe:search.show<CR>\n",
        preset().text
    );
    assert_eq!(listed_but_mapped(&listed, &both), ["<leader>sk"]);
}

fn vim() -> HeadlessVim {
    let mut vim = HeadlessVim::new("one\ntwo", (0, 0));
    let (set, issues) = RemapSet::new(
        &parse_with_leader(preset().text, "<Space>").remaps,
        "<Space>",
    );
    assert_eq!(issues, []);
    vim.state_mut().set_remaps(Arc::new(set));
    vim
}

/// The id of the next command a map ran, as the host would dispatch it.
fn taken(vim: &mut HeadlessVim) -> Option<String> {
    let command = vim.state_mut().take_remap_command()?;
    Some(command.0.command.to_string())
}

#[test]
fn space_ff_runs_quick_open() {
    let mut vim = vim();
    vim.feed("<Space>ff").unwrap();
    assert_eq!(taken(&mut vim).as_deref(), Some("quick_open.toggle"));
    assert_eq!(taken(&mut vim), None);

    // `<Space>w` is the window group: it waits for the next key.
    vim.feed("<Space>w").unwrap();
    assert_eq!(taken(&mut vim), None);
    vim.feed("d").unwrap();
    assert_eq!(taken(&mut vim).as_deref(), Some("pane.close"));
    assert_eq!(vim.text(), "one\ntwo");
}
