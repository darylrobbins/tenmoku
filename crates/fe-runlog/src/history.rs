//! Each test's last results, and which tests are flaky.
//!
//! One [`History`] per project root. It keeps the last [`WINDOW`] results of
//! each test, saves itself to `<state_dir>/test-history/<hash of root>.json`
//! (temp file, then rename) after each recorded run, and keeps at most
//! [`MAX_TESTS`] tests, dropping the least recently run. The state directory
//! is passed in; nothing here reads `$HOME`.

use std::collections::{HashMap, VecDeque};
use std::io::Write;
use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use crate::TestId;

/// How many results a test keeps.
const WINDOW: usize = 8;
/// How many tests a history keeps.
const MAX_TESTS: usize = 20_000;

/// How a test ended in a run.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// It passed.
    Passed,
    /// It failed.
    Failed,
}

/// A test that both passed and failed within its window.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Flaky {
    /// How many of the window's results failed.
    pub failed: usize,
    /// How many results the window holds.
    pub of: usize,
}

struct Entry {
    window: VecDeque<Outcome>,
    /// The run that last recorded this test; higher is newer.
    seq: u64,
}

/// The last results of every test in one project.
pub struct History {
    path: Option<PathBuf>,
    tests: HashMap<TestId, Entry>,
    seq: u64,
}

impl History {
    /// An empty history that isn't saved anywhere.
    pub fn in_memory() -> Self {
        Self {
            path: None,
            tests: HashMap::new(),
            seq: 0,
        }
    }

    /// The history of `root` kept under `state_dir`. A missing file loads
    /// empty; a corrupt one loads empty too, and says so once on stderr.
    pub fn load(state_dir: &Path, root: &Path) -> Self {
        let path = state_dir
            .join("test-history")
            .join(format!("{:016x}.json", hash(root)));
        let mut history = Self {
            path: Some(path.clone()),
            ..Self::in_memory()
        };
        match std::fs::read_to_string(&path) {
            Ok(text) => match parse(&text) {
                Some((seq, tests)) => {
                    history.seq = seq;
                    history.tests = tests;
                }
                None => eprintln!(
                    "fe-runlog: ignoring corrupt test history {}",
                    path.display()
                ),
            },
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => eprintln!("fe-runlog: can't read test history {}: {e}", path.display()),
        }
        history
    }

    /// Records one run: each test's outcome joins its window (a test listed
    /// twice counts once, the last outcome), tests not in `run` keep theirs.
    /// Then saves, when this history has a file.
    pub fn record(&mut self, run: &[(TestId, Outcome)]) -> std::io::Result<()> {
        let mut last: HashMap<&TestId, Outcome> = HashMap::new();
        for (id, outcome) in run {
            last.insert(id, *outcome);
        }
        if !last.is_empty() {
            self.seq += 1;
        }
        for (id, outcome) in last {
            let entry = self.tests.entry(id.clone()).or_insert_with(|| Entry {
                window: VecDeque::new(),
                seq: 0,
            });
            if entry.window.len() == WINDOW {
                entry.window.pop_front();
            }
            entry.window.push_back(outcome);
            entry.seq = self.seq;
        }
        if self.tests.len() > MAX_TESTS {
            let mut by_age: Vec<(u64, TestId)> = self
                .tests
                .iter()
                .map(|(id, e)| (e.seq, id.clone()))
                .collect();
            by_age.sort();
            let excess = self.tests.len() - MAX_TESTS;
            for (_, id) in by_age.into_iter().take(excess) {
                self.tests.remove(&id);
            }
        }
        self.save()
    }

    /// The test's last results, oldest first, at most [`WINDOW`].
    pub fn last8(&self, id: &TestId) -> Vec<Outcome> {
        self.tests
            .get(id)
            .map(|e| e.window.iter().copied().collect())
            .unwrap_or_default()
    }

    /// `Some` when the test's window holds both a pass and a fail.
    pub fn flaky(&self, id: &TestId) -> Option<Flaky> {
        let window = &self.tests.get(id)?.window;
        let failed = window.iter().filter(|o| **o == Outcome::Failed).count();
        (failed > 0 && failed < window.len()).then_some(Flaky {
            failed,
            of: window.len(),
        })
    }

    /// How many tests are kept.
    pub fn len(&self) -> usize {
        self.tests.len()
    }

    /// Whether no test is kept.
    pub fn is_empty(&self) -> bool {
        self.tests.is_empty()
    }

    fn save(&self) -> std::io::Result<()> {
        let Some(path) = &self.path else {
            return Ok(());
        };
        let tests: Vec<Value> = self
            .tests
            .iter()
            .map(|(id, e)| {
                let results: String = e
                    .window
                    .iter()
                    .map(|o| if *o == Outcome::Passed { 'P' } else { 'F' })
                    .collect();
                json!({"binary": id.binary, "name": id.name, "results": results, "seq": e.seq})
            })
            .collect();
        let text = json!({"seq": self.seq, "tests": tests}).to_string();
        let dir = path.parent().unwrap_or(Path::new("."));
        std::fs::create_dir_all(dir)?;
        // A temp file of this writer's own, made exclusively (so a planted
        // symlink is never followed), renamed over the history.
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let tmp = path.with_extension(format!(
            "json.{}.{}.tmp",
            std::process::id(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        let written = (|| {
            let mut file = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&tmp)?;
            file.write_all(text.as_bytes())?;
            file.sync_all()?;
            drop(file);
            std::fs::rename(&tmp, path)
        })();
        if written.is_err() {
            let _ = std::fs::remove_file(&tmp);
        }
        written
    }
}

fn parse(text: &str) -> Option<(u64, HashMap<TestId, Entry>)> {
    let value: Value = serde_json::from_str(text).ok()?;
    let seq = value.get("seq")?.as_u64()?;
    if seq == u64::MAX {
        return None;
    }
    let mut tests = HashMap::new();
    for t in value.get("tests")?.as_array()? {
        let id = TestId::new(t.get("binary")?.as_str()?, t.get("name")?.as_str()?);
        let window: VecDeque<Outcome> = t
            .get("results")?
            .as_str()?
            .chars()
            .map(|c| match c {
                'P' => Some(Outcome::Passed),
                'F' => Some(Outcome::Failed),
                _ => None,
            })
            .collect::<Option<_>>()?;
        if window.len() > WINDOW {
            return None;
        }
        let entry_seq = t.get("seq")?.as_u64()?;
        if entry_seq > seq {
            return None;
        }
        tests.insert(
            id,
            Entry {
                window,
                seq: entry_seq,
            },
        );
    }
    if tests.len() > MAX_TESTS {
        return None;
    }
    Some((seq, tests))
}

/// FNV-1a over the root's bytes: stable across runs and platforms.
fn hash(root: &Path) -> u64 {
    #[cfg(unix)]
    let bytes = std::os::unix::ffi::OsStrExt::as_bytes(root.as_os_str()).to_vec();
    #[cfg(not(unix))]
    let bytes = root.to_string_lossy().into_owned().into_bytes();
    bytes.into_iter().fold(0xcbf2_9ce4_8422_2325, |h, b| {
        (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3)
    })
}
