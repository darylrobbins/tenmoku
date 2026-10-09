//! fe-vim's `:` command line (ranges, `:{range}`, `:d`, `:noh`) against
//! recorded Neovim: `tests/corpus/ex.toml` and its expectations, which the
//! ignored recorder in `tests/oracle.rs` writes.

#[allow(dead_code)]
mod support;

use support::{check, load, parse_corpus};

#[test]
fn ex_matches_neovim() {
    let recorded = load("ex").unwrap();
    assert!(recorded.len() >= 30, "ex: {} cases", recorded.len());
    // What the card asks the corpus to cover, by case name.
    for name in [
        "goto_line",
        "goto_last",
        "goto_plus_2",
        "goto_minus_1",
        "visual_range_goto",
        "percent_delete",
        "pattern_forward",
        "pattern_backward",
        "delete_dot_plus_2",
        "delete_into_register",
        "delete_count",
        "semicolon_offset",
        "comma_offset",
        "empty_range",
        "past_end",
        "backwards_range",
        "noh_keeps_cursor_and_text",
        "unknown_command",
    ] {
        assert!(
            recorded.iter().any(|case| case.name == name),
            "ex: no case {name}"
        );
    }
    let failures: Vec<String> = recorded
        .iter()
        .filter_map(|case| check(case).err())
        .collect();
    assert!(
        failures.is_empty(),
        "ex: {} of {} cases differ from nvim:\n{}",
        failures.len(),
        recorded.len(),
        failures.join("\n")
    );
}

/// A one-case corpus with these keys.
fn corpus(keys: &str) -> String {
    let keys = toml::Value::String(keys.to_string());
    format!("[[case]]\nname = \"c\"\nbuffer = \"a+b*\"\ncursor = [0, 0]\nkeys = {keys}\n")
}

#[test]
fn loader_allows_plus_and_star_in_ex_ranges() {
    for keys in [
        ":.,+2d<CR>",
        ":s/a*/b/<CR>",
        ":+<CR>:-1;+2d<CR>",
        "3:<CR>",
        ":/a*/+1<CR>",
        ":su/a+/b*/<CR>",
        ":&&<CR>:'<,'>+1d<CR>",
    ] {
        assert!(parse_corpus(&corpus(keys)).is_ok(), "{keys}");
    }
    for keys in [
        "\"+p",
        "\"*yy",
        ":d +<CR>",
        ":d *<CR>",
        ":d*<CR>",
        ":3,4d +<CR>",
        ":pu +<CR>",
        ":y *<CR>",
        ":reg +<CR>",
        ":@+<CR>",
        ":<C-r>+<CR>",
        ":s/a/<C-r>*/<CR>",
        ":s/a/<C-r><C-r>+/<CR>",
        ":norm \"+p<CR>",
        ":pu =getreg('+')<CR>",
        ":sil d +<CR>",
        ":keepj d *<CR>",
        ":windo d +<CR>",
        ":g/a/d +<CR>",
        ":v/a/y *<CR>",
        ":exe \"d +\"<CR>",
        ":call setreg('+', 'x')<CR>",
        ":echo getreg('*')<CR>",
        ":/<C-r>+/<CR>",
        ":s/a/b/|d +<CR>",
        ":.,+2d<CR>\"+p",
        ":<Esc>f+",
    ] {
        let error = parse_corpus(&corpus(keys)).expect_err(keys);
        assert!(error.contains("register + or *"), "{keys}: {error}");
    }
}
