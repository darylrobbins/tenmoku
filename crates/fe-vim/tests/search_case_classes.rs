//! `[:upper:]` and `[:lower:]` in a collection match Unicode letters and are
//! not folded by `\c`, as nvim's do.

use fe_vim::HeadlessVim;

const MIXED: &str = "aZ é É ß Ω ω";
const ODD: &str = "ǅ İ ı ß ẞ ﬁ ª 𝐀 Ⅰ ⓐ µ";

#[test]
fn upper_and_lower_classes_match_as_nvim() {
    let subs: &[(&str, &str, &str)] = &[
        (MIXED, ":s/[[:upper:]]/#/g", "a# é # ß # ω"),
        (MIXED, ":s/[[:lower:]]/#/g", "#Z # É # Ω #"),
        (MIXED, ":s/[^[:lower:]]/#/g", "a##é###ß###ω"),
        (MIXED, ":s/\\c[[:lower:]]/#/g", "#Z # É # Ω #"),
        (MIXED, ":s/\\c[[:upper:]]/#/g", "a# é # ß # ω"),
        (MIXED, ":s/[a[:upper:]]/#/g", "## é # ß # ω"),
        (ODD, ":s/[[:upper:]]/U/g", "U U ı ß U ﬁ ª 𝐀 U ⓐ µ"),
        (ODD, ":s/[[:lower:]]/L/g", "L İ L L ẞ ﬁ ª 𝐀 Ⅰ L L"),
        ("abc", ":s/[[:alpha:]]/#/g", "###"),
    ];
    for &(buffer, cmd, want) in subs {
        let mut vim = HeadlessVim::new(buffer, (0, 0));
        vim.feed(&format!("{cmd}<CR>")).unwrap();
        assert_eq!(vim.text(), want, "{cmd} on {buffer:?}");
    }
    let mut vim = HeadlessVim::new("ab Cd", (0, 0));
    vim.feed("/[[:upper:]]<CR>").unwrap();
    assert_eq!(vim.cursor(), (0, 3));
    let mut vim = HeadlessVim::new("x\nÉa", (0, 0));
    vim.feed("/[[:upper:]]<CR>").unwrap();
    assert_eq!(vim.cursor(), (1, 0));
    let mut vim = HeadlessVim::new("AB cD", (0, 0));
    vim.feed("d/[[:lower:]]<CR>").unwrap();
    assert_eq!(vim.text(), "cD");
}
