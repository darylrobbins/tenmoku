//! The bundled `lazyvim` preset's harpoon keys: `<leader>H` marks the
//! file, `<leader>h` shows the list, `<leader>1` to `5` jump.

use std::sync::Arc;

use fe_gpui::fe_keymap::{CatalogError, CommandCatalog as _};
use fe_vim::{HeadlessVim, RemapMode, RemapSet, VimKey, parse_keys, parse_vimrc_with_leader};
use kiln::command::{CommandId, Value};

use crate::vim_remaps::PRESETS;

/// The lazyvim preset's text.
fn text() -> &'static str {
    let (_, preset) = (PRESETS.iter())
        .find(|(_, preset)| preset.name == "lazyvim.vimrc")
        .expect("the lazyvim preset is bundled");
    preset.text
}

/// The harpoon keys, with `é` for `<leader>`, and their commands.
const HARPOON: &[(&str, &str)] = &[
    ("éH", "workbench.harpoon_add"),
    ("éh", "workbench.harpoon_toggle_list"),
    ("é1", "workbench.harpoon_jump1"),
    ("é2", "workbench.harpoon_jump2"),
    ("é3", "workbench.harpoon_jump3"),
    ("é4", "workbench.harpoon_jump4"),
    ("é5", "workbench.harpoon_jump5"),
];

/// The preset's normal-mode maps, with `é` as `<leader>`: each one's keys
/// and right side.
fn normal_maps() -> Vec<(Vec<VimKey>, String)> {
    let leader = parse_keys("é").unwrap();
    let parsed = parse_vimrc_with_leader(text(), &leader);
    assert_eq!(parsed.issues, []);
    (parsed.remaps.into_iter())
        .filter(|remap| remap.modes.contains(&RemapMode::Normal))
        .map(|remap| (parse_keys(&remap.from).unwrap(), remap.to))
        .collect()
}

/// Every `<Cmd>fe:…<CR>` id in the preset that the app's catalog doesn't
/// know.
fn unknown_commands() -> Vec<String> {
    let catalog = fe_workbench::command_catalog();
    let leader = parse_keys("\\").unwrap();
    (parse_vimrc_with_leader(text(), &leader).remaps.into_iter())
        .filter_map(|remap| {
            let id = remap.to.strip_prefix("<Cmd>fe:")?.strip_suffix("<CR>")?;
            let known = CommandId::parse(id).is_ok_and(|parsed| {
                !matches!(
                    catalog.check(&parsed, &Value::Null),
                    Err(CatalogError::UnknownCommand)
                )
            });
            (!known).then(|| id.to_owned())
        })
        .collect()
}

#[test]
fn preset_maps_harpoon_keys() {
    let maps = normal_maps();
    for (keys, command) in HARPOON {
        let keys = parse_keys(keys).unwrap();
        let to = format!("<Cmd>fe:{command}<CR>");
        // Mapped once, to its command.
        let found: Vec<_> = (maps.iter()).filter(|(from, _)| *from == keys).collect();
        assert_eq!(found.len(), 1, "{command}: {found:?}");
        assert_eq!(found[0].1, to);
        // No other map shares its keys, or waits on them as a prefix, or
        // is one of them: none shadows another.
        let clashes: Vec<_> = (maps.iter())
            .filter(|(from, other)| *other != to || *from != keys)
            .filter(|(from, _)| from.starts_with(&keys) || keys.starts_with(from))
            .collect();
        assert_eq!(clashes, Vec::<&(Vec<VimKey>, String)>::new(), "{command}");
    }
    // Every command the preset names is registered: the new ones too.
    assert_eq!(unknown_commands(), Vec::<String>::new());

    // Typed with the preset's leader, each runs at once.
    let mut vim = HeadlessVim::new("one\ntwo", (0, 0));
    let leader = parse_keys("<Space>").unwrap();
    let parsed = parse_vimrc_with_leader(text(), &leader);
    let (set, issues) = RemapSet::new(&parsed.remaps, "<Space>");
    assert_eq!(issues, []);
    vim.state_mut().set_remaps(Arc::new(set));
    for (keys, command) in HARPOON {
        vim.feed(&keys.replace('é', "<Space>")).unwrap();
        let taken = vim.state_mut().take_remap_command();
        let taken = taken.map(|command| command.0.command.to_string());
        assert_eq!(taken.as_deref(), Some(*command), "{keys}");
    }
    assert_eq!(vim.text(), "one\ntwo");
}
