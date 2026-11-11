// SPDX-License-Identifier: Apache-2.0

//! What a plugin reaches on disk through WASI: the guest in `tests/guests/fs/` reads, writes and
//! looks at its environment under the context [`wasi_context`] builds from its grants.

use std::fs;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::OnceLock;

use kiln_command::capability::{Capability, Grants};
use kiln_plugin::fs::Workspace;
use kiln_plugin::runner::wasi_context;
use wasmtime::component::{Component, ComponentNamedList, Instance, Lift, Linker, Lower};
use wasmtime::{Engine, Store};
use wasmtime_wasi::{ResourceTable, WasiCtx, WasiCtxView, WasiView};

/// What the guest writes to its stdout and stderr in [`stdio_reaches_nothing_of_the_host`].
const MARKER: &str = "kiln-plugin-fs-guest-wrote-this";

fn grants(capabilities: &[&str]) -> Grants {
    capabilities
        .iter()
        .map(|capability| Capability::parse(capability).unwrap())
        .collect()
}

/// A directory of its own under cargo's temporary directory for tests, with a `root` folder
/// holding `hello.txt` and an `outside` folder beside it holding `secret.txt`.
fn temp_dir(name: &str) -> PathBuf {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("kiln-plugin-fs-{}-{name}", std::process::id()));
    fs::remove_dir_all(&dir).ok();
    fs::create_dir_all(dir.join("root")).unwrap();
    fs::create_dir_all(dir.join("outside")).unwrap();
    fs::write(dir.join("root/hello.txt"), "hello").unwrap();
    fs::write(dir.join("outside/secret.txt"), "secret").unwrap();
    kiln_core::platform::canonicalize(&dir).unwrap()
}

fn workspace(dir: &Path) -> Workspace {
    Workspace {
        roots: vec![dir.join("root")],
    }
}

/// A path as the guest is given it.
fn guest_path(path: &Path) -> String {
    path.to_str().unwrap().to_owned()
}

struct Ctx {
    wasi: WasiCtx,
    table: ResourceTable,
}

impl WasiView for Ctx {
    fn ctx(&mut self) -> WasiCtxView<'_> {
        WasiCtxView {
            ctx: &mut self.wasi,
            table: &mut self.table,
        }
    }
}

/// The fs guest, instantiated with the WASI context a plugin with `grants` gets in `workspace`.
struct Guest {
    store: Store<Ctx>,
    instance: Instance,
}

/// The guest, compiled once per test process: compiling it takes most of a second.
fn component() -> &'static (Engine, Component) {
    static COMPILED: OnceLock<(Engine, Component)> = OnceLock::new();
    COMPILED.get_or_init(|| {
        let engine = Engine::default();
        let component = Component::from_file(&engine, "tests/guests/fs/fs.wasm").unwrap();
        (engine, component)
    })
}

impl Guest {
    fn load(grants: &Grants, workspace: &Workspace) -> Self {
        let (engine, component) = component();
        let mut linker = Linker::new(engine);
        wasmtime_wasi::p2::add_to_linker_sync(&mut linker).unwrap();
        let ctx = Ctx {
            wasi: wasi_context(grants, workspace),
            table: ResourceTable::new(),
        };
        let mut store = Store::new(engine, ctx);
        let instance = linker.instantiate(&mut store, component).unwrap();
        Self { store, instance }
    }

    fn call<P, R>(&mut self, name: &str, params: P) -> R
    where
        P: ComponentNamedList + Lower,
        R: Lift + 'static,
    {
        let func = self
            .instance
            .get_typed_func::<P, (R,)>(&mut self.store, name)
            .unwrap();
        func.call(&mut self.store, params).unwrap().0
    }

    fn read(&mut self, path: &Path) -> Result<String, String> {
        self.call("read-file", (guest_path(path),))
    }

    fn write(&mut self, path: &Path, contents: &str) -> Result<(), String> {
        self.call("write-file", (guest_path(path), contents.to_owned()))
    }
}

#[test]
#[cfg_attr(
    windows,
    ignore = "a root is preopened at its host path, which wasi-libc can't resolve on Windows"
)]
fn a_guest_reads_inside_a_preopened_folder() {
    let dir = temp_dir("reads");
    let mut guest = Guest::load(&grants(&["fs.read:workspace"]), &workspace(&dir));
    assert_eq!(guest.read(&dir.join("root/hello.txt")), Ok("hello".into()));
}

#[test]
fn nothing_is_preopened_without_a_grant() {
    let dir = temp_dir("no-grant");
    for granted in [&[][..], &["buffer.read", "process.spawn:*", "net:*"]] {
        let mut guest = Guest::load(&grants(granted), &workspace(&dir));
        let read = guest.read(&dir.join("root/hello.txt"));
        assert!(read.is_err(), "{granted:?} read {read:?}");
    }
}

