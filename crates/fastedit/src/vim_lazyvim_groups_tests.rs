//! The bundled `lazyvim` preset's which-key group names and icons
//! (`:fegroup`).

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
            ("<leader>t".to_owned(), "test"),
            ("<leader>u".to_owned(), "ui"),
            ("<leader>w".to_owned(), "windows"),
            ("<leader>x".to_owned(), "diagnostics/quickfix"),
        ]
    );
}

/// Each group the preset names draws an icon, as LazyVim's which-key does:
/// a bundled one, the one LazyVim's resembles.
#[test]
fn every_named_group_has_an_icon() {
    let parsed = parsed();
    assert_eq!(parsed.issues, []);
    assert!(!parsed.groups.is_empty());
    // The UI set kiln-ui bundles, one SVG per icon (its `Icon`).
    let set = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../kiln/crates/kiln-ui/assets/icons");
    let bundled = |name: &str| set.join(format!("{name}.svg")).is_file();
    assert!(bundled("search"), "{set:?} holds the set");
    let mut icons = Vec::new();
    for group in &parsed.groups {
        let icon = (group.icon.as_deref()).unwrap_or_else(|| panic!("{group:?} has no icon"));
        assert!(bundled(icon), "{group:?}: {icon} isn't bundled");
        icons.push((group.name.as_str(), icon));
    }
    assert_eq!(
        icons,
        [
            ("buffer", "files"),
            ("code", "code"),
            ("file/find", "search"),
            ("git", "git-branch"),
            ("hunks", "file-diff"),
            ("quit/session", "log-out"),
            ("search", "search"),
            ("ui", "toggle-right"),
            ("windows", "app-window"),
            ("diagnostics/quickfix", "triangle-alert"),
        ]
    );
}

/// A user's name for a preset group keeps the preset's icon; their icon
/// replaces it.
#[test]
fn a_users_name_keeps_the_preset_icon() {
    let dir = tempfile::tempdir().unwrap();
    let vimrc = dir.path().join("vimrc");
    std::fs::write(
        &vimrc,
        "fegroup <leader>f files\nfegroup <leader>c lsp icon=bug\n",
    )
    .unwrap();
    let settings = dir.path().join("settings.toml");
    std::fs::write(
        &settings,
        "[vim]\npreset = \"lazyvim\"\n\
         [[vim.which_key_group]]\nkeys = \"<leader>g\"\nname = \"vcs\"\nicon = \"git-graph\"\n\
         [[vim.which_key_group]]\nkeys = \"<leader>b\"\nname = \"bufs\"\nicon = \"Bad Icon\"\n",
    )
    .unwrap();
    let store = SettingsStore::load(Some(settings), None);
    assert_eq!(store.diagnostics(), []);
    let remaps = VimRemaps::new(Some(vimrc), &store);
    let groups = remaps.groups(true, "<Space>", &mut Vec::new());
    let icon = |keys: &str| {
        let keys = parse_keys(keys).unwrap();
        (groups.iter().rev())
            .filter(|group| group.mode == RemapMode::Normal && group.keys == keys)
            .find_map(|group| group.icon.as_deref())
    };
    assert_eq!(icon("<Space>f"), Some("search"));
    assert_eq!(icon("<Space>c"), Some("bug"));
    assert_eq!(icon("<Space>g"), Some("git-graph"));
    // A malformed icon: the name loads, the icon is a warning at `icon`.
    assert_eq!(icon("<Space>b"), Some("files"));
    let name = (groups.iter().rev())
        .find(|group| group.keys == parse_keys("<Space>b").unwrap())
        .map(|group| group.name.as_str());
    assert_eq!(name, Some("bufs"));
    let diagnostics = remaps.diagnostics();
    assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
    assert_eq!(
        diagnostics[0].key.as_deref(),
        Some("vim.which_key_group[1].icon")
    );
    assert_eq!(diagnostics[0].line, 10);
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
