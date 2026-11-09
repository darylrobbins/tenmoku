// SPDX-License-Identifier: Apache-2.0

//! A mount-error detail resolves in the active locale: the three
//! `/proc/self/fdinfo` diagnostics are catalogue messages, not English
//! literals folded into the outer message. Its own binary, because the
//! locale is process-wide.

use kiln_l10n::{LanguageIdentifier, tr};

kiln_l10n::resources! {
    "xx" => ["locale_fixtures/xx.ftl"],
}

#[test]
fn mount_error_details_follow_the_locale() {
    // Link fe-project's own catalogue into this binary.
    let _ = &fe_project::l10n::RESOURCES;
    assert_eq!(
        tr!("project-io-mount-not-procfs").to_string(),
        "/proc is not a procfs"
    );

    let xx: Vec<LanguageIdentifier> = vec!["xx".parse().unwrap()];
    kiln_l10n::set_locales(&xx);
    assert_eq!(
        tr!("project-io-mount-not-procfs").to_string(),
        "XX-NOT-PROCFS"
    );
    assert_eq!(
        tr!("project-io-mount-fdinfo-other-file").to_string(),
        "XX-OTHER-FILE"
    );
    assert_eq!(
        tr!("project-io-mount-no-mount-id").to_string(),
        "XX-NO-MOUNT-ID"
    );
    let detail = tr!("project-io-mount-no-mount-id").to_string();
    assert_eq!(
        tr!("project-io-mount-unknown", detail = detail).to_string(),
        "XX-MOUNT XX-NO-MOUNT-ID"
    );
}
