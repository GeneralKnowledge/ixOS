//! The lab against Forge's fake services: what starts when, and what the
//! snapshot shows.

use std::sync::Arc;

use forge_runtime::block_on;

use super::*;
use native::bluetooth::testing::FakeBluetooth;
use native::connectivity::testing::FakeConnectivity;
use native::connectivity::{NetworkInterface, Reachability};
use native::device_status::testing::FakeDevice;
use native::device_status::{DeviceStatusProvider, ThermalState};
use native::location::testing::FakeLocation;
use native::motion::testing::FakeMotion;
use native::testing::Fakes;

struct Lab {
    lab: DeviceLab,
    fakes: Fakes,
    net: Arc<FakeConnectivity>,
    device: Arc<FakeDevice>,
    motion: Arc<FakeMotion>,
    location: Arc<FakeLocation>,
    bluetooth: Arc<FakeBluetooth>,
    _dir: tempfile::TempDir,
}

fn lab() -> Lab {
    let dir = tempfile::tempdir().unwrap();
    let (native, fakes) = NativeServices::for_tests(dir.path());
    let (connectivity, net) = ConnectivityService::for_tests();
    let (device_service, device) = DeviceStatusService::for_tests();
    let (motion_service, motion) = MotionService::for_tests();
    let (location_service, location) = LocationService::for_tests();
    let (bluetooth_service, bluetooth) = BluetoothService::for_tests();
    let lab = DeviceLab::with_services(LabServices {
        native: Some(native),
        connectivity: Some(connectivity),
        device: Some(device_service),
        motion: Some(motion_service),
        location: Some(location_service),
        bluetooth: Some(bluetooth_service),
    });
    Lab {
        lab,
        fakes,
        net,
        device,
        motion,
        location,
        bluetooth,
        _dir: dir,
    }
}

fn status(interfaces: Vec<NetworkInterface>, expensive: bool, at: i64) -> ConnectivityStatus {
    ConnectivityStatus {
        reachability: if interfaces.is_empty() {
            Reachability::Offline
        } else {
            Reachability::Online
        },
        interfaces,
        is_expensive: expensive,
        is_constrained: false,
        supports_ipv4: true,
        supports_ipv6: true,
        supports_dns: true,
        offline_reason: None,
        observed_at_ms: at,
    }
}

fn accel(x: f64, t: f64) -> MotionReading {
    MotionReading::Acceleration {
        value: Vector3 { x, y: 0.0, z: -1.0 },
        timestamp_s: t,
    }
}

#[test]
fn nothing_runs_until_active_and_everything_stops_in_background() {
    let t = lab();
    assert!(!t.net.is_started());
    t.lab.set_active(true);
    assert!(t.net.is_started());
    assert!(t.device.is_started());
    assert_eq!(
        t.motion.interval_ms(MotionSensor::Accelerometer),
        Some(MOTION_INTERVAL_MS)
    );
    assert!(t.motion.interval_ms(MotionSensor::DeviceMotion).is_some());
    let snap = t.lab.snapshot();
    assert!(snap.accelerometer.running);
    assert!(snap.network.available);
    assert_eq!(snap.device.info.unwrap().model_identifier, "iPhone16,2");

    t.lab.set_active(false);
    assert!(!t.net.is_started());
    assert!(!t.device.is_started());
    assert_eq!(t.motion.interval_ms(MotionSensor::Accelerometer), None);
    assert!(!t.lab.snapshot().accelerometer.running);
}

