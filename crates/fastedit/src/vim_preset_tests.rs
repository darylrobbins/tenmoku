//! `vim.preset`: a bundled remap file beneath the vimrc and
//! `[[vim.remap]]`, with its leader while `vim.leader` is left at `\`.

use std::path::Path;

use fe_gpui::fe_keymap::{CatalogError, CommandCatalog};
use fe_vim::{Lookup, VimKey, parse_vimrc};
use gpui::TestAppContext;
use kiln::{
    Config,
    command::{CommandId, Value},
    platform::config::{BatchSender, batch_channel},
};

use super::tests::{Files, edit, insert_layer, keys, normal, problems, warnings};
use super::*;

const SAVE: &str = "<Cmd>fe:editor.save<CR>";
const QUIT: &str = "<Cmd>fe:app.quit<CR>";

/// `kiln::init` with the settings and vimrc of `files`, and a
/// [`VimRemaps`] source whose presets are `presets`.
fn start(
    files: &Files,
    presets: fn(VimPreset) -> Option<Preset>,
    cx: &mut TestAppContext,
) -> BatchSender {
    let store = SettingsStore::load(Some(files.settings()), Some(files.project()));
    let mut remaps = VimRemaps::new(Some(files.vimrc()), &store);
    remaps.presets = presets;
    let (sender, batches) = batch_channel();
    let config = Config::new("dev.fastedit.test", "fastedit")
        .settings_store(store.into_inner())
        .source(remaps)
        .watch_with(batches);
    cx.update(|cx| kiln::init(cx, config));
    cx.run_until_parked();
    sender
}

/// The records and the leader every editor's set was built from.
fn in_use(cx: &mut TestAppContext) -> RemapsInUse {
    cx.update(|cx| cx.global::<RemapsInUse>().clone())
}

/// Whether `from` runs `fe:editor.save` in normal mode: the record that
/// wins for its keys is the preset's.
fn saves(from: &str, cx: &mut TestAppContext) -> bool {
    runs(from, SAVE, cx)
}

/// Whether `from` runs `to` in normal mode, as [`saves`].
fn runs(from: &str, to: &str, cx: &mut TestAppContext) -> bool {
    let in_use = in_use(cx);
    let (set, _) = RemapSet::new(&in_use.remaps, &in_use.leader);
    let from = parse_keys(from).unwrap();
    let mapped = matches!(
        set.lookup(RemapMode::Normal, &from),
        Lookup::Exact { to: [], .. } | Lookup::ExactAndPrefix { to: [], .. }
    );
    let last = (in_use.remaps.iter()).rfind(|remap| {
        parse_keys(&remap.from.replace("<leader>", &in_use.leader)).as_deref() == Ok(&from[..])
    });
    mapped && last.is_some_and(|remap| remap.to == to)
}

/// The `fe:` command ids the `<Cmd>…<CR>`s of `text`'s maps name.
fn fe_commands(text: &str) -> Vec<String> {
    let mut ids = Vec::new();
    for remap in parse_vimrc(text).remaps {
        let lower = remap.to.to_ascii_lowercase();
        let mut at = 0;
        while let Some(found) = lower[at..].find("<cmd>fe:") {
            let start = at + found + "<cmd>fe:".len();
            let end = lower[start..]
                .find("<cr>")
                .map_or(lower.len(), |end| start + end);
            ids.push(remap.to[start..end].to_owned());
            at = end;
        }
    }
    ids
}

/// The `fe:` command ids of `text` that fastedit's catalog doesn't know.
pub(super) fn unknown_commands(text: &str) -> Vec<String> {
    let catalog = fe_workbench::command_catalog();
    (fe_commands(text).into_iter())
        .filter(|id| {
            CommandId::parse(id).map_or(true, |parsed| {
                matches!(
                    catalog.check(&parsed, &Value::Null),
                    Err(CatalogError::UnknownCommand)
                )
            })
        })
        .collect()
}

#[gpui::test]
fn preset_off_is_plain_vim(cx: &mut TestAppContext) {
    for settings in ["", "[vim]\npreset = \"none\"\n"] {
        let files = Files::new(settings, None);
        start(&files, bundled, cx);
        let set = in_use(cx);
        assert_eq!(set.remaps, [], "{settings:?}: no preset maps");
        assert_eq!(set.leader, "\\");
        assert!(!saves(" w", cx));
        assert!(!saves("\\w", cx));
    }
    assert_eq!(bundled(VimPreset::None), None);
}

