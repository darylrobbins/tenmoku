//! Languages picked by hand, remembered per project in
//! `<state_dir>/languages/<key>.toml`: they round-trip, land from many
//! writers at once under the lock, are found from the nearest project
//! naming the file or the rootless list, and a corrupt list reads as empty.

use std::fs;
use std::path::Path;

use fe_config::language_choices::{
    self, LanguageChoices, choices_file, forget, remember, remembered,
};

#[test]
fn a_choice_round_trips_and_plain_text_is_kept() {
    let state = tempfile::tempdir().unwrap();
    let project = tempfile::tempdir().unwrap();
    let root = project.path();
    let (a, b) = (root.join("a.zz"), root.join("sub/b.txt"));
    fs::write(&a, "x").unwrap();

    assert_eq!(remembered(state.path(), &a), None);
    remember(state.path(), Some(root), &a, Some("python")).unwrap();
    remember(state.path(), Some(root), &b, None).unwrap();
    assert_eq!(remembered(state.path(), &a), Some(Some("python".into())));
    assert_eq!(remembered(state.path(), &b), Some(None));

    // Kept in the project's list, by absolute path.
    let list = choices_file(state.path(), Some(root));
    assert!(list.starts_with(state.path().join("languages")));
    let (choices, error) = LanguageChoices::load(&list);
    assert!(error.is_none());
    assert_eq!(choices.len(), 2);
    assert_eq!(
        choices.get(&language_choices::key(&a)),
        Some(Some("python"))
    );

    // Picked again, it's replaced; forgotten, it's gone.
    remember(state.path(), Some(root), &a, Some("rust")).unwrap();
    assert_eq!(remembered(state.path(), &a), Some(Some("rust".into())));
    forget(state.path(), Some(root), &a).unwrap();
    assert_eq!(remembered(state.path(), &a), None);
    assert_eq!(remembered(state.path(), &b), Some(None));
}

#[test]
fn rootless_windows_and_files_outside_the_root_share_the_empty_key() {
    let state = tempfile::tempdir().unwrap();
    let project = tempfile::tempdir().unwrap();
    let elsewhere = tempfile::tempdir().unwrap();
    let file = elsewhere.path().join("notes.zz");
    fs::write(&file, "x").unwrap();

    remember(state.path(), None, &file, Some("json")).unwrap();
    assert!(choices_file(state.path(), None).exists());
    assert_eq!(remembered(state.path(), &file), Some(Some("json".into())));

    // Outside the window's root, it goes to the rootless list too.
    remember(state.path(), Some(project.path()), &file, Some("xml")).unwrap();
    assert!(!choices_file(state.path(), Some(project.path())).exists());
    assert_eq!(remembered(state.path(), &file), Some(Some("xml".into())));
}

#[test]
fn the_nearest_project_naming_the_file_wins() {
    let state = tempfile::tempdir().unwrap();
    let outer = tempfile::tempdir().unwrap();
    let inner = outer.path().join("inner");
    let file = inner.join("a.zz");
    fs::create_dir_all(&inner).unwrap();
    fs::write(&file, "x").unwrap();

    remember(state.path(), None, &file, Some("json")).unwrap();
    remember(state.path(), Some(outer.path()), &file, Some("xml")).unwrap();
    assert_eq!(remembered(state.path(), &file), Some(Some("xml".into())));
    // A list of the inner project that doesn't name it is passed over.
    let other = inner.join("b.zz");
    remember(state.path(), Some(&inner), &other, Some("rust")).unwrap();
    assert_eq!(remembered(state.path(), &file), Some(Some("xml".into())));
    remember(state.path(), Some(&inner), &file, Some("python")).unwrap();
    assert_eq!(remembered(state.path(), &file), Some(Some("python".into())));

    // Forgetting it in one window forgets it everywhere it'd be found.
    forget(state.path(), Some(&inner), &file).unwrap();
    assert_eq!(remembered(state.path(), &file), None);
    assert_eq!(remembered(state.path(), &other), Some(Some("rust".into())));
}

#[test]
fn a_root_through_a_symlink_is_the_same_project() {
    let state = tempfile::tempdir().unwrap();
    let project = tempfile::tempdir().unwrap();
    let file = project.path().join("a.zz");
    fs::write(&file, "x").unwrap();
    let link = state.path().join("link");
    #[cfg(unix)]
    std::os::unix::fs::symlink(project.path(), &link).unwrap();
    #[cfg(windows)]
    std::os::windows::fs::symlink_dir(project.path(), &link).unwrap();

    remember(state.path(), Some(&link), &link.join("a.zz"), Some("json")).unwrap();
    assert_eq!(
        choices_file(state.path(), Some(&link)),
        choices_file(state.path(), Some(project.path()))
    );
    assert_eq!(remembered(state.path(), &file), Some(Some("json".into())));
}

#[test]
fn concurrent_writers_all_land_under_the_lock() {
    let state = tempfile::tempdir().unwrap();
    let project = tempfile::tempdir().unwrap();
    let root = project.path().to_owned();
    let threads: Vec<_> = (0..8)
        .map(|i| {
            let (state, root) = (state.path().to_owned(), root.clone());
            std::thread::spawn(move || {
                for j in 0..5 {
                    let file = root.join(format!("f{i}-{j}.zz"));
                    remember(&state, Some(&root), &file, Some("json")).unwrap();
                }
            })
        })
        .collect();
    for thread in threads {
        thread.join().unwrap();
    }
    let (choices, error) = LanguageChoices::load(&choices_file(state.path(), Some(&root)));
    assert!(error.is_none());
    assert_eq!(choices.len(), 40);
}

#[test]
fn a_corrupt_list_reads_as_empty_and_is_replaced_on_change() {
    let state = tempfile::tempdir().unwrap();
    let project = tempfile::tempdir().unwrap();
    let root = project.path();
    let file = root.join("a.zz");
    let list = choices_file(state.path(), Some(root));
    fs::create_dir_all(list.parent().unwrap()).unwrap();
    fs::write(&list, "files = [not toml").unwrap();

    let (choices, error) = LanguageChoices::load(&list);
    assert!(choices.is_empty());
    assert_eq!(error.unwrap().kind(), std::io::ErrorKind::InvalidData);
    assert_eq!(remembered(state.path(), &file), None);
    // Reading leaves it as it is.
    assert_eq!(fs::read_to_string(&list).unwrap(), "files = [not toml");

    remember(state.path(), Some(root), &file, Some("json")).unwrap();
    assert_eq!(remembered(state.path(), &file), Some(Some("json".into())));
    assert!(LanguageChoices::load(&list).1.is_none());
}

#[test]
fn no_lists_is_no_choice() {
    let state = tempfile::tempdir().unwrap();
    assert_eq!(remembered(state.path(), Path::new("/a/b.zz")), None);
    forget(state.path(), None, Path::new("/a/b.zz")).unwrap();
}