#[test]
fn permission_gated_sources_wait_for_the_user() {
    let t = lab();
    t.lab.set_active(true);
    // No prompt without a tap: the barometer, location and Bluetooth wait.
    assert_eq!(t.motion.interval_ms(MotionSensor::Altimeter), None);
    assert_eq!(t.location.options(), None);
    assert_eq!(
        t.lab.snapshot().bluetooth.state,
        Some(BluetoothState::Unknown)
    );

    // The fakes grant whatever is requested.
    assert!(block_on(t.lab.request_permission(NativePermission::Location)).allows_use());
    let options = t
        .location
        .options()
        .expect("location started after the grant");
    assert!(options.include_heading);
    assert!(block_on(t.lab.request_permission(NativePermission::Motion)).allows_use());
    assert!(t.motion.interval_ms(MotionSensor::Altimeter).is_some());
    block_on(t.lab.request_permission(NativePermission::Bluetooth));
    t.bluetooth.set(BluetoothState::PoweredOn);
    assert_eq!(
        t.lab.snapshot().bluetooth.state,
        Some(BluetoothState::PoweredOn)
    );

    // Denied in Settings: refreshing reports it and nothing new starts.
    t.fakes
        .permissions
        .set(NativePermission::Location, PermissionStatus::Denied);
    let statuses = block_on(t.lab.refresh_permissions());
    assert_eq!(statuses.location, PermissionStatus::Denied);
    assert_eq!(
        t.lab.snapshot().permissions.location,
        PermissionStatus::Denied
    );
}

#[test]
fn motion_readings_become_stats() {
    let t = lab();
    t.lab.set_active(true);
    for i in 0..40 {
        t.motion
            .send(accel(if i == 10 { 2.0 } else { 0.0 }, i as f64 * 0.05));
    }
    t.motion.send(MotionReading::DeviceMotion {
        sample: DeviceMotionSample {
            attitude: native::motion::Attitude {
                roll: 0.0,
                pitch: 0.0,
                yaw: 0.0,
            },
            gravity: Vector3 {
                x: 0.0,
                y: -1.0,
                z: 0.0,
            },
            user_acceleration: Vector3 {
                x: 0.3,
                y: 0.4,
                z: 0.0,
            },
            rotation_rate: Vector3 {
                x: 0.0,
                y: 0.0,
                z: 0.0,
            },
            magnetic_field: None,
            magnetic_accuracy: native::motion::MagneticAccuracy::High,
            heading_degrees: Some(90.0),
        },
        timestamp_s: 2.0,
    });
    let snap = t.lab.snapshot();
    let a = snap.accelerometer;
    assert_eq!(a.history.len(), 40);
    assert!((a.rate_hz - 20.0).abs() < 0.1, "{}", a.rate_hz);
    assert!((a.peak_magnitude.unwrap() - 5f64.sqrt()).abs() < 1e-9);
    assert_eq!(a.magnitude, Some(1.0));
    assert!((snap.attitude.tilt_degrees.unwrap() - 90.0).abs() < 1e-9);
    assert_eq!(snap.attitude.peak_movement, Some(0.5));

    t.lab.reset();
    assert_eq!(t.lab.snapshot().accelerometer.peak_magnitude, Some(1.0));

    t.motion.send(MotionReading::Failed {
        sensor: MotionSensor::Gyroscope,
        reason: "stopped".into(),
    });
    assert_eq!(t.lab.snapshot().gyroscope.error.as_deref(), Some("stopped"));
}

#[test]
fn missing_sensor_is_reported_not_started() {
    let t = lab();
    t.motion.availability.lock().unwrap().magnetometer = false;
    let lab = DeviceLab::with_services(t.lab.services.clone());
    lab.set_active(true);
    let snap = lab.snapshot();
    assert!(!snap.magnetometer.available);
    assert!(!snap.magnetometer.running);
    assert!(snap.gyroscope.running);
}

#[test]
fn barometer_tracks_pressure_range() {
    let t = lab();
    t.lab.set_active(true);
    block_on(t.lab.request_permission(NativePermission::Motion));
    for (i, p) in [101.30, 101.32, 101.28].iter().enumerate() {
        t.motion.send(MotionReading::Altitude {
            relative_altitude_m: i as f64 * 0.5,
            pressure_kpa: *p,
            timestamp_s: i as f64,
        });
    }
    let b = t.lab.snapshot().barometer;
    assert!(b.running);
    assert_eq!(b.pressure_kpa, Some(101.28));
    assert_eq!(b.min_pressure_kpa, Some(101.28));
    assert_eq!(b.max_pressure_kpa, Some(101.32));
    assert_eq!(b.relative_altitude_m, Some(1.0));
}

