//! Which-key group names: a vimrc's `:fegroup {keys} {name}` (or a
//! `WhichKeyGroupName` from the settings) titles the group of maps that go
//! on from a held prefix, per mode, the last name given winning.

use std::sync::Arc;

use fe_vim::{
    HeadlessVim, RemapMode, RemapSet, VimKey, WhichKeyGroupName, WhichKeyLabel, parse_keys,
    parse_vimrc,
};

const N: RemapMode = RemapMode::Normal;
const X: RemapMode = RemapMode::Visual;

/// An editor with `vimrc`'s maps and group names, plus `extra` names.
fn vim(vimrc: &str, extra: &[WhichKeyGroupName]) -> HeadlessVim {
    let parsed = parse_vimrc(vimrc);
    assert!(parsed.issues.is_empty(), "{:?}", parsed.issues);
    let (set, issues) = RemapSet::new(&parsed.remaps, "\\");
    assert!(issues.is_empty(), "{issues:?}");
    let mut groups = parsed.groups;
    groups.extend_from_slice(extra);
    let mut vim = HeadlessVim::new("one two\n", (0, 0));
    vim.state_mut().set_remaps(Arc::new(set));
    vim.state_mut().set_which_key_groups(Arc::new(groups));
    vim
}

fn keys(text: &str) -> Vec<VimKey> {
    parse_keys(text).unwrap()
}

/// The label of `key` in the popup for the held `prefix`.
fn label(vim: &HeadlessVim, prefix: &str, key: &str) -> WhichKeyLabel {
    let which = vim.state().which_key_for(&keys(prefix)).unwrap();
    let entry = (which.entries.into_iter().find(|entry| entry.key == key))
        .unwrap_or_else(|| panic!("{key} not listed after {prefix}"));
    entry.label
}

fn group(count: usize, name: Option<&str>) -> WhichKeyLabel {
    WhichKeyLabel::Group {
        count,
        name: name.map(str::to_string),
    }
}

#[test]
fn a_named_prefix_titles_its_group() {
    let vim = vim(
        "let mapleader = ' '\n\
         nnoremap <leader>ff <Cmd>fe:quick_open.toggle<CR>\n\
         fegroup <leader>f file/find\n",
        &[],
    );
    assert_eq!(label(&vim, "<Space>", "f"), group(1, Some("file/find")));
    // A deeper name: keys-so-far plus the entry's key.
    let deep = self::vim(
        "nnoremap \\ghs x\nnnoremap \\ghr y\n:FeGroup <leader>gh hunks\n",
        &[],
    );
    assert_eq!(label(&deep, "\\g", "h"), group(2, Some("hunks")));
    assert_eq!(label(&deep, "\\", "g"), group(2, None));
}

#[test]
fn an_unnamed_group_keeps_its_count() {
    let vim = vim(
        "nnoremap \\ff x\nnnoremap \\fg y\nnnoremap \\bd z\nfegroup \\b buffer\n",
        &[],
    );
    assert_eq!(label(&vim, "\\", "f"), group(2, None));
    assert_eq!(label(&vim, "\\", "b"), group(1, Some("buffer")));
    // A name for keys that end a map labels nothing: the map's own.
    let leaf = self::vim(
        "nnoremap \\w x\nnnoremap \\ww y\nfegroup \\w windows\n",
        &[],
    );
    assert_eq!(label(&leaf, "\\", "w"), WhichKeyLabel::Keys("x".into()));
}

#[test]
fn names_are_per_mode() {
    let visual = WhichKeyGroupName::parse(&[X], "\\c", "code (visual)", "\\").unwrap();
    let mut vim = vim(
        "nnoremap \\cf x\nxnoremap \\cf y\nfegroup \\c code\n",
        &visual,
    );
    assert_eq!(label(&vim, "\\", "c"), group(1, Some("code")));
    vim.feed("v").unwrap();
    assert_eq!(label(&vim, "\\", "c"), group(1, Some("code (visual)")));
    // `:fegroup` names normal mode only.
    let mut only_n = self::vim("nnoremap \\cf x\nxnoremap \\cf y\nfegroup \\c code\n", &[]);
    only_n.feed("v").unwrap();
    assert_eq!(label(&only_n, "\\", "c"), group(1, None));
}

#[test]
fn a_later_name_wins() {
    let settings = WhichKeyGroupName::parse(&[N], "\\f", "files", "\\").unwrap();
    let vimrc = "nnoremap \\ff x\nfegroup \\f find\nfegroup \\f file/find\n";
    assert_eq!(
        label(&vim(vimrc, &[]), "\\", "f"),
        group(1, Some("file/find"))
    );
    // A settings name, loaded after the vimrc's, wins over it.
    assert_eq!(
        label(&vim(vimrc, &settings), "\\", "f"),
        group(1, Some("files"))
    );
    // `<leader>` is read as the leader when the line is.
    let leader = "nnoremap ,ff x\nfegroup <leader>f backslash\nlet mapleader = ','\n\
                  fegroup <leader>f comma\n";
    assert_eq!(label(&vim(leader, &[]), ",", "f"), group(1, Some("comma")));
}

#[test]
fn a_malformed_fegroup_line_is_an_issue() {
    let parsed = parse_vimrc(
        "nnoremap \\ff x\n\
         fegroup \\f\n\
         fegroup\n\
         fegroup   \n\
         fegroup! \\f find\n\
         fegroup <Plug>x name\n\
         fegroup \\f file/find\n",
    );
    let lines: Vec<usize> = parsed.issues.iter().map(|issue| issue.line).collect();
    assert_eq!(lines, [2, 3, 4, 5, 6], "{:?}", parsed.issues);
    assert!(
        parsed.issues[0].message.contains("name"),
        "{:?}",
        parsed.issues
    );
    assert!(
        parsed.issues[1].message.contains("keys"),
        "{:?}",
        parsed.issues
    );
    // Only the good line names a group, and the map still loads.
    assert_eq!(
        parsed.groups,
        [WhichKeyGroupName {
            mode: N,
            keys: keys("\\f"),
            name: "file/find".into(),
        }]
    );
    assert_eq!(parsed.remaps.len(), 1);
    // The settings' form rejects the same.
    assert!(WhichKeyGroupName::parse(&[N], "", "x", "\\").is_err());
    assert!(WhichKeyGroupName::parse(&[N], "\\f", " ", "\\").is_err());
    assert!(WhichKeyGroupName::parse(&[N], "<Bogus>", "x", "\\").is_err());
}
