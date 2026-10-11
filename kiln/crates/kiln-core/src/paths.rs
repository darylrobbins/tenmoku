// SPDX-License-Identifier: LicenseRef-Proprietary

//! Where an app keeps its files, keyed by its [`AppId`]: its config
//! (settings, keymap, themes), its state (what it records about this
//! machine, such as trust decisions and layouts), and its cache.

use std::{
    ffi::OsString,
    path::{Path, PathBuf},
};

use crate::platform::Os;

/// The settings file's name, in the user and project config directories.
pub const SETTINGS_FILE: &str = "settings.toml";

/// The keymap file's name, in the user config directory.
pub const KEYMAP_FILE: &str = "keymap.toml";

/// The themes directory's name, in the user config directory.
pub const THEMES_DIR: &str = "themes";

/// The workspace trust file's name, in the state directory.
pub const TRUST_FILE: &str = "trust.toml";

/// Who the app is, for the directories it keeps files in and for the
/// platform (a macOS bundle ID, a Wayland app ID).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AppId {
    /// Reverse DNS, such as `com.example.notes`.
    pub reverse_dns: String,
    /// A short lowercase name, such as `notes`: the XDG directory name, the
    /// project directory (`.notes`) and the environment prefix
    /// (`NOTES_CONFIG_DIR`).
    pub short: String,
    /// The name people see, such as `Notes`.
    pub display: String,
    /// The publisher, for Windows' `%APPDATA%\<vendor>\<display>`.
    pub vendor: Option<String>,
}

impl AppId {
    pub fn new(
        reverse_dns: impl Into<String>,
        short: impl Into<String>,
        display: impl Into<String>,
    ) -> Self {
        Self {
            reverse_dns: reverse_dns.into(),
            short: short.into(),
            display: display.into(),
            vendor: None,
        }
    }

    pub fn vendor(mut self, vendor: impl Into<String>) -> Self {
        self.vendor = Some(vendor.into());
        self
    }

    /// The environment variable that overrides a directory: `NOTES_CONFIG_DIR`.
    fn env_var(&self, suffix: &str) -> String {
        let prefix: String = self
            .short
            .chars()
            .map(|ch| {
                if ch.is_ascii_alphanumeric() {
                    ch.to_ascii_uppercase()
                } else {
                    '_'
                }
            })
            .collect();
        format!("{prefix}_{suffix}")
    }
}

/// Where the app's directories go.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub enum PathStyle {
    /// Each platform's own place: `~/Library/Application Support/<id>` on
    /// macOS, the XDG directories on Linux, `%APPDATA%` on Windows.
    #[default]
    Native,
    /// The XDG directories on every platform (`~/.config/<short>`), as
    /// command-line tools and editors often keep them.
    Xdg,
}

/// The app's directories, resolved once. A directory is `None` when the
/// environment names no home to put it in.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AppPaths {
    short: String,
    config: Option<PathBuf>,
    state: Option<PathBuf>,
    cache: Option<PathBuf>,
}

impl AppPaths {
    /// The directories from the process environment, on this platform.
    pub fn new(id: &AppId, style: PathStyle) -> Self {
        Self::resolve(id, style, Os::current(), |name| std::env::var_os(name))
    }

    /// The directories on `os`, reading variables through `env`.
    ///
    /// `<SHORT>_CONFIG_DIR` and `<SHORT>_STATE_DIR` win over everything.
    /// Under [`PathStyle::Xdg`], and natively on Linux, the config directory
    /// is `$XDG_CONFIG_HOME/<short>` else `$HOME/.config/<short>`, and the
    /// state and cache ones follow `XDG_STATE_HOME` (`~/.local/state`) and
    /// `XDG_CACHE_HOME` (`~/.cache`). Empty variables count as unset, and so
    /// does a relative `XDG_*` one, as the XDG spec says.
    pub fn resolve(
        id: &AppId,
        style: PathStyle,
        os: Os,
        env: impl Fn(&str) -> Option<OsString>,
    ) -> Self {
        let var = |name: &str| {
            env(name)
                .filter(|value| !value.is_empty())
                .map(PathBuf::from)
        };
        let home = var("HOME");
        let xdg = |name: &str, fallback: &[&str]| {
            var(name)
                .filter(|dir| dir.is_absolute())
                .or_else(|| {
                    home.as_ref().map(|home| {
                        fallback
                            .iter()
                            .fold(home.clone(), |dir, part| dir.join(part))
                    })
                })
                .map(|dir| dir.join(&id.short))
        };
        let (config, state, cache) = match (style, os) {
            (PathStyle::Xdg, _) | (PathStyle::Native, Os::Linux) => (
                xdg("XDG_CONFIG_HOME", &[".config"]),
                xdg("XDG_STATE_HOME", &[".local", "state"]),
                xdg("XDG_CACHE_HOME", &[".cache"]),
            ),
            (PathStyle::Native, Os::MacOs) => {
                let library = home.as_ref().map(|home| home.join("Library"));
                let support = library
                    .as_ref()
                    .map(|dir| dir.join("Application Support").join(&id.reverse_dns));
                (
                    support.clone(),
                    support.map(|dir| dir.join("State")),
                    library.map(|dir| dir.join("Caches").join(&id.reverse_dns)),
                )
            }
            (PathStyle::Native, Os::Windows) => {
                let under = |root: Option<PathBuf>| {
                    root.map(|root| match &id.vendor {
                        Some(vendor) => root.join(vendor).join(&id.display),
                        None => root.join(&id.display),
                    })
                };
                let roaming = under(var("APPDATA"));
                let local = under(var("LOCALAPPDATA"));
                (
                    roaming,
                    local.as_ref().map(|dir| dir.join("State")),
                    local.map(|dir| dir.join("Cache")),
                )
            }
        };
        Self {
            short: id.short.clone(),
            config: var(&id.env_var("CONFIG_DIR")).or(config),
            state: var(&id.env_var("STATE_DIR")).or(state),
            cache,
        }
    }

    pub fn config_dir(&self) -> Option<&Path> {
        self.config.as_deref()
    }

    pub fn state_dir(&self) -> Option<&Path> {
        self.state.as_deref()
    }

    pub fn cache_dir(&self) -> Option<&Path> {
        self.cache.as_deref()
    }

    /// `<config>/settings.toml`.
    pub fn settings_file(&self) -> Option<PathBuf> {
        self.config.as_ref().map(|dir| dir.join(SETTINGS_FILE))
    }

    /// `<config>/keymap.toml`.
    pub fn keymap_file(&self) -> Option<PathBuf> {
        self.config.as_ref().map(|dir| dir.join(KEYMAP_FILE))
    }

    /// `<config>/themes`.
    pub fn themes_dir(&self) -> Option<PathBuf> {
        self.config.as_ref().map(|dir| dir.join(THEMES_DIR))
    }

    /// `<state>/trust.toml`.
    pub fn trust_file(&self) -> Option<PathBuf> {
        self.state.as_ref().map(|dir| dir.join(TRUST_FILE))
    }

    /// `<state>/layouts`: the saved window layouts, a file per root folder
    /// ([`crate::layout`]).
    pub fn layouts_dir(&self) -> Option<PathBuf> {
        self.state.as_ref().map(|dir| dir.join("layouts"))
    }

    /// `<root>/.<short>`: a project's own config directory.
    pub fn project_dir(&self, root: &Path) -> PathBuf {
        root.join(format!(".{}", self.short))
    }

    /// `<root>/.<short>/settings.toml`.
    pub fn project_settings_file(&self, root: &Path) -> PathBuf {
        self.project_dir(root).join(SETTINGS_FILE)
    }
}
