// SPDX-License-Identifier: Apache-2.0

use std::{collections::BTreeSet, path::PathBuf, sync::Arc};

use gpui::{AssetSource, SvgRenderer};

use crate::{Assets, Icon};

#[test]
fn every_icon_is_bundled_and_parses() {
    let names: BTreeSet<&str> = Icon::ALL.iter().map(|icon| icon.name()).collect();
    assert_eq!(names.len(), Icon::ALL.len(), "no two icons share a name");
    let renderer = SvgRenderer::new(Arc::new(Assets));
    for icon in Icon::ALL {
        let bytes = Assets
            .load(icon.path())
            .expect("load")
            .unwrap_or_else(|| panic!("{icon:?} isn't bundled"));
        assert!(renderer.parse_svg(&bytes).is_ok(), "{icon:?} doesn't parse");
    }
    assert!(matches!(Assets.load("kiln-ui/icons/nope.svg"), Ok(None)));
    assert!(matches!(Assets.load("icons/file.svg"), Ok(None)));

    let listed: BTreeSet<String> = Assets
        .list("kiln-ui/icons/")
        .expect("list")
        .iter()
        .map(|p| p.to_string())
        .collect();
    let expected: BTreeSet<String> = Icon::ALL.iter().map(|i| i.path().to_owned()).collect();
    assert_eq!(listed, expected);
    assert_eq!(
        Assets.list("kiln-ui/icons/").expect("list").len(),
        Icon::ALL.len()
    );
    assert!(Assets.list("other/").expect("list").is_empty());
}

#[test]
fn bundled_files_match_the_enum() {
    // At run time, not `env!`: a shared target can keep another checkout's path.
    let dir = std::env::var_os("CARGO_MANIFEST_DIR")
        .map_or_else(|| env!("CARGO_MANIFEST_DIR").into(), PathBuf::from)
        .join("assets/icons");
    let mut on_disk = BTreeSet::new();
    for entry in std::fs::read_dir(&dir).expect("assets/icons") {
        let path = entry.expect("entry").path();
        if path.extension().is_some_and(|e| e == "svg") {
            on_disk.insert(path);
        }
    }
    let names: BTreeSet<String> = on_disk
        .iter()
        .map(|p| p.file_stem().expect("stem").to_string_lossy().into_owned())
        .collect();
    let expected: BTreeSet<String> = Icon::ALL.iter().map(|i| i.name().to_owned()).collect();
    assert_eq!(names, expected);

    for icon in Icon::ALL {
        let file = std::fs::read(dir.join(format!("{}.svg", icon.name()))).expect("read");
        let served = Assets.load(icon.path()).expect("load").expect("bundled");
        assert_eq!(file, *served, "{icon:?} differs from its file");
    }

    let license = std::fs::read_to_string(dir.join("LICENSE")).expect("LICENSE");
    assert!(license.contains("ISC License"));
    assert!(license.contains("Cole Bemis"));
}
