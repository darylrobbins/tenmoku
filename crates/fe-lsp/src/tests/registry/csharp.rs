//! csharp-ls, the built-in C# server, and the `*.ext` root markers it
//! roots with (docs/decisions/2pj9dv7s78-csharp-server.md).

use std::fs;
use std::path::{Path, PathBuf};

use fe_buffer::Buffer;
use fe_config::settings::LspSettings;
use serde_json::json;

use super::{open, root, routed, server, strings, uri};
use crate::document::language_id;
use crate::registry::{self, Registry, server_for, server_root};
use crate::tests::ROOT;

const CSHARP_LS: &str = "csharp-ls";
const MARKERS: [&str; 4] = ["*.sln", "*.slnx", "*.csproj", ".git"];

fn touch(path: &Path) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, "").unwrap();
}

#[test]
fn csharp_gets_csharp_ls_by_default() {
    let specs = Registry::default().specs();
    let csharp = server_for(&specs, "csharp").unwrap();
    assert_eq!(csharp.name, CSHARP_LS);
    assert_eq!(csharp.command.to_str(), Some("csharp-ls"));
    assert!(csharp.args.is_empty());
    assert_eq!(csharp.languages, strings(&["csharp"]));
    assert_eq!(csharp.root_markers, strings(&MARKERS));
    let claiming: Vec<_> = registry::builtin_specs()
        .into_iter()
        .filter(|s| s.languages.iter().any(|l| l == "csharp"))
        .map(|s| s.name)
        .collect();
    assert_eq!(claiming, [CSHARP_LS]);
}

#[test]
fn csharp_language_id_passes_through_and_the_fake_server_gets_didopen() {
    let path = PathBuf::from(ROOT).join("Program.cs");
    assert_eq!(language_id("csharp", &path), "csharp");

    let (fake, mut manager, _events) = routed();
    manager.update_settings(&LspSettings::default());
    let mut buffer = Buffer::new("class A {}\n");
    let name = format!("{ROOT}/Program.cs");
    open(&mut manager, root(), &mut buffer, &name, "csharp").expect("a server takes it");
    fake.wait_for_document(&uri(&name), "didOpen", |r| r.is_some());
    assert_eq!(fake.document(&uri(&name)).unwrap().language_id, "csharp");
}

#[test]
fn settings_can_disable_or_override_csharp_ls() {
    let mut registry = Registry::default();
    let mut settings = LspSettings::default();
    settings
        .servers
        .insert(CSHARP_LS.into(), server(json!({ "enabled": false })));
    registry.set_settings(settings);
    assert!(server_for(&registry.specs(), "csharp").is_none());

    let mut settings = LspSettings::default();
    settings.servers.insert(
        CSHARP_LS.into(),
        server(json!({ "command": "/opt/csharp-ls" })),
    );
    registry.set_settings(settings);
    let specs = registry.specs();
    let csharp = server_for(&specs, "csharp").unwrap();
    assert_eq!(csharp.name, CSHARP_LS);
    assert_eq!(csharp.command.to_str(), Some("/opt/csharp-ls"));
    assert!(csharp.args.is_empty());
    assert_eq!(csharp.languages, strings(&["csharp"]));
    assert_eq!(csharp.root_markers, strings(&MARKERS));
}

#[test]
fn server_root_matches_extension_globs() {
    let dir = tempfile::tempdir().unwrap();
    let markers = registry::builtin_specs()
        .into_iter()
        .find(|s| s.name == CSHARP_LS)
        .unwrap()
        .root_markers;
    let outside = dir.path().join("window");

    // A solution above beats the nearer project; with no solution, the
    // project's folder.
    let src = dir.path().join("solution/src");
    let app = src.join("App");
    let file = app.join("Program.cs");
    touch(&file);
    touch(&app.join("App.csproj"));
    assert_eq!(server_root(&file, Some(&outside), &markers), app);
    touch(&src.join("Repo.sln"));
    assert_eq!(server_root(&file, Some(&outside), &markers), src);

    // An .slnx counts as a solution too.
    let slnx = dir.path().join("slnx");
    let file = slnx.join("Lib/Lib.cs");
    touch(&file);
    touch(&slnx.join("Lib/Lib.csproj"));
    touch(&slnx.join("All.slnx"));
    assert_eq!(server_root(&file, Some(&outside), &markers), slnx);

    // A tree with only .git roots there.
    let git = dir.path().join("git");
    let file = git.join("tools/Script.cs");
    touch(&file);
    fs::create_dir_all(git.join(".git")).unwrap();
    assert_eq!(server_root(&file, Some(&outside), &markers), git);

    // A literal marker still matches literally.
    let cargo = dir.path().join("cargo");
    let file = cargo.join("src/main.rs");
    touch(&file);
    touch(&cargo.join("Cargo.toml"));
    let literal = strings(&["Cargo.toml"]);
    assert_eq!(server_root(&file, Some(&outside), &literal), cargo);
    touch(&cargo.join("src/Cargo.toml.bak"));
    assert_eq!(server_root(&file, Some(&outside), &literal), cargo);

    // `*.sln` wants a name ending in `.sln` after a non-empty stem: a folder
    // so named counts, a file `sln`, `.sln` or `a.slnx.bak` doesn't, and
    // case matters.
    let sln = strings(&["*.sln"]);
    let globbed = dir.path().join("globbed");
    let near = globbed.join("near");
    let file = near.join("A.cs");
    touch(&file);
    for name in ["sln", ".sln", "a.slnx.bak", "a.slnx", "A.SLN"] {
        touch(&near.join(name));
    }
    assert_eq!(server_root(&file, Some(&outside), &sln), near);
    fs::create_dir_all(globbed.join("x.sln")).unwrap();
    assert_eq!(server_root(&file, Some(&outside), &sln), globbed);

    // Under the window root, the window root still wins.
    assert_eq!(server_root(&file, Some(&globbed), &markers), globbed);
    assert_eq!(server_root(&file, Some(&near), &sln), near);
    let inside = outside.join("Deep/App/Program.cs");
    touch(&inside);
    touch(&outside.join("Deep/App/App.csproj"));
    assert_eq!(server_root(&inside, Some(&outside), &markers), outside);
}
