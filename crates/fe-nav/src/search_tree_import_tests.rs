//! JavaScript and TypeScript imports read from a syntax tree: the request
//! text's own, or one of its start when it has none.

use std::{cell::Cell, rc::Rc};

use super::*;

/// A project with `helper` defined in each of `files`.
fn project(root: &Path, files: &[&str]) -> SearchNavigation {
    let mut index = SymbolIndex::new(root);
    index.extend(
        (files.iter())
            .map(|rel| FileSymbols {
                rel: PathBuf::from(rel),
                language: "typescript",
                len: 0,
                modified: None,
                symbols: vec![crate::Symbol {
                    name: "helper".into(),
                    kind: OutlineKind::Function,
                    name_range: 0..6,
                    range: 0..6,
                    row: 0,
                    container: None,
                }],
                truncated: false,
            })
            .collect(),
    );
    index.finish();
    SearchNavigation::new(root, Arc::new(RwLock::new(index))).with_budget(Duration::from_secs(60))
}

fn request(root: &Path, text: &str) -> NavRequest {
    NavRequest {
        root: root.to_path_buf(),
        rel: PathBuf::from("main.ts"),
        language: Some("typescript"),
        offset: text.rfind("helper").unwrap(),
        text: Text::new(text),
    }
}

#[test]
fn a_request_too_large_to_parse_ranks_by_its_prefix_imports() {
    let dir = tempfile::tempdir().unwrap();
    // `a/helper.ts` would sort first if nothing were imported.
    let nav = project(dir.path(), &["a/helper.ts", "util.ts"]);
    let limit = language_by_name("typescript").unwrap().parse_limit();
    let mut text = String::from("import { helper } from './util'\n");
    text.push_str(&"// padding\n".repeat(limit / 11 + 1));
    text.push_str("helper();\n");
    assert!(text.len() > limit);
    let found = nav
        .definitions(&request(dir.path(), &text), &AtomicBool::new(false))
        .unwrap();
    let first = &found.items[0];
    assert_eq!(first.location.rel, Path::new("util.ts"));
    assert_eq!(first.tier, Tier::Imported);
    assert_eq!(found.items[1].tier, Tier::Project);
}

#[test]
fn a_cancel_during_the_import_query_is_answered() {
    let dir = tempfile::tempdir().unwrap();
    let ranked = Arc::new(AtomicBool::new(false));
    let nav = project(dir.path(), &["util.ts"]).with_on_phase({
        let ranked = ranked.clone();
        move |phase| {
            if phase == Phase::Rank {
                ranked.store(true, Ordering::Relaxed);
            }
        }
    });
    let mut text = "require('./util');\n".repeat(8 * CANCEL_EVERY);
    text.push_str("helper();\n");
    // The flag is set by the query's third look at it, partway through.
    let cancel = Arc::new(AtomicBool::new(false));
    let polls = Rc::new(Cell::new(0));
    QUERY_POLLED.with(|slot| {
        let (cancel, polls) = (cancel.clone(), polls.clone());
        *slot.borrow_mut() = Some(Box::new(move || {
            polls.set(polls.get() + 1);
            if polls.get() == 3 {
                cancel.store(true, Ordering::Relaxed);
            }
        }));
    });
    let answer = nav.definitions(&request(dir.path(), &text), &cancel);
    QUERY_POLLED.with(|slot| *slot.borrow_mut() = None);
    assert_eq!(answer, Err(NavError::Cancelled));
    // The query stopped at once, long before reading every call, and
    // nothing was ranked.
    assert_eq!(polls.get(), 3);
    assert!(!ranked.load(Ordering::Relaxed));
}
