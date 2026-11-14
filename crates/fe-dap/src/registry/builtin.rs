// SPDX-License-Identifier: Apache-2.0

//! §3's built-in adapters as data, and the rules that finish them for a
//! launch: the project's Python interpreter, the Rust toolchain's LLDB
//! formatters, and where lldb-dap is looked for on each OS.

use std::path::{Path, PathBuf};

use crate::launcher::{AdapterSpec, AdapterTransport, PORT_PLACEHOLDER};

use super::{Entry, Hint, Program, Source};

/// The built-in adapters, in the order a language's default is chosen:
/// lldb-dap before codelldb for Rust, C and C++ (§13 Q3). js-debug waits
/// for multi-session support (D23).
pub(super) fn entries() -> Vec<Entry> {
    let lldb = ["rust", "c", "cpp"];
    let spec = |name: &str, command: &str, args: &[&str], languages: &[&str], types: &[&str]| {
        AdapterSpec {
            args: args.iter().map(|&arg| arg.into()).collect(),
            languages: languages.iter().map(|&language| language.into()).collect(),
            types: types.iter().map(|&ty| ty.into()).collect(),
            ..AdapterSpec::new(name, command)
        }
    };
    let entry = |spec: AdapterSpec, program: Program, hint: Hint| Entry {
        source: Source::Builtin,
        spec,
        program,
        search: Vec::new(),
        rust_formatters: false,
        hint,
    };
    let delve_listen = format!("127.0.0.1:{PORT_PLACEHOLDER}");
    vec![
        // LLVM's, also in Xcode: on macOS found with `xcrun` too.
        Entry {
            rust_formatters: true,
            ..entry(
                spec(LLDB_DAP, LLDB_DAP, &[], &lldb, &[LLDB_DAP]),
                Program::LldbDap,
                Hint::LldbDap,
            )
        },
        // The binary in the CodeLLDB extension's `adapter/` folder. It has
        // its own Rust support, so it needs no formatters of ours.
        entry(
            spec("codelldb", "codelldb", &[], &lldb, &["lldb"]),
            Program::Command,
            Hint::Codelldb,
        ),
        // `<python> -m debugpy.adapter`: the command is the project's
        // interpreter ([`python_interpreter`]).
        entry(
            spec(
                "debugpy",
                PYTHON3,
                &["-m", "debugpy.adapter"],
                &["python"],
                &["debugpy", "python"],
            ),
            Program::Python,
            Hint::Debugpy,
        ),
        // As data: tested end to end in D19.
        entry(
            AdapterSpec {
                transport: AdapterTransport::Tcp,
                ..spec(
                    "delve",
                    "dlv",
                    &["dap", "--listen", &delve_listen],
                    &["go"],
                    &["go"],
                )
            },
            Program::Command,
            Hint::Delve,
        ),
    ]
}

const LLDB_DAP: &str = "lldb-dap";

/// The interpreter looked up on the `PATH` with no virtual environment.
const PYTHON3: &str = if cfg!(windows) { "python" } else { "python3" };

/// The OS whose ways of finding adapters apply: on macOS, lldb-dap is also
/// found through `xcrun` ([`lldb_dap_lookups`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HostOs {
    MacOs,
    Linux,
    Windows,
}

impl HostOs {
    /// The OS this is built for.
    pub const fn current() -> Self {
        if cfg!(target_os = "macos") {
            Self::MacOs
        } else if cfg!(windows) {
            Self::Windows
        } else {
            Self::Linux
        }
    }
}

/// One way of finding an adapter's program.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lookup {
    /// Its command, on the login shell's `PATH`.
    OnPath,
    /// What `program args`, run in the root, prints: an absolute path.
    Run {
        program: &'static str,
        args: &'static [&'static str],
    },
}

/// Xcode's tool finder, which knows where the selected Xcode keeps
/// lldb-dap.
pub const XCRUN: &str = "/usr/bin/xcrun";

/// Where lldb-dap is looked for on `os`, in order: on the `PATH`, then on
/// macOS what `xcrun -f lldb-dap` prints.
pub fn lldb_dap_lookups(os: HostOs) -> Vec<Lookup> {
    let mut lookups = vec![Lookup::OnPath];
    if os == HostOs::MacOs {
        lookups.push(Lookup::Run {
            program: XCRUN,
            args: &["-f", LLDB_DAP],
        });
    }
    lookups
}

/// The project's Python (§3), the first of `<root>/.venv/bin/python`,
/// `$VIRTUAL_ENV/bin/python` (`Scripts\python.exe` on Windows) that
/// `runnable` passes, else `python3` (`python` on Windows), a bare name to
/// look up on the `PATH`.
///
/// `.venv` is the project's own: running it is behind folder trust (§8),
/// as launching anything is.
pub fn python_interpreter(
    root: &Path,
    virtual_env: Option<&Path>,
    runnable: impl Fn(&Path) -> bool,
) -> PathBuf {
    let in_env = if cfg!(windows) {
        Path::new(kiln_l10n::verbatim!("Scripts")).join("python.exe")
    } else {
        Path::new("bin").join("python")
    };
    std::iter::once(root.join(".venv").join(&in_env))
        .chain(virtual_env.map(|env| env.join(&in_env)))
        .find(|python| runnable(python))
        .unwrap_or_else(|| PYTHON3.into())
}

/// The LLDB commands that import the Rust toolchain's formatters from
/// `sysroot`, what `rustc --print sysroot` printed (§3), so `Vec` and
/// `String` show as values rather than raw structs. None for a sysroot
/// that isn't one absolute path on one line.
pub fn rust_init_commands(sysroot: &str) -> Vec<String> {
    let sysroot = sysroot.trim_end_matches(['\n', '\r']);
    if sysroot.is_empty() || sysroot.contains(['\n', '\r']) || !Path::new(sysroot).is_absolute() {
        return Vec::new();
    }
    let etc = Path::new(sysroot).join("lib").join("rustlib").join("etc");
    let (Some(lookup), Some(commands)) = (
        etc.join("lldb_lookup.py").to_str().map(lldb_quoted),
        etc.join("lldb_commands").to_str().map(lldb_quoted),
    ) else {
        return Vec::new();
    };
    vec![
        format!("command script import {lookup}"),
        format!("command source {commands}"),
    ]
}

/// `text` as one LLDB command argument: in double quotes, with `\` and `"`
/// escaped, so a path with spaces stays whole.
fn lldb_quoted(text: &str) -> String {
    let mut quoted = String::with_capacity(text.len() + 2);
    quoted.push('"');
    for c in text.chars() {
        if matches!(c, '\\' | '"') {
            quoted.push('\\');
        }
        quoted.push(c);
    }
    quoted.push('"');
    quoted
}