#[gpui::test]
fn preset_sits_under_the_users_maps_and_sets_its_leader(cx: &mut TestAppContext) {
    let lazyvim = bundled(VimPreset::Lazyvim).unwrap();
    assert_eq!(lazyvim.leader, "<Space>");
    let preset = "[vim]\npreset = \"lazyvim\"\n";
    let files = Files::new(preset, None);
    let sender = start(&files, bundled, cx);
    let set = in_use(cx);
    assert_eq!(set.leader, "<Space>");
    assert_eq!(
        set.remaps,
        parse_with_leader(lazyvim.text, "<Space>").remaps
    );
    assert!(runs(" qq", QUIT, cx), "the preset's <leader>qq");
    assert!(!runs("\\qq", QUIT, cx));
    assert_eq!(problems(Path::new(lazyvim.name), cx), []);

    // The vimrc's `<leader>` starts as the preset's, and its map wins.
    let vimrc = "nnoremap <leader>qq :w<CR>\n";
    edit(&files.vimrc(), vimrc, &[files.vimrc()], &sender, cx);
    assert_eq!(normal(" qq", cx), keys(":w<CR>"));
    assert_eq!(problems(&files.vimrc(), cx), []);

    // So does a `[[vim.remap]]` entry's, over both.
    let table =
        format!("{preset}\n[[vim.remap]]\nmode = \"n\"\nfrom = \"<leader>qq\"\nto = \"x\"\n");
    edit(&files.settings(), &table, &[files.settings()], &sender, cx);
    assert_eq!(normal(" qq", cx), keys("x"));

    // An explicit `vim.leader` beats the preset's, for its maps too.
    std::fs::remove_file(files.vimrc()).unwrap();
    let comma = "[vim]\npreset = \"lazyvim\"\nleader = \",\"\n";
    let batch = [files.vimrc(), files.settings()];
    edit(&files.settings(), comma, &batch, &sender, cx);
    assert_eq!(in_use(cx).leader, ",");
    assert!(runs(",qq", QUIT, cx));
    assert!(!runs(" qq", QUIT, cx));

    // A vimrc's own `let mapleader` still sets its leader from there.
    let vimrc = "nnoremap <leader>q :q<CR>\nlet mapleader = \"-\"\nnnoremap <leader>q ZZ\n";
    edit(&files.vimrc(), vimrc, &[files.vimrc()], &sender, cx);
    assert_eq!(normal(",q", cx), keys(":q<CR>"));
    assert_eq!(normal("-q", cx), keys("ZZ"));
    assert!(runs(",qq", QUIT, cx));
}

#[gpui::test]
fn any_leader_keeps_the_users_maps_over_the_presets(cx: &mut TestAppContext) {
    // Each `vim.leader` as TOML writes it, and as vim notation.
    let leaders = [
        ("\"<C-i>\"", "<C-i>"),
        ("\"<C-m>\"", "<C-m>"),
        ("\"\"", ""),
        ("\"\\n\"", "\n"),
    ];
    for (toml, leader) in leaders {
        let preset = format!("[vim]\npreset = \"lazyvim\"\nleader = {toml}\n");
        let files = Files::new(&preset, None);
        let sender = start(&files, bundled, cx);
        assert_eq!(in_use(cx).leader, leader);
        let w = format!("{leader}qq");
        assert!(runs(&w, QUIT, cx), "{leader:?}: the preset's");

        let vimrc = "nnoremap <leader>qq :w<CR>\n";
        edit(&files.vimrc(), vimrc, &[files.vimrc()], &sender, cx);
        assert_eq!(problems(&files.vimrc(), cx), [], "{leader:?}");
        assert_eq!(normal(&w, cx), keys(":w<CR>"), "{leader:?}: the vimrc's");

        let entry = "[[vim.remap]]\nmode = \"n\"\nfrom = \"<leader>qq\"\nto = \"x\"\n";
        let table = format!("{preset}\n{entry}");
        edit(&files.settings(), &table, &[files.settings()], &sender, cx);
        assert_eq!(problems(&files.settings(), cx), [], "{leader:?}");
        assert_eq!(normal(&w, cx), keys("x"), "{leader:?}: the table's");
    }
}

#[test]
fn a_preset_leader_types_as_written() {
    let leaders = [
        ",", "<Space>", "\\", "\"", "<lt>", "<Bar>", "|", "<C-a>", "<C-i>", "<C-m>", "<Tab>",
        "<BS>", "<Esc>", "<CR>", "<Left>", "<Down>", "ab", "é", "", "\n", "<",
    ];
    for leader in leaders {
        let parsed = parse_with_leader("\" a comment\nnnoremap <leader>x y\nbogus\n", leader);
        let mut expected = parse_keys(leader).unwrap();
        expected.push(VimKey::Char('x'));
        assert_eq!(parsed.remaps.len(), 1, "{leader:?}: {parsed:?}");
        assert_eq!(
            parse_keys(&parsed.remaps[0].from),
            Ok(expected),
            "{leader:?}"
        );
        // The same keys as a `[[vim.remap]]` entry's `<leader>x` with it,
        // which replaces the record.
        let table = Remap {
            modes: vec![RemapMode::Normal],
            from: "<leader>x".to_owned(),
            to: "z".to_owned(),
            recursive: false,
        };
        let (set, issues) = RemapSet::new(&[parsed.remaps[0].clone(), table], leader);
        assert_eq!(issues, [], "{leader:?}");
        let typed = parse_keys(&parsed.remaps[0].from).unwrap();
        assert!(
            matches!(
                set.lookup(RemapMode::Normal, &typed),
                Lookup::Exact {
                    to: [VimKey::Char('z')],
                    ..
                }
            ),
            "{leader:?}"
        );
        assert_eq!(parsed.lines, [2], "{leader:?}");
        let lines: Vec<_> = parsed.issues.iter().map(|issue| issue.line).collect();
        assert_eq!(lines, [3], "{leader:?}");
    }
}

