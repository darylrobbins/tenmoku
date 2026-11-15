use std::path::{Path, PathBuf};

use super::*;
use crate::folder_markers::{Call, take_calls};

/// Probes each of `dirs` that `cache` asks for, as a frame drawing them
/// would, checking each probe's IO: the folder opened and its names read
/// once, at most [`MAX_STATS`] metadata calls, and nothing below the folder
/// (only its own entries, a `Cargo.toml` the one file opened). Returns the
/// folders probed.
fn frame(cache: &mut MarkerCache, dirs: &[PathBuf]) -> Vec<PathBuf> {
    let probes: Vec<_> = (dirs.iter()).filter_map(|dir| cache.request(dir)).collect();
    assert_eq!(take_calls(), [], "requesting does no IO");
    let mut probed = Vec::new();
    for probe in probes {
        let dir = probe.dir().to_owned();
        let result = probe.run();
        check_calls(&dir, &take_calls());
        assert!(cache.store(result), "{}", dir.display());
        probed.push(dir);
    }
    probed
}

fn check_calls(dir: &Path, calls: &[(Call, PathBuf)]) {
    let count = |want| calls.iter().filter(|(call, _)| *call == want).count();
    assert!(
        count(Call::Stat) <= MAX_STATS,
        "{}: {calls:?}",
        dir.display()
    );
    assert_eq!(count(Call::ReadDir), 1, "{}: {calls:?}", dir.display());
    for (call, path) in calls {
        match call {
            Call::ReadDir => assert_eq!(path, dir),
            Call::Open if path == dir => {}
            // No recursion: only the folder's own entries, and only a
            // `Cargo.toml` opened.
            Call::Open => assert_eq!(path, &dir.join("Cargo.toml")),
            Call::Stat => assert_eq!(path.parent(), Some(dir), "{calls:?}"),
        }
    }
}

fn with(f: impl FnOnce(&mut Markers)) -> Markers {
    let mut markers = Markers::default();
    f(&mut markers);
    markers
}

