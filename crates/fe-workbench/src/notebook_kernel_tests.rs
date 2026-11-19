//! The notebook kernel, through the real kernel script and the interpreter
//! the Python tasks use (`python3` on the `PATH`: these tests need it, and
//! fail without it rather than skip).

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use super::{
    CellId, DeathCause, Failure, Kernel, KernelEvent, KernelSpec, MAX_LINE, OUTPUT_CAP, QUEUE_CAP,
    StreamName, TrustedFolder, write_script,
};

/// The longest a test waits for the kernel, which a loaded machine slows.
const PATIENCE: Duration = Duration::from_secs(60);

struct Setup {
    _dir: tempfile::TempDir,
    root: PathBuf,
    file: PathBuf,
    script: PathBuf,
}

fn setup() -> Setup {
    let dir = tempfile::tempdir().unwrap();
    let root = dunce::canonicalize(dir.path()).unwrap();
    let file = root.join("nb.py");
    std::fs::write(&file, "# %%\n").unwrap();
    let script = write_script(&root.join("state")).unwrap();
    // Written once: a second write finds it there.
    assert_eq!(write_script(&root.join("state")).unwrap(), script);
    Setup {
        _dir: dir,
        root,
        file,
        script,
    }
}

/// A kernel for `setup`'s file running `script`, just started: its events.
fn spawn(setup: &Setup, script: &Path) -> (Kernel, async_channel::Receiver<KernelEvent>) {
    let python = fe_dap::registry::python_interpreter(&setup.root, None, Path::is_file);
    let spec = KernelSpec {
        python,
        cwd: TrustedFolder::check(&setup.root, |_| Some(true)).unwrap(),
        file: setup.file.clone(),
        script: script.to_owned(),
        env: Vec::new(),
    };
    let kernel = Kernel::start(spec).expect("python3 on the PATH starts");
    let events = kernel.events();
    (kernel, events)
}

/// A kernel for `setup`'s file, up and ready: its events.
fn start(setup: &Setup) -> (Kernel, async_channel::Receiver<KernelEvent>) {
    let (kernel, events) = spawn(setup, &setup.script);
    match next(&events) {
        KernelEvent::Ready { python } => assert!(python.starts_with("3."), "{python}"),
        other => panic!("expected ready, got {other:?}"),
    }
    (kernel, events)
}

