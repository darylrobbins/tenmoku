//! `EditorModel::save` writes a buffer back in the encoding it was read in, and
//! a refused save leaves the file alone.

use fe_buffer::Buffer;
use fe_editor::EditorModel;

const FRENCH: &str = "Le café est très bon. Une idée naïve, déjà vue à côté de l'église.\n\
                      Café naïve: élève, garçon, où, être, fenêtre, hôtel, Noël.\n";

/// windows-1252 bytes for text within Latin-1's printable range, which it
/// encodes as the code point itself.
fn windows_1252(text: &str) -> Vec<u8> {
    text.chars()
        .map(|ch| {
            let code = u32::from(ch);
            assert!(code < 0x80 || (0xA0..0x100).contains(&code), "{ch:?}");
            code as u8
        })
        .collect()
}

fn open(path: &std::path::Path) -> EditorModel {
    let file = std::fs::File::open(path).unwrap();
    let buffer = Buffer::from_reader_detecting(file).unwrap();
    assert_eq!(buffer.encoding().label(), "windows-1252");
    EditorModel::new(buffer, Some(path.to_path_buf()))
}

fn leftover_temp_files(dir: &std::path::Path) -> Vec<String> {
    std::fs::read_dir(dir)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|name| name.ends_with(".fastedit-save"))
        .collect()
}

#[test]
fn saves_in_the_encoding_read() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("french.txt");
    std::fs::write(&path, windows_1252(FRENCH)).unwrap();

    let mut model = open(&path);
    model.insert("Noël à côté. ");
    assert!(model.is_dirty());
    model.save().unwrap();
    assert!(!model.is_dirty());
    let expected = windows_1252(&format!("Noël à côté. {FRENCH}"));
    assert!(std::fs::read(&path).unwrap() == expected);
}

#[test]
fn a_refused_save_leaves_the_file_and_dirty_flag() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("french.txt");
    let original = windows_1252(FRENCH);
    std::fs::write(&path, &original).unwrap();

    let mut model = open(&path);
    model.insert("😀");
    let error = model.save().unwrap_err();
    assert_eq!(error.kind(), std::io::ErrorKind::InvalidData);
    assert!(std::fs::read(&path).unwrap() == original);
    assert_eq!(leftover_temp_files(dir.path()), Vec::<String>::new());
    assert!(model.is_dirty());
}