#[test]
fn probe_is_bounded_cached_and_invalidated_by_watcher_batches() {
    let (_dir, root) = temp();
    make(
        &root,
        &[
            "core/build.gradle",
            "core/src/main/java/",
            "core/nested/Cargo.toml",
            "core/nested/package.json",
            "web/package.json",
            "web/src/index.ts",
            "plain/notes.txt",
            "plain/deep/pom.xml",
            "Api/Service.csproj",
            "Fs/Lib.fsproj",
            "Vb/App.vbproj",
            "Shop/Shop.slnx",
            "lib/src/lib.rs",
            "lib/Cargo.toml",
            "ws/Cargo.toml",
            "tool/src/Cargo.toml",
            "tool/src/main/",
            "ignored/pyproject.toml",
            "ignored/.ignore",
        ],
    );
    fs::write(
        root.join("ws/Cargo.toml"),
        "[package]\nname = \"ws\"\n\n[workspace]\nmembers = []\n",
    )
    .unwrap();
    fs::write(root.join("ignored/.ignore"), "pyproject.toml\n").unwrap();
    let dir = |rel: &str| root.join(rel);
    let rels = [
        "core", "core/src", "web", "plain", "ws", "Api", "Fs", "Vb", "Shop", "lib", "tool/src",
        "ignored",
    ];
    let dirs: Vec<PathBuf> = rels.into_iter().map(dir).collect();

    let mut cache = MarkerCache::default();
    // Until a probe lands, nothing is known.
    assert_eq!(cache.markers(&dir("core"), None), Markers::default());
    // One probe per folder, however many frames draw it.
    assert_eq!(frame(&mut cache, &dirs), dirs);
    for _ in 0..3 {
        assert_eq!(frame(&mut cache, &dirs), [] as [PathBuf; 0]);
    }
    let markers = |cache: &MarkerCache, rel| cache.markers(&dir(rel), None);
    // What's below a folder isn't looked at: `core/nested`'s files aren't
    // `core`'s.
    assert_eq!(markers(&cache, "core"), with(|m| m.gradle = true));
    assert_eq!(markers(&cache, "core/src"), with(|m| m.main_or_test = true));
    assert_eq!(markers(&cache, "web"), with(|m| m.npm = true));
    assert_eq!(markers(&cache, "plain"), Markers::default());
    let workspace = with(|m| (m.cargo, m.cargo_workspace) = (true, true));
    assert_eq!(markers(&cache, "ws"), workspace);
    let crate_ = with(|m| m.cargo = true);
    assert_eq!(markers(&cache, "lib"), crate_);
    // Every .NET project and solution, by any name.
    // (`Fs`, not `Lib`: on a case-insensitive file system, macOS's, that
    // is `lib`, and its `Lib.fsproj` would be `lib`'s.)
    for rel in ["Api", "Fs", "Vb"] {
        assert_eq!(markers(&cache, rel), with(|m| m.dotnet = true), "{rel}");
    }
    assert_eq!(markers(&cache, "Shop"), with(|m| m.dotnet_solution = true));
    // A `src` has its own build files as well as its layout.
    assert_eq!(
        markers(&cache, "tool/src"),
        with(|m| (m.cargo, m.main_or_test) = (true, true))
    );
    // What the tree hides counts too.
    assert_eq!(markers(&cache, "ignored"), with(|m| m.python = true));

    // A batch naming a folder forgets it with what's below it; one naming
    // an entry that can count forgets its folder; one naming any other
    // entry, or something deeper, forgets nothing. Each says whether it
    // forgot anything.
    assert!(!cache.apply(&changes(&root, &["core/notes.md", "web/src/index.ts"])));
    assert!(!cache.apply(&changes(&root, &["web/node_modules/x/package.json"])));
    assert_eq!(frame(&mut cache, &dirs), [] as [PathBuf; 0]);
    assert!(cache.apply(&changes(&root, &["core/build.gradle", "plain"])));
    assert_eq!(markers(&cache, "core"), Markers::default());
    assert_eq!(markers(&cache, "core/src"), with(|m| m.main_or_test = true));
    assert_eq!(frame(&mut cache, &dirs), [dir("core"), dir("plain")]);
    assert!(cache.apply(&changes(&root, &["core"])));
    assert_eq!(frame(&mut cache, &dirs), [dir("core"), dir("core/src")]);
    // An edit of a `Cargo.toml` is read again.
    fs::write(root.join("lib/Cargo.toml"), "[workspace]\n").unwrap();
    assert!(cache.apply(&changes(&root, &["lib/Cargo.toml"])));
    assert_eq!(frame(&mut cache, &dirs), [dir("lib")]);
    assert_eq!(markers(&cache, "lib"), workspace);
    // `main` and `test` count, in any case.
    assert!(cache.apply(&changes(&root, &["core/src/TEST"])));
    assert_eq!(frame(&mut cache, &dirs), [dir("core/src")]);

    // A result for a folder forgotten since it was requested is dropped,
    // and the folder probed again.
    assert!(cache.apply(&changes(&root, &["web/package.json"])));
    let stale = cache.request(&dir("web")).unwrap();
    fs::remove_file(root.join("web/package.json")).unwrap();
    assert!(cache.apply(&changes(&root, &["web/package.json"])));
    assert!(!cache.store(stale.run()));
    take_calls();
    assert_eq!(frame(&mut cache, &dirs), [dir("web")]);
    assert_eq!(markers(&cache, "web"), Markers::default());

    // A rescan forgets every folder.
    assert!(cache.apply(&[FsChange::Rescan]));
    assert_eq!(frame(&mut cache, &dirs), dirs);
    assert!(!MarkerCache::default().apply(&[FsChange::Rescan]));

    // A listed folder has the tree's entries' markers until its probe lands,
    // then the probe's, which count what the tree hides: expanding a folder
    // doesn't change its role.
    let mut tree = FileTree::open(&root, TreeOptions::default()).unwrap();
    for rel in ["Api", "ignored", "ws"] {
        let id = id(&tree, rel);
        tree.expand(id);
    }
    let listed = |rel| Markers::of_listing(&tree, id(&tree, rel));
    assert_eq!(Markers::of_listing(&tree, id(&tree, "plain")), None);
    assert_eq!(listed("ignored"), Some(Markers::default()));
    assert_eq!(listed("ws"), Some(crate_));
    let mut fresh = MarkerCache::default();
    assert_eq!(fresh.markers(&dir("ws"), listed("ws")), crate_);
    assert_eq!(frame(&mut fresh, &[dir("ws"), dir("ignored")]).len(), 2);
    assert_eq!(fresh.markers(&dir("ws"), listed("ws")), workspace);
    let ignored = with(|m| m.python = true);
    assert_eq!(fresh.markers(&dir("ignored"), listed("ignored")), ignored);
    assert_eq!(cache.markers(&dir("ignored"), listed("ignored")), ignored);

    // Parity: with every entry shown, every folder's listing, expanded,
    // gives what its probe found collapsed (a workspace aside, which only
    // the probe reads).
    let mut tree = FileTree::open(&root, TreeOptions { show_ignored: true }).unwrap();
    for rel in rels {
        let mut path = PathBuf::new();
        for component in Path::new(rel).components() {
            path.push(component);
            let id = id(&tree, path.to_str().unwrap());
            tree.expand(id);
        }
    }
    for rel in rels {
        let listed = Markers::of_listing(&tree, id(&tree, rel)).unwrap();
        let probed = Markers {
            cargo_workspace: false,
            ..cache.markers(&dir(rel), None)
        };
        assert_eq!(listed, probed, "{rel}");
    }
    take_calls();
}

