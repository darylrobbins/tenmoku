// SPDX-License-Identifier: Apache-2.0

//! Count-bearing messages take the count as a number and select a plural
//! form from it: en-US keeps today's wording for every count, and a locale
//! with more categories picks its own.

use kiln_l10n::{LanguageIdentifier, tr};

kiln_l10n::resources! {
    "ru" => ["plural_fixtures/ru.ftl"],
}

#[test]
fn counts_select_plural_forms_by_number() {
    // Linking the crate registers its catalogue.
    let _ = fe_lsp::l10n::RESOURCES;
    kiln_l10n::set_locales(&[]);
    for seconds in [0u64, 1, 2, 5, 30] {
        assert_eq!(
            tr!(
                "lsp-server-initialize-timeout",
                name = "rust-analyzer",
                seconds = seconds
            ),
            format!("rust-analyzer didn't answer initialize within {seconds} s")
        );
    }
    for count in [0usize, 1, 2, 5, 1024] {
        assert_eq!(
            tr!("lsp-backlog-over-bytes", count = count),
            format!("server output backlog over {count} bytes")
        );
    }
    assert_eq!(
        tr!("lsp-backlog-over-mib", count = 64usize),
        "server output backlog over 64 MiB"
    );
    assert_eq!(
        tr!("lsp-server-orphans", name = "gopls", count = 1usize),
        "gopls left more than 1 cancelled requests unanswered"
    );

    let ru: Vec<LanguageIdentifier> = vec!["ru".parse().unwrap()];
    kiln_l10n::set_locales(&ru);
    let shown = |seconds: u64| {
        tr!(
            "lsp-server-initialize-timeout",
            name = "x",
            seconds = seconds
        )
        .to_string()
    };
    assert_eq!(shown(1), "RU-ONE x 1");
    assert_eq!(shown(2), "RU-FEW x 2");
    assert_eq!(shown(5), "RU-MANY x 5");
    assert_eq!(shown(0), "RU-MANY x 0");
    assert_eq!(tr!("lsp-backlog-over-bytes", count = 3usize), "RU-FEW 3");
    kiln_l10n::set_locales(&[]);
}
