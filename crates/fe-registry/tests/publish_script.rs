//! Runs `scripts/test-publish-grammar-registry.sh`, the publish script's
//! fixture test against a fake `gh`, with this package's `grammar-registry`,
//! so the gate runs it whenever fe-registry is tested.

#![cfg(unix)]

use std::{path::Path, process::Command};

#[test]
fn test_publish_grammar_registry_passes() {
    // Read at run time: a path compiled in can name a deleted snapshot.
    let manifest = std::env::var_os("CARGO_MANIFEST_DIR").expect("run by cargo");
    let script = Path::new(&manifest).join("../../scripts/test-publish-grammar-registry.sh");
    let output = Command::new("bash")
        .arg(&script)
        .env("GRAMMAR_REGISTRY", env!("CARGO_BIN_EXE_grammar-registry"))
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        output.status.success(),
        "{} failed:\n{stdout}{}",
        script.display(),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!stdout.contains("FAIL"), "{stdout}");
}
