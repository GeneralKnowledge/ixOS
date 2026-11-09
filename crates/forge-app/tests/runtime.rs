//! The runtime's contracts, on Linux, against the real implementation.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use forge_app::{
    AppPhase, Cancellation, DataDirectory, ErrorKind, OpenError, Published, Runtime, RuntimeEvent,
    Stopped,
};
use forge_runtime::block_on;

#[derive(Debug, PartialEq)]
enum AppError {
    Invalid,
    Stopped(ErrorKind),
}

impl From<Stopped> for AppError {
    fn from(s: Stopped) -> Self {
        AppError::Stopped(s.kind)
    }
}

#[derive(Default)]
struct Notes {
    items: Vec<String>,
    view: Arc<Published<Vec<String>>>,
}

fn runtime() -> (Runtime<Notes>, Arc<Published<Vec<String>>>) {
    let view = Arc::new(Published::default());
    let notes = Notes {
        items: Vec::new(),
        view: Arc::clone(&view),
    };
    (Runtime::new("test", notes), view)
}

fn add(
    rt: &Runtime<Notes>,
    text: &str,
) -> impl std::future::Future<Output = Result<usize, AppError>> {
    let text = text.to_string();
    rt.command("add", move |notes: &mut Notes| {
        if text.is_empty() {
            return Err(AppError::Invalid);
        }
        notes.items.push(text);
        notes.view.set(notes.items.clone());
        Ok(notes.items.len())
    })
}

#[test]
fn commands_run_in_arrival_order_and_publish_after_commit() {
    let (rt, view) = runtime();
    let events = rt.events().unwrap();
    // Start many without awaiting: order is the order of the calls.
    let futures: Vec<_> = (0..50).map(|i| add(&rt, &i.to_string())).collect();
    for (i, f) in futures.into_iter().enumerate() {
        assert_eq!(block_on(f), Ok(i + 1));
    }
    let expected: Vec<String> = (0..50).map(|i| i.to_string()).collect();
    assert_eq!(*view.get(), expected);
    assert_eq!(rt.revision(), 50);
    for revision in 1..=50 {
        assert_eq!(
            block_on(events.next()),
            Some(RuntimeEvent::Changed { revision })
        );
    }
}

#[test]
fn a_failed_command_changes_no_revision() {
    let (rt, _) = runtime();
    let events = rt.events().unwrap();
    assert_eq!(block_on(add(&rt, "")), Err(AppError::Invalid));
    assert_eq!(rt.revision(), 0);
    block_on(add(&rt, "a")).unwrap();
    assert_eq!(
        block_on(events.next()),
        Some(RuntimeEvent::Changed { revision: 1 })
    );
}

#[test]
fn queries_never_wait_for_a_slow_command() {
    let (rt, view) = runtime();
    block_on(add(&rt, "first")).unwrap();
    let slow = rt.command("slow", |notes: &mut Notes| -> Result<(), AppError> {
        std::thread::sleep(Duration::from_millis(300));
        notes.items.push("second".into());
        notes.view.set(notes.items.clone());
        Ok(())
    });
    let started = std::time::Instant::now();
    std::thread::sleep(Duration::from_millis(20));
    assert_eq!(*view.get(), vec!["first".to_string()]);
    assert!(started.elapsed() < Duration::from_millis(200));
    block_on(slow).unwrap();
    assert_eq!(view.get().len(), 2);
}

#[test]
fn shutdown_refuses_new_commands_but_finishes_accepted_ones() {
    let (rt, view) = runtime();
    let ran = Arc::new(AtomicUsize::new(0));
    let accepted: Vec<_> = (0..5)
        .map(|i| {
            let ran = Arc::clone(&ran);
            rt.command("slow", move |notes: &mut Notes| -> Result<(), AppError> {
                std::thread::sleep(Duration::from_millis(20));
                notes.items.push(i.to_string());
                notes.view.set(notes.items.clone());
                ran.fetch_add(1, Ordering::SeqCst);
                Ok(())
            })
        })
        .collect();
    let events = rt.events().unwrap();
    rt.shutdown();
    assert_eq!(
        block_on(add(&rt, "late")),
        Err(AppError::Stopped(ErrorKind::ShutDown))
    );
    for f in accepted {
        assert_eq!(block_on(f), Ok(()));
    }
    assert_eq!(ran.load(Ordering::SeqCst), 5);
    assert_eq!(view.get().len(), 5);
    // The stream ends after the terminated phase.
    let mut last = None;
    while let Some(event) = block_on(events.next()) {
        last = Some(event);
    }
    assert_eq!(
        last,
        Some(RuntimeEvent::PhaseChanged {
            phase: AppPhase::Terminated,
            resumed: false
        })
    );
    assert!(rt.events().is_none());
    rt.shutdown(); // idempotent
}

