//! Device Lab's Rust core: a live view of everything the phone will tell an
//! app about itself.
//!
//! The device capabilities are Forge's, not this app's: connectivity,
//! device status, motion, location and Bluetooth come from the optional
//! native services selected below (`crate::native::*`, see
//! docs/native-services.md). This crate is their consumer. It decides what
//! runs when, and keeps the app's own view of the values: rolling histories
//! for the sparklines, measured update rates, peaks, distance travelled and
//! a log of connectivity changes ([`stats`]).
//!
//! SwiftUI polls [`DeviceLab::snapshot`] about ten times a second while the
//! dashboard is on screen; a snapshot only copies memory. Sensors run only
//! while the app is active ([`DeviceLab::set_active`]).

pub mod crash_test;
pub mod stats;
pub mod transfer_test;

use std::sync::{Arc, Mutex};

use forge_runtime::{lock, now_ms};
use forge_services::Watch;

uniffi::setup_scaffolding!("forge_ffi");

// The showcase's other apps (Cargo.toml). Naming them links their Rust cores,
// and so their UniFFI exports, into this library; `forge bindings generate`
// gives each its own Swift module, which ForgeFFI re-exports.
use chat_app as _;
use checklist_core as _;
use commonplace_core as _;
use fieldkit_core as _;
use good_dog_app as _;
use pocket_library_core as _;
use weather_core as _;

// The framework services this app uses. Each optional one brings its own
// Swift adapter into the app, and nothing else does: Core Location, Core
// Motion and Core Bluetooth are linked because they are named here.
forge_services::native_services!(
    connectivity,
    device_status,
    motion,
    location,
    bluetooth,
    transfers
);

use native::bluetooth::{BluetoothService, BluetoothState};
use native::connectivity::{ConnectivityService, ConnectivityStatus, TransferPolicy};
use native::device_status::{DeferReason, DeviceInfo, DeviceStatus, DeviceStatusService};
use native::location::{
    Heading, LocationAccuracy, LocationEvent, LocationFix, LocationOptions, LocationService,
};
use native::motion::{
    DeviceMotionSample, MotionAvailability, MotionReading, MotionSensor, MotionService, Vector3,
};
use native::{NativePermission, NativeServices, PermissionStatus};
use stats::{tilt_degrees, ChangeLog, Series, Trip};

/// Update interval for the motion sensors: 20 Hz, smooth enough to read and
/// light on the battery.
pub const MOTION_INTERVAL_MS: u32 = 50;
/// Device status (storage and memory) is re-read at most this often.
pub const STATUS_POLL_MS: i64 = 1_000;

// MARK: Snapshot records

/// One raw sensor (accelerometer, gyroscope, magnetometer).
#[derive(uniffi::Record, Clone, Debug, PartialEq)]
pub struct AxisStats {
    pub available: bool,
    pub running: bool,
    pub latest: Option<Vector3>,
    /// Length of `latest`.
    pub magnitude: Option<f64>,
    /// Highest magnitude since the last reset.
    pub peak_magnitude: Option<f64>,
    /// Measured updates per second.
    pub rate_hz: f64,
    /// Recent magnitudes, oldest first.
    pub history: Vec<f64>,
    pub error: Option<String>,
}

#[derive(uniffi::Record, Clone, Debug, PartialEq)]
pub struct AttitudeStats {
    pub available: bool,
    pub running: bool,
    pub sample: Option<DeviceMotionSample>,
    /// 0 lying flat face up, 90 upright, 180 face down.
    pub tilt_degrees: Option<f64>,
    /// Recent user-acceleration magnitudes (movement without gravity), g.
    pub movement_history: Vec<f64>,
    pub peak_movement: Option<f64>,
    pub rate_hz: f64,
    pub error: Option<String>,
}

#[derive(uniffi::Record, Clone, Debug, PartialEq)]
pub struct BarometerStats {
    pub available: bool,
    pub running: bool,
    pub pressure_kpa: Option<f64>,
    pub min_pressure_kpa: Option<f64>,
    pub max_pressure_kpa: Option<f64>,
    /// Metres since the barometer started.
    pub relative_altitude_m: Option<f64>,
    /// Recent pressures, oldest first.
    pub history: Vec<f64>,
    pub error: Option<String>,
}

