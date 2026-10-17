//! The `languageId` a document opens with is the LSP specification's.

use std::path::PathBuf;

use fe_buffer::Buffer;
use fe_config::settings::LspSettings;
use serde_json::json;

use super::{open, root, routed, server, uri};
use crate::document::language_id;
use crate::tests::ROOT;

#[test]
fn shell_and_make_documents_open_with_the_spec_language_ids() {
    let file = |name: &str| PathBuf::from(ROOT).join(name);
    let table = [
        ("bash", "a.sh", "shellscript"),
        ("bash", ".bashrc", "shellscript"),
        ("make", "Makefile", "makefile"),
        ("make", "rules.mk", "makefile"),
        ("rust", "a.rs", "rust"),
        ("python", "a.py", "python"),
        ("json", "a.json", "json"),
        ("javascript", "a.jsx", "javascriptreact"),
        ("tsx", "a.tsx", "typescriptreact"),
    ];
    for (language, name, id) in table {
        assert_eq!(language_id(language, &file(name)), id, "{name}");
    }

    let (fake, mut manager, _events) = routed();
    let mut settings = LspSettings::default();
    for (name, language) in [("bash-language-server", "bash"), ("make-server", "make")] {
        settings.servers.insert(
            name.into(),
            server(json!({ "command": name, "languages": [language] })),
        );
    }
    manager.update_settings(&settings);
    for (language, name, id) in [
        ("bash", "a.sh", "shellscript"),
        ("make", "Makefile", "makefile"),
    ] {
        let mut buffer = Buffer::new("x\n");
        let path = format!("{ROOT}/{name}");
        open(&mut manager, root(), &mut buffer, &path, language).unwrap();
        fake.wait_for_document(&uri(&path), "didOpen", |r| r.is_some());
        assert_eq!(fake.document(&uri(&path)).unwrap().language_id, id);
    }
}
