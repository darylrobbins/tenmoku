//! A C++ file's `#include` lines rank their headers `Imported`, as a C file's do.

use std::{
    fs,
    path::{Path, PathBuf},
    sync::{Arc, RwLock, atomic::AtomicBool},
    time::Duration,
};

use fe_buffer::Text;
use fe_nav::{
    BuildOptions, NavRequest, NavigationProvider, SearchNavigation, SymbolIndex, Tier, build,
};

fn write(root: &Path, rel: &str, text: &str) {
    let path = root.join(rel);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
}

/// Search over a complete index of `root`.
fn indexed(root: &Path) -> SearchNavigation {
    let mut index = SymbolIndex::new(root);
    build(
        root,
        &BuildOptions { threads: 1 },
        Arc::new(AtomicBool::new(false)),
        |batch| index.extend(batch),
    );
    index.finish();
    SearchNavigation::new(root, Arc::new(RwLock::new(index))).with_budget(Duration::from_secs(60))
}

/// Builds the two-header project with `main`, `header` and `other` names,
/// then asks for the definition of `area(2, 3)` in main.
fn check(main: &str, language: &'static str, header: &str, other: &str) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let include = header.rsplit('/').next().unwrap();
    let text = format!("#include \"{include}\"\n\nint main() {{\n    return area(2, 3);\n}}\n");
    let def = "int area(int w, int h) { return w * h; }\n";
    write(root, main, &text);
    write(root, header, def);
    write(root, other, def);
    let nav = indexed(root);
    let offset = text.find("area(2").unwrap();
    let req = NavRequest {
        root: root.to_path_buf(),
        rel: PathBuf::from(main),
        language: Some(language),
        text: Text::new(&text),
        offset,
        buffer: None,
    };
    let found = nav.definitions(&req, &AtomicBool::new(false)).unwrap();
    let got: Vec<(String, Tier)> = (found.items.iter())
        .map(|d| (d.location.rel.display().to_string(), d.tier))
        .collect();
    assert_eq!(
        got,
        vec![
            (header.to_string(), Tier::Imported),
            (other.to_string(), Tier::Project)
        ],
        "{language}"
    );
    assert!(found.unambiguous, "{language}");
}

#[test]
fn cpp_includes_rank_their_headers_imported() {
    check("src/main.c", "c", "src/geometry.h", "src/zzz/shapes.h");
    check(
        "src/main.cpp",
        "cpp",
        "src/geometry.hpp",
        "src/zzz/shapes.hpp",
    );
}