#[derive(uniffi::Record, Clone, Debug, PartialEq)]
pub struct LocationStats {
    pub available: bool,
    pub running: bool,
    pub fix: Option<LocationFix>,
    pub heading: Option<Heading>,
    pub precise: bool,
    pub heading_available: bool,
    pub fixes: u64,
    /// Metres travelled over accurate fixes since the last reset.
    pub distance_m: f64,
    pub max_speed_mps: Option<f64>,
    pub error: Option<String>,
}

#[derive(uniffi::Record, Clone, Debug, PartialEq)]
pub struct ConnectivityChange {
    pub at_ms: i64,
    pub status: ConnectivityStatus,
}

#[derive(uniffi::Record, Clone, Debug, PartialEq)]
pub struct NetworkStats {
    pub available: bool,
    pub status: Option<ConnectivityStatus>,
    /// Forge's `TransferPolicy::Unmetered` applied to `status`: whether a
    /// Wi-Fi-only download (such as an on-device model) would start now.
    pub unmetered: bool,
    /// Newest first.
    pub changes: Vec<ConnectivityChange>,
}

#[derive(uniffi::Record, Clone, Debug, PartialEq)]
pub struct DeviceStats {
    pub available: bool,
    pub info: Option<DeviceInfo>,
    pub status: Option<DeviceStatus>,
    /// Forge's default heavy-work policy, applied to `status` (what a model
    /// download would do now). `None`: go ahead.
    pub heavy_work_deferred: Option<DeferReason>,
}

#[derive(uniffi::Record, Clone, Debug, PartialEq)]
pub struct BluetoothStats {
    pub available: bool,
    pub state: Option<BluetoothState>,
}

#[derive(uniffi::Record, Clone, Copy, Debug, PartialEq, Eq)]
pub struct PermissionStatuses {
    pub location: PermissionStatus,
    pub motion: PermissionStatus,
    pub bluetooth: PermissionStatus,
}

/// Everything on the dashboard.
#[derive(uniffi::Record, Clone, Debug, PartialEq)]
pub struct LabSnapshot {
    pub active: bool,
    pub permissions: PermissionStatuses,
    pub network: NetworkStats,
    pub device: DeviceStats,
    pub bluetooth: BluetoothStats,
    pub accelerometer: AxisStats,
    pub gyroscope: AxisStats,
    pub magnetometer: AxisStats,
    pub attitude: AttitudeStats,
    pub barometer: BarometerStats,
    pub location: LocationStats,
}

// MARK: State

#[derive(Default)]
struct Axis {
    series: Series,
    latest: Option<Vector3>,
    error: Option<String>,
}

impl Axis {
    fn push(&mut self, value: Vector3, timestamp_s: f64) {
        self.series.push(timestamp_s, value.magnitude());
        self.latest = Some(value);
        self.error = None;
    }

    fn stats(&self, available: bool, running: bool) -> AxisStats {
        AxisStats {
            available,
            running,
            latest: self.latest,
            magnitude: self.series.latest(),
            peak_magnitude: self.series.peak(),
            rate_hz: if running { self.series.rate_hz() } else { 0.0 },
            history: self.series.values(),
            error: self.error.clone(),
        }
    }
}

struct State {
    active: bool,
    permissions: PermissionStatuses,
    connectivity: Option<ConnectivityStatus>,
    connectivity_log: ChangeLog<ConnectivityStatus>,
    device_status: Option<DeviceStatus>,
    status_read_at_ms: i64,
    bluetooth: Option<BluetoothState>,
    accelerometer: Axis,
    gyroscope: Axis,
    magnetometer: Axis,
    attitude: Option<DeviceMotionSample>,
    movement: Series,
    attitude_error: Option<String>,
    pressure: Series,
    relative_altitude_m: Option<f64>,
    barometer_error: Option<String>,
    fix: Option<LocationFix>,
    heading: Option<Heading>,
    trip: Trip,
    location_error: Option<String>,
}

impl Default for State {
    fn default() -> Self {
        Self {
            active: false,
            permissions: PermissionStatuses {
                location: PermissionStatus::NotDetermined,
                motion: PermissionStatus::NotDetermined,
                bluetooth: PermissionStatus::NotDetermined,
            },
            connectivity: None,
            connectivity_log: ChangeLog::default(),
            device_status: None,
            status_read_at_ms: 0,
            bluetooth: None,
            accelerometer: Axis::default(),
            gyroscope: Axis::default(),
            magnetometer: Axis::default(),
            attitude: None,
            movement: Series::default(),
            attitude_error: None,
            pressure: Series::default(),
            relative_altitude_m: None,
            barometer_error: None,
            fix: None,
            heading: None,
            trip: Trip::default(),
            location_error: None,
        }
    }
}

