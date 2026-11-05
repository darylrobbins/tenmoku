// SPDX-License-Identifier: Apache-2.0

//! What a plugin's capabilities let it do, in plain words, for the prompt
//! that grants them (`docs/decisions/vmtnmkay62-extensions-and-plugins.md`
//! §7): what each capability does ([`describe`]), the warnings its
//! combinations call for ([`combination_warnings`]), and whether it holds
//! a wildcard, full access ([`is_full_access`]).
//!
//! A capability list alone doesn't say that `buffer.read` with
//! `net:api.example.com` can send your code to that host, or that
//! `process.spawn:sh` runs anything: the warnings do. `net:*` and
//! `process.spawn:*` are extreme, with a warning, and an acceptance, of
//! their own ([`crate::grants::needs_wildcard_acceptance`]). Plain
//! functions: the prompt itself is the app's.

use std::fmt;

use kiln_command::capability::{Capability, Grants, Target};

/// Programs that run whatever code or program they're given, so granting
/// one is nearly `process.spawn:*`: shells, language runtimes and
/// launchers. Matched without case or a version suffix, so `python3.12`
/// and `Python3` are `python`'s.
const INTERPRETERS: &[&str] = &[
    "sh",
    "bash",
    "zsh",
    "fish",
    "dash",
    "ksh",
    "csh",
    "tcsh",
    "ash",
    "mksh",
    "pwsh",
    "node",
    "nodejs",
    "python",
    "pypy",
    "deno",
    "bun",
    "bunx",
    "npx",
    "uvx",
    "ruby",
    "perl",
    "php",
    "lua",
    "luajit",
    "java",
    "osascript",
    "busybox",
    "env",
    "xargs",
    "sudo",
    "doas",
];

/// Something a plugin's capabilities let it do that the user should know
/// before granting them: [`combination_warnings`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Warning {
    /// `buffer.read` or `fs.read` with `net:<host>` grants: it can read your
    /// code and send it to these hosts.
    SendsCode { hosts: Vec<String> },
    /// `net:*`, extreme: it can send your code to any host.
    SendsAnywhere,
    /// `process.spawn:<program>` grants: these programs run outside the
    /// sandbox, with your permissions.
    OutsideSandbox { programs: Vec<String> },
    /// One of those programs runs any code it's given, as a shell or a
    /// language runtime does.
    NearWildcard { program: String },
    /// `process.spawn:*`, extreme: it can run any program.
    RunsAnyProgram,
}

impl Warning {
    /// Whether this is a wildcard's own warning.
    pub fn is_extreme(&self) -> bool {
        matches!(self, Self::SendsAnywhere | Self::RunsAnyProgram)
    }
}

impl fmt::Display for Warning {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SendsCode { hosts } => {
                write!(
                    f,
                    "It can read your code and send it to {}.",
                    and_list(hosts)
                )
            }
            Self::SendsAnywhere => f.write_str(
                "Extreme: with net:* it can send your code anywhere, to any host on the internet.",
            ),
            Self::OutsideSandbox { programs } => write!(
                f,
                "It runs {} outside the sandbox, with your permissions.",
                and_list(programs)
            ),
            Self::NearWildcard { program } => write!(
                f,
                "{program} is near-wildcard: it can run any code, so granting it is almost \
                 process.spawn:*."
            ),
            Self::RunsAnyProgram => f.write_str(
                "Extreme: with process.spawn:* it can run any program as you, outside the sandbox.",
            ),
        }
    }
}

/// The warnings `grants` call for, extreme ones first.
///
/// Reading code (`buffer.read`, `fs.read`) with named hosts can send it to
/// them; `net:*` can send it anywhere, read grant or not, and stands in for
/// the named hosts. Named programs run outside the sandbox, and an
/// interpreter among them can run anything; `process.spawn:*` can run any
/// program, and stands in for them.
pub fn combination_warnings(grants: &Grants) -> Vec<Warning> {
    let mut warnings = Vec::new();
    let reads = grants
        .iter()
        .any(|capability| matches!(capability, Capability::BufferRead | Capability::FsRead(_)));
    let net = grants.net_hosts();
    if net.any {
        warnings.push(Warning::SendsAnywhere);
    }
    let spawns_any = grants.allows(&Capability::ProcessSpawn(Target::Any));
    if spawns_any {
        warnings.push(Warning::RunsAnyProgram);
    }
    if !net.any && reads && !net.hosts.is_empty() {
        warnings.push(Warning::SendsCode {
            hosts: net.hosts.iter().map(|host| (*host).to_owned()).collect(),
        });
    }
    if !spawns_any {
        let programs: Vec<String> = grants
            .iter()
            .filter_map(|capability| match capability {
                Capability::ProcessSpawn(Target::Named(program)) => Some(program.clone()),
                _ => None,
            })
            .collect();
        let interpreters = programs
            .iter()
            .filter(|program| is_interpreter(program))
            .map(|program| Warning::NearWildcard {
                program: program.clone(),
            })
            .collect::<Vec<_>>();
        if !programs.is_empty() {
            warnings.push(Warning::OutsideSandbox { programs });
        }
        warnings.extend(interpreters);
    }
    warnings
}

/// Whether `grants` hold `net:*` or `process.spawn:*`: full access, which
/// the plugin list marks.
pub fn is_full_access(grants: &Grants) -> bool {
    crate::grants::needs_wildcard_acceptance(grants)
}

/// Whether `program`, a `process.spawn` grant's name, runs any code it's
/// given: one of [`INTERPRETERS`], without case or a version suffix.
pub fn is_interpreter(program: &str) -> bool {
    let name = program.to_ascii_lowercase();
    let unversioned = name.trim_end_matches(|c: char| c.is_ascii_digit() || c == '.' || c == '-');
    INTERPRETERS.contains(&name.as_str()) || INTERPRETERS.contains(&unversioned)
}

/// What `capability` lets a plugin do, in plain words.
pub fn describe(capability: &Capability) -> String {
    match capability {
        Capability::BufferRead => "read the text of your open files".to_owned(),
        Capability::BufferWrite => "edit your open files".to_owned(),
        Capability::Commands => "add commands, and run other commands".to_owned(),
        Capability::Decorations => "mark up text and the gutter in your files".to_owned(),
        Capability::Status => "show items in the status bar".to_owned(),
        Capability::FsRead(scope) => format!("read any file in your {scope} folders"),
        Capability::FsWrite(scope) => format!("write any file in your {scope} folders"),
        Capability::ProcessSpawn(Target::Named(program)) => {
            format!("run the program {program}, outside the sandbox")
        }
        Capability::ProcessSpawn(Target::Any) => "run any program, outside the sandbox".to_owned(),
        Capability::Net(Target::Named(host)) => format!("connect to {host}"),
        Capability::Net(Target::Any) => "connect to any host on the internet".to_owned(),
    }
}

/// `items` as "a", "a and b" or "a, b and c".
fn and_list(items: &[String]) -> String {
    match items {
        [] => String::new(),
        [one] => one.clone(),
        [rest @ .., last] => format!("{} and {last}", rest.join(", ")),
    }
}
