// SPDX-License-Identifier: Apache-2.0

//! Count-bearing messages take the count as a number and select a plural
//! form from it: en-US keeps today's wording for every count, and a locale
//! with more categories picks its own.

use std::path::PathBuf;

use fe_git::{RemoteError, SwitchError};
use kiln_l10n::LanguageIdentifier;

kiln_l10n::resources! {
    "ru" => ["plural_fixtures/ru.ftl"],
}

fn paths(count: usize) -> Vec<PathBuf> {
    (0..count)
        .map(|n| PathBuf::from(format!("/r/{n}")))
        .collect()
}

#[test]
fn counts_select_plural_forms_by_number() {
    for count in [0, 1, 2, 5] {
        assert_eq!(
            SwitchError::WouldOverwrite {
                paths: paths(count)
            }
            .to_string(),
            format!("local changes would be overwritten by switching branches ({count} files)")
        );
        assert_eq!(
            RemoteError::Conflicts {
                paths: paths(count)
            }
            .to_string(),
            format!("conflicts in {count} files")
        );
    }

    let ru: Vec<LanguageIdentifier> = vec!["ru".parse().unwrap()];
    kiln_l10n::set_locales(&ru);
    let shown = |count| {
        SwitchError::WouldOverwrite {
            paths: paths(count),
        }
        .to_string()
    };
    assert_eq!(shown(1), "RU-ONE 1");
    assert_eq!(shown(2), "RU-FEW 2");
    assert_eq!(shown(5), "RU-MANY 5");
    assert_eq!(shown(0), "RU-MANY 0");
    kiln_l10n::set_locales(&[]);
}
