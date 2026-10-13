//! `ScipNavigation::references` doesn't hold every referencing file's text
//! until the request ends: a counting allocator watches the peak heap.

use std::{
    alloc::{GlobalAlloc, Layout, System},
    fs,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
};

use fe_buffer::Text;
use fe_nav::{NavRequest, NavigationProvider, ScipIndex, ScipNavigation};
use protobuf::{EnumOrUnknown, Message};
use scip::types::{Document, Index, Occurrence, PositionEncoding, SymbolRole};

struct Counting;

static LIVE: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);

fn grow(bytes: usize) {
    let live = LIVE.fetch_add(bytes, Ordering::Relaxed) + bytes;
    PEAK.fetch_max(live, Ordering::Relaxed);
}

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let ptr = unsafe { System.alloc(layout) };
        if !ptr.is_null() {
            grow(layout.size());
        }
        ptr
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        let ptr = unsafe { System.alloc_zeroed(layout) };
        if !ptr.is_null() {
            grow(layout.size());
        }
        ptr
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) };
        LIVE.fetch_sub(layout.size(), Ordering::Relaxed);
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        let new = unsafe { System.realloc(ptr, layout, new_size) };
        if !new.is_null() {
            LIVE.fetch_sub(layout.size(), Ordering::Relaxed);
            grow(new_size);
        }
        new
    }
}

#[global_allocator]
static ALLOC: Counting = Counting;

const SYMBOL: &str = "npm pkg 1.0 m/foo.";
const FILES: usize = 64;
const FILLER: usize = 1 << 20;

fn occurrence(roles: i32) -> Occurrence {
    Occurrence {
        range: vec![0, 0, 3],
        symbol: SYMBOL.to_owned(),
        symbol_roles: roles,
        ..Default::default()
    }
}

fn document(rel: &str, roles: i32) -> Document {
    Document {
        language: "javascript".to_owned(),
        relative_path: rel.to_owned(),
        position_encoding: EnumOrUnknown::new(PositionEncoding::UTF8CodeUnitOffsetFromLineStart),
        occurrences: vec![occurrence(roles)],
        ..Default::default()
    }
}

#[test]
fn references_drop_each_file_once_reported() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    fs::write(root.join("main.js"), "foo;\n").unwrap();
    let mut documents = vec![document("main.js", SymbolRole::Definition as i32)];
    let body = format!("foo;\n{}", " ".repeat(FILLER));
    for n in 0..FILES {
        let rel = format!("f{n:02}.js");
        fs::write(root.join(&rel), &body).unwrap();
        documents.push(document(&rel, 0));
    }
    let bytes = Index {
        documents,
        ..Default::default()
    }
    .write_to_bytes()
    .unwrap();
    let nav = ScipNavigation::new(Arc::new(ScipIndex::from_bytes(&bytes).unwrap()), root);
    let req = NavRequest {
        root: root.to_path_buf(),
        rel: PathBuf::from("main.js"),
        language: Some("javascript"),
        text: Text::new("foo;\n"),
        offset: 0,
        buffer: None,
    };
    let cancel = AtomicBool::new(false);

    let mut delivered = 0;
    let before = LIVE.load(Ordering::Relaxed);
    PEAK.store(before, Ordering::Relaxed);
    nav.references(&req, &cancel, &mut |batch| delivered += batch.len())
        .unwrap();
    let growth = PEAK.load(Ordering::Relaxed).saturating_sub(before);

    assert_eq!(delivered, FILES + 1);
    assert!(growth < 16 << 20, "peak heap grew by {} MiB", growth >> 20);
}
