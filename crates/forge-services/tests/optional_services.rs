//! Expands `native_services!` with every optional service, the way an app
//! crate selects them, and checks each Rust-side API against its fake.

use std::sync::{Arc, Mutex};

uniffi::setup_scaffolding!("optional_services_test");
forge_services::native_services!(
    connectivity,
    device_status,
    motion,
    location,
    bluetooth,
    audio,
    background,
    biometrics
);

use native::*;

fn collect<T: Clone + Send + 'static>() -> (Arc<Mutex<Vec<T>>>, impl Fn(&T) + Send + Sync) {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let sink = seen.clone();
    (seen, move |value: &T| {
        sink.lock().unwrap().push(value.clone())
    })
}

fn wifi() -> connectivity::ConnectivityStatus {
    use connectivity::*;
    ConnectivityStatus {
        reachability: Reachability::Online,
        interfaces: vec![NetworkInterface::Wifi],
        is_expensive: false,
        is_constrained: false,
        supports_ipv4: true,
        supports_ipv6: true,
        supports_dns: true,
        offline_reason: None,
        observed_at_ms: 1,
    }
}

#[test]
fn connectivity_watch_and_transfer_policy() {
    use connectivity::*;
    let (service, fake) = ConnectivityService::for_tests();
    assert!(!service.current().is_online());

    let (seen, listener) = collect();
    let watch = service.watch(listener);
    assert!(fake.is_started());
    fake.set(wifi());
    let mut cellular = wifi();
    cellular.interfaces = vec![NetworkInterface::Cellular];
    cellular.is_expensive = true;
    fake.set(cellular.clone());

    let seen = seen.lock().unwrap().clone();
    assert_eq!(seen.len(), 3, "current status on start, then two changes");
    assert_eq!(seen[1].primary_interface(), Some(NetworkInterface::Wifi));
    assert!(seen[1].allows(TransferPolicy::Unmetered));
    assert!(cellular.allows(TransferPolicy::Any));
    assert!(!cellular.allows(TransferPolicy::Unmetered));

    let mut low_data = wifi();
    low_data.is_constrained = true;
    assert!(!low_data.is_unmetered());
    let mut offline = wifi();
    offline.reachability = Reachability::Offline;
    assert_eq!(offline.primary_interface(), None);
    assert!(!offline.allows(TransferPolicy::Any));

    drop(watch);
    assert!(!fake.is_started(), "the last watch stops the monitor");
}

#[test]
fn optional_services_install_through_their_exports() {
    let (service, fake) = connectivity::ConnectivityService::for_tests();
    drop(service);
    connectivity::ConnectivityService::uninstall();
    assert!(matches!(
        connectivity::ConnectivityService::installed(),
        Err(NativeServiceError::NotInstalled { .. })
    ));
    fake.set(wifi());
    connectivity::install_native_connectivity(fake.clone());
    assert!(connectivity::ConnectivityService::installed()
        .unwrap()
        .current()
        .is_online());

    let (_, motion_fake) = motion::MotionService::for_tests();
    motion::install_native_motion(motion_fake);
    assert!(
        motion::MotionService::installed()
            .unwrap()
            .availability()
            .altimeter
    );

    let (_, device_fake) = device_status::DeviceStatusService::for_tests();
    device_status::install_native_device_status(device_fake);
    let (_, location_fake) = location::LocationService::for_tests();
    location::install_native_location(location_fake);
    let (_, bluetooth_fake) = bluetooth::BluetoothService::for_tests();
    bluetooth::install_native_bluetooth(bluetooth_fake);
    assert!(device_status::DeviceStatusService::installed().is_ok());
    assert!(location::LocationService::installed().is_ok());
    assert!(bluetooth::BluetoothService::installed().is_ok());
}