#[test]
fn probe_follows_no_link_and_reads_manifests_as_toml() {
    let (_dir, root) = temp();
    let (_outside, outside) = temp();
    make(&outside, &["Cargo.toml", "build.gradle"]);
    fs::write(outside.join("Cargo.toml"), "[workspace]\n").unwrap();
    let mut cache = MarkerCache::default();
    let probe = |dir: &Path| {
        let mut cache = MarkerCache::default();
        let probe = cache.request(dir).unwrap();
        let result = probe.run();
        let calls = take_calls();
        assert!(cache.store(result));
        (cache.markers(dir, None), calls)
    };

    // The workspace table, however it's spelled; nothing that only looks
    // like it.
    for (i, (text, want)) in [
        ("[workspace]\n", true),
        ("[ workspace ]\n", true),
        ("[\"workspace\"]\n", true),
        ("  [workspace.package]\nversion = \"1\"\n", true),
        ("workspace.members = []\n", true),
        ("workspace = { members = [] }\n", true),
        ("[package]\n# [workspace]\n", false),
        (
            "[package]\ndescription = \"\"\"\n[workspace]\n\"\"\"\n",
            false,
        ),
        ("[workspaces]\n", false),
        ("[package]\nworkspace = \"..\"\n", false),
        ("[workspace\n", false),
    ]
    .into_iter()
    .enumerate()
    {
        let dir = root.join(format!("cargo{i}"));
        make(&root, &[&format!("cargo{i}/Cargo.toml")]);
        fs::write(dir.join("Cargo.toml"), text).unwrap();
        let (markers, calls) = probe(&dir);
        check_calls(&dir, &calls);
        assert_eq!(
            (markers.cargo, markers.cargo_workspace),
            (true, want),
            "{text:?}"
        );
    }
    // One too big isn't read past the limit.
    make(&root, &["big/Cargo.toml"]);
    let mut big = b"[workspace]\n".to_vec();
    big.resize(MAX_READ as usize + 1, b'\n');
    fs::write(root.join("big/Cargo.toml"), big).unwrap();
    let (markers, _) = probe(&root.join("big"));
    assert_eq!((markers.cargo, markers.cargo_workspace), (true, false));

    // A linked `Cargo.toml` is a marker, as the tree lists it, but isn't read
    // through; a FIFO isn't waited on.
    #[cfg(unix)]
    {
        make(&root, &["linked/"]);
        let link = root.join("linked/Cargo.toml");
        std::os::unix::fs::symlink(outside.join("Cargo.toml"), link).unwrap();
        let (markers, _) = probe(&root.join("linked"));
        assert_eq!((markers.cargo, markers.cargo_workspace), (true, false));

        make(&root, &["fifo/"]);
        let fifo = std::ffi::CString::new(
            root.join("fifo/Cargo.toml")
                .into_os_string()
                .into_encoded_bytes(),
        )
        .unwrap();
        // SAFETY: `fifo` is a NUL-terminated path.
        assert_eq!(unsafe { libc::mkfifo(fifo.as_ptr(), 0o600) }, 0);
        let (markers, _) = probe(&root.join("fifo"));
        assert_eq!((markers.cargo, markers.cargo_workspace), (true, false));
    }

    // A folder replaced by a link after its probe was requested reads
    // nothing through it: it has no markers.
    make(&root, &["swapped/build.gradle"]);
    let swapped = root.join("swapped");
    let request = cache.request(&swapped).unwrap();
    swap_for_link(&swapped, &outside);
    assert!(cache.store(request.run()));
    let calls = take_calls();
    assert_eq!(cache.markers(&swapped, None), Markers::default());
    assert!(
        !calls.iter().any(|(call, _)| *call == Call::ReadDir),
        "{calls:?}"
    );
    swap_back(&swapped);
    // So does one a component above it.
    make(&root, &["up/core/build.gradle"]);
    let core = root.join("up/core");
    let request = cache.request(&core).unwrap();
    fs::rename(root.join("up"), root.join("up.real")).unwrap();
    make(&outside, &["core/build.gradle"]);
    dir_link(&outside, &root.join("up"));
    assert!(cache.store(request.run()));
    assert_eq!(cache.markers(&core, None), Markers::default());
    take_calls();
}