impl State {
    fn apply_motion(&mut self, reading: &MotionReading) {
        match reading {
            MotionReading::Acceleration { value, timestamp_s } => {
                self.accelerometer.push(*value, *timestamp_s)
            }
            MotionReading::RotationRate { value, timestamp_s } => {
                self.gyroscope.push(*value, *timestamp_s)
            }
            MotionReading::MagneticField { value, timestamp_s } => {
                self.magnetometer.push(*value, *timestamp_s)
            }
            MotionReading::DeviceMotion {
                sample,
                timestamp_s,
            } => {
                self.movement
                    .push(*timestamp_s, sample.user_acceleration.magnitude());
                self.attitude = Some(*sample);
                self.attitude_error = None;
            }
            MotionReading::Altitude {
                relative_altitude_m,
                pressure_kpa,
                timestamp_s,
            } => {
                self.pressure.push(*timestamp_s, *pressure_kpa);
                self.relative_altitude_m = Some(*relative_altitude_m);
                self.barometer_error = None;
            }
            MotionReading::Failed { sensor, reason } => {
                self.set_motion_error(*sensor, reason.clone());
            }
        }
    }

    fn set_motion_error(&mut self, sensor: MotionSensor, reason: String) {
        let slot = match sensor {
            MotionSensor::Accelerometer => &mut self.accelerometer.error,
            MotionSensor::Gyroscope => &mut self.gyroscope.error,
            MotionSensor::Magnetometer => &mut self.magnetometer.error,
            MotionSensor::DeviceMotion => &mut self.attitude_error,
            MotionSensor::Altimeter => &mut self.barometer_error,
        };
        *slot = Some(reason);
    }

    fn apply_location(&mut self, event: &LocationEvent) {
        match event {
            LocationEvent::Fix { fix } => {
                self.trip.add(
                    fix.latitude,
                    fix.longitude,
                    fix.horizontal_accuracy_m,
                    fix.speed_mps,
                );
                self.fix = Some(fix.clone());
                self.location_error = None;
            }
            LocationEvent::Heading { heading } => self.heading = Some(*heading),
            LocationEvent::Failed { reason, .. } => self.location_error = Some(reason.clone()),
        }
    }

    fn apply_connectivity(&mut self, status: &ConnectivityStatus) {
        // The log records what changed, not every report of the same path.
        let at = if status.observed_at_ms > 0 {
            status.observed_at_ms
        } else {
            now_ms()
        };
        self.connectivity_log.record(at, status.clone(), |a, b| {
            let mut a = a.clone();
            a.observed_at_ms = b.observed_at_ms;
            a == *b
        });
        self.connectivity = Some(status.clone());
    }
}

/// The framework services the lab found installed. Each is optional: a
/// host without one shows that section as unavailable.
#[derive(Clone, Default)]
pub struct LabServices {
    pub native: Option<NativeServices>,
    pub connectivity: Option<ConnectivityService>,
    pub device: Option<DeviceStatusService>,
    pub motion: Option<MotionService>,
    pub location: Option<LocationService>,
    pub bluetooth: Option<BluetoothService>,
}

impl LabServices {
    /// What the host installed (`ForgeServices.install()`).
    pub fn installed() -> Self {
        Self {
            native: NativeServices::installed().ok(),
            connectivity: ConnectivityService::installed().ok(),
            device: DeviceStatusService::installed().ok(),
            motion: MotionService::installed().ok(),
            location: LocationService::installed().ok(),
            bluetooth: BluetoothService::installed().ok(),
        }
    }
}

#[derive(Default)]
struct Watches {
    connectivity: Option<Watch>,
    device: Option<Watch>,
    bluetooth: Option<Watch>,
    motion: Vec<(MotionSensor, Watch)>,
    location: Option<Watch>,
}

impl Watches {
    fn motion_running(&self, sensor: MotionSensor) -> bool {
        self.motion.iter().any(|(s, _)| *s == sensor)
    }
}

// MARK: API

/// The lab the host opens once. Thread-safe.
#[derive(uniffi::Object)]
pub struct DeviceLab {
    services: LabServices,
    state: Arc<Mutex<State>>,
    watches: Mutex<Watches>,
    info: Option<DeviceInfo>,
    availability: Option<MotionAvailability>,
}

#[uniffi::export]
impl DeviceLab {
    /// Uses the services the host installed at launch.
    #[uniffi::constructor]
    pub fn new() -> Arc<Self> {
        forge_runtime::logging::init();
        Arc::new(Self::with_services(LabServices::installed()))
    }

