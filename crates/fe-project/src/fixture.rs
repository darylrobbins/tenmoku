//! A generated project tree shared by tests and benches.

use std::{fs, io, path::Path, thread};

/// The ignored directory [`generate`] writes, listed in its `.gitignore`.
pub const IGNORED_DIR: &str = "ignored";

/// Writes `dirs` directories (`dir_0000`, …) of `files_per_dir` empty files
/// (`file_00000.txt`, …) under `root`, plus a `.gitignore` that ignores
/// [`IGNORED_DIR`], which holds a few files. The root then lists `dirs + 1`
/// entries: the directories and `.gitignore`.
#[doc(hidden)]
pub fn generate(root: &Path, dirs: usize, files_per_dir: usize) -> io::Result<()> {
    fs::write(root.join(".gitignore"), format!("{IGNORED_DIR}/\n"))?;
    let ignored = root.join(IGNORED_DIR);
    fs::create_dir_all(&ignored)?;
    for ix in 0..10 {
        fs::File::create(ignored.join(format!("ignored_{ix}.txt")))?;
    }
    for dir in 0..dirs {
        fs::create_dir_all(root.join(dir_name(dir)))?;
    }
    // Creating files is all syscall time. Two threads were fastest on APFS
    // (about 3 s for 100k files); more contend inside the file system.
    let threads = 2;
    thread::scope(|scope| {
        let workers: Vec<_> = (0..threads)
            .map(|worker| {
                scope.spawn(move || -> io::Result<()> {
                    for dir in (worker..dirs).step_by(threads) {
                        let dir = root.join(dir_name(dir));
                        for file in 0..files_per_dir {
                            fs::File::create(dir.join(file_name(file)))?;
                        }
                    }
                    Ok(())
                })
            })
            .collect();
        workers
            .into_iter()
            .try_for_each(|worker| worker.join().expect("fixture worker panicked"))
    })
}

#[doc(hidden)]
pub fn dir_name(ix: usize) -> String {
    format!("dir_{ix:04}")
}

#[doc(hidden)]
pub fn file_name(ix: usize) -> String {
    format!("file_{ix:05}.txt")
}
