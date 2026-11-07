//! Extensions: Install from File or Folder: a `.zip`, a `.tar.gz` and a
//! folder each land as `<extensions dir>/<id>`; entries that escape or
//! link, a bad manifest and an oversized archive are refused, leaving
//! nothing; an installed id, in whatever directory, is replaced only after
//! asking, and a failed replace leaves it; and an installed plugin waits
//! for its grant. The archives are built here.

use std::{
    cell::Cell,
    fs,
    io::{self, Seek, Write},
    path::{Path, PathBuf},
};

use fe_command::{ContextFrame, ContextStack, Invocation, Value};
use fe_gpui::{EditorSettings, EditorTheme, commands::RunCommand};
use flate2::{Compression, write::GzEncoder};
use gpui::{Action as _, Entity, Focusable as _, TestAppContext, VisualTestContext};
use kiln::core::ExtensionSet;
use zip::{CompressionMethod, ZipWriter, write::SimpleFileOptions};

#[cfg(unix)]
use super::same_file;
use super::{
    InstallError, Installed, MAX_ENTRIES, MAX_UNPACKED, check_api, install_extension,
    set_extensions_dir, stage,
};
use crate::{
    CACHE_DIR, Launch, PluginState, Workbench, plugin_host::set_preview, plugin_states,
    set_plugin_dirs, set_plugin_extensions,
};

/// Registers commands; asks for `commands`.
const COMMANDS: &str = include_str!("../../../kiln/crates/kiln-plugin/tests/fixtures/commands.wat");

/// `extension.toml` for `id` at `version`, with `rest` after.
fn manifest(id: &str, version: &str, rest: &str) -> String {
    format!(
        "id = \"{id}\"\nname = \"{id}\"\nversion = \"{version}\"\nlicense = \"MIT\"\n\n\
         [engine]\ntenmoku = \"1.1\"\n{rest}"
    )
}

/// A plugin's `[contributes.plugin]`, its component `plugin.wasm`.
const PLUGIN: &str =
    "\n[contributes.plugin]\nwasm = \"plugin.wasm\"\ncapabilities = [\"commands\"]\n";

/// A zip at `path` of `files`, deflated.
fn zip_of(path: &Path, files: &[(&str, &[u8])]) -> PathBuf {
    let mut zip = ZipWriter::new(fs::File::create(path).unwrap());
    for (name, contents) in files {
        if name.ends_with('/') {
            zip.add_directory(*name, SimpleFileOptions::default())
                .unwrap();
        } else {
            zip.start_file(*name, SimpleFileOptions::default()).unwrap();
            zip.write_all(contents).unwrap();
        }
    }
    zip.finish().unwrap();
    path.to_owned()
}

/// A `.tar.gz` at `path` of what `build` appends.
fn tar_gz_of(path: &Path, build: impl FnOnce(&mut tar::Builder<GzEncoder<fs::File>>)) -> PathBuf {
    let gz = GzEncoder::new(fs::File::create(path).unwrap(), Compression::fast());
    let mut tar = tar::Builder::new(gz);
    build(&mut tar);
    tar.into_inner().unwrap().finish().unwrap();
    path.to_owned()
}

/// Appends the entry `name` of `kind`, the name written as is.
fn append_raw(
    tar: &mut tar::Builder<impl Write>,
    name: &str,
    kind: tar::EntryType,
    contents: &[u8],
) {
    let mut header = tar::Header::new_gnu();
    header.as_old_mut().name[..name.len()].copy_from_slice(name.as_bytes());
    header.set_entry_type(kind);
    header.set_mode(0o644);
    header.set_size(contents.len() as u64);
    if kind == tar::EntryType::Link || kind == tar::EntryType::Symlink {
        header.as_old_mut().linkname[..1].copy_from_slice(b"x");
    }
    header.set_cksum();
    tar.append(&header, contents).unwrap();
}

