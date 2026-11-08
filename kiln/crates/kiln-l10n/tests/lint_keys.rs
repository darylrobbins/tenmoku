// SPDX-License-Identifier: Apache-2.0

//! The per-crate key check's fixtures (layer 3 of section 8): keys used
//! exist, other crates' keys are `## public`, every `.ftl` parses and is
//! listed. Two fixture crates, `tests/fixtures/keys/good` and `…/bad`, use a
//! library's keys, `tests/fixtures/keys/lib/lib.ftl`.

use std::path::PathBuf;

use kiln_l10n::lint::keys;
use kiln_l10n::{ResourceFile, Resources};

mod good {
    kiln_l10n::resources! {
        "en-US" => ["fixtures/keys/good/l10n/en-US/app.ftl"],
    }
}

mod bad {
    kiln_l10n::resources! {
        "en-US" => ["fixtures/keys/bad/l10n/en-US/app.ftl"],
    }
}

/// Another crate's resources, by name, so ownership differs.
static LIB: Resources = Resources {
    crate_name: "lib",
    manifest_dir: "",
    declared_in: file!(),
    files: &[ResourceFile {
        locale: "en-US",
        path: "fixtures/keys/lib/lib.ftl",
        source: include_str!("fixtures/keys/lib/lib.ftl"),
    }],
    dynamic_prefixes: &[],
};

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/keys")
        .join(name)
}

#[test]
fn own_keys_and_public_keys_of_a_dependency_pass() {
    let report = keys(fixture("good"), &good::RESOURCES)
        .linked(vec![&LIB])
        .report();
    assert!(report.errors.is_empty(), "{report}");
}

#[test]
fn each_problem_is_reported() {
    let report = keys(fixture("bad"), &bad::RESOURCES)
        .linked(vec![&LIB])
        .report();
    let errors = &report.errors;
    let has = |needle: &str| errors.iter().any(|e| e.contains(needle));
    assert!(
        has("en-US: fixtures/keys/bad/l10n/en-US/app.ftl: doesn't parse"),
        "{report}"
    );
    assert!(
        has("l10n/en-US/unlisted.ftl isn't listed in the crate's resources!"),
        "{report}"
    );
    assert!(
        has("src/lib.rs:6: `lib-private` belongs to lib, and isn't in its `## public` group"),
        "{report}"
    );
    assert!(
        has("src/lib.rs:7: `nowhere-key` isn't in any en-US resource linked into this test"),
        "{report}"
    );
    assert_eq!(errors.len(), 4, "{report}");
}

#[test]
fn a_key_is_missing_when_its_crate_isnt_linked() {
    let report = keys(fixture("good"), &good::RESOURCES)
        .linked(vec![])
        .report();
    assert_eq!(report.errors.len(), 2, "{report}");
    assert!(
        report
            .errors
            .iter()
            .all(|e| e.contains("`lib-public` isn't in any en-US resource")),
        "{report}"
    );
}

#[test]
#[should_panic(expected = "nowhere-key")]
fn check_panics_with_the_errors() {
    keys(fixture("bad"), &bad::RESOURCES)
        .linked(vec![&LIB])
        .check();
}
