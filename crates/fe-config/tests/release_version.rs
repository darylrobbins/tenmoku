//! `parse_release_version`: stable and `-preview.N` versions, ordered.

use fe_config::whats_new::parse_release_version as parse;

fn v(text: &str) -> fe_config::whats_new::ReleaseVersion {
    parse(text).unwrap_or_else(|| panic!("{text}"))
}

#[test]
fn preview_sorts_before_its_stable() {
    assert!(v("0.3.0-preview.9") < v("0.3.0"));
    assert!(v("0.3.0") > v("0.2.9"));
    assert_eq!(v("0.3.0"), v("0.3.0"));
}

#[test]
fn previews_order_by_number() {
    assert!(v("0.3.0-preview.2") < v("0.3.0-preview.10"));
    assert_eq!(v("0.3.0-preview.1"), v("0.3.0-preview.1"));
}

#[test]
fn next_minor_preview_is_newer_than_the_stable() {
    assert!(v("0.4.0-preview.1") > v("0.3.0"));
    assert!(v("0.3.0-preview.3") > v("0.2.0"));
}

#[test]
fn malformed_prerelease_is_refused() {
    for bad in [
        "0.3.0-preview",
        "0.3.0-preview.01",
        "0.3.0-beta.1",
        "0.3.0-preview.1.2",
        "0.3.0-preview.",
        "0.3.0-preview.+1",
    ] {
        assert_eq!(parse(bad), None, "{bad}");
    }
}