/// The zip at `path` with each name `renamed` picks renamed `to`, of its
/// length, as no writer would write one.
fn rename_in_zip(path: &Path, renamed: impl Fn(&[u8]) -> bool, to: &str) {
    let mut bytes = fs::read(path).unwrap();
    let mut at = 0;
    while at + to.len() <= bytes.len() {
        if renamed(&bytes[at..at + to.len()]) {
            bytes[at..at + to.len()].copy_from_slice(to.as_bytes());
            at += to.len();
        } else {
            at += 1;
        }
    }
    fs::write(path, bytes).unwrap();
}

/// A pax record of `key` and `value`, led by its own length.
fn pax_record(key: &str, value: &str) -> String {
    let body = format!(" {key}={value}\n");
    // The length counts its own digits.
    let mut length = body.len() + 1;
    while format!("{length}{body}").len() != length {
        length += 1;
    }
    format!("{length}{body}")
}

/// The IDs the extensions service lists in `dir`, as a rescan reads them.
fn listed(dir: &Path) -> Vec<String> {
    let set = ExtensionSet::discover(dir, &check_api(Vec::new()));
    (set.extensions().iter())
        .map(|manifest| manifest.id.clone())
        .collect()
}

/// The names in `dir`, sorted: none left over from a refused install.
fn names(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(dir)
        .map(|entries| {
            entries
                .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default();
    names.sort();
    names
}

fn refusal(result: Result<Installed, InstallError>) -> String {
    match result {
        Err(InstallError::Refused(reason)) => reason,
        other => panic!("not refused: {other:?}"),
    }
}

#[test]
fn installs_a_zip_a_tar_gz_and_a_folder() {
    let sources = tempfile::tempdir().unwrap();
    let extensions = tempfile::tempdir().unwrap();
    let extensions = extensions.path();

    // A zip with the manifest at its top level.
    let zipped = manifest("acme.zipped", "1.2.0", "");
    let zip = zip_of(
        &sources.path().join("zipped.ZIP"),
        &[
            ("extension.toml", zipped.as_bytes()),
            ("themes/", b""),
            ("themes/dark.toml", b"name = \"dark\"\n"),
        ],
    );
    let installed = install_extension(&zip, extensions).unwrap();
    let dir = extensions.canonicalize().unwrap().join("acme.zipped");
    assert_eq!(
        installed,
        Installed {
            id: "acme.zipped".to_owned(),
            name: "acme.zipped".to_owned(),
            version: (1, 2, 0),
            dir: dir.clone(),
            plugin: false,
        }
    );
    assert_eq!(
        fs::read_to_string(dir.join("extension.toml")).unwrap(),
        zipped
    );
    assert_eq!(
        fs::read_to_string(dir.join("themes/dark.toml")).unwrap(),
        "name = \"dark\"\n"
    );

    // A .tar.gz with it in its single folder, an executable kept so.
    let tarred = manifest("acme.tarred", "0.1.0", "");
    let tgz = tar_gz_of(&sources.path().join("tarred.tar.gz"), |tar| {
        let mut header = tar::Header::new_gnu();
        header.set_size(tarred.len() as u64);
        header.set_mode(0o644);
        let path = "acme.tarred-0.1.0/extension.toml";
        tar.append_data(&mut header, path, tarred.as_bytes())
            .unwrap();
        let mut header = tar::Header::new_gnu();
        header.set_size(3);
        header.set_mode(0o755);
        let path = "acme.tarred-0.1.0/bin/serve";
        tar.append_data(&mut header, path, &b"#!\n"[..]).unwrap();
        // Named in a GNU long-name header of its own.
        let mut header = tar::Header::new_gnu();
        header.set_size(1);
        let path = format!("acme.tarred-0.1.0/{}.txt", "long".repeat(30));
        tar.append_data(&mut header, path, &b"l"[..]).unwrap();
        // Named, and sized as its own header says, in a pax header.
        let records = pax_record("path", "acme.tarred-0.1.0/pax.txt") + &pax_record("size", "3");
        append_raw(tar, "pax", tar::EntryType::XHeader, records.as_bytes());
        append_raw(tar, "ignored", tar::EntryType::Regular, b"pax");
    });
    let installed = install_extension(&tgz, extensions).unwrap();
    assert_eq!(installed.id, "acme.tarred");
    let long = installed.dir.join(format!("{}.txt", "long".repeat(30)));
    assert_eq!(fs::read_to_string(long).unwrap(), "l");
    assert_eq!(
        fs::read_to_string(installed.dir.join("pax.txt")).unwrap(),
        "pax"
    );
    assert_eq!(
        fs::read_to_string(installed.dir.join("bin/serve")).unwrap(),
        "#!\n"
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        let mode = fs::metadata(installed.dir.join("bin/serve"))
            .unwrap()
            .permissions()
            .mode();
        assert_ne!(mode & 0o100, 0, "{mode:o}");
    }

    // A folder with it in its single folder, copied.
    let folder = sources.path().join("checkout");
    fs::create_dir_all(folder.join("acme.copied/snippets")).unwrap();
    let copied = manifest("acme.copied", "2.0.0", "");
    fs::write(folder.join("acme.copied/extension.toml"), &copied).unwrap();
    fs::write(folder.join("acme.copied/snippets/rust.toml"), "x").unwrap();
    let installed = install_extension(&folder, extensions).unwrap();
    assert_eq!(installed.id, "acme.copied");
    assert_eq!(
        fs::read_to_string(installed.dir.join("snippets/rust.toml")).unwrap(),
        "x"
    );
    assert!(!fs::symlink_metadata(&installed.dir).unwrap().is_symlink());
    assert!(
        folder.join("acme.copied/extension.toml").exists(),
        "copied, not moved"
    );

    // Each is a directory of its own, nothing else is left, and a rescan
    // lists them all.
    assert_eq!(
        names(extensions),
        ["acme.copied", "acme.tarred", "acme.zipped"]
    );
    assert_eq!(
        listed(extensions),
        ["acme.copied", "acme.tarred", "acme.zipped"]
    );
}

#[test]
fn refuses_entries_that_escape_or_link() {
    let sources = tempfile::tempdir().unwrap();
    let extensions = tempfile::tempdir().unwrap();
    let extensions = extensions.path();
    let good = manifest("acme.bad", "1.0.0", "");
    let good = good.as_bytes();

    // An absolute entry and a `..` escape, in a zip.
    let absolute = zip_of(
        &sources.path().join("absolute.zip"),
        &[("extension.toml", good), ("/etc/passwd", b"x")],
    );
    let escape = zip_of(
        &sources.path().join("escape.zip"),
        &[("extension.toml", good), ("themes/../../outside", b"x")],
    );
    // A symlink, a hard link and a fifo, in a .tar.gz.
    let with = |name: &str, kind: tar::EntryType| {
        let path = sources.path().join(format!("{name}.tgz"));
        tar_gz_of(&path, |tar| {
            append_raw(tar, "extension.toml", tar::EntryType::Regular, good);
            append_raw(tar, name, kind, b"");
        })
    };
    let cases = [
        (absolute, "/etc/passwd is an absolute path"),
        (escape, "themes/../../outside leads outside the extension"),
        (
            with("link", tar::EntryType::Symlink),
            "link is a symbolic link",
        ),
        (with("hard", tar::EntryType::Link), "hard is a hard link"),
        (
            with("fifo", tar::EntryType::Fifo),
            "fifo isn't a file or a folder",
        ),
        (
            with("device", tar::EntryType::Char),
            "device isn't a file or a folder",
        ),
    ];
    // Under a name that's otherwise skipped, or the top level's.
    let skipped = sources.path().join("skipped.tgz");
    let skipped = tar_gz_of(&skipped, |tar| {
        append_raw(tar, "extension.toml", tar::EntryType::Regular, good);
        append_raw(tar, "__MACOSX/link", tar::EntryType::Symlink, b"");
    });
    let top = sources.path().join("top.tgz");
    let top = tar_gz_of(&top, |tar| {
        append_raw(tar, "extension.toml", tar::EntryType::Regular, good);
        append_raw(tar, "./", tar::EntryType::Fifo, b"");
    });
    // A zip's fifo named as a folder is.
    let named = sources.path().join("named.zip");
    let mut zip = ZipWriter::new(fs::File::create(&named).unwrap());
    zip.start_file("extension.toml", SimpleFileOptions::default())
        .unwrap();
    zip.write_all(good).unwrap();
    // Made on Unix, as the mode needs: written on Windows, an entry is DOS's.
    let fifo = SimpleFileOptions::default()
        .system(zip::System::Unix)
        .external_attributes(0o010_644 << 16);
    zip.start_file("device/", fifo).unwrap();
    zip.finish().unwrap();
    // A symlink a later entry of its name would hide.
    let hidden = sources.path().join("hidden.zip");
    let mut zip = ZipWriter::new(fs::File::create(&hidden).unwrap());
    zip.start_file("extension.toml", SimpleFileOptions::default())
        .unwrap();
    zip.write_all(good).unwrap();
    zip.add_symlink("linkA", "/etc", SimpleFileOptions::default())
        .unwrap();
    zip.start_file("linkB", SimpleFileOptions::default())
        .unwrap();
    zip.finish().unwrap();
    rename_in_zip(&hidden, |name| name == b"linkB", "linkA");
    let cases = cases.into_iter().chain([
        (skipped, "__MACOSX/link is a symbolic link"),
        (top, "./ isn't a file or a folder"),
        (named, "device/ isn't a file or a folder"),
        (hidden, "It has two entries with one name"),
    ]);
    for (source, reason) in cases {
        assert_eq!(refusal(install_extension(&source, extensions)), reason);
        assert_eq!(names(extensions), Vec::<String>::new(), "{reason}");
    }

    // A symlink in a folder isn't followed.
    #[cfg(unix)]
    {
        let folder = sources.path().join("linked");
        fs::create_dir(&folder).unwrap();
        fs::write(folder.join("extension.toml"), good).unwrap();
        std::os::unix::fs::symlink("/etc", folder.join("etc")).unwrap();
        let reason = refusal(install_extension(&folder, extensions));
        assert_eq!(reason, "etc is a symbolic link");
        assert_eq!(names(extensions), Vec::<String>::new());

        // Nor is a file with other names copied.
        fs::remove_file(folder.join("etc")).unwrap();
        fs::write(sources.path().join("shared"), "x").unwrap();
        fs::hard_link(sources.path().join("shared"), folder.join("shared")).unwrap();
        let reason = refusal(install_extension(&folder, extensions));
        assert_eq!(reason, "shared is a hard link");
        assert_eq!(names(extensions), Vec::<String>::new());
    }
}

#[test]
fn refuses_a_bad_manifest_or_an_oversized_archive() {
    let sources = tempfile::tempdir().unwrap();
    let extensions = tempfile::tempdir().unwrap();
    let extensions = extensions.path();
    let zip = |name: &str, files: &[(&str, &[u8])]| zip_of(&sources.path().join(name), files);
    let refused = |source: PathBuf| {
        let reason = refusal(install_extension(&source, extensions));
        assert_eq!(names(extensions), Vec::<String>::new(), "{reason}");
        reason
    };

    let none = zip("none.zip", &[("README.md", b"hi")]);
    assert_eq!(
        refused(none),
        "There's no extension.toml at its top level, or in a single folder there"
    );
    let bad = manifest("acme.bad", "one", "");
    let reason = refused(zip("version.zip", &[("extension.toml", bad.as_bytes())]));
    assert!(
        reason.starts_with("extension.toml:3:") && reason.contains("version"),
        "{reason}"
    );
    let plugin = manifest(
        "acme.bad",
        "1.0.0",
        "\n[contributes.plugin]\nwasm = \"plugin.wasm\"\ncapabilities = [\"no such\"]\n",
    );
    let plugin = zip(
        "plugin.zip",
        &[("extension.toml", plugin.as_bytes()), ("plugin.wasm", b"")],
    );
    assert!(refused(plugin).starts_with("acme.bad: "));

    // One entry too many, counted as they're unpacked.
    let good = manifest("acme.big", "1.0.0", "");
    let many = tar_gz_of(&sources.path().join("many.tgz"), |tar| {
        append_raw(
            tar,
            "extension.toml",
            tar::EntryType::Regular,
            good.as_bytes(),
        );
        for index in 0..MAX_ENTRIES {
            append_raw(tar, &format!("d{index}/"), tar::EntryType::Directory, b"");
        }
    });
    assert_eq!(refused(many), "It has more than 10000 entries");
    // A tar's own headers count, and are read only so far.
    let pax = |name: &str, payload: &[u8], count: usize| {
        tar_gz_of(&sources.path().join(name), |tar| {
            append_raw(
                tar,
                "extension.toml",
                tar::EntryType::Regular,
                good.as_bytes(),
            );
            for _ in 0..count {
                append_raw(tar, "pax", tar::EntryType::XHeader, payload);
            }
        })
    };
    let headers = pax("headers.tgz", b"10 path=a\n", MAX_ENTRIES);
    assert_eq!(refused(headers), "It has more than 10000 entries");
    let long = pax("long.tgz", &vec![b'a'; 64 * 1024 + 1], 1);
    assert_eq!(
        refused(long),
        "It has a long-name or pax header over 64 KiB"
    );
    // A pax size is the entry's own header's, and within the limit.
    let sized = |name: &str, size: u64| {
        tar_gz_of(&sources.path().join(name), |tar| {
            append_raw(
                tar,
                "extension.toml",
                tar::EntryType::Regular,
                good.as_bytes(),
            );
            let record = pax_record("size", &size.to_string());
            append_raw(tar, "pax", tar::EntryType::XHeader, record.as_bytes());
            append_raw(tar, "file", tar::EntryType::Regular, b"x");
        })
    };
    assert_eq!(
        refused(sized("huge.tgz", MAX_UNPACKED + 1)),
        "It unpacks to more than 64 MiB"
    );
    assert_eq!(
        refused(sized("other.tgz", 5)),
        "It has a pax header giving an entry 5 bytes, and the entry's own header 1"
    );
    // A folder's entries are counted as they're listed.
    let folder = sources.path().join("many");
    fs::create_dir(&folder).unwrap();
    fs::write(folder.join("extension.toml"), &good).unwrap();
    for index in 0..MAX_ENTRIES {
        fs::write(folder.join(format!("f{index}")), "").unwrap();
    }
    assert_eq!(refused(folder), "It has more than 10000 entries");
    // An entry whose contents would be skipped counts too, before they're
    // read: the archive ends after its header.
    let skipped = sources.path().join("skipped.tgz");
    let mut gz = GzEncoder::new(fs::File::create(&skipped).unwrap(), Compression::fast());
    let mut header = tar::Header::new_gnu();
    header.set_path("folder/").unwrap();
    header.set_entry_type(tar::EntryType::Directory);
    header.set_size(MAX_UNPACKED + 1);
    header.set_cksum();
    gz.write_all(header.as_bytes()).unwrap();
    gz.finish().unwrap();
    assert_eq!(refused(skipped), "It unpacks to more than 64 MiB");
    // A zip's names each count, used twice or not.
    let names_used: Vec<String> = (0..=MAX_ENTRIES)
        .map(|index| format!("dup-{index:05}"))
        .collect();
    let mut files: Vec<(&str, &[u8])> = vec![("extension.toml", good.as_bytes())];
    files.extend(names_used.iter().map(|name| (name.as_str(), &b""[..])));
    let twice = zip("twice.zip", &files);
    let numbered =
        |name: &[u8]| name.starts_with(b"dup-") && name[4..].iter().all(u8::is_ascii_digit);
    rename_in_zip(&twice, numbered, "dup-00000");
    assert_eq!(refused(twice), "It has two entries with one name");

    // One byte too many, stored so the test writes no more than it reads.
    let path = sources.path().join("big.zip");
    let mut big = ZipWriter::new(fs::File::create(&path).unwrap());
    let stored = SimpleFileOptions::default()
        .compression_method(CompressionMethod::Stored)
        .large_file(true);
    big.start_file("extension.toml", stored).unwrap();
    big.write_all(good.as_bytes()).unwrap();
    big.start_file("blob", stored).unwrap();
    let chunk = vec![0u8; 1 << 20];
    for _ in 0..MAX_UNPACKED >> 20 {
        big.write_all(&chunk).unwrap();
    }
    big.write_all(b"!").unwrap();
    let mut file = big.finish().unwrap();
    assert!(file.stream_position().unwrap() > MAX_UNPACKED);
    assert_eq!(refused(path), "It unpacks to more than 64 MiB");

    let other = sources.path().join("notes.txt");
    fs::write(&other, "x").unwrap();
    assert_eq!(
        install_extension(&other, extensions),
        Err(InstallError::Unsupported)
    );
}

/// The app with the workbench's commands, installing into `extensions`.
fn start(cx: &mut TestAppContext, extensions: &Path) {
    let extensions = extensions.to_owned();
    cx.update(|cx| {
        fe_gpui::init(cx);
        crate::init(cx);
        set_extensions_dir(cx, extensions);
    });
}

/// A workbench with no folder, focused, in a window of its own.
fn window(cx: &mut TestAppContext) -> (Entity<Workbench>, &mut VisualTestContext) {
    let launch = Launch {
        root: None,
        file: None,
    };
    let (workbench, vcx) = cx.add_window_view(move |window, cx| {
        let theme = EditorTheme::default_dark(cx);
        Workbench::new(launch, theme, EditorSettings::default(), window, cx)
    });
    vcx.update(|window, cx| {
        window.activate_window();
        window.focus(&workbench.focus_handle(cx), cx);
    });
    vcx.run_until_parked();
    (workbench, vcx)
}

/// Runs Extensions: Install from File or Folder… and picks `source`.
fn install(vcx: &mut VisualTestContext, source: &Path) {
    let invocation = Invocation::new("workbench.install_extension".parse().unwrap())
        .args(Value::Null)
        .context(ContextStack::new().with(ContextFrame::new("workbench")));
    vcx.update(|window, cx| window.dispatch_action(RunCommand(invocation).boxed_clone(), cx));
    vcx.run_until_parked();
    assert!(vcx.did_prompt_for_paths());
    let source = source.to_owned();
    vcx.simulate_path_prompt_response(move |options| {
        assert!(options.files && options.directories && !options.multiple);
        Some(vec![source])
    });
    vcx.run_until_parked();
}

/// The toasts shown, each as its message and detail.
fn toasts(workbench: &Entity<Workbench>, vcx: &mut VisualTestContext) -> Vec<(String, String)> {
    let layer = workbench.read_with(vcx, |workbench, _| workbench.toasts().clone());
    layer.read_with(vcx, |layer, _| {
        (layer.queue().shown().iter())
            .map(|entry| {
                let detail = entry.toast.detail.clone().unwrap_or_default();
                (entry.toast.message.clone(), detail)
            })
            .collect()
    })
}

fn installed_version(dir: &Path) -> String {
    let text = fs::read_to_string(dir.join("extension.toml")).unwrap();
    let table: toml::Table = text.parse().unwrap();
    table["version"].as_str().unwrap().to_owned()
}

#[gpui::test]
fn replacing_an_installed_id_asks_first(cx: &mut TestAppContext) {
    let sources = tempfile::tempdir().unwrap();
    let extensions = tempfile::tempdir().unwrap();
    let extensions = extensions.path().canonicalize().unwrap();
    start(cx, &extensions);
    let first = manifest("acme.tool", "1.0.0", "");
    let first = zip_of(
        &sources.path().join("first.zip"),
        &[("extension.toml", first.as_bytes()), ("old.txt", b"old")],
    );
    install_extension(&first, &extensions).unwrap();
    // Installed under another name, as the extensions service allows.
    let old = extensions.join("000-old");
    fs::rename(extensions.join("acme.tool"), &old).unwrap();
    let dir = extensions.join("acme.tool");
    let second = manifest("acme.tool", "2.0.0", "");
    let second = zip_of(
        &sources.path().join("second.zip"),
        &[("extension.toml", second.as_bytes()), ("new.txt", b"new")],
    );
    let (workbench, vcx) = window(cx);

    // Cancel keeps the old one; nothing unpacked is left.
    install(vcx, &second);
    let (question, detail) = vcx.pending_prompt().expect("the replace prompt");
    assert_eq!(question, "Replace acme.tool 1.0.0 with 2.0.0?");
    let installed_in = format!("acme.tool 1.0.0 is installed in {}.", old.display());
    assert!(detail.contains(&installed_in), "{detail}");
    vcx.simulate_prompt_answer("Cancel");
    vcx.run_until_parked();
    assert_eq!(installed_version(&old), "1.0.0");
    assert!(old.join("old.txt").exists());
    assert_eq!(names(&extensions), ["000-old"]);
    assert_eq!(toasts(&workbench, vcx), []);

    // Replace swaps it, into the place its id names.
    install(vcx, &second);
    assert!(vcx.pending_prompt().is_some());
    vcx.simulate_prompt_answer("Replace");
    vcx.run_until_parked();
    assert_eq!(installed_version(&dir), "2.0.0");
    assert!(dir.join("new.txt").exists());
    assert_eq!(names(&extensions), ["acme.tool"]);
    assert_eq!(listed(&extensions), ["acme.tool"]);
    let shown = toasts(&workbench, vcx);
    assert_eq!(
        shown,
        [("Installed acme.tool 2.0.0".to_owned(), String::new())]
    );
}

#[test]
fn a_replace_finds_the_old_one_and_a_failure_keeps_it() {
    let sources = tempfile::tempdir().unwrap();
    let extensions = tempfile::tempdir().unwrap();
    let extensions = extensions.path().canonicalize().unwrap();
    let version = |version: &str| {
        let text = manifest("acme.tool", version, "");
        let path = sources.path().join(format!("{version}.zip"));
        zip_of(&path, &[("extension.toml", text.as_bytes())])
    };
    install_extension(&version("1.0.0"), &extensions).unwrap();
    let dir = extensions.join("acme.tool");
    let api = check_api(Vec::new());
    // Renames: the old one aside, the new one in, the old one back.
    let failing = |fails: &'static [usize]| {
        let calls = Cell::new(0);
        move |from: &Path, to: &Path| {
            calls.set(calls.get() + 1);
            if fails.contains(&calls.get()) {
                return Err(io::Error::other("the disk is full"));
            }
            fs::rename(from, to)
        }
    };

    // The new one not moved in, the old one is put back.
    let staged = stage(&version("2.0.0"), &extensions, &api).unwrap();
    let error = staged.place_with(failing(&[2])).unwrap_err();
    assert_eq!(error, InstallError::Io("the disk is full".to_owned()));
    assert_eq!(installed_version(&dir), "1.0.0");
    assert_eq!(names(&extensions), ["acme.tool"]);

    // Nor that, it's kept, and the error says where.
    let staged = stage(&version("2.0.0"), &extensions, &api).unwrap();
    let Err(InstallError::Io(error)) = staged.place_with(failing(&[2, 3])) else {
        panic!("not an error");
    };
    let (_, kept) = error
        .split_once("The installed version couldn't be put back: it's kept in ")
        .unwrap_or_else(|| panic!("{error}"));
    assert_eq!(installed_version(Path::new(kept)), "1.0.0");
    assert!(!dir.exists());

    // One installed in a directory named as a staging directory is still
    // the one replaced: the extensions service loads it.
    let hidden = extensions.join(".installing-old");
    install_extension(&version("1.0.0"), &extensions).unwrap();
    fs::rename(&dir, &hidden).unwrap();
    let staged = stage(&version("2.0.0"), &extensions, &api).unwrap();
    assert_eq!(staged.replaced_dir(), Some(hidden.as_path()));
    assert_eq!(staged.replaces(), Some("1.0.0"));
    staged.place().unwrap();
    let kept = names(&extensions)
        .into_iter()
        .filter(|name| name.starts_with(".installing-"));
    let kept: Vec<String> = kept.filter(|name| name != ".installing-old").collect();
    assert_eq!(installed_version(&dir), "2.0.0");
    assert!(!hidden.exists());
    // The copy kept above is the only other.
    assert_eq!(kept.len(), 1, "{kept:?}");

    // One file under two spellings is one: it's whose the directory is.
    #[cfg(unix)]
    {
        let linked = sources.path().join("linked");
        std::os::unix::fs::symlink(&extensions, &linked).unwrap();
        assert!(same_file(&dir, &linked.join("acme.tool")));
        assert!(!same_file(&dir, &extensions.join(&kept[0])));
    }

    // A directory of its id holding another isn't replaced.
    let other = tempfile::tempdir().unwrap();
    fs::create_dir(other.path().join("acme.tool")).unwrap();
    let text = manifest("acme.other", "1.0.0", "");
    fs::write(other.path().join("acme.tool/extension.toml"), text).unwrap();
    let reason = refusal(install_extension(&version("2.0.0"), other.path()));
    assert!(
        reason.ends_with("acme.tool is there already, and isn't acme.tool"),
        "{reason}"
    );
    assert_eq!(installed_version(&other.path().join("acme.tool")), "1.0.0");
}

#[gpui::test]
fn an_installed_plugin_waits_for_its_grant(cx: &mut TestAppContext) {
    // Plugins run on threads of their own.
    cx.executor().allow_parking();
    let sources = tempfile::tempdir().unwrap();
    let dirs = tempfile::tempdir().unwrap();
    let extensions = dirs.path().join("extensions");
    let cache = dirs.path().join("cache");
    start(cx, &extensions);
    cx.update(|cx| {
        set_plugin_dirs(cx, dirs.path().join("state"), cache.clone());
        set_preview(cx, true);
    });
    let plugin = |id: &str| {
        let text = manifest(id, "1.0.0", PLUGIN);
        let files: [(&str, &[u8]); 2] = [
            ("extension.toml", text.as_bytes()),
            ("plugin.wasm", COMMANDS.as_bytes()),
        ];
        zip_of(&sources.path().join(format!("{id}.zip")), &files)
    };
    let (workbench, vcx) = window(cx);

    // Installed with the preview on, it says it asks; once the extensions
    // are read again, it asks, and loads nothing meanwhile.
    install(vcx, &plugin("acme.tool"));
    let detail = "It asks before it runs".to_owned();
    assert_eq!(
        toasts(&workbench, vcx),
        [("Installed acme.tool 1.0.0".to_owned(), detail)]
    );
    let set = ExtensionSet::discover(&extensions, &check_api(Vec::new()));
    vcx.update(|_, cx| set_plugin_extensions(cx, &set));
    vcx.run_until_parked();
    let (question, _) = vcx.pending_prompt().expect("the plugin's grant prompt");
    assert_eq!(question, "Allow the plugin acme.tool to load?");
    let state = vcx.update(|_, cx| plugin_states(cx)).remove(0).state;
    assert_eq!(state, PluginState::WaitingForGrant);
    let compiled = fs::read_dir(cache.join(CACHE_DIR)).map_or(0, |entries| entries.count());
    assert_eq!(compiled, 0, "nothing compiled or loaded before Allow");
    vcx.simulate_prompt_answer("Don't Allow");
    vcx.run_until_parked();

    // With it off, the toast says to turn it on.
    vcx.update(|_, cx| set_preview(cx, false));
    install(vcx, &plugin("acme.other"));
    let last = toasts(&workbench, vcx).pop().unwrap();
    let detail = "Turn on extensions.plugins_preview to run it".to_owned();
    assert_eq!(last, ("Installed acme.other 1.0.0".to_owned(), detail));
}
