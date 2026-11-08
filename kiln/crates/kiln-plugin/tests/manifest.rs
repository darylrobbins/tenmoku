// SPDX-License-Identifier: Apache-2.0

//! Plugin manifests, written into temporary directories.

use std::{fs, path::Path};

use kiln_command::{Capability, Grants};
use kiln_core::{ApiVersion, ExtensionApi, Manifest, load_manifest};
use kiln_plugin::manifest::{ExtensionOrigin, PluginManifest, PluginManifestError, plugin_kind};

fn api() -> ExtensionApi {
    ExtensionApi {
        app: "notes".into(),
        version: ApiVersion { major: 1, minor: 0 },
        reserved: vec![],
        kinds: vec![plugin_kind()],
        allowed_licenses: None,
        only_kinds: None,
    }
}

fn dir_with(plugin: &str) -> tempdir::Dir {
    let dir = tempdir::Dir::new();
    let text = format!(
        "id = \"acme.tool\"\nname = \"Tool\"\nversion = \"1.0.0\"\nlicense = \"MIT\"\n\n\
         [engine]\nnotes = \"1.0\"\n\n{plugin}"
    );
    fs::write(dir.path().join("extension.toml"), text).unwrap();
    fs::write(dir.path().join("p.wasm"), b"\0asm").unwrap();
    dir
}

fn load(dir: &Path) -> Manifest {
    let (manifest, diagnostics) = load_manifest(dir, &api());
    manifest.unwrap_or_else(|| panic!("{diagnostics:?}"))
}

const PLUGIN: &str = "[contributes.plugin]\nwasm = \"p.wasm\"\n\
    capabilities = [\"buffer.read\", \"commands\", \"net:api.example.com\"]\n";

mod tempdir {
    use std::path::{Path, PathBuf};

    pub(crate) struct Dir(PathBuf);

    impl Dir {
        pub(crate) fn new() -> Self {
            Self::new_named("dir")
        }
        pub(crate) fn new_named(name: &str) -> Self {
            let path = std::env::temp_dir().join(format!(
                "kiln-plugin-manifest-{name}-{}-{:?}",
                std::process::id(),
                std::thread::current().id()
            ));
            std::fs::remove_dir_all(&path).ok();
            std::fs::create_dir_all(&path).unwrap();
            Self(path)
        }
        pub(crate) fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for Dir {
        fn drop(&mut self) {
            std::fs::remove_dir_all(&self.0).ok();
        }
    }
}

#[test]
fn a_plugin_entry_parses_wasm_and_capabilities() {
    let dir = dir_with(PLUGIN);
    let manifest = load(dir.path());
    let plugin = PluginManifest::from_extension(&manifest, ExtensionOrigin::User)
        .unwrap()
        .unwrap();
    let expected: Grants = [
        Capability::BufferRead,
        Capability::Commands,
        Capability::parse("net:api.example.com").unwrap(),
    ]
    .into_iter()
    .collect();
    assert_eq!(plugin.capabilities, expected);
    assert_eq!(plugin.id, "acme.tool");
    assert_eq!(
        plugin.wasm,
        kiln_core::platform::canonicalize(&dir.path().join("p.wasm")).unwrap()
    );
}

#[test]
fn an_unknown_capability_is_an_error_naming_it() {
    let dir =
        dir_with("[contributes.plugin]\nwasm = \"p.wasm\"\ncapabilities = [\"buffer.reed\"]\n");
    let manifest = load(dir.path());
    let error = PluginManifest::from_extension(&manifest, ExtensionOrigin::User).unwrap_err();
    assert!(error.to_string().contains("buffer.reed"), "{error}");
}

#[test]
fn the_wasm_path_must_be_a_file_inside_the_extension() {
    for path in ["../x.wasm", "/etc/passwd", "missing.wasm"] {
        let dir = dir_with(&format!("[contributes.plugin]\nwasm = {path:?}\n"));
        let manifest = load(dir.path());
        let error = PluginManifest::from_extension(&manifest, ExtensionOrigin::User).unwrap_err();
        assert!(
            matches!(error, PluginManifestError::WasmPath { .. }),
            "{path}: {error}"
        );
        assert!(error.to_string().contains(path), "{error}");
    }
}

#[test]
fn internal_parent_traversal_is_allowed() {
    let dir = dir_with("[contributes.plugin]\nwasm = \"components/../p.wasm\"\n");
    fs::create_dir(dir.path().join("components")).unwrap();
    let manifest = load(dir.path());
    let plugin = PluginManifest::from_extension(&manifest, ExtensionOrigin::User)
        .unwrap()
        .unwrap();
    assert!(plugin.wasm.ends_with("p.wasm"));
}

#[cfg(unix)]
#[test]
fn a_symlink_leading_outside_is_rejected() {
    let outside = tempdir::Dir::new_named("outside");
    fs::write(outside.path().join("o.wasm"), b"\0asm").unwrap();
    for (link, wasm) in [("evil.wasm", "evil.wasm"), ("dir", "dir/o.wasm")] {
        let target = if link == "dir" {
            outside.path().to_path_buf()
        } else {
            outside.path().join("o.wasm")
        };
        let dir = dir_with(&format!("[contributes.plugin]\nwasm = {wasm:?}\n"));
        std::os::unix::fs::symlink(&target, dir.path().join(link)).unwrap();
        let manifest = load(dir.path());
        let error = PluginManifest::from_extension(&manifest, ExtensionOrigin::User).unwrap_err();
        assert!(
            matches!(error, PluginManifestError::WasmPath { .. }),
            "{wasm}: {error}"
        );
    }
}

#[test]
fn a_workspace_extension_cannot_declare_a_plugin() {
    let dir = dir_with(PLUGIN);
    let manifest = load(dir.path());
    assert!(matches!(
        PluginManifest::from_extension(&manifest, ExtensionOrigin::Workspace),
        Err(PluginManifestError::WorkspaceCannotInstallPlugins { .. })
    ));
    assert!(
        PluginManifest::from_extension(&manifest, ExtensionOrigin::User)
            .unwrap()
            .is_some()
    );
    let plain = dir_with("");
    let manifest = load(plain.path());
    for origin in [ExtensionOrigin::User, ExtensionOrigin::Workspace] {
        assert!(
            PluginManifest::from_extension(&manifest, origin)
                .unwrap()
                .is_none()
        );
    }
}

#[test]
fn the_signature_slot_is_accepted_and_never_verified() {
    let dir = dir_with("[contributes.plugin]\nwasm = \"p.wasm\"\nsignature = \"abc123\"\n");
    let manifest = load(dir.path());
    let plugin = PluginManifest::from_extension(&manifest, ExtensionOrigin::User)
        .unwrap()
        .unwrap();
    assert_eq!(plugin.signature.as_deref(), Some("abc123"));
    assert!(!plugin.signature_verified());
}