#[test]
#[cfg_attr(
    windows,
    ignore = "a root is preopened at its host path, which wasi-libc can't resolve on Windows"
)]
fn read_alone_cannot_write_and_write_can() {
    let dir = temp_dir("write");
    let written = dir.join("root/written.txt");
    let mut guest = Guest::load(&grants(&["fs.read:workspace"]), &workspace(&dir));
    assert!(guest.write(&written, "from the guest").is_err());
    assert!(guest.write(&dir.join("root/hello.txt"), "changed").is_err());
    assert!(!written.exists());
    assert_eq!(
        fs::read_to_string(dir.join("root/hello.txt")).unwrap(),
        "hello"
    );

    let mut guest = Guest::load(&grants(&["fs.write:workspace"]), &workspace(&dir));
    assert_eq!(guest.write(&written, "from the guest"), Ok(()));
    assert_eq!(fs::read_to_string(&written).unwrap(), "from the guest");
    assert_eq!(guest.read(&written), Ok("from the guest".into()));
}

/// A symbolic link at `link` to `target`, a file, or a folder if `folder`: Windows tells the two
/// apart.
fn symlink(target: &Path, link: &Path, folder: bool) {
    #[cfg(unix)]
    {
        let _ = folder;
        std::os::unix::fs::symlink(target, link).unwrap();
    }
    #[cfg(windows)]
    if folder {
        std::os::windows::fs::symlink_dir(target, link).unwrap();
    } else {
        std::os::windows::fs::symlink_file(target, link).unwrap();
    }
}

#[test]
#[cfg_attr(
    windows,
    ignore = "a root is preopened at its host path, which wasi-libc can't resolve on Windows"
)]
fn dotdot_and_symlinks_cannot_leave_the_root() {
    let dir = temp_dir("escape");
    // Link targets in this OS's own separators: Windows reads a `/` in one as part of a name.
    let secret = dir.join("outside").join("secret.txt");
    symlink(&secret, &dir.join("root/absolute"), false);
    let relative: PathBuf = ["..", "outside", "secret.txt"].iter().collect();
    symlink(&relative, &dir.join("root/relative"), false);
    symlink(&dir.join("outside"), &dir.join("root/folder"), true);
    // The widest grant: nothing it allows reaches outside the root either.
    let mut guest = Guest::load(&grants(&["fs.write:workspace"]), &workspace(&dir));

    let dotdot = dir.join("root/../outside/secret.txt");
    let through_folder = dir.join("root/folder/secret.txt");
    for path in [
        &dotdot,
        &dir.join("root/absolute"),
        &dir.join("root/relative"),
        &through_folder,
    ] {
        let read = guest.read(path);
        assert!(read.is_err(), "{} read {read:?}", path.display());
        let write = guest.write(path, "overwritten");
        assert!(write.is_err(), "{} written: {write:?}", path.display());
    }
    assert!(
        guest
            .write(&dir.join("root/../outside/new.txt"), "new")
            .is_err()
    );
    assert!(
        guest
            .write(&dir.join("root/folder/new.txt"), "new")
            .is_err()
    );
    assert_eq!(fs::read_to_string(&secret).unwrap(), "secret");
    assert!(!dir.join("outside/new.txt").exists());
    // The root itself still works.
    assert_eq!(guest.read(&dir.join("root/hello.txt")), Ok("hello".into()));
}

#[test]
fn the_wasi_environment_is_empty() {
    // Stdio is checked in a child test process, with stdin of its own to read and its stdout
    // and stderr captured: the guest must see none of the first, and write to neither. It runs
    // while this process checks the rest.
    let child = stdio_child();
    let dir = temp_dir("environment");
    let mut guest = Guest::load(&grants(&["fs.write:workspace"]), &workspace(&dir));
    assert_eq!(
        guest.call::<_, Vec<(String, String)>>("environment", ()),
        []
    );
    assert_eq!(guest.call::<_, Vec<String>>("arguments", ()), [""; 0]);

    let output = child.wait_with_output().unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "{stdout}\n{stderr}");
    assert!(
        stdout.contains("1 passed"),
        "the child ran no test:\n{stdout}"
    );
    assert!(
        !stdout.contains(MARKER),
        "the guest wrote to stdout:\n{stdout}"
    );
    assert!(
        !stderr.contains(MARKER),
        "the guest wrote to stderr:\n{stderr}"
    );
}

/// This test binary, running [`stdio_reaches_nothing_of_the_host`] with bytes on its stdin.
fn stdio_child() -> Child {
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "stdio_reaches_nothing_of_the_host",
            "--ignored",
            "--nocapture",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    stdin.write_all(b"the host's stdin\n").unwrap();
    drop(stdin);
    child
}

/// Run by [`the_wasi_environment_is_empty`] in a child process.
#[test]
#[ignore = "run by the_wasi_environment_is_empty in a child process"]
fn stdio_reaches_nothing_of_the_host() {
    let dir = temp_dir("stdio");
    let mut guest = Guest::load(&grants(&["fs.write:workspace"]), &workspace(&dir));
    let stdin = guest.call::<_, Result<Vec<u8>, String>>("stdio", (MARKER.to_owned(),));
    assert_eq!(stdin, Ok(Vec::new()));
}
