// SPDX-License-Identifier: LicenseRef-Proprietary

//! The component cache compiles once per component, and trusts only entries that are ours.

use std::{
    fs::{self, File},
    path::{Path, PathBuf},
    time::{Duration, SystemTime},
};

use kiln_plugin::cache::{ComponentCache, MAX_ENTRIES};
use wasmtime::{Config, Engine};

fn engine() -> Engine {
    let mut config = Config::new();
    config.wasm_component_model(true);
    Engine::new(&config).unwrap()
}

/// A component that differs from the others by `n`.
fn component(n: usize) -> Vec<u8> {
    format!("(component (core module $m (func (export \"f\") (result i32) i32.const {n})))")
        .into_bytes()
}

/// A directory of its own under cargo's temporary directory for tests.
fn temp_dir(name: &str) -> PathBuf {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("kiln-plugin-cache-{}-{name}", std::process::id()));
    fs::remove_dir_all(&dir).ok();
    dir
}

fn entries(dir: &Path) -> Vec<PathBuf> {
    fs::read_dir(dir)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "cwasm"))
        .collect()
}

#[test]
fn the_second_load_uses_the_cache() {
    let dir = temp_dir("second");
    let (cache, engine) = (ComponentCache::new(&dir), engine());
    cache.load(&engine, &component(1)).unwrap();
    cache.load(&engine, &component(1)).unwrap();
    assert_eq!(cache.compiled(), 1);
    assert_eq!(entries(&dir).len(), 1);
}

#[test]
fn changed_bytes_get_a_new_entry() {
    let dir = temp_dir("changed");
    let (cache, engine) = (ComponentCache::new(&dir), engine());
    cache.load(&engine, &component(1)).unwrap();
    cache.load(&engine, &component(2)).unwrap();
    assert_eq!(cache.compiled(), 2);
    assert_eq!(entries(&dir).len(), 2);
}

#[test]
fn a_corrupt_entry_is_recompiled_and_replaced() {
    let dir = temp_dir("corrupt");
    let (cache, engine) = (ComponentCache::new(&dir), engine());
    let bytes = component(1);
    cache.load(&engine, &bytes).unwrap();
    let path = cache.entry_path(&engine, &bytes);
    let whole = fs::metadata(&path).unwrap().len();
    File::options()
        .write(true)
        .open(&path)
        .unwrap()
        .set_len(whole / 2)
        .unwrap();
    cache.load(&engine, &bytes).unwrap();
    assert_eq!(cache.compiled(), 2);
    assert_eq!(fs::metadata(&path).unwrap().len(), whole);
    cache.load(&engine, &bytes).unwrap();
    assert_eq!(cache.compiled(), 2);
}

#[test]
fn an_entry_for_another_engine_config_is_ignored() {
    let dir = temp_dir("config");
    let (cache, engine) = (ComponentCache::new(&dir), engine());
    let bytes = component(1);
    let mut config = Config::new();
    config.wasm_component_model(true).epoch_interruption(true);
    let other = Engine::new(&config).unwrap();
    let foreign = wasmtime::component::Component::new(&other, &bytes)
        .unwrap()
        .serialize()
        .unwrap();
    fs::write(cache.entry_path(&engine, &bytes), foreign).unwrap();
    cache.load(&engine, &bytes).unwrap();
    assert_eq!(cache.compiled(), 1);
}

#[cfg(unix)]
#[test]
fn a_group_writable_directory_or_file_is_not_trusted() {
    use std::os::unix::fs::PermissionsExt;
    let dir = temp_dir("modes");
    let (cache, engine) = (ComponentCache::new(&dir), engine());
    let bytes = component(1);
    let chmod =
        |path: &Path, mode| fs::set_permissions(path, fs::Permissions::from_mode(mode)).unwrap();
    cache.load(&engine, &bytes).unwrap();
    let path = cache.entry_path(&engine, &bytes);

    chmod(&path, 0o666);
    cache.load(&engine, &bytes).unwrap();
    assert_eq!(cache.compiled(), 2);
    // The entry was replaced by one only we can write.
    cache.load(&engine, &bytes).unwrap();
    assert_eq!(cache.compiled(), 2);

    chmod(&dir, 0o777);
    cache.load(&engine, &bytes).unwrap();
    assert_eq!(cache.compiled(), 3);
    chmod(&dir, 0o700);
}

