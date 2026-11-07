//! The author guide against the code it describes.

use std::fs;
use std::path::{Path, PathBuf};

use kiln_command::Capability;
use kiln_plugin::limits::{CALL_DEADLINE, EVENT_QUEUE, MAX_READ_BYTES, MEMORY_BYTES};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn read(path: &str) -> String {
    fs::read_to_string(root().join(path)).unwrap_or_else(|e| panic!("{path}: {e}"))
}

fn guide() -> String {
    read("docs/plugins/authoring.md")
}

/// The `package` line of every WIT file in `dir`, as `name@version`.
fn packages(dir: &str) -> Vec<String> {
    let mut out = Vec::new();
    for entry in fs::read_dir(root().join(dir)).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().is_some_and(|e| e == "wit") {
            let text = fs::read_to_string(path).unwrap();
            let line = text
                .lines()
                .find_map(|l| l.strip_prefix("package "))
                .unwrap();
            out.push(line.trim_end_matches(';').to_string());
        }
    }
    out.sort();
    out.dedup();
    out
}

#[test]
fn guide_matches_the_code() {
    let guide = guide();
    assert!(guide.contains("extensions.plugins_preview"));

    for dir in ["kiln/crates/kiln-plugin/wit", "crates/fe-plugin/wit"] {
        let packages = packages(dir);
        assert_eq!(packages.len(), 1, "{dir}: {packages:?}");
        assert!(
            guide.contains(&packages[0]),
            "the guide lacks {}",
            packages[0]
        );
    }

    let ms = CALL_DEADLINE.as_millis();
    let mib = |n: usize| n >> 20;
    for stated in [
        format!("{ms} ms per call"),
        format!("{} MiB per plugin", mib(MEMORY_BYTES)),
        format!("{EVENT_QUEUE} events wait"),
        format!("{} MiB for one buffer read", mib(MAX_READ_BYTES)),
    ] {
        assert!(guide.contains(&stated), "the guide lacks {stated:?}");
    }

    // Every capability in the table parses.
    let mut listed = 0;
    for line in guide.lines().filter(|l| l.starts_with("| `")) {
        let cell = line.split('`').nth(1).unwrap();
        let capability = cell
            .replace("<program>", "rg")
            .replace("<host>", "example.com");
        Capability::parse(&capability).unwrap_or_else(|e| panic!("{e}"));
        listed += 1;
    }
    assert_eq!(listed, 9);

    assert!(
        root()
            .join("crates/tenmoku-plugin/template/extension.toml")
            .exists()
    );
    assert!(guide.contains("crates/tenmoku-plugin/template/"));

    // The quick start names each file of the template that spells its name.
    let template = root().join("crates/tenmoku-plugin/template");
    for file in ["extension.toml", "Cargo.toml", "build.zsh", "src/lib.rs"] {
        let text = fs::read_to_string(template.join(file)).unwrap();
        let spells_name = text.contains("word_count") || text.contains("word-count");
        assert!(spells_name, "{file} no longer spells the template's name");
        assert!(
            guide.contains(&format!("`{file}`")),
            "the guide skips {file}"
        );
    }
    let lib = fs::read_to_string(template.join("src/lib.rs")).unwrap();
    let constant = lib.lines().find(|l| l.starts_with("const COUNT")).unwrap();
    let command_id = constant.split('"').nth(1).unwrap();
    assert!(
        guide.contains(&format!("`\"{command_id}\"`")),
        "the guide lacks {command_id}"
    );

    let record = read("docs/decisions/vmtnmkay62-extensions-and-plugins.md");
    assert!(record.contains("Stage 2 decisions (user, 2026-11-03)"));
    assert!(record.contains("extensions.plugins_preview"));
}
