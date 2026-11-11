//! `native_services!(transfers)`: the ledger, verification, delivery and
//! recovery of background downloads, against a fake session. These cover
//! acceptance tests 4 (the file is kept and verified before the host is
//! released) and 5 (orphans) from docs/native-capability-roadmap.md; 1, 2,
//! 3 and 6 need an iPhone.

use std::path::Path;
use std::sync::{Arc, Mutex};

use forge_services::transfers::testing::FakeSession;
use forge_services::transfers::{
    Failure, HostEvent, SessionTask, TransferRequest, TransferState, TransferUpdate, Transfers,
};
use forge_storage::FileStore;

uniffi::setup_scaffolding!("transfers_test");
forge_services::native_services!(transfers);

const HELLO_SHA256: &str = "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824";

fn open(dir: &Path) -> (Transfers, Arc<FakeSession>) {
    let fake = Arc::new(FakeSession::default());
    (
        Transfers::open(dir.join("transfers"), fake.clone()).unwrap(),
        fake,
    )
}

fn request(name: &str) -> TransferRequest {
    TransferRequest::new("https://example.com/file", "models", name)
}

fn store(dir: &Path) -> FileStore {
    FileStore::new(dir.join("models"))
}

fn state(transfers: &Transfers, id: &str) -> TransferState {
    transfers.get(id).unwrap().state
}

#[test]
fn the_record_is_saved_before_the_host_is_asked() {
    struct Checking {
        ledger: std::path::PathBuf,
        seen: Mutex<Option<String>>,
    }
    impl forge_services::transfers::Session for Checking {
        fn start(&self, request: &forge_services::transfers::SessionRequest) -> Result<(), String> {
            let ledger = std::fs::read_to_string(&self.ledger).unwrap();
            assert!(ledger.contains(&request.id), "not in the ledger yet");
            *self.seen.lock().unwrap() = Some(request.id.clone());
            Ok(())
        }
        fn cancel(&self, _: &str) {}
    }
    let dir = tempfile::tempdir().unwrap();
    let session = Arc::new(Checking {
        ledger: dir.path().join("transfers.json"),
        seen: Mutex::new(None),
    });
    let transfers = Transfers::open(dir.path(), session.clone()).unwrap();
    let id = transfers.start(request("a.bin")).unwrap();
    assert_eq!(session.seen.lock().unwrap().as_deref(), Some(id.as_str()));
    assert_eq!(state(&transfers, &id), TransferState::Queued);
}

#[test]
fn a_finished_download_is_verified_and_delivered_into_its_store() {
    let dir = tempfile::tempdir().unwrap();
    let (transfers, fake) = open(dir.path());
    transfers.deliver_to("models", store(dir.path()));
    let id = transfers
        .start(request("hello.txt").expected_bytes(5).sha256(HELLO_SHA256))
        .unwrap();
    let started = fake.started();
    assert_eq!(started[0].id, id);
    assert_eq!(started[0].expected_bytes, Some(5));

    transfers.handle(HostEvent::Progress {
        id: id.clone(),
        received: 2,
        expected: Some(5),
    });
    assert_eq!(
        state(&transfers, &id),
        TransferState::Running {
            received: 2,
            expected: Some(5)
        }
    );
    // Returns only once the file is verified and in its store.
    fake.finish(&transfers, &id, 200, b"hello");
    assert_eq!(state(&transfers, &id), TransferState::Delivered { size: 5 });
    assert_eq!(
        std::fs::read(dir.path().join("models/hello.txt")).unwrap(),
        b"hello"
    );
    assert!(!transfers.inbox_file(&id).exists());
}

#[test]
fn a_file_that_fails_a_check_is_deleted_never_kept() {
    let dir = tempfile::tempdir().unwrap();
    let (transfers, fake) = open(dir.path());
    transfers.deliver_to("models", store(dir.path()));

    let wrong_hash = transfers.start(request("a").sha256(HELLO_SHA256)).unwrap();
    fake.finish(&transfers, &wrong_hash, 200, b"HELLO");
    assert!(matches!(
        state(&transfers, &wrong_hash),
        TransferState::Failed(Failure::Verification { .. })
    ));

    let wrong_size = transfers.start(request("b").expected_bytes(4)).unwrap();
    fake.finish(&transfers, &wrong_size, 200, b"hello");
    assert!(matches!(
        state(&transfers, &wrong_size),
        TransferState::Failed(Failure::Verification { .. })
    ));

    let not_found = transfers.start(request("c")).unwrap();
    fake.finish(&transfers, &not_found, 404, b"<html>not found</html>");
    assert_eq!(
        state(&transfers, &not_found),
        TransferState::Failed(Failure::Http { status: 404 })
    );

    for id in [&wrong_hash, &wrong_size, &not_found] {
        assert!(!transfers.inbox_file(id).exists());
    }
    assert!(store(dir.path()).names().is_empty());
}

