//! Targets in files never opened, mapped against the text the editor loads
//! (`Buffer::from_reader_detecting`), not a lossy UTF-8 reading of the bytes.

use std::sync::atomic::AtomicBool;

use fe_buffer::Buffer;
use fe_nav::Location;
use serde_json::json;

use super::{Nav, location, range};
use crate::{LspNavigation, ReadinessMode};

/// Writes `bytes` to `src/<name>` and has the server answer a definition, a
/// references reply and a workspace symbol at row 1, columns 3..9. Returns
/// the three locations, and the text the editor would load.
fn locations_in(name: &str, bytes: &[u8]) -> ([Location; 3], Buffer) {
    let s = Nav::ready(ReadinessMode::ServerStatus);
    let rel = format!("src/{name}");
    std::fs::write(s.root.join(&rel), bytes).unwrap();
    let at = location(&s.uri(&rel), range(1, 3, 9));
    s.reply("textDocument/definition", json!([at]));
    s.reply("textDocument/references", json!([at]));
    s.reply(
        "workspace/symbol",
        json!([{ "name": "target", "kind": 12, "location": at }]),
    );
    let cancel = AtomicBool::new(false);
    let req = s.request();

    let definition = s.navigation.definitions(&req, &cancel).unwrap();
    assert_eq!(definition.provider, LspNavigation::NAME);
    let definition = definition.value.items[0].location.clone();

    let mut files = Vec::new();
    let references = s
        .navigation
        .references(&req, &cancel, &mut |batch| files.extend(batch))
        .unwrap();
    assert_eq!(references.provider, LspNavigation::NAME);
    let reference = (files.iter())
        .find(|file| file.rel == std::path::Path::new(&rel))
        .expect("the reference in the unopened file")
        .refs[0]
        .location
        .clone();

    let symbols = s
        .navigation
        .workspace_symbols("target", 10, &cancel)
        .unwrap();
    assert_eq!(symbols.provider, LspNavigation::NAME);
    let symbol = symbols.value[0].location.clone();

    let loaded = Buffer::from_reader_detecting(bytes).unwrap();
    ([definition, reference, symbol], loaded)
}

fn assert_selects_target(name: &str, bytes: &[u8]) -> Buffer {
    let (locations, loaded) = locations_in(name, bytes);
    let text = loaded.text().to_string();
    let target = text.find("target").unwrap();
    for location in locations {
        assert_eq!(location.rel, std::path::Path::new(&format!("src/{name}")));
        assert_eq!(location.row, 1);
        assert_eq!(&text[location.range.clone()], "target");
        assert_eq!(location.range, target..target + 6);
    }
    loaded
}

#[test]
fn targets_in_a_bom_file_select_the_word() {
    let mut bytes = vec![0xEF, 0xBB, 0xBF];
    bytes.extend_from_slice(b"// heading\nfn target() {}\n");
    let loaded = assert_selects_target("bom.rs", &bytes);
    assert_eq!(loaded.text().to_string().find("target"), Some(14));
}

#[test]
fn targets_in_a_windows_1252_file_select_the_word() {
    // "// café naïve", é = E9, ï = EF.
    let mut bytes = b"// caf\xE9 na\xEFve\n".to_vec();
    bytes.extend_from_slice(b"fn target() {}\n");
    let detected = Buffer::from_reader_detecting(bytes.as_slice()).unwrap();
    assert_eq!(detected.encoding().name(), "windows-1252");
    assert_selects_target("latin.rs", &bytes);
}
