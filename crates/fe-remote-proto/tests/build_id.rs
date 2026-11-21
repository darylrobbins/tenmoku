//! `BUILD` is `<workspace version>+<commit>[-dirty]` (D § 2.5).

use fe_remote_proto::BUILD;

fn workspace_version() -> String {
    let manifest = include_str!("../../../Cargo.toml");
    let manifest: toml::Table = toml::from_str(manifest).unwrap();
    manifest["workspace"]["package"]["version"]
        .as_str()
        .unwrap()
        .to_owned()
}

#[test]
fn the_build_starts_with_the_workspace_version() {
    assert!(
        BUILD.starts_with(&format!("{}+", workspace_version())),
        "{BUILD}"
    );
}

#[test]
fn the_build_names_a_commit_after_the_version() {
    let (_, commit) = BUILD.split_once('+').unwrap();
    let commit = commit.strip_suffix("-dirty").unwrap_or(commit);
    let hex = commit.len() == 12 && commit.bytes().all(|b| b.is_ascii_hexdigit());
    let overridden = option_env!("TENMOKU_BUILD_COMMIT").map(str::trim) == Some(commit);
    assert!(hex || overridden, "{BUILD}");
}