#[test]
fn a_transfer_growing_past_its_limit_is_cancelled() {
    let dir = tempfile::tempdir().unwrap();
    let (transfers, fake) = open(dir.path());
    let id = transfers.start(request("big").max_bytes(10)).unwrap();
    transfers.handle(HostEvent::Progress {
        id: id.clone(),
        received: 11,
        expected: None,
    });
    assert!(matches!(
        state(&transfers, &id),
        TransferState::Failed(Failure::Verification { .. })
    ));
    assert_eq!(fake.cancelled(), vec![id.clone()]);
    // The host's report of the cancelled task changes nothing.
    transfers.handle(HostEvent::Failed {
        id: id.clone(),
        code: -999,
        reason: "cancelled".into(),
        cancelled: true,
    });
    assert!(matches!(
        state(&transfers, &id),
        TransferState::Failed(Failure::Verification { .. })
    ));
}

#[test]
fn a_file_waits_in_the_inbox_until_its_store_is_registered() {
    let dir = tempfile::tempdir().unwrap();
    let (transfers, fake) = open(dir.path());
    let id = transfers.start(request("later.bin")).unwrap();
    fake.finish(&transfers, &id, 200, b"bytes");
    assert_eq!(
        state(&transfers, &id),
        TransferState::Downloaded { size: 5 }
    );

    // Next launch: the app registers its store and gets the file.
    drop(transfers);
    let (transfers, _fake) = open(dir.path());
    transfers.reconcile(&[]);
    assert!(transfers.inbox_file(&id).exists(), "a waiting file is kept");
    transfers.deliver_to("models", store(dir.path()));
    assert_eq!(state(&transfers, &id), TransferState::Delivered { size: 5 });
    assert!(store(dir.path()).get("later.bin").is_some());
}

#[test]
fn reconcile_cancels_orphans_and_marks_lost_transfers_interrupted() {
    let dir = tempfile::tempdir().unwrap();
    let (transfers, _fake) = open(dir.path());
    let running = transfers.start(request("running")).unwrap();
    let lost = transfers.start(request("lost")).unwrap();
    drop(transfers);

    // Relaunch. The session still has `running` and a task from an older
    // install; `lost` is gone. A stray file sits in the inbox.
    let (transfers, fake) = open(dir.path());
    std::fs::write(transfers.inbox_file("t0-0-0"), b"stray").unwrap();
    transfers.reconcile(&[
        SessionTask {
            id: running.clone(),
            received: 3,
            expected: Some(9),
        },
        SessionTask {
            id: "t1-1-1".into(),
            received: 0,
            expected: None,
        },
    ]);
    assert_eq!(
        state(&transfers, &running),
        TransferState::Running {
            received: 3,
            expected: Some(9)
        }
    );
    assert_eq!(
        state(&transfers, &lost),
        TransferState::Failed(Failure::Interrupted)
    );
    assert_eq!(fake.cancelled(), vec!["t1-1-1".to_string()]);
    assert!(!transfers.inbox_file("t0-0-0").exists());
}

#[test]
fn a_late_report_for_an_interrupted_transfer_still_counts() {
    let dir = tempfile::tempdir().unwrap();
    let (transfers, _fake) = open(dir.path());
    let id = transfers.start(request("late.bin")).unwrap();
    drop(transfers);

    // The session lists its tasks before delivering the finished one.
    let (transfers, fake) = open(dir.path());
    transfers.deliver_to("models", store(dir.path()));
    transfers.reconcile(&[]);
    assert_eq!(
        state(&transfers, &id),
        TransferState::Failed(Failure::Interrupted)
    );
    fake.finish(&transfers, &id, 200, b"done");
    assert_eq!(state(&transfers, &id), TransferState::Delivered { size: 4 });
}

