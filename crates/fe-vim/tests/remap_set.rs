use fe_vim::{Lookup, Remap, RemapMode, RemapSet, VimKey, parse_keys};

const N: RemapMode = RemapMode::Normal;
const X: RemapMode = RemapMode::Visual;
const O: RemapMode = RemapMode::OperatorPending;

fn map(modes: &[RemapMode], from: &str, to: &str) -> Remap {
    Remap {
        modes: modes.to_vec(),
        from: from.into(),
        to: to.into(),
        recursive: false,
    }
}

fn keys(s: &str) -> Vec<VimKey> {
    parse_keys(s).unwrap()
}

#[test]
fn remap_set_looks_up_by_mode() {
    let remaps = [
        map(&[N], "Y", "y$"),
        map(&[N], "gb", ":ls<CR>"),
        map(&[N], "g", "x"),
        Remap {
            recursive: true,
            ..map(&[X], "a", "b")
        },
    ];
    let (set, issues) = RemapSet::new(&remaps, "\\");
    assert!(issues.is_empty());
    assert!(!set.is_empty());
    assert!(RemapSet::default().is_empty());

    assert_eq!(
        set.lookup(N, &keys("Y")),
        Lookup::Exact {
            to: &keys("y$"),
            recursive: false
        }
    );
    assert_eq!(set.lookup(N, &keys("q")), Lookup::None);
    assert_eq!(set.lookup(N, &[]), Lookup::None);
    assert_eq!(
        set.lookup(N, &keys("gb")),
        Lookup::Exact {
            to: &keys(":ls<CR>"),
            recursive: false
        }
    );
    assert_eq!(
        set.lookup(N, &keys("g")),
        Lookup::ExactAndPrefix {
            to: &keys("x"),
            recursive: false
        }
    );
    assert_eq!(set.lookup(O, &keys("Y")), Lookup::None);
    assert_eq!(set.lookup(X, &keys("Y")), Lookup::None);
    assert_eq!(
        set.lookup(X, &keys("a")),
        Lookup::Exact {
            to: &keys("b"),
            recursive: true
        }
    );

    let (prefixed, _) = RemapSet::new(&[map(&[O], "abc", "d")], "\\");
    assert_eq!(prefixed.lookup(O, &keys("a")), Lookup::Prefix);
    assert_eq!(prefixed.lookup(O, &keys("ab")), Lookup::Prefix);
    assert_eq!(prefixed.lookup(N, &keys("a")), Lookup::None);

    let (later, _) = RemapSet::new(&[map(&[N, X], "H", "^"), map(&[N], "H", "0")], "\\");
    assert_eq!(
        later.lookup(N, &keys("H")),
        Lookup::Exact {
            to: &keys("0"),
            recursive: false
        }
    );
    assert_eq!(
        later.lookup(X, &keys("H")),
        Lookup::Exact {
            to: &keys("^"),
            recursive: false
        }
    );

    assert_eq!(set.hides_builtin(N), vec!["g".to_string()]);
    assert!(set.hides_builtin(X).is_empty());
}

#[test]
fn remap_set_resolves_notation() {
    let remaps = [
        map(&[N], "<leader>w", ":w<CR><Leader>"),
        map(&[N], "<Nop>x", "<nop>"),
        map(&[N], "q", "<Nop>"),
        map(&[N], "<C-W>", "<Space><Bar><Bslash>"),
    ];
    let (set, issues) = RemapSet::new(&remaps, "\\");
    assert_eq!(issues.len(), 1);
    assert_eq!(issues[0].index, 1);
    assert_eq!(
        set.lookup(N, &keys("\\w")),
        Lookup::Exact {
            to: &keys(":w<CR>\\"),
            recursive: false
        }
    );
    assert_eq!(
        set.lookup(N, &keys("q")),
        Lookup::Exact {
            to: &[],
            recursive: false
        }
    );
    assert_eq!(
        set.lookup(N, &[VimKey::Ctrl('w')]),
        Lookup::Exact {
            to: &keys(" |\\"),
            recursive: false
        }
    );

    let (space, _) = RemapSet::new(&[map(&[N], "<leader>w", "<LEADER>")], "<Space>");
    assert_eq!(
        space.lookup(N, &keys(" w")),
        Lookup::Exact {
            to: &[VimKey::Char(' ')],
            recursive: false
        }
    );

    assert_eq!(
        parse_keys("<Space><bar><BSLASH>"),
        Ok(vec![
            VimKey::Char(' '),
            VimKey::Char('|'),
            VimKey::Char('\\')
        ])
    );
    assert_eq!(keys("<C-W>"), keys("<C-w>"));
}

#[test]
fn remap_set_reports_bad_entries() {
    let bad = [
        ("", "x"),
        ("<Nop>", "x"),
        ("a<Cmd>", "x"),
        ("<F13>", "x"),
        ("a", "<F13>"),
        ("<Plug>a", "x"),
        ("a", "<SID>b"),
        ("<SNR>", "x"),
        ("<LocalLeader>a", "x"),
        ("a", "<Cmd>w<CR>"),
    ];
    let mut remaps = vec![map(&[N], "good", "ok")];
    remaps.extend(bad.iter().map(|(f, t)| map(&[N], f, t)));
    remaps.push(map(&[N], "zzz", "ok"));
    let (set, issues) = RemapSet::new(&remaps, "\\");
    let indexes: Vec<_> = issues.iter().map(|i| i.index).collect();
    assert_eq!(indexes, (1..=bad.len()).collect::<Vec<_>>());
    assert_eq!(issues[3].message, "unknown key <F13>");
    assert_eq!(issues[4].message, "unknown key <F13>");
    assert!(matches!(set.lookup(N, &keys("good")), Lookup::Exact { .. }));
    assert!(matches!(set.lookup(N, &keys("zzz")), Lookup::Exact { .. }));
    assert_eq!(set.lookup(N, &keys("b")), Lookup::None);
}

#[test]
fn remap_set_leader_keeps_token_boundaries() {
    let (set, issues) = RemapSet::new(&[map(&[N], "<leader>Space>", "x<leader>")], "<");
    assert!(issues.is_empty());
    assert_eq!(set.lookup(N, &keys("<Space>")), Lookup::None);
    assert_eq!(
        set.lookup(
            N,
            &[
                VimKey::Char('<'),
                VimKey::Char('S'),
                VimKey::Char('p'),
                VimKey::Char('a'),
                VimKey::Char('c'),
                VimKey::Char('e'),
                VimKey::Char('>')
            ]
        ),
        Lookup::Exact {
            to: &[VimKey::Char('x'), VimKey::Char('<')],
            recursive: false
        }
    );
}

#[test]
fn remap_set_hides_builtin_multi_key_operators() {
    let (set, _) = RemapSet::new(
        &[
            map(&[N], "gU", "x"),
            map(&[N], "gUU", "y"),
            map(&[N], "gb", "z"),
        ],
        "\\",
    );
    assert_eq!(set.hides_builtin(N), vec!["gU".to_string()]);
    assert!(set.hides_builtin(X).is_empty());
}
