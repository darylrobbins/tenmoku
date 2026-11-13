//! Install 'tenmoku' Command in PATH, and its uninstall: a symlink at
//! `~/.local/bin/tenmoku` to the running binary (inside the macOS bundle,
//! `Contents/MacOS/tenmoku`, wherever the .app lives). It needs no admin
//! prompt. An existing link to a tenmoku binary is replaced silently; any
//! other file is left alone and named. Uninstall removes only such a link.
//! The file work runs off the main thread; a toast reports it.

use kiln_l10n::tr;
use std::{
    ffi::OsStr,
    fs, io,
    os::unix::fs::symlink,
    path::{Path, PathBuf},
};

use fe_workbench::Workbench;
use gpui::App;

/// The name of the command, and of the binary a link must point at.
const NAME: &str = "tenmoku";

/// What a finished install or uninstall tells the user.
pub(crate) struct Report {
    pub(crate) level: log::Level,
    pub(crate) message: String,
    pub(crate) detail: Option<String>,
}

impl Report {
    fn error(message: String) -> Report {
        Report {
            level: log::Level::Error,
            message,
            detail: None,
        }
    }
}

/// `~/.local/bin/tenmoku` under `home`.
fn link_in(home: &Path) -> PathBuf {
    home.join(".local/bin").join(NAME)
}

/// Whether `link` is a symlink to a file named `tenmoku`.
fn is_ours(link: &Path) -> bool {
    fs::read_link(link).is_ok_and(|target| target.file_name() == Some(OsStr::new(NAME)))
}

/// The line that puts `dir` on PATH, if `path` (the value of `$PATH`) doesn't
/// have it.
pub(crate) fn path_hint(dir: &Path, path: Option<&OsStr>) -> Option<String> {
    let on_path = path.is_some_and(|path| std::env::split_paths(path).any(|entry| entry == dir));
    (!on_path).then(|| {
        let dir = dir.display().to_string();
        // Plain paths read as the usual line; any with a character the shell
        // would expand inside double quotes is single-quoted, its own quotes
        // closed and escaped, with only the appended `$PATH` left to expand.
        let plain = |c: char| c.is_alphanumeric() || " /._-+@:,=%~".contains(c);
        match dir.chars().all(plain) {
            true => format!("export PATH=\"{dir}:$PATH\""),
            false => format!("export PATH='{}':\"$PATH\"", dir.replace('\'', "'\\''")),
        }
    })
}

/// Links `link` to `target`, creating its directory.
pub(crate) fn install(link: &Path, target: &Path, path: Option<&OsStr>) -> Report {
    let result = (|| -> io::Result<()> {
        let dir = link.parent().expect("a link has a directory");
        fs::create_dir_all(dir)?;
        match fs::symlink_metadata(link) {
            Ok(meta) if meta.is_symlink() && is_ours(link) => fs::remove_file(link)?,
            Ok(_) => {
                return Err(io::Error::new(
                    io::ErrorKind::AlreadyExists,
                    tr!(
                        "fastedit-shell-exists-not-ours",
                        path = link.display().to_string()
                    )
                    .to_string(),
                ));
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
        symlink(target, link)
    })();
    match result {
        Err(error) => Report::error(
            tr!("fastedit-shell-install-failed", error = error.to_string()).to_string(),
        ),
        Ok(()) => {
            let hint = path_hint(link.parent().expect("a link has a directory"), path);
            Report {
                level: if hint.is_some() {
                    log::Level::Warn
                } else {
                    log::Level::Info
                },
                message: tr!(
                    "fastedit-shell-linked",
                    link = link.display().to_string(),
                    target = target.display().to_string()
                )
                .to_string(),
                detail: hint.map(|line| {
                    tr!(
                        "fastedit-shell-not-on-path",
                        dir = link
                            .parent()
                            .expect("a link has a directory")
                            .display()
                            .to_string(),
                        line = line
                    )
                    .to_string()
                }),
            }
        }
    }
}

fn remove_failed(link: &Path, error: &io::Error) -> String {
    tr!(
        "fastedit-shell-remove-failed",
        path = link.display().to_string(),
        error = error.to_string()
    )
    .to_string()
}

/// Removes `link` if it's a symlink to a tenmoku binary.
pub(crate) fn uninstall(link: &Path) -> Report {
    match fs::symlink_metadata(link) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => Report {
            level: log::Level::Info,
            message: tr!(
                "fastedit-shell-not-installed",
                path = link.display().to_string()
            )
            .to_string(),
            detail: None,
        },
        Ok(meta) if meta.is_symlink() && is_ours(link) => match fs::remove_file(link) {
            Ok(()) => Report {
                level: log::Level::Info,
                message: tr!("fastedit-shell-removed", path = link.display().to_string())
                    .to_string(),
                detail: None,
            },
            Err(error) => Report::error(remove_failed(link, &error)),
        },
        Ok(_) => Report::error(
            tr!(
                "fastedit-shell-remove-not-ours",
                path = link.display().to_string()
            )
            .to_string(),
        ),
        Err(error) => Report::error(remove_failed(link, &error)),
    }
}

fn run(install_it: bool, cx: &mut App) {
    cx.spawn(async move |cx| {
        let report = cx
            .background_executor()
            .spawn(async move {
                let Some(home) = std::env::var_os("HOME").map(PathBuf::from) else {
                    return Report::error(tr!("fastedit-shell-no-home").to_string());
                };
                let link = link_in(&home);
                if !install_it {
                    return uninstall(&link);
                }
                match std::env::current_exe().and_then(dunce::canonicalize) {
                    Ok(target) => install(&link, &target, std::env::var_os("PATH").as_deref()),
                    Err(error) => Report::error(
                        tr!("fastedit-shell-no-binary", error = error.to_string()).to_string(),
                    ),
                }
            })
            .await;
        cx.update(|cx| Workbench::notify(cx, report.level, report.message, report.detail));
    })
    .detach();
}

/// Has the Install and Uninstall commands, which the workbench owns, run
/// this.
pub(crate) fn init(cx: &mut App) {
    Workbench::set_shell_command_handler(cx, run);
}

#[cfg(test)]
#[path = "shell_command_tests.rs"]
mod tests;
