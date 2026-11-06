//! Windows: files reached through open folders, never by path, as Unix
//! reaches them with `openat`: a folder is opened a name at a time from
//! one already open, following no symlink or junction, so what's opened is
//! beneath it or the open fails. For writers outside this crate that must
//! write beneath a project folder or nowhere (fe-workbench's replace in
//! files).

use std::ffi::OsStr;
use std::fs::File;
use std::io;
use std::path::{Component, Path};

use crate::listing::Access;
use crate::win::{self, Open};

/// Opens the folder at absolute `path` a component at a time from its
/// volume's root, to look up what's in it, following no symlink or
/// junction: one on the way fails the open.
pub fn open_folder(path: &Path) -> io::Result<File> {
    let mut at = win::open_volume_root(path, Access::Search)?;
    for component in path.components() {
        match component {
            Component::Prefix(_) | Component::RootDir => {}
            Component::Normal(name) => at = open_folder_in(&at, name)?,
            Component::CurDir | Component::ParentDir => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "not a resolved path",
                ));
            }
        }
    }
    Ok(at)
}

/// Opens folder `name` in `folder`, to look up what's in it, following no
/// symlink or junction.
pub fn open_folder_in(folder: &File, name: &OsStr) -> io::Result<File> {
    win::open_dir(folder, name, Access::Search)
}

/// Opens file `name` in `folder` to read, following no symlink or junction.
pub fn open_file_in(folder: &File, name: &OsStr) -> io::Result<File> {
    win::open_in(folder, name, Open::Read)
}

/// Creates file `name` in `folder` to write; fails if anything is there.
pub fn create_new_in(folder: &File, name: &OsStr) -> io::Result<File> {
    win::open_in(folder, name, Open::CreateNew)
}

/// Renames file `from` in `folder` to `to`, atomically replacing a file
/// there.
pub fn rename_replacing_in(folder: &File, from: &OsStr, to: &OsStr) -> io::Result<()> {
    win::rename_replacing(folder, from, to)
}

/// Removes file `name` from `folder`.
pub fn remove_file_in(folder: &File, name: &OsStr) -> io::Result<()> {
    win::remove_at(folder, name, false)
}

/// The identity of open `file`: its volume's serial number and its index
/// on the volume.
pub fn identity(file: &File) -> io::Result<(u64, u64)> {
    win::identity(file)
}

/// Whether `error` is an open refused for a symlink or junction there, or
/// for something other than a folder where one was asked for.
pub fn is_link_or_not_folder(error: &io::Error) -> bool {
    win::is_symlink_error(error) || win::is_not_dir_error(error)
}