    /// Everything on screen, copied from memory. Re-reads storage and memory
    /// at most once a second.
    pub fn snapshot(&self) -> LabSnapshot {
        self.poll_device_status();
        let watches = lock(&self.watches);
        let state = lock(&self.state);
        let has = |sensor| self.availability.is_some_and(|a| a.has(sensor));
        let s = &self.services;
        LabSnapshot {
            active: state.active,
            permissions: state.permissions,
            network: NetworkStats {
                available: s.connectivity.is_some(),
                status: state.connectivity.clone(),
                unmetered: state
                    .connectivity
                    .as_ref()
                    .is_some_and(|status| status.allows(TransferPolicy::Unmetered)),
                changes: state
                    .connectivity_log
                    .entries()
                    .into_iter()
                    .map(|(at_ms, status)| ConnectivityChange { at_ms, status })
                    .collect(),
            },
            device: DeviceStats {
                available: s.device.is_some(),
                info: self.info.clone(),
                heavy_work_deferred: state
                    .device_status
                    .as_ref()
                    .and_then(|status| status.defer_heavy_work(None)),
                status: state.device_status.clone(),
            },
            bluetooth: BluetoothStats {
                available: s.bluetooth.is_some(),
                state: state.bluetooth,
            },
            accelerometer: state.accelerometer.stats(
                has(MotionSensor::Accelerometer),
                watches.motion_running(MotionSensor::Accelerometer),
            ),
            gyroscope: state.gyroscope.stats(
                has(MotionSensor::Gyroscope),
                watches.motion_running(MotionSensor::Gyroscope),
            ),
            magnetometer: state.magnetometer.stats(
                has(MotionSensor::Magnetometer),
                watches.motion_running(MotionSensor::Magnetometer),
            ),
            attitude: AttitudeStats {
                available: has(MotionSensor::DeviceMotion),
                running: watches.motion_running(MotionSensor::DeviceMotion),
                sample: state.attitude,
                tilt_degrees: state
                    .attitude
                    .and_then(|m| tilt_degrees(m.gravity.x, m.gravity.y, m.gravity.z)),
                movement_history: state.movement.values(),
                peak_movement: state.movement.peak(),
                rate_hz: if watches.motion_running(MotionSensor::DeviceMotion) {
                    state.movement.rate_hz()
                } else {
                    0.0
                },
                error: state.attitude_error.clone(),
            },
            barometer: BarometerStats {
                available: has(MotionSensor::Altimeter),
                running: watches.motion_running(MotionSensor::Altimeter),
                pressure_kpa: state.pressure.latest(),
                min_pressure_kpa: state.pressure.min(),
                max_pressure_kpa: state.pressure.peak(),
                relative_altitude_m: state.relative_altitude_m,
                history: state.pressure.values(),
                error: state.barometer_error.clone(),
            },
            location: LocationStats {
                available: s.location.is_some(),
                running: watches.location.is_some(),
                fix: state.fix.clone(),
                heading: state.heading,
                precise: s.location.as_ref().is_some_and(|l| l.is_precise()),
                heading_available: s.location.as_ref().is_some_and(|l| l.heading_available()),
                fixes: state.trip.fixes,
                distance_m: state.trip.distance_m,
                max_speed_mps: state.trip.max_speed_mps,
                error: state.location_error.clone(),
            },
        }
    }

    /// The host reports whether the app is on screen. Sensors, location and
    /// the monitors run only while it is; going to the background stops them
    /// all (the app has no background modes).
    pub fn set_active(&self, active: bool) {
        lock(&self.state).active = active;
        if active {
            self.start_allowed();
        } else {
            // Dropping the watches stops the platform updates.
            *lock(&self.watches) = Watches::default();
        }
    }

    /// Re-reads the permission statuses (on launch and when the app comes
    /// back from Settings) and starts what they now allow.
    pub async fn refresh_permissions(&self) -> PermissionStatuses {
        let statuses = match &self.services.native {
            Some(native) => PermissionStatuses {
                location: native.permission_status(NativePermission::Location).await,
                motion: native.permission_status(NativePermission::Motion).await,
                bluetooth: native.permission_status(NativePermission::Bluetooth).await,
            },
            None => PermissionStatuses {
                location: PermissionStatus::Unsupported,
                motion: PermissionStatus::Unsupported,
                bluetooth: PermissionStatus::Unsupported,
            },
        };
        lock(&self.state).permissions = statuses;
        if lock(&self.state).active {
            self.start_allowed();
        }
        statuses
    }