#[test]
fn force_quit_interrupts_and_an_app_cancel_cancels() {
    let dir = tempfile::tempdir().unwrap();
    let (transfers, fake) = open(dir.path());
    let quit = transfers.start(request("quit")).unwrap();
    let cancelled = transfers.start(request("cancelled")).unwrap();

    // iOS cancels every task when the person swipes the app away.
    transfers.handle(HostEvent::Failed {
        id: quit.clone(),
        code: -999,
        reason: "cancelled".into(),
        cancelled: true,
    });
    assert_eq!(
        state(&transfers, &quit),
        TransferState::Failed(Failure::Interrupted)
    );

    transfers.cancel(&cancelled).unwrap();
    assert_eq!(fake.cancelled(), vec![cancelled.clone()]);
    transfers.handle(HostEvent::Failed {
        id: cancelled.clone(),
        code: -999,
        reason: "cancelled".into(),
        cancelled: true,
    });
    assert_eq!(state(&transfers, &cancelled), TransferState::Cancelled);

    // Retrying starts it again under the same id.
    transfers.retry(&quit).unwrap();
    let record = transfers.get(&quit).unwrap();
    assert_eq!(record.state, TransferState::Queued);
    assert_eq!(record.attempts, 2);
    assert_eq!(fake.started().iter().filter(|r| r.id == quit).count(), 2);
    assert!(transfers.retry(&quit).is_err(), "still running");
}

#[test]
fn a_refused_start_is_a_failure_not_a_lost_record() {
    let dir = tempfile::tempdir().unwrap();
    let (transfers, fake) = open(dir.path());
    fake.refuse_starts(Some("no session"));
    let id = transfers.start(request("x")).unwrap();
    assert!(matches!(
        state(&transfers, &id),
        TransferState::Failed(Failure::Network { .. })
    ));
}

#[test]
fn bad_requests_and_bad_host_ids_are_refused() {
    let dir = tempfile::tempdir().unwrap();
    let (transfers, _fake) = open(dir.path());
    assert!(transfers
        .start(TransferRequest::new("file:///etc/passwd", "m", "a"))
        .is_err());
    assert!(transfers.start(request("../escape")).is_err());
    assert!(transfers.start(request("a").sha256("abc")).is_err());
    assert!(transfers.list().is_empty());

    // A host id that names a path outside the inbox is ignored.
    let outside = dir.path().join("transfers/victim");
    std::fs::write(&outside, b"keep me").unwrap();
    transfers.handle(HostEvent::Downloaded {
        id: "../victim".into(),
        status: 200,
    });
    assert!(outside.exists());
}

#[test]
fn listeners_see_each_change_and_removal() {
    let dir = tempfile::tempdir().unwrap();
    let (transfers, fake) = open(dir.path());
    let seen = Arc::new(Mutex::new(Vec::new()));
    let sink = seen.clone();
    let _watch = transfers.watch(move |update| sink.lock().unwrap().push(update.clone()));
    let id = transfers.start(request("w")).unwrap();
    fake.finish(&transfers, &id, 500, b"");
    transfers.remove(&id).unwrap();
    let seen = seen.lock().unwrap();
    assert!(matches!(&seen[0], TransferUpdate::Changed(r) if r.state == TransferState::Queued));
    assert!(
        matches!(&seen[1], TransferUpdate::Changed(r) if r.state == TransferState::Failed(Failure::Http { status: 500 }))
    );
    assert_eq!(seen[2], TransferUpdate::Removed { id: id.clone() });
    assert!(transfers.list().is_empty());
}

/// The host side through the generated interface: install, sink, start.
#[test]
fn the_host_installs_the_service_and_reports_through_the_sink() {
    use native::transfers::*;

    struct Host {
        dir: String,
        sink: Mutex<Option<Arc<TransferSink>>>,
        tasks: Mutex<Vec<TransferTask>>,
    }
    impl TransferSession for Host {
        fn directory(&self) -> String {
            self.dir.clone()
        }
        fn attach(&self, sink: Arc<TransferSink>) {
            sink.reconcile(Vec::new());
            *self.sink.lock().unwrap() = Some(sink);
        }
        fn start(&self, task: TransferTask) -> Result<(), native::NativeServiceError> {
            self.tasks.lock().unwrap().push(task);
            Ok(())
        }
        fn cancel(&self, _: String) {}
    }

    let dir = tempfile::tempdir().unwrap();
    let host = Arc::new(Host {
        dir: dir.path().join("ForgeTransfers").display().to_string(),
        sink: Mutex::new(None),
        tasks: Mutex::new(Vec::new()),
    });
    install_native_transfers(host.clone()).unwrap();
    let service = TransferService::installed().unwrap();
    service.deliver_to("models", store(dir.path()));
    let id = service
        .start(request("via-host").header("Accept", "application/octet-stream"))
        .unwrap();
    let task = host.tasks.lock().unwrap()[0].clone();
    assert_eq!(task.id, id);
    assert_eq!(task.headers[0].name, "Accept");

    std::fs::write(service.inbox_file(&id), b"payload").unwrap();
    let sink = host.sink.lock().unwrap().clone().unwrap();
    sink.downloaded(id.clone(), 200);
    assert_eq!(
        service.get(&id).unwrap().state,
        TransferState::Delivered { size: 7 }
    );
    TransferService::uninstall();
}