fn slow_work(
    rt: &Runtime<Notes>,
    host: Option<&Cancellation>,
    foreground: bool,
) -> impl std::future::Future<Output = Result<u32, AppError>> {
    let work = if foreground {
        rt.foreground(host)
    } else {
        rt.lifetime(host)
    };
    rt.run("slow", work, |cancel| {
        if cancel.wait_timeout(Duration::from_secs(10)) {
            Err(AppError::Invalid) // what an I/O call reports when cut off
        } else {
            Ok(1)
        }
    })
}

fn after(ms: u64, f: impl FnOnce() + Send + 'static) {
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(ms));
        f();
    });
}

#[test]
fn the_background_interrupts_foreground_work_only() {
    let (rt, _) = runtime();
    let rt = Arc::new(rt);
    rt.set_phase(AppPhase::Active);
    let fg = slow_work(&rt, None, true);
    let host = Cancellation::new();
    let bg = slow_work(&rt, Some(&host), false);
    let r = Arc::clone(&rt);
    after(50, move || r.set_phase(AppPhase::Background));
    assert_eq!(block_on(fg), Err(AppError::Stopped(ErrorKind::Interrupted)));
    // Lifetime work is still running; the host cancels it.
    host.cancel();
    assert_eq!(block_on(bg), Err(AppError::Stopped(ErrorKind::Cancelled)));
}

#[test]
fn coming_back_is_announced_and_foreground_work_runs_again() {
    let (rt, _) = runtime();
    let events = rt.events().unwrap();
    rt.set_phase(AppPhase::Active);
    rt.set_phase(AppPhase::Active); // repeat ignored
    rt.set_phase(AppPhase::Background);
    rt.set_phase(AppPhase::Active);
    let got: Vec<_> = (0..3).map(|_| block_on(events.next()).unwrap()).collect();
    assert_eq!(
        got,
        vec![
            RuntimeEvent::PhaseChanged {
                phase: AppPhase::Active,
                resumed: false
            },
            RuntimeEvent::PhaseChanged {
                phase: AppPhase::Background,
                resumed: false
            },
            RuntimeEvent::PhaseChanged {
                phase: AppPhase::Active,
                resumed: true
            },
        ]
    );
    let quick = rt.run("quick", rt.foreground(None), |_| Ok::<_, AppError>(7));
    assert_eq!(block_on(quick), Ok(7));
}

#[test]
fn shutdown_stops_work_in_flight_and_refuses_new_work() {
    let (rt, _) = runtime();
    let rt = Arc::new(rt);
    rt.set_phase(AppPhase::Active);
    let fg = slow_work(&rt, None, true);
    let r = Arc::clone(&rt);
    after(50, move || r.set_phase(AppPhase::Terminated));
    assert_eq!(block_on(fg), Err(AppError::Stopped(ErrorKind::ShutDown)));
    assert_eq!(
        block_on(slow_work(&rt, None, false)),
        Err(AppError::Stopped(ErrorKind::ShutDown))
    );
}

#[test]
fn dropping_the_future_cancels_the_work() {
    let (rt, _) = runtime();
    let stopped = Arc::new(Mutex::new(false));
    let s = Arc::clone(&stopped);
    let work = rt.foreground(None);
    let f = rt.run("drop", work, move |cancel| {
        let cut = cancel.wait_timeout(Duration::from_secs(10));
        *s.lock().unwrap() = cut;
        Ok::<_, AppError>(())
    });
    drop(f);
    for _ in 0..100 {
        if *stopped.lock().unwrap() {
            return;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    panic!("work kept running after its future was dropped");
}

#[test]
fn a_panicking_command_reaches_its_caller_and_the_core_keeps_working() {
    let (rt, _) = runtime();
    let boom = rt.command("boom", |_: &mut Notes| -> Result<(), AppError> {
        panic!("bug")
    });
    assert!(std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| block_on(boom))).is_err());
    assert_eq!(block_on(add(&rt, "after")), Ok(1));
    assert_eq!(rt.revision(), 1);
}

#[test]
fn a_data_directory_has_one_holder() {
    let dir = tempfile::tempdir().unwrap();
    let first = DataDirectory::open(dir.path().join("data")).unwrap();
    let second = DataDirectory::open(dir.path().join("data"));
    assert!(matches!(second, Err(OpenError::InUse { .. })));
    assert_eq!(second.unwrap_err().kind(), ErrorKind::Unavailable);
    let rt = Runtime::with_directory("held", first, ());
    assert!(rt.directory().unwrap().ends_with("data"));
    drop(rt);
    // Released with the runtime (the command thread exits on its own).
    let mut reopened = None;
    for _ in 0..100 {
        if let Ok(d) = DataDirectory::open(dir.path().join("data")) {
            reopened = Some(d);
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(reopened.is_some());
}

#[test]
fn the_contract_version_is_exported() {
    assert_eq!(
        forge_app::forge_app_contract_version(),
        forge_app::CONTRACT_VERSION
    );
}
