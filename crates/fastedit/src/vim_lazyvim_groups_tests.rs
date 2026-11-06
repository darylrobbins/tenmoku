//! The bundled `lazyvim` preset's which-key group names (`:fegroup`).

use fe_vim::{RemapMode, VimKey};

use super::*;

/// The preset read with `<leader>` as `é`, so its keys stand out.
fn parsed() -> Vimrc {
    let preset = bundled(VimPreset::Lazyvim).unwrap();
    parse_with_leader(preset.text, "é")
}

/// The keys of each `<leader>` map of the preset in normal mode.
fn leader_maps(parsed: &Vimrc) -> Vec<Vec<VimKey>> {
    (parsed.remaps.iter())
        .filter(|remap| remap.modes.contains(&RemapMode::Normal))
        .map(|remap| parse_keys(&remap.from).unwrap())
        .filter(|keys| keys.first() == Some(&VimKey::Char('é')))
        .collect()
}

#[test]
fn the_preset_names_every_group_it_has_maps_in() {
    let parsed = parsed();
    assert_eq!(parsed.issues, []);
    let maps = leader_maps(&parsed);
    let named = |keys: &[VimKey]| {
        (parsed.groups.iter()).any(|group| group.mode == RemapMode::Normal && group.keys == keys)
    };
    // Every prefix of two or more keys that a longer map goes on from.
    for map in &maps {
        for len in 2..map.len() {
            let prefix = &map[..len];
            assert!(named(prefix), "{prefix:?} (of {map:?}) has no name");
        }
    }
    // And no name for keys no map goes on from.
    for group in &parsed.groups {
        let has_map =
            (maps.iter()).any(|map| map.len() > group.keys.len() && map.starts_with(&group.keys));
        assert!(has_map, "{group:?} names no group of maps");
    }
    // LazyVim's names.
    let names: Vec<(String, &str)> = (parsed.groups.iter())
        .map(|group| {
            let keys: String = group
                .keys
                .iter()
                .map(|key| match key {
                    VimKey::Char('é') => "<leader>".to_owned(),
                    VimKey::Char(ch) => ch.to_string(),
                    other => format!("{other:?}"),
                })
                .collect();
            (keys, group.name.as_str())
        })
        .collect();
    assert_eq!(
        names,
        [
            ("<leader>b".to_owned(), "buffer"),
            ("<leader>c".to_owned(), "code"),
            ("<leader>f".to_owned(), "file/find"),
            ("<leader>g".to_owned(), "git"),
            ("<leader>gh".to_owned(), "hunks"),
            ("<leader>q".to_owned(), "quit/session"),
            ("<leader>s".to_owned(), "search"),
            ("<leader>u".to_owned(), "ui"),
            ("<leader>w".to_owned(), "windows"),
            ("<leader>x".to_owned(), "diagnostics/quickfix"),
        ]
    );
}

/// The names reach the editors as the preset loads, beneath the vimrc's and
/// `[[vim.which_key_group]]`'s, the later winning; a table entry whose keys
/// can't be read is a warning at its `keys`.
#[test]
fn the_table_and_the_vimrc_name_over_the_preset() {
    let dir = tempfile::tempdir().unwrap();
    let vimrc = dir.path().join("vimrc");
    std::fs::write(&vimrc, "fegroup <leader>f files\nfegroup <leader>b bufs\n").unwrap();
    let settings = dir.path().join("settings.toml");
    std::fs::write(
        &settings,
        "[vim]\npreset = \"lazyvim\"\n\
         [[vim.which_key_group]]\nkeys = \"<leader>b\"\nname = \"buffers\"\n\
         [[vim.which_key_group]]\nkeys = \"<Bogus>\"\nname = \"x\"\n",
    )
    .unwrap();
    let store = SettingsStore::load(Some(settings), None);
    assert_eq!(store.diagnostics(), []);
    let remaps = VimRemaps::new(Some(vimrc), &store);
    let groups = remaps.groups(true, "<Space>", &mut Vec::new());
    let name = |keys: &str| {
        let keys = parse_keys(keys).unwrap();
        (groups.iter().rev())
            .find(|group| group.mode == RemapMode::Normal && group.keys == keys)
            .map(|group| group.name.as_str())
    };
    assert_eq!(name("<Space>f"), Some("files"));
    assert_eq!(name("<Space>b"), Some("buffers"));
    assert_eq!(name("<Space>g"), Some("git"));
    let diagnostics = remaps.diagnostics();
    assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
    assert_eq!(
        diagnostics[0].key.as_deref(),
        Some("vim.which_key_group[1].keys")
    );
    assert_eq!((diagnostics[0].line, diagnostics[0].column), (7, 1));
}