#[test]
fn device_status_policy_and_changes() {
    use device_status::*;
    let (service, fake) = DeviceStatusService::for_tests();
    assert_eq!(service.info().model_identifier, "iPhone16,2");
    let status = service.status();
    assert_eq!(status.defer_heavy_work(Some(1 << 30)), None);
    assert_eq!(
        status.defer_heavy_work(Some(4 << 30)),
        Some(DeferReason::LowMemory)
    );

    let (seen, listener) = collect();
    let _watch = service.watch(listener);
    let mut hot = status.clone();
    hot.thermal_state = ThermalState::Serious;
    fake.set(hot);
    let mut low = status.clone();
    low.battery_level = Some(0.1);
    fake.set(low.clone());
    assert_eq!(low.defer_heavy_work(None), Some(DeferReason::LowBattery));
    low.battery_state = BatteryState::Charging;
    assert_eq!(
        low.defer_heavy_work(None),
        None,
        "charging lifts the battery rule"
    );

    let seen = seen.lock().unwrap();
    assert_eq!(seen.len(), 3);
    assert_eq!(seen[1].defer_heavy_work(None), Some(DeferReason::Thermal));
}

#[test]
fn motion_feeds_are_per_sensor_and_checked() {
    use motion::*;
    let (service, fake) = MotionService::for_tests();
    assert!(matches!(
        service.watch(MotionSensor::Accelerometer, 5, |_| {}),
        Err(NativeServiceError::InvalidInput { .. })
    ));

    let (accel, listener) = collect();
    let a = service
        .watch(MotionSensor::Accelerometer, 20, listener)
        .unwrap();
    // A second watcher shares the sensor at the first one's rate.
    let (accel2, listener2) = collect();
    let b = service
        .watch(MotionSensor::Accelerometer, 100, listener2)
        .unwrap();
    assert_eq!(fake.interval_ms(MotionSensor::Accelerometer), Some(20));
    assert_eq!(fake.interval_ms(MotionSensor::Gyroscope), None);

    let reading = MotionReading::Acceleration {
        value: Vector3 {
            x: 0.0,
            y: 0.0,
            z: -1.0,
        },
        timestamp_s: 10.0,
    };
    fake.send(reading.clone());
    fake.send(MotionReading::RotationRate {
        value: Vector3 {
            x: 1.0,
            y: 0.0,
            z: 0.0,
        },
        timestamp_s: 10.0,
    });
    assert_eq!(*accel.lock().unwrap(), vec![reading.clone()]);
    assert_eq!(*accel2.lock().unwrap(), vec![reading.clone()]);
    assert_eq!(service.latest(MotionSensor::Accelerometer), Some(reading));
    assert_eq!(service.latest(MotionSensor::Gyroscope), None);

    drop(a);
    assert!(fake.interval_ms(MotionSensor::Accelerometer).is_some());
    drop(b);
    assert!(fake.interval_ms(MotionSensor::Accelerometer).is_none());

    fake.availability.lock().unwrap().altimeter = false;
    assert!(matches!(
        service.watch(MotionSensor::Altimeter, 1000, |_| {}),
        Err(NativeServiceError::Unavailable { .. })
    ));
    assert_eq!(
        Vector3 {
            x: 3.0,
            y: 4.0,
            z: 0.0
        }
        .magnitude(),
        5.0
    );
}

#[test]
fn location_watch_permission_and_distance() {
    use location::*;
    let (service, fake) = LocationService::for_tests();
    let options = LocationOptions {
        accuracy: LocationAccuracy::Best,
        distance_filter_m: Some(-1.0),
        include_heading: true,
    };
    assert!(matches!(
        service.watch(options, |_| {}),
        Err(NativeServiceError::InvalidInput { .. })
    ));
    let options = LocationOptions {
        distance_filter_m: None,
        ..options
    };

    fake.denied.store(true, std::sync::atomic::Ordering::SeqCst);
    assert!(matches!(
        service.watch(options, |_| {}),
        Err(NativeServiceError::PermissionDenied { .. })
    ));
    fake.denied
        .store(false, std::sync::atomic::Ordering::SeqCst);

    let (seen, listener) = collect();
    let watch = service.watch(options, listener).unwrap();
    assert_eq!(fake.options(), Some(options));
    let fix = |lat: f64, lon: f64| LocationFix {
        latitude: lat,
        longitude: lon,
        horizontal_accuracy_m: 5.0,
        altitude_m: Some(12.0),
        vertical_accuracy_m: Some(3.0),
        speed_mps: None,
        speed_accuracy_mps: None,
        course_degrees: None,
        course_accuracy_degrees: None,
        floor: None,
        timestamp_ms: 1,
    };
    fake.send(LocationEvent::Fix {
        fix: fix(51.5, -0.12),
    });
    assert_eq!(seen.lock().unwrap().len(), 1);
    assert_eq!(service.last_known().unwrap().latitude, 51.5);
    assert!(fix(0.0, 0.0).distance_to(&fix(0.0, 1.0)) > 111_000.0);
    drop(watch);
    assert_eq!(fake.options(), None);
}