#[test]
fn the_cache_is_pruned_to_its_cap() {
    let dir = temp_dir("prune");
    let (cache, engine) = (ComponentCache::new(&dir), engine());
    let oldest = cache.entry_path(&engine, &component(0));
    let now = SystemTime::now();
    for n in 0..MAX_ENTRIES {
        cache.load(&engine, &component(n)).unwrap();
        // Age them apart, the first the oldest.
        let age = Duration::from_secs((MAX_ENTRIES - n) as u64 * 10);
        File::options()
            .write(true)
            .open(cache.entry_path(&engine, &component(n)))
            .unwrap()
            .set_modified(now - age)
            .unwrap();
    }
    assert_eq!(entries(&dir).len(), MAX_ENTRIES);
    cache.load(&engine, &component(MAX_ENTRIES)).unwrap();
    assert_eq!(entries(&dir).len(), MAX_ENTRIES);
    assert!(!oldest.exists());
}

#[cfg(unix)]
#[test]
fn an_unwritable_cache_directory_still_loads() {
    use std::os::unix::fs::PermissionsExt;
    let dir = temp_dir("readonly");
    fs::create_dir_all(&dir).unwrap();
    fs::set_permissions(&dir, fs::Permissions::from_mode(0o500)).unwrap();
    let (cache, engine) = (ComponentCache::new(&dir), engine());
    cache.load(&engine, &component(1)).unwrap();
    cache.load(&engine, &component(1)).unwrap();
    assert_eq!(cache.compiled(), 2);
    fs::set_permissions(&dir, fs::Permissions::from_mode(0o700)).unwrap();
}

#[test]
fn a_payload_changed_in_place_is_recompiled() {
    let dir = temp_dir("payload");
    let (cache, engine) = (ComponentCache::new(&dir), engine());
    let bytes = component(1);
    cache.load(&engine, &bytes).unwrap();
    let path = cache.entry_path(&engine, &bytes);
    let mut entry = fs::read(&path).unwrap();
    let last = entry.len() - 1;
    entry[last] ^= 0xff;
    fs::write(&path, entry).unwrap();
    cache.load(&engine, &bytes).unwrap();
    assert_eq!(cache.compiled(), 2);
}

#[test]
fn overlapping_stores_publish_whole_entries() {
    let dir = temp_dir("overlap");
    let engine = engine();
    let cache = ComponentCache::new(&dir);
    std::thread::scope(|scope| {
        for _ in 0..4 {
            scope.spawn(|| cache.load(&engine, &component(1)).unwrap());
        }
    });
    let fresh = ComponentCache::new(&dir);
    fresh.load(&engine, &component(1)).unwrap();
    assert_eq!(fresh.compiled(), 0);
    assert_eq!(entries(&dir).len(), 1);
}

#[cfg(unix)]
#[test]
fn a_fifo_at_the_entry_is_a_miss() {
    let dir = temp_dir("fifo");
    let (cache, engine) = (ComponentCache::new(&dir), engine());
    let bytes = component(1);
    let path = cache.entry_path(&engine, &bytes);
    let status = std::process::Command::new("mkfifo")
        .arg(&path)
        .status()
        .unwrap();
    assert!(status.success());
    let (sender, received) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        cache.load(&engine, &bytes).unwrap();
        sender.send(cache.compiled()).unwrap();
    });
    let compiled = received
        .recv_timeout(Duration::from_secs(2))
        .expect("blocked on the FIFO");
    assert_eq!(compiled, 1);
}