/// A preset with a line that can't load, and one map.
fn broken(preset: VimPreset) -> Option<Preset> {
    (preset == VimPreset::Lazyvim).then_some(Preset {
        name: "broken.vimrc",
        text: "\" broken\nnnoremap <leader>w <Cmd>fe:editor.save<CR>\nset number\n",
        leader: "<Space>",
    })
}

#[gpui::test]
fn bundled_presets_name_real_commands(cx: &mut TestAppContext) {
    assert!(!PRESETS.is_empty());
    for (preset, file) in PRESETS {
        let parsed = parse_with_leader(file.text, file.leader);
        assert_eq!(parsed.issues, [], "{preset:?} loads whole");
        assert!(!parsed.remaps.is_empty(), "{preset:?} maps keys");
        assert_eq!(bundled(*preset), Some(*file));
        assert_eq!(
            unknown_commands(file.text),
            Vec::<String>::new(),
            "{preset:?}"
        );
    }

    // A renamed command is caught.
    let renamed = "nnoremap <leader>w <Cmd>fe:editor.save<CR>\n\
                   nnoremap <leader>x <Cmd>fe:editor.no_such_command<CR>\n";
    assert_eq!(unknown_commands(renamed), ["editor.no_such_command"]);

    // A preset's problem is listed against its name, at its line.
    let files = Files::new("[vim]\npreset = \"lazyvim\"\n", None);
    start(&files, broken, cx);
    let found = problems(Path::new("broken.vimrc"), cx);
    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!(
        (found[0].severity, found[0].line, found[0].column),
        (Severity::Warning, 3, 1)
    );
    assert!(saves(" w", cx), "the rest of it loads");
}

#[gpui::test]
fn reload_applies_the_preset(cx: &mut TestAppContext) {
    let files = Files::new("", Some("nnoremap H ^\n"));
    let sender = start(&files, broken, cx);
    assert!(!saves(" w", cx));
    assert_eq!(in_use(cx).leader, "\\");

    // Set: the editors get its maps and its leader, the vimrc's kept.
    let preset = "[vim]\npreset = \"lazyvim\"\n";
    edit(&files.settings(), preset, &[files.settings()], &sender, cx);
    assert_eq!(in_use(cx).leader, "<Space>");
    assert!(saves(" w", cx));
    assert_eq!(normal("H", cx), keys("^"));
    assert_eq!(problems(Path::new("broken.vimrc"), cx).len(), 1);
    cx.update(|cx| {
        let in_use = cx.global::<RemapsInUse>();
        let (set, _) = RemapSet::new(&in_use.remaps, &in_use.leader);
        assert!(!set.is_empty());
    });

    // Unset: plain vim again, and its problem gone.
    edit(&files.settings(), "", &[files.settings()], &sender, cx);
    assert_eq!(in_use(cx).leader, "\\");
    assert!(!saves(" w", cx));
    assert_eq!(normal("H", cx), keys("^"));
    assert_eq!(problems(Path::new("broken.vimrc"), cx), []);
}

/// A preset with an insert map.
fn jk(preset: VimPreset) -> Option<Preset> {
    (preset == VimPreset::Lazyvim).then_some(Preset {
        name: "jk.vimrc",
        text: "\" escape\ninoremap jk <Esc>\n",
        leader: "<Space>",
    })
}

#[gpui::test]
fn a_preset_map_the_keymap_hides_is_warned_at_its_line(cx: &mut TestAppContext) {
    use kiln::{core::KeymapStore, platform::config::ConfigService};

    let files = Files::new("[vim]\npreset = \"lazyvim\"\n", None);
    let store = SettingsStore::load(Some(files.settings()), Some(files.project()));
    let mut remaps = VimRemaps::new(Some(files.vimrc()), &store);
    remaps.presets = jk;
    let keymap = KeymapStore::with_defaults(Vec::new(), Vec::new(), None, None);
    let (_sender, batches) = batch_channel();
    let service = ConfigService::new().keymap(keymap).source(remaps);
    let shared = service.keymap_store().unwrap();
    cx.update(|cx| drop(service.start_with(batches, cx)));
    cx.run_until_parked();
    assert_eq!(warnings(Path::new("jk.vimrc"), cx), []);

    cx.update(|cx| {
        shared.update(cx, |store| {
            store.set_defaults(insert_layer("j k"), Vec::new(), None)
        })
    });
    let found = warnings(Path::new("jk.vimrc"), cx);
    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!(found[0].line, 2);
    assert_eq!(warnings(&files.vimrc(), cx), []);
}