#[test]
fn connectivity_log_records_changes_only() {
    let t = lab();
    t.lab.set_active(true);
    t.net.set(status(vec![NetworkInterface::Wifi], false, 10));
    assert!(t.lab.snapshot().network.unmetered);
    t.net.set(status(vec![NetworkInterface::Wifi], false, 20));
    t.net
        .set(status(vec![NetworkInterface::Cellular], true, 30));
    t.net.set(status(vec![], false, 40));
    let n = t.lab.snapshot().network;
    let kinds: Vec<_> = n
        .changes
        .iter()
        .map(|c| (c.at_ms, c.status.primary_interface()))
        .collect();
    // Unknown at start, then Wi-Fi, cellular, offline; the repeat is skipped.
    assert_eq!(
        kinds,
        vec![
            (40, None),
            (30, Some(NetworkInterface::Cellular)),
            (10, Some(NetworkInterface::Wifi)),
            (n.changes[3].at_ms, None),
        ]
    );
    assert!(!n.status.unwrap().is_online());
    assert!(!n.unmetered);
}

#[test]
fn device_status_changes_and_heavy_work_advice() {
    let t = lab();
    t.lab.set_active(true);
    assert_eq!(t.lab.snapshot().device.heavy_work_deferred, None);
    let mut hot = t.device.status();
    hot.thermal_state = ThermalState::Critical;
    t.device.set(hot);
    let d = t.lab.snapshot().device;
    assert_eq!(d.status.unwrap().thermal_state, ThermalState::Critical);
    assert_eq!(d.heavy_work_deferred, Some(DeferReason::Thermal));
}

#[test]
fn location_fixes_and_headings() {
    let t = lab();
    t.lab.set_active(true);
    block_on(t.lab.request_permission(NativePermission::Location));
    let fix = |lat: f64| LocationFix {
        latitude: lat,
        longitude: -0.12,
        horizontal_accuracy_m: 5.0,
        altitude_m: Some(20.0),
        vertical_accuracy_m: Some(4.0),
        speed_mps: Some(1.5),
        speed_accuracy_mps: Some(0.5),
        course_degrees: Some(0.0),
        course_accuracy_degrees: Some(10.0),
        floor: None,
        timestamp_ms: 1,
    };
    t.location.send(LocationEvent::Fix { fix: fix(51.5) });
    t.location.send(LocationEvent::Fix { fix: fix(51.501) });
    t.location.send(LocationEvent::Heading {
        heading: Heading {
            magnetic_degrees: 10.0,
            true_degrees: Some(11.0),
            accuracy_degrees: Some(5.0),
            timestamp_ms: 2,
        },
    });
    let l = t.lab.snapshot().location;
    assert!(l.running);
    assert_eq!(l.fixes, 2);
    assert!((l.distance_m - 111.2).abs() < 1.0);
    assert_eq!(l.heading.unwrap().true_degrees, Some(11.0));
    assert!(l.precise);

    t.location.send(LocationEvent::Failed {
        reason: "denied".into(),
        permission_denied: true,
    });
    assert_eq!(t.lab.snapshot().location.error.as_deref(), Some("denied"));
}

#[test]
fn a_host_without_services_shows_sections_unavailable() {
    let lab = DeviceLab::with_services(LabServices::default());
    lab.set_active(true);
    let snap = lab.snapshot();
    assert!(!snap.network.available);
    assert!(!snap.device.available);
    assert!(!snap.accelerometer.available);
    assert!(!snap.location.available);
    assert_eq!(
        block_on(lab.refresh_permissions()).location,
        PermissionStatus::Unsupported
    );
}
