//! A lead not found launches once per open, though its thread ends before
//! the open's pass over the documents waiting for it.

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use super::*;

/// Refuses `missing` as not found, counting its launches.
struct Counting {
    missing: &'static str,
    launches: Arc<AtomicUsize>,
}

impl ServerLauncher for Counting {
    fn launch(&self, spec: &ServerSpec, _: &Path, _: &LaunchCancel) -> io::Result<ServerIo> {
        if spec.name == self.missing {
            self.launches.fetch_add(1, Ordering::SeqCst);
        }
        Err(io::ErrorKind::NotFound.into())
    }
}

#[test]
fn a_lead_not_found_launches_once_per_open() {
    let project = Project::new(false);
    let launches = Arc::new(AtomicUsize::new(0));
    let launcher = Arc::new(Counting {
        missing: "ty",
        launches: launches.clone(),
    });
    let (mut manager, events) = managed(launcher, Some(Arc::new(ManualClock::new())));
    manager.update_settings(&settings(Some(PythonServer::Ty), &["ty"]));
    let statuses: Arc<Mutex<Vec<(ServerId, ServerStatus)>>> = Arc::default();
    let stopped = Arc::new(AtomicBool::new(false));
    {
        let (statuses, stopped) = (statuses.clone(), stopped.clone());
        events.probe(move |event| {
            if let LspEvent::Status {
                server,
                name,
                status,
            } = event
                && name == "ty"
            {
                if matches!(status, ServerStatus::Stopped(_)) {
                    stopped.store(true, Ordering::SeqCst);
                }
                statuses.lock().unwrap().push((*server, status.clone()));
            }
        });
    }
    // Asked on the manager's thread for each server the open places, ty
    // first: from the next ask on, ty's thread has ended (its stop sent,
    // and a moment for the thread to go), as a launch that fails at once
    // can before the open is done.
    let asks = Arc::new(AtomicUsize::new(0));
    {
        let stopped = stopped.clone();
        manager.set_trust(move |_| {
            if asks.fetch_add(1, Ordering::SeqCst) > 0 {
                wait_until("ty's stop", || stopped.load(Ordering::SeqCst));
                std::thread::sleep(Duration::from_millis(100));
            }
            true
        });
    }
    let mut buffer = Buffer::new("x = 1\n");
    let path = project.root.join("main.py");
    let ty = manager
        .open(
            Some(&project.root),
            buffer.id(),
            buffer.text().clone(),
            buffer.version(),
            buffer.subscribe(),
            &path,
            "python",
        )
        .unwrap()
        .unwrap();
    let why = stopped_with(&events, ty).expect("why it stopped");
    assert!(why.starts_with("ty not found"), "{why}");
    // What the status item shows on hearing it: that stop, not another
    // instance starting.
    assert_eq!(
        manager.status_at("ty", &project.root),
        Some(ServerStatus::Stopped(Some(why.clone())))
    );
    std::thread::sleep(Duration::from_millis(100));
    assert_eq!(launches.load(Ordering::SeqCst), 1);
    assert_eq!(
        *statuses.lock().unwrap(),
        [
            (ty, ServerStatus::Starting),
            (ty, ServerStatus::Stopped(Some(why)))
        ]
    );
}
