// SPDX-License-Identifier: Apache-2.0

//! What differs between operating systems, as data. This directory and
//! `kiln-platform`'s sources are the only places in kiln that branch on the
//! operating system (kiln's `docs/framework.md` §4); everything else
//! asks [`Os`], so it can be tested "as Linux" on any host.

#[cfg(windows)]
mod acl;
pub(crate) mod env;
pub(crate) mod fs;

pub use env::{
    minimal_environment, minimal_environment_for, system_path, system_path_for, unix_environment,
    windows_environment,
};

pub use fs::{
    Owner, canonicalize, create_private_dir, file_identity, is_executable, modified_parts,
    open_dir, open_for_reading, owned_and_unshared, path_identity, private, program_candidates,
    replace_file, sync_dir,
};
pub(crate) mod keychain;
pub(crate) mod process_tree;
pub(crate) mod stdio;

pub use process_tree::ProcessTree;
pub use stdio::{OutputDrain, OutputSource, StdinFeed, without_console_window};

use std::fmt;

use serde::{Deserialize, Serialize};

/// An operating system kiln runs on.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Os {
    MacOs,
    Linux,
    Windows,
}

impl Os {
    /// Every one, in the order filters and keymaps name them.
    pub const ALL: [Os; 3] = [Os::MacOs, Os::Linux, Os::Windows];

    /// Its name in keymaps and menu tables: `macos`, `linux`, `windows`.
    pub const fn name(self) -> &'static str {
        match self {
            Os::MacOs => "macos",
            Os::Linux => "linux",
            Os::Windows => "windows",
        }
    }

    /// The one named `name`, as [`Os::name`] gives it.
    pub fn from_name(name: &str) -> Option<Os> {
        Os::ALL.into_iter().find(|os| os.name() == name)
    }

    /// Whether `secondary-` in a keystroke means cmd (macOS) rather than
    /// ctrl (everywhere else): the modifier an app's own shortcuts use.
    pub const fn secondary_is_cmd(self) -> bool {
        matches!(self, Os::MacOs)
    }

    /// The one this process runs on. The BSDs and other Unixes count as
    /// Linux: they share its conventions (XDG directories, ctrl shortcuts).
    pub const fn current() -> Os {
        if cfg!(target_os = "macos") {
            Os::MacOs
        } else if cfg!(target_os = "windows") {
            Os::Windows
        } else {
            Os::Linux
        }
    }
}

/// Which platforms a binding or menu item is for: `"macos"`, `"!macos"`
/// (all but macOS), or a comma-separated list of either kind, such as
/// `"linux, windows"`. Negations exclude; names include, and a filter with
/// no names includes everything not excluded.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlatformFilter {
    include: Vec<Os>,
    exclude: Vec<Os>,
}

impl PlatformFilter {
    pub fn parse(source: &str) -> Result<Self, String> {
        let mut filter = PlatformFilter {
            include: Vec::new(),
            exclude: Vec::new(),
        };
        for part in source.split(',').map(str::trim) {
            let (name, list) = match part.strip_prefix('!') {
                Some(name) => (name.trim(), &mut filter.exclude),
                None => (part, &mut filter.include),
            };
            let os = Os::from_name(name).ok_or_else(|| {
                let name = format!("{name:?}");
                kiln_l10n::tr!("kiln-core-platform-unknown", name = name).to_string()
            })?;
            list.push(os);
        }
        Ok(filter)
    }

    pub fn matches(&self, os: Os) -> bool {
        !self.exclude.contains(&os) && (self.include.is_empty() || self.include.contains(&os))
    }
}

impl fmt::Display for PlatformFilter {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let names = self
            .include
            .iter()
            .map(|os| os.name().to_owned())
            .chain(self.exclude.iter().map(|os| format!("!{}", os.name())));
        f.write_str(&names.collect::<Vec<_>>().join(", "))
    }
}
