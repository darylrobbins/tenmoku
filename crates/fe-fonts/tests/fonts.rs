//! Each bundled file is the face [`fe_fonts::FACES`] lists it as, and every
//! font file in `fonts/` is listed.

use std::path::Path;

use fe_fonts::{CODE, FACES, TITLE, UI};
use ttf_parser::{Face, name_id};

/// The name table's English (or first readable) string for `id`.
fn name(face: &Face, id: u16) -> Option<String> {
    let names: Vec<_> = face
        .names()
        .into_iter()
        .filter(|n| n.name_id == id)
        .collect();
    names
        .iter()
        .filter(|n| n.language() == ttf_parser::Language::English_UnitedStates)
        .chain(&names)
        .find_map(|n| n.to_string())
}

#[test]
fn each_file_names_its_family_and_weight() {
    let mut failures = Vec::new();
    for listed in FACES {
        assert!(
            [UI, CODE, TITLE].contains(&listed.family),
            "{}: listed under {:?}, not one of the three families",
            listed.file,
            listed.family
        );
        let face = match Face::parse(listed.bytes, 0) {
            Ok(face) => face,
            Err(e) => {
                failures.push(format!("{}: doesn't parse: {e}", listed.file));
                continue;
            }
        };
        // The typographic family when the file has one (a Medium's legacy
        // family is "Instrument Sans Medium"), else the legacy one.
        let family =
            name(&face, name_id::TYPOGRAPHIC_FAMILY).or_else(|| name(&face, name_id::FAMILY));
        if family.as_deref() != Some(listed.family) {
            failures.push(format!(
                "{}: names its family {family:?}, listed as {:?}",
                listed.file, listed.family
            ));
        }
        let weight = face.weight().to_number();
        if weight != listed.weight {
            failures.push(format!(
                "{}: weight class {weight}, listed as {}",
                listed.file, listed.weight
            ));
        }
        if face.is_italic() != listed.italic {
            failures.push(format!(
                "{}: italic {}, listed as {}",
                listed.file,
                face.is_italic(),
                listed.italic
            ));
        }
        if face.is_variable() {
            failures.push(format!(
                "{}: a variable font; GPUI registers it at its default weight only",
                listed.file
            ));
        }
    }
    assert!(failures.is_empty(), "\n{}", failures.join("\n"));
}

#[test]
fn every_font_file_is_listed() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("fonts");
    let mut on_disk = Vec::new();
    for family in std::fs::read_dir(&dir).unwrap() {
        let family = family.unwrap().path();
        if !family.is_dir() {
            continue;
        }
        for file in std::fs::read_dir(&family).unwrap() {
            let file = file.unwrap().path();
            if matches!(
                file.extension().and_then(|e| e.to_str()),
                Some("ttf" | "otf")
            ) {
                on_disk.push(
                    file.strip_prefix(&dir)
                        .unwrap()
                        .to_str()
                        .unwrap()
                        .to_string(),
                );
            }
        }
        assert!(
            family.join("OFL.txt").is_file(),
            "{}: no OFL.txt beside the family's files",
            family.display()
        );
    }
    on_disk.sort();
    let mut listed: Vec<String> = FACES.iter().map(|f| f.file.to_string()).collect();
    listed.sort();
    assert_eq!(on_disk, listed, "fonts/ and FACES differ");
    assert_eq!(fe_fonts::all().len(), FACES.len());
}