#[test]
fn bluetooth_state_changes() {
    use bluetooth::*;
    let (service, fake) = BluetoothService::for_tests();
    let (seen, listener) = collect();
    let _watch = service.watch(listener);
    fake.set(BluetoothState::PoweredOff);
    fake.set(BluetoothState::PoweredOn);
    assert_eq!(service.state(), BluetoothState::PoweredOn);
    assert_eq!(
        *seen.lock().unwrap(),
        vec![
            BluetoothState::Unknown,
            BluetoothState::PoweredOff,
            BluetoothState::PoweredOn
        ]
    );
}

#[test]
fn new_permissions_go_through_the_one_provider() {
    let dir = tempfile::tempdir().unwrap();
    let (services, fakes) = NativeServices::for_tests(dir.path());
    fakes
        .permissions
        .set(NativePermission::Location, PermissionStatus::Denied);
    assert_eq!(
        forge_runtime::block_on(services.request_permission(NativePermission::Location)),
        PermissionStatus::Denied
    );
    assert!(
        forge_runtime::block_on(services.request_permission(NativePermission::Motion)).allows_use()
    );
}

#[test]
fn audio_measures_files_through_the_host() {
    use audio::*;
    let (service, fake) = AudioService::for_tests();
    fake.set("/voice/a.m4a", 2.5);
    assert_eq!(
        service
            .duration(std::path::Path::new("/voice/a.m4a"))
            .unwrap(),
        std::time::Duration::from_millis(2500)
    );
    assert!(matches!(
        service.duration(std::path::Path::new("relative.m4a")),
        Err(NativeServiceError::InvalidInput { .. })
    ));
    assert!(matches!(
        service.duration(std::path::Path::new("/voice/missing.m4a")),
        Err(NativeServiceError::Platform { .. })
    ));
    fake.set("/voice/bad.m4a", -1.0);
    assert!(service
        .duration(std::path::Path::new("/voice/bad.m4a"))
        .is_err());

    AudioService::uninstall();
    assert!(matches!(
        AudioService::installed(),
        Err(NativeServiceError::NotInstalled { .. })
    ));
    install_native_audio(fake);
    assert!(AudioService::installed().is_ok());
}

