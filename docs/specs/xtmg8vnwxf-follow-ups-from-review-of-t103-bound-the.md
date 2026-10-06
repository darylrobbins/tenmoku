---
task_uid: "01m497ambx5tx5dcxtmg8vnwxf"
task: "T131"
rev: 1
type: "chore"
complexity: "m"
criteria:
  - id: "C1"
    text: "A walk whose sink blocks keeps at most a bounded number of batches queued; a test with a stalled sink over a large generated tree shows the walkers stop listing"
    required: true
    manual: false
  - id: "C2"
    text: "Cancelling, or dropping the receiver, during a stalled walk unblocks and joins every walker thread promptly"
    required: true
    manual: false
  - id: "C3"
    text: "fe_project exposes a safe public open-under-root helper: no-follow at every component, O_NONBLOCK, rejects absolute paths and .. components; a test swaps a component for a symlink and the open fails"
    required: true
    manual: false
  - id: "C4"
    text: "fe-nav extraction opens files only through that helper; the canonicalize check is gone and fe-nav has no unsafe"
    required: true
    manual: false
  - id: "C5"
    text: "Existing finder and fe-nav tests pass unchanged"
    required: true
    manual: false
---

# Follow-ups from review of T103: bound the finder walk and open fe-nav files race-free

Codex raised two findings on T103 (fe-nav), F1 and F2, in rounds 1 and 2. Both need fe-project changes that are outside T103's scope (fe-nav + Cargo only), and finder.rs was reserved by T118 at the time. T103 lands with them recorded as known limits; this card fixes them.

## F1 (high): the finder walk has no backpressure

`FinderIndex::walk` (crates/fe-project/src/finder.rs, `walk_dir` ~l.769, channel ~l.796) sends batches over an unbounded `mpsc`. When the sink is slow, as fe-nav's bounded extraction queue in `crates/fe-nav/src/build.rs` is, the walker threads keep listing and the queued batches grow without bound on a large tree.

Fix: bound the walk's channel (`sync_channel`), so a blocking sink stalls the walkers. Cancellation and a dropped receiver must still unblock every walker promptly, with no deadlock and no thread left behind. Keep the finder's existing callers and their behaviour (T62/T91 protections: no-follow, cycle and mount handling) unchanged.

## F2 (medium): fe-nav's file open can race a swap

`crates/fe-nav/src/extract.rs` (~l.97, ~l.165) checks a path with `fs::canonicalize` plus an inode comparison, then opens it by pathname, so a symlink swapped in between is followed. fe-project already opens race-free through no-follow directory handles (`listing::open_dirs` / `open_at`, `pub(crate)`).

Fix: expose a safe public helper, for example `fe_project::open_file_under(root: &Path, rel: &Path) -> io::Result<File>`. It opens no-follow at every component, uses O_NONBLOCK, and rejects absolute paths and `..` components. Switch fe-nav's extraction to it and drop the canonicalize check. No `unsafe` in fe-nav.

## Tests

New test files only: `crates/fe-project/src/tests/finder_backpressure.rs` and `crates/fe-nav/tests/open_under.rs`. Register the fe-project file in `tests/mod.rs`.
