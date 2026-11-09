// SPDX-License-Identifier: Apache-2.0

//! The literal lint (kiln-l10n): no hard-coded user-facing text in this
//! crate's non-test source outside `l10n-allow.toml`.

#[test]
fn ui_text_is_in_the_catalogue() {
    kiln_l10n::lint::literals(env!("CARGO_MANIFEST_DIR"))
        .allow_list("l10n-allow.toml")
        .check();
}

#[test]
fn keys_used_here_exist() {
    kiln_l10n::lint::keys(env!("CARGO_MANIFEST_DIR"), &fe_project::l10n::RESOURCES).check();
}

/// Representative filesystem errors, ignore diagnostics and search
/// diagnostics come out of the catalogue: en-US text, not a key shown back.
#[test]
fn errors_and_diagnostics_resolve_through_the_catalogue() {
    use kiln_l10n::tr;

    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    std::fs::write(root.join("a.txt"), "x").unwrap();
    std::fs::write(root.join("b.txt"), "x").unwrap();

    // A filesystem error's detail.
    let err = fe_project::fs_ops::rename(&root, std::path::Path::new("../other.txt"), "y.txt")
        .expect_err("outside the root");
    assert!(
        err.to_string()
            .contains("`..` in a path under the project root"),
        "{err}"
    );

    // A search diagnostic.
    let err = fe_project::PathFilter::new(&root, &["[abc"], &[])
        .map(|_| ())
        .expect_err("an unclosed class");
    assert!(
        err.to_string().contains("unclosed character class"),
        "{err}"
    );

    // An ignore diagnostic and a count-free listing message by key.
    assert_eq!(
        tr!("project-ignore-unknown-class", name = "nope").to_string(),
        "unknown character class `[:nope:]`"
    );
    assert_eq!(
        tr!("project-io-listing-replaced", path = "/p").to_string(),
        "/p was replaced while listing"
    );
}

/// A mount-error detail (the three `/proc/self/fdinfo` diagnostics that
/// `Volume::of` folds into its error) comes out of the catalogue: en-US text
/// now, a locale's own once one has them.
#[test]
fn mount_error_details_resolve_through_the_catalogue() {
    use kiln_l10n::tr;

    assert_eq!(
        tr!("project-io-mount-not-procfs").to_string(),
        "/proc is not a procfs"
    );
    assert_eq!(
        tr!("project-io-mount-fdinfo-other-file").to_string(),
        "fdinfo for another file"
    );
    assert_eq!(
        tr!("project-io-mount-no-mount-id").to_string(),
        "no mount id in fdinfo"
    );
    assert_eq!(
        tr!("project-io-mount-unknown", detail = "no mount id in fdinfo").to_string(),
        "can't tell which mount the entry is on: no mount id in fdinfo"
    );
}
