//! `\%d123`, `\%o40`, `\%x2a`, `\%u20AC` and `\%U1234abcd` outside a
//! collection match the character they code, as in nvim 0.12.6.

use fe_vim::HeadlessVim;

const BUFFER: &str = "a.b Foo €x\ttab\n  *star  00 é";

fn land(pattern: &str) -> ((u32, usize), Option<String>) {
    let mut vim = HeadlessVim::new(BUFFER, (0, 0));
    vim.feed(&format!("/{pattern}<CR>")).unwrap();
    (vim.cursor(), vim.state().message().map(str::to_owned))
}

#[test]
fn character_code_atoms_match_as_nvim() {
    let cases: &[(&[&str], (u32, usize))] = &[
        (
            &[
                r"\%d70",
                r"\%x46",
                r"\%o106",
                r"\%x46\%x6f\{2}",
                r"\v%x46oo",
                r"\V\%x46oo",
                r"\c\%x66oo",
            ],
            (0, 4),
        ),
        (&[r"o\%x6f"], (0, 5)),
        (
            &[
                r"\%u20AC",
                r"\%u20acx",
                r"\%U000020AC",
                r"\%d8364",
                r"\%u20AC\{1}",
            ],
            (0, 8),
        ),
        (&[r"\%x2e", r"\%x2E"], (0, 1)),
        (&[r"\%d9tab"], (0, 12)),
        (&[r"\%x2a\+", r"\%o52", r"\%o052s"], (1, 2)),
        (&[r"\%o4000"], (1, 8)),
        (&[r"\%d233", r"\%d00233", r"\%u0e9"], (1, 12)),
    ];
    for (patterns, want) in cases {
        for pattern in *patterns {
            assert_eq!(land(pattern).0, *want, "{pattern}");
        }
    }
    for pattern in [r"\%d", r"\%xg", r"\%u", r"\%d2147483648"] {
        let (at, message) = land(pattern);
        assert_eq!(at, (0, 0), "{pattern}");
        assert_eq!(
            message.as_deref(),
            Some("invalid pattern: invalid character after \\%[dxouU]"),
            "{pattern}"
        );
    }
}

#[test]
fn unmatchable_codes_stay_unsupported() {
    for pattern in [
        r"\%d0",
        r"\%d10",
        r"\%ud800",
        r"\%U00110000",
        r"\%V",
        r"\%23l",
    ] {
        let (at, message) = land(pattern);
        assert_eq!(at, (0, 0), "{pattern}");
        assert!(
            message
                .as_deref()
                .is_some_and(|m| m.contains("not supported")),
            "{pattern}: {message:?}"
        );
    }
}