    /// Shows the system prompt for `permission` (if never asked) and starts
    /// what it allows. Call it from a button the user tapped.
    pub async fn request_permission(&self, permission: NativePermission) -> PermissionStatus {
        let Some(native) = self.services.native.clone() else {
            return PermissionStatus::Unsupported;
        };
        let status = native.request_permission(permission).await;
        {
            let mut state = lock(&self.state);
            match permission {
                NativePermission::Location => state.permissions.location = status,
                NativePermission::Motion => state.permissions.motion = status,
                NativePermission::Bluetooth => state.permissions.bluetooth = status,
                // Device Lab does not show these.
                NativePermission::Notifications | NativePermission::Microphone => {}
            }
        }
        if lock(&self.state).active {
            self.start_allowed();
        }
        status
    }

    /// Forgets peaks, minimums and the distance travelled.
    pub fn reset(&self) {
        let mut state = lock(&self.state);
        state.accelerometer.series.reset_extremes();
        state.gyroscope.series.reset_extremes();
        state.magnetometer.series.reset_extremes();
        state.movement.reset_extremes();
        state.pressure.reset_extremes();
        state.trip.reset();
    }
}

impl DeviceLab {
    pub fn with_services(services: LabServices) -> Self {
        let info = services.device.as_ref().map(|d| d.info());
        let availability = services.motion.as_ref().map(|m| m.availability());
        Self {
            services,
            state: Arc::new(Mutex::new(State::default())),
            watches: Mutex::new(Watches::default()),
            info,
            availability,
        }
    }

    fn poll_device_status(&self) {
        let Some(device) = &self.services.device else {
            return;
        };
        let now = now_ms();
        {
            let state = lock(&self.state);
            if !state.active || now - state.status_read_at_ms < STATUS_POLL_MS {
                return;
            }
        }
        let status = device.status();
        let mut state = lock(&self.state);
        state.device_status = Some(status);
        state.status_read_at_ms = now;
    }

    /// Starts every watch that is not running and that the permissions
    /// allow. Watches that need a permission never start before it is
    /// granted, so no system prompt appears unless the user asked.
    fn start_allowed(&self) {
        let permissions = lock(&self.state).permissions;
        let mut watches = lock(&self.watches);
        let s = &self.services;

        if let (Some(service), None) = (&s.connectivity, &watches.connectivity) {
            let state = self.state.clone();
            watches.connectivity = Some(service.watch(move |status| {
                lock(&state).apply_connectivity(status);
            }));
        }
        if let (Some(service), None) = (&s.device, &watches.device) {
            let state = self.state.clone();
            watches.device = Some(service.watch(move |status| {
                let mut state = lock(&state);
                state.device_status = Some(status.clone());
                state.status_read_at_ms = now_ms();
            }));
        }
        if let (Some(service), None) = (&s.bluetooth, &watches.bluetooth) {
            if permissions.bluetooth.allows_use() {
                let state = self.state.clone();
                watches.bluetooth = Some(service.watch(move |bt| {
                    lock(&state).bluetooth = Some(*bt);
                }));
            } else {
                lock(&self.state).bluetooth = Some(service.state());
            }
        }
        if let Some(motion) = &s.motion {
            for sensor in MotionSensor::ALL {
                if watches.motion_running(sensor) || !motion.availability().has(sensor) {
                    continue;
                }
                if sensor == MotionSensor::Altimeter && !permissions.motion.allows_use() {
                    continue;
                }
                let state = self.state.clone();
                match motion.watch(sensor, MOTION_INTERVAL_MS, move |reading| {
                    lock(&state).apply_motion(reading);
                }) {
                    Ok(watch) => watches.motion.push((sensor, watch)),
                    Err(err) => lock(&self.state).set_motion_error(sensor, err.to_string()),
                }
            }
        }
        if let (Some(service), None) = (&s.location, &watches.location) {
            if permissions.location.allows_use() {
                let state = self.state.clone();
                let options = LocationOptions {
                    accuracy: LocationAccuracy::Best,
                    distance_filter_m: None,
                    include_heading: true,
                };
                match service.watch(options, move |event| {
                    lock(&state).apply_location(event);
                }) {
                    Ok(watch) => watches.location = Some(watch),
                    Err(err) => lock(&self.state).location_error = Some(err.to_string()),
                }
            }
        }
    }
}

#[cfg(test)]
mod tests;