fn next(events: &async_channel::Receiver<KernelEvent>) -> KernelEvent {
    let deadline = Instant::now() + PATIENCE;
    loop {
        if let Ok(event) = events.try_recv() {
            return event;
        }
        assert!(Instant::now() < deadline, "no kernel event in {PATIENCE:?}");
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// Every event up to and including `cell`'s end, its `Done` or `Failed`,
/// but the `Started`s.
fn until_end(events: &async_channel::Receiver<KernelEvent>, cell: CellId) -> Vec<KernelEvent> {
    let mut seen = Vec::new();
    loop {
        let event = next(events);
        if let KernelEvent::Started { .. } = event {
            continue;
        }
        let end = matches!(
            event,
            KernelEvent::Done { cell: ended, .. } | KernelEvent::Failed { cell: ended, .. }
                if ended == cell
        );
        seen.push(event);
        if end {
            return seen;
        }
    }
}

/// The text `cell` wrote to `name`, from `seen`.
fn stream(seen: &[KernelEvent], cell: CellId, name: StreamName) -> String {
    seen.iter()
        .filter_map(|event| match event {
            KernelEvent::Stream {
                cell: Some(of),
                name: on,
                text,
            } if *of == cell && *on == name => Some(text.as_str()),
            _ => None,
        })
        .collect()
}

fn result(seen: &[KernelEvent]) -> Option<&str> {
    seen.iter().find_map(|event| match event {
        KernelEvent::ExecuteResult { text, .. } => Some(text.as_str()),
        _ => None,
    })
}

#[test]
fn namespace_persists_across_cells() {
    let setup = setup();
    let (kernel, events) = start(&setup);
    kernel.execute(1, "x = 2", &setup.file, 2);
    assert_eq!(
        until_end(&events, 1),
        [KernelEvent::Done { cell: 1, ok: true }]
    );

    kernel.execute(2, "print(x * 3)", &setup.file, 4);
    let seen = until_end(&events, 2);
    assert_eq!(stream(&seen, 2, StreamName::Stdout), "6\n");
    assert_eq!(result(&seen), None);
    assert_eq!(seen.last(), Some(&KernelEvent::Done { cell: 2, ok: true }));

    // The last expression's repr is the result; None gives none.
    kernel.execute(3, "y = x * 7\ny", &setup.file, 6);
    let seen = until_end(&events, 3);
    assert_eq!(result(&seen), Some("14"));
    kernel.execute(4, "None", &setup.file, 9);
    assert_eq!(
        until_end(&events, 4),
        [KernelEvent::Done { cell: 4, ok: true }]
    );
    kernel.execute(5, "[1, 'two']", &setup.file, 11);
    assert_eq!(result(&until_end(&events, 5)), Some("[1, 'two']"));

    // The namespace is __main__'s, with the file's __file__ and folder.
    let code = "import sys\n(__name__, __file__, sys.path[0])";
    kernel.execute(6, code, &setup.file, 13);
    let expected = format!(
        "('__main__', {:?}, {:?})",
        setup.file.display().to_string(),
        setup.root.display().to_string()
    )
    .replace('"', "'");
    assert_eq!(result(&until_end(&events, 6)), Some(expected.as_str()));

    // stderr is a stream of its own, and so is a subprocess's output.
    let code = "import os, sys\nprint('oops', file=sys.stderr)\nos.system('echo from-a-shell')";
    kernel.execute(7, code, &setup.file, 16);
    let seen = until_end(&events, 7);
    assert_eq!(stream(&seen, 7, StreamName::Stderr), "oops\n");
    assert_eq!(stream(&seen, 7, StreamName::Stdout), "from-a-shell\n");
    assert_eq!(result(&seen), Some("0"));

    // A thread's text written between cells is nobody's, and isn't the next
    // cell's.
    let code = "import threading, time\n\
        def later():\n    time.sleep(0.2)\n    print('between', flush=True)\n\
        threading.Thread(target=later).start()";
    kernel.execute(8, code, &setup.file, 20);
    until_end(&events, 8);
    let between = KernelEvent::Stream {
        cell: None,
        name: StreamName::Stdout,
        text: "between\n".into(),
    };
    assert_eq!(next(&events), between);
    kernel.execute(9, "print('nine')", &setup.file, 26);
    let seen = until_end(&events, 9);
    assert_eq!(
        seen,
        [
            KernelEvent::Stream {
                cell: Some(9),
                name: StreamName::Stdout,
                text: "nine\n".into()
            },
            KernelEvent::Done { cell: 9, ok: true },
        ]
    );
}

#[test]
fn error_traceback_uses_file_lines() {
    let setup = setup();
    let (kernel, events) = start(&setup);
    kernel.execute(1, "raise ValueError('bad value')", &setup.file, 14);
    let seen = until_end(&events, 1);
    let [
        KernelEvent::Error {
            cell: 1,
            ename,
            evalue,
            traceback,
        },
        KernelEvent::Done { cell: 1, ok: false },
    ] = seen.as_slice()
    else {
        panic!("expected an error and a failed done, got {seen:?}");
    };
    assert_eq!(ename, "ValueError");
    assert_eq!(evalue, "bad value");
    let frame = format!("File \"{}\", line 14, in <module>", setup.file.display());
    assert!(
        traceback.iter().any(|entry| entry.contains(&frame)),
        "{traceback:?} names {frame}"
    );
    // The line's source comes from the cell, not the file.
    assert!(
        traceback
            .iter()
            .any(|entry| entry.contains("raise ValueError('bad value')")),
        "{traceback:?}"
    );
    let script = setup.script.display().to_string();
    assert!(
        traceback
            .iter()
            .all(|entry| !entry.contains(&script) && !entry.contains("notebook_kernel")),
        "{traceback:?} has kernel frames"
    );
    assert_eq!(
        traceback.last().map(String::as_str),
        Some("ValueError: bad value")
    );

    // A function from an earlier cell keeps that cell's lines; input() says
    // there's no stdin, again without the kernel's frames.
    kernel.execute(2, "def f():\n    return input()", &setup.file, 3);
    until_end(&events, 2);
    kernel.execute(3, "f()", &setup.file, 20);
    let seen = until_end(&events, 3);
    let Some(KernelEvent::Error {
        ename, traceback, ..
    }) = seen.first()
    else {
        panic!("expected an error, got {seen:?}");
    };
    assert_eq!(ename, "EOFError");
    let lines: Vec<&str> = traceback
        .iter()
        .filter_map(|entry| entry.trim_start().strip_prefix("File "))
        .collect();
    assert_eq!(lines.len(), 2, "{traceback:?}");
    assert!(lines[0].contains(", line 20, in <module>"), "{lines:?}");
    assert!(lines[1].contains(", line 4, in f"), "{lines:?}");
}

#[test]
fn output_caps_and_queue() {
    let setup = setup();
    let (kernel, events) = start(&setup);

    // Output past the cap is dropped and counted; a long line is split.
    let code = format!("import sys\nsys.stdout.write('x' * {})", OUTPUT_CAP + 5000);
    kernel.execute(1, &code, &setup.file, 1);
    let seen = until_end(&events, 1);
    let out = stream(&seen, 1, StreamName::Stdout);
    assert_eq!(out.replace('\n', "").len(), OUTPUT_CAP);
    assert!(out.lines().all(|line| line.len() <= MAX_LINE));
    assert_eq!(out.lines().count(), OUTPUT_CAP.div_ceil(MAX_LINE));
    let note = stream(&seen, 1, StreamName::Stderr);
    assert!(note.contains("5000 bytes dropped"), "{note:?}");
    // The cap is per cell run.
    kernel.execute(2, "print('small')", &setup.file, 1);
    assert_eq!(
        stream(&until_end(&events, 2), 2, StreamName::Stdout),
        "small\n"
    );

    // Queued cells run first in, first out.
    kernel.execute(
        3,
        "import time\ntime.sleep(0.3)\norder = [3]",
        &setup.file,
        1,
    );
    kernel.execute(4, "order.append(4)", &setup.file, 1);
    kernel.execute(5, "order.append(5)\norder", &setup.file, 1);
    let seen = until_end(&events, 5);
    let done: Vec<CellId> = seen
        .iter()
        .filter_map(|event| match event {
            KernelEvent::Done { cell, ok: true } => Some(*cell),
            _ => None,
        })
        .collect();
    assert_eq!(done, [3, 4, 5]);
    assert_eq!(result(&seen), Some("[3, 4, 5]"));

    // While a cell loops, the queue takes QUEUE_CAP and refuses the rest.
    let looping = "seen = []\nprint('looping', flush=True)\nwhile True:\n    pass";
    kernel.execute(10, looping, &setup.file, 1);
    let first = 100;
    let queued: Vec<CellId> = (first..first + QUEUE_CAP as CellId).collect();
    for &cell in &queued {
        kernel.execute(cell, &format!("seen.append({cell})"), &setup.file, 1);
    }
    kernel.execute(999, "seen.append(999)", &setup.file, 1);
    // A queued cell can be cancelled, once.
    assert!(kernel.cancel_queued(first + 1));
    assert!(!kernel.cancel_queued(first + 1));
    let mut expected = vec![
        KernelEvent::Failed {
            cell: 999,
            why: Failure::QueueFull,
        },
        KernelEvent::Failed {
            cell: first + 1,
            why: Failure::Cancelled,
        },
    ];
    let mut looping = false;
    while !looping || !expected.is_empty() {
        let event = next(&events);
        if let KernelEvent::Stream { cell: Some(10), .. } = event {
            looping = true;
        } else if event != (KernelEvent::Started { cell: 10 }) {
            assert_eq!(event, expected.remove(0));
        }
    }

    // Interrupt stops the loop and keeps the queue, which runs in order.
    assert!(kernel.interrupt(false));
    let seen = until_end(&events, 10);
    assert!(
        matches!(
            seen.as_slice(),
            [
                ..,
                KernelEvent::Error { cell: 10, ename, .. },
                KernelEvent::Done { cell: 10, ok: false },
            ] if ename == "KeyboardInterrupt"
        ),
        "{seen:?}"
    );
    let last = *queued.last().unwrap();
    let seen = until_end(&events, last);
    let done: Vec<CellId> = seen
        .iter()
        .filter_map(|event| match event {
            KernelEvent::Done { cell, ok: true } => Some(*cell),
            _ => None,
        })
        .collect();
    let expected: Vec<CellId> = queued
        .iter()
        .copied()
        .filter(|&cell| cell != first + 1)
        .collect();
    assert_eq!(done, expected);
    kernel.execute(11, "len(seen)", &setup.file, 1);
    let len = (QUEUE_CAP - 1).to_string();
    assert_eq!(result(&until_end(&events, 11)), Some(len.as_str()));

    // Nothing running, nothing to interrupt; a flushing interrupt cancels
    // the queue.
    assert!(!kernel.interrupt(false));
    kernel.execute(
        20,
        "print('again', flush=True)\nwhile True:\n    pass",
        &setup.file,
        1,
    );
    kernel.execute(21, "print(21)", &setup.file, 1);
    kernel.execute(22, "print(22)", &setup.file, 1);
    loop {
        if let KernelEvent::Stream { cell: Some(20), .. } = next(&events) {
            break;
        }
    }
    assert!(kernel.interrupt(true));
    let seen = until_end(&events, 20);
    assert_eq!(
        seen[..2],
        [
            KernelEvent::Failed {
                cell: 21,
                why: Failure::Cancelled
            },
            KernelEvent::Failed {
                cell: 22,
                why: Failure::Cancelled
            },
        ]
    );
    assert_eq!(
        seen.last(),
        Some(&KernelEvent::Done {
            cell: 20,
            ok: false
        })
    );
    // The kernel's still good for more.
    kernel.execute(23, "1 + 1", &setup.file, 1);
    assert_eq!(result(&until_end(&events, 23)), Some("2"));

    // An interrupt straight after execute, before the cell has started,
    // reaches that cell when it starts, and not the one after it.
    kernel.execute(30, "while True:\n    pass", &setup.file, 1);
    assert!(kernel.interrupt(false));
    kernel.execute(31, "print('after')", &setup.file, 1);
    assert_interrupted_then_runs(&events, 30, 31);
    drop(kernel);

    // So does one before the kernel is even up.
    let (kernel, events) = spawn(&setup, &setup.script);
    kernel.execute(1, "while True:\n    pass", &setup.file, 1);
    assert!(kernel.interrupt(false));
    kernel.execute(2, "print('after')", &setup.file, 1);
    assert!(matches!(next(&events), KernelEvent::Ready { .. }));
    assert_interrupted_then_runs(&events, 1, 2);
}

/// `looped` started, was interrupted, and `then` ran after it, whole.
fn assert_interrupted_then_runs(
    events: &async_channel::Receiver<KernelEvent>,
    looped: CellId,
    then: CellId,
) {
    assert_eq!(next(events), KernelEvent::Started { cell: looped });
    let seen = until_end(events, looped);
    assert!(
        matches!(
            seen.as_slice(),
            [
                KernelEvent::Error { cell, ename, .. },
                KernelEvent::Done { ok: false, .. },
            ] if *cell == looped && ename == "KeyboardInterrupt"
        ),
        "{seen:?}"
    );
    assert_eq!(next(events), KernelEvent::Started { cell: then });
    assert_eq!(
        until_end(events, then),
        [
            KernelEvent::Stream {
                cell: Some(then),
                name: StreamName::Stdout,
                text: "after\n".into()
            },
            KernelEvent::Done {
                cell: then,
                ok: true
            },
        ]
    );
}

/// Whether process group `group` has a process left in it.
fn group_lives(group: u32) -> bool {
    let group = libc::pid_t::try_from(group).unwrap();
    // SAFETY: signal 0 checks for processes and sends nothing.
    unsafe { libc::kill(-group, 0) == 0 }
}

/// Waits for process group `group` to be gone.
fn wait_gone(group: u32) {
    let deadline = Instant::now() + PATIENCE;
    while group_lives(group) {
        assert!(
            Instant::now() < deadline,
            "group {group} still has processes"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// A kernel running a cell that has started a subprocess, which waits.
fn with_subprocess(setup: &Setup) -> (Kernel, async_channel::Receiver<KernelEvent>) {
    let (kernel, events) = start(setup);
    let code = "import subprocess\nchild = subprocess.Popen(['sleep', '600'])\nprint(child.pid, flush=True)";
    kernel.execute(1, code, &setup.file, 1);
    let seen = until_end(&events, 1);
    let pid: u32 = stream(&seen, 1, StreamName::Stdout).trim().parse().unwrap();
    assert_ne!(pid, kernel.pid());
    assert!(group_lives(kernel.pid()));
    (kernel, events)
}

#[test]
fn kernel_death_fails_running_and_queued_cells() {
    let setup = setup();
    let (kernel, events) = start(&setup);
    let code = "import time\nprint('up', flush=True)\ntime.sleep(600)";
    kernel.execute(1, code, &setup.file, 1);
    kernel.execute(2, "print(2)", &setup.file, 1);
    kernel.execute(3, "print(3)", &setup.file, 1);
    loop {
        if let KernelEvent::Stream { cell: Some(1), .. } = next(&events) {
            break;
        }
    }
    let pid = libc::pid_t::try_from(kernel.pid()).unwrap();
    // SAFETY: the kernel's leader isn't reaped until the kernel has seen it
    // exit, so the pid is still its.
    unsafe { libc::kill(pid, libc::SIGKILL) };
    let KernelEvent::Died { status, cause, .. } = next(&events) else {
        panic!("expected the kernel to die");
    };
    assert_eq!(cause, DeathCause::Exited);
    let signal = status.and_then(|status| crate::process_group::signal_of(&status));
    assert_eq!(signal, Some(libc::SIGKILL));
    for cell in 1..=3 {
        assert_eq!(
            next(&events),
            KernelEvent::Failed {
                cell,
                why: Failure::Died
            }
        );
    }
    assert!(!kernel.is_alive());
    // A cell given a dead kernel fails, and Died isn't said again.
    kernel.execute(4, "print(4)", &setup.file, 1);
    assert_eq!(
        next(&events),
        KernelEvent::Failed {
            cell: 4,
            why: Failure::Died
        }
    );
    assert!(!kernel.interrupt(false));
    std::thread::sleep(Duration::from_millis(100));
    assert!(events.try_recv().is_err());
    wait_gone(kernel.pid());

    // Shutdown stops the group, the cell's subprocess too, and waits for it.
    let (mut kernel, events) = with_subprocess(&setup);
    kernel.execute(5, "import time\ntime.sleep(600)", &setup.file, 1);
    kernel.execute(6, "print(6)", &setup.file, 1);
    let group = kernel.pid();
    kernel.shutdown();
    assert!(!kernel.is_alive());
    assert_eq!(
        until_end(&events, 6),
        [
            KernelEvent::Failed {
                cell: 5,
                why: Failure::ShutDown
            },
            KernelEvent::Failed {
                cell: 6,
                why: Failure::ShutDown
            },
        ]
    );
    wait_gone(group);

    // So does a drop, without waiting.
    let (kernel, events) = with_subprocess(&setup);
    let group = kernel.pid();
    drop(kernel);
    wait_gone(group);
    // Its threads end, and with them the events: none says it died.
    let deadline = Instant::now() + PATIENCE;
    while !events.is_closed() {
        assert!(
            Instant::now() < deadline,
            "the dropped kernel's threads still run"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    while let Ok(event) = events.try_recv() {
        assert!(!matches!(event, KernelEvent::Died { .. }), "{event:?}");
    }
}

#[test]
fn transport_failures_end_the_kernel() {
    let setup = setup();
    let ready = "import os, time\nos.write(1, b'{\"kind\": \"ready\", \"python\": \"3\"}\\n')\n";
    let cases = [
        // Its stdout closes, but it lives on.
        ("closes.py", "os.close(1)\n", DeathCause::OutputClosed),
        (
            "garbles.py",
            "os.write(1, b'not json\\n')\n",
            DeathCause::BadMessage,
        ),
        (
            "rambles.py",
            "os.write(1, b'x' * (16 * 1024 * 1024 + 1))\n",
            DeathCause::MessageTooLong,
        ),
        // A message for a cell that isn't running.
        (
            "strays.py",
            "os.write(1, b'{\"id\": 7, \"kind\": \"done\", \"ok\": true}\\n')\n",
            DeathCause::BadMessage,
        ),
    ];
    for (name, then, expected) in cases {
        let script = setup.root.join(name);
        std::fs::write(&script, format!("{ready}{then}time.sleep(600)\n")).unwrap();
        let (kernel, events) = spawn(&setup, &script);
        assert!(matches!(next(&events), KernelEvent::Ready { .. }), "{name}");
        kernel.execute(1, "pass", &setup.file, 1);
        let mut seen = Vec::new();
        let died = loop {
            match next(&events) {
                KernelEvent::Died { cause, .. } => break cause,
                other => seen.push(other),
            }
        };
        assert_eq!(died, expected, "{name}: {seen:?}");
        assert_eq!(
            next(&events),
            KernelEvent::Failed {
                cell: 1,
                why: Failure::Died
            },
            "{name}"
        );
        assert!(!kernel.is_alive());
        wait_gone(kernel.pid());
    }
}

#[test]
fn untrusted_folder_starts_nothing() {
    let setup = setup();
    assert!(TrustedFolder::check(&setup.root, |_| Some(false)).is_err());
    assert!(TrustedFolder::check(&setup.root, |_| None).is_err());
    assert!(TrustedFolder::check(&setup.root.join("missing"), |_| Some(true)).is_err());
    let trusted = TrustedFolder::check(&setup.root, |folder| Some(folder == setup.root)).unwrap();
    assert_eq!(trusted.path(), setup.root);
}