/// Waits (up to two seconds) for a background run to report its result.
fn finished(results: &Arc<Mutex<Vec<bool>>>) -> Vec<bool> {
    for _ in 0..200 {
        if !results.lock().unwrap().is_empty() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    results.lock().unwrap().clone()
}

#[test]
fn background_refresh_recurs_and_runs_its_handler() {
    use background::testing::FakeScheduler;
    use background::*;

    let (service, fake) = BackgroundService::for_tests(&["app.refresh", "app.upkeep"]);
    let ran = Arc::new(Mutex::new(Vec::new()));
    let seen = ran.clone();
    service
        .register_refresh("app.refresh", 900.0, move |run| {
            let seen = seen.clone();
            async move {
                seen.lock().unwrap().push(run.identifier().to_string());
                true
            }
        })
        .unwrap();
    // Registering asks iOS for the first window.
    assert_eq!(
        *fake.submitted.lock().unwrap(),
        vec![BackgroundTaskRequest {
            identifier: "app.refresh".into(),
            kind: BackgroundTaskKind::Refresh,
            earliest_begin_seconds: 900.0,
        }]
    );

    let (run, results) = FakeScheduler::start(&service, "app.refresh");
    assert_eq!(finished(&results), vec![true]);
    assert_eq!(*ran.lock().unwrap(), vec!["app.refresh".to_string()]);
    assert!(run.is_finished());
    // Each run asks for the next one.
    assert_eq!(fake.submitted.lock().unwrap().len(), 2);
    // Only the first finish counts.
    run.finish(false);
    assert_eq!(*results.lock().unwrap(), vec![true]);
}

#[test]
fn background_tasks_must_be_listed_and_expiry_cancels() {
    use background::testing::FakeScheduler;
    use background::*;

    let (service, fake) = BackgroundService::for_tests(&["app.upkeep"]);
    let unlisted = service.register_refresh("app.other", 60.0, |_run| async { true });
    assert!(matches!(
        unlisted,
        Err(NativeServiceError::InvalidInput { .. })
    ));
    assert!(service
        .register_refresh("app.upkeep", -1.0, |_run| async { true })
        .is_err());

    // A handler that works until iOS says time is up, then reports success;
    // a cancelled run still counts as a failure.
    service
        .register_processing("app.upkeep", 3600.0, true, false, |run| async move {
            let _ = run.cancelled().recv();
            true
        })
        .unwrap();
    assert_eq!(
        fake.submitted.lock().unwrap()[0].kind,
        BackgroundTaskKind::Processing {
            requires_external_power: true,
            requires_network: false
        }
    );
    let (run, results) = FakeScheduler::start(&service, "app.upkeep");
    assert!(!run.is_cancelled());
    run.expire();
    assert!(run.is_cancelled());
    assert_eq!(finished(&results), vec![false]);

    service.cancel("app.upkeep");
    assert_eq!(
        *fake.cancelled.lock().unwrap(),
        vec!["app.upkeep".to_string()]
    );
}

#[test]
fn a_background_run_waits_for_its_handler() {
    use background::testing::FakeScheduler;
    use background::*;

    // iOS launched the app for a task before the core registered it.
    let (service, _fake) = BackgroundService::for_tests(&["app.refresh"]);
    let (_run, results) = FakeScheduler::start(&service, "app.refresh");
    assert_eq!(service.waiting(), 1);
    service
        .register_handler("app.refresh", |_run| async { true })
        .unwrap();
    assert_eq!(finished(&results), vec![true]);
    assert_eq!(service.waiting(), 0);

    // One whose window ends first fails without running.
    let (run, results) = FakeScheduler::start(&service.clone(), "app.missing");
    run.expire();
    assert_eq!(*results.lock().unwrap(), vec![false]);
}

#[test]
fn biometrics_confirm_and_name_what_the_device_has() {
    use biometrics::*;

    let face = BiometricAvailability {
        kind: BiometryKind::FaceId,
        biometrics_usable: true,
        passcode_set: true,
    };
    assert_eq!(face.name(), "Face ID");
    let (service, fake) = BiometricsService::for_tests(face);
    assert!(forge_runtime::block_on(service.confirm(" ")).is_err());
    assert_eq!(
        forge_runtime::block_on(service.confirm("show your recovery key")).unwrap(),
        AuthenticationResult::Authenticated
    );
    *fake.result.lock().unwrap() = AuthenticationResult::Cancelled;
    assert_eq!(
        forge_runtime::block_on(service.confirm("delete everything")).unwrap(),
        AuthenticationResult::Cancelled
    );
    assert_eq!(
        *fake.reasons.lock().unwrap(),
        vec!["show your recovery key", "delete everything"]
    );

    // Face ID not enrolled: the passcode still locks, and the setting says so.
    let passcode = BiometricAvailability {
        kind: BiometryKind::FaceId,
        biometrics_usable: false,
        passcode_set: true,
    };
    assert!(passcode.can_lock());
    assert_eq!(passcode.name(), "Passcode");
    // Nothing at all: no prompt is shown.
    let none = BiometricAvailability {
        kind: BiometryKind::None,
        biometrics_usable: false,
        passcode_set: false,
    };
    let (service, fake) = BiometricsService::for_tests(none);
    assert_eq!(
        forge_runtime::block_on(service.confirm("unlock")).unwrap(),
        AuthenticationResult::Unavailable
    );
    assert!(fake.reasons.lock().unwrap().is_empty());
}
