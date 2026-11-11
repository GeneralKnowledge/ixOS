//! The optional services an app selects in `native_services!(...)`:
//! `connectivity`, `device_status`, `motion`, `location`, `bluetooth`,
//! `audio` and `transfers` (which needs this crate's `transfers` feature).
//!
//! Each one emits a submodule of the app's `native` module with its records,
//! one host trait the Swift adapter implements, a sink object the adapter
//! sends live values to, an `install_native_<name>` function, and the
//! Rust-side API (`ConnectivityService`, ...). `generate-bindings.sh` copies
//! a service's Swift adapter (`swift/optional/ForgeServices+<Name>.swift`)
//! only when the bindings contain its `installNative<Name>` function, so an
//! app links only the frameworks of the services it selected.

#[doc(hidden)]
#[macro_export]
macro_rules! __native_service {
    (connectivity) => {
        $crate::__native_connectivity!();
    };
    (device_status) => {
        $crate::__native_device_status!();
    };
    (motion) => {
        $crate::__native_motion!();
    };
    (location) => {
        $crate::__native_location!();
    };
    (bluetooth) => {
        $crate::__native_bluetooth!();
    };
    (audio) => {
        $crate::__native_audio!();
    };
    (transfers) => {
        $crate::__native_transfers!();
    };
    ($other:ident) => {
        compile_error!(concat!(
            "unknown native service `",
            stringify!($other),
            "`: use connectivity, device_status, motion, location, bluetooth, audio or transfers"
        ));
    };
}

/// Emits the common parts of an optional service: the installed slot and
/// `installed`/`install`/`uninstall`.
#[doc(hidden)]
#[macro_export]
macro_rules! __native_slot {
    ($service:ident, $what:literal) => {
        static INSTALLED: $crate::Slot<$service> = $crate::Slot::new();

        impl $service {
            /// The service the host installed, or `NotInstalled`.
            pub fn installed() -> Result<Self, super::NativeServiceError> {
                INSTALLED
                    .get()
                    .ok_or_else(|| super::NativeServiceError::NotInstalled {
                        service: concat!(
                            $what,
                            " (select it in native_services!(...) and call ForgeServices.install() at launch)"
                        )
                        .into(),
                    })
            }

            /// Makes this the installed service.
            pub fn install(self) {
                INSTALLED.set(self);
            }

            /// Removes the installed service (for tests).
            pub fn uninstall() {
                INSTALLED.clear();
            }
        }
    };
}

#[doc(hidden)]
#[macro_export]
macro_rules! __native_connectivity {
    () => {
        /// Network connectivity: online or offline, over which interface,
        /// and whether the connection is metered. iOS: `NWPathMonitor`.
        pub mod connectivity {
            use ::std::sync::{Arc, Weak};

            /// Whether the device can reach the network now.
            #[derive(::uniffi::Enum, Clone, Copy, Debug, PartialEq, Eq, Hash)]
            pub enum Reachability {
                Online,
                Offline,
                /// Not connected, but a connection would be made on demand
                /// (a VPN or Wi-Fi Assist waiting to start).
                RequiresConnection,
                /// The host has not reported yet.
                Unknown,
            }

            #[derive(::uniffi::Enum, Clone, Copy, Debug, PartialEq, Eq, Hash)]
            pub enum NetworkInterface {
                Wifi,
                Cellular,
                Wired,
                Loopback,
                Other,
            }

            /// Why the device is offline, when the platform says.
            #[derive(::uniffi::Enum, Clone, Copy, Debug, PartialEq, Eq, Hash)]
            pub enum OfflineReason {
                /// No network is available (airplane mode, no signal).
                NotAvailable,
                /// The user turned off mobile data for this app.
                CellularDenied,
                /// The user turned off Wi-Fi for this app (some regions).
                WifiDenied,
                /// The user refused local network access.
                LocalNetworkDenied,
                Other,
            }

            #[derive(::uniffi::Record, Clone, Debug, PartialEq, Eq)]
            pub struct ConnectivityStatus {
                pub reachability: Reachability,
                /// The interfaces the current route uses, preferred first.
                pub interfaces: Vec<NetworkInterface>,
                /// Metered: cellular, or a personal hotspot.
                pub is_expensive: bool,
                /// Low Data Mode is on for this network.
                pub is_constrained: bool,
                pub supports_ipv4: bool,
                pub supports_ipv6: bool,
                pub supports_dns: bool,
                pub offline_reason: Option<OfflineReason>,
                /// When the host saw this status (Unix milliseconds).
                pub observed_at_ms: i64,
            }

            /// Which connections a transfer may use.
            #[derive(Clone, Copy, Debug, PartialEq, Eq)]
            pub enum TransferPolicy {
                /// Any connection.
                Any,
                /// Only networks that are neither metered nor in Low Data
                /// Mode (in practice Wi-Fi or wired). For large downloads
                /// such as on-device models.
                Unmetered,
            }

            impl ConnectivityStatus {
                /// What Rust sees before the host has reported.
                pub fn unknown() -> Self {
                    Self {
                        reachability: Reachability::Unknown,
                        interfaces: Vec::new(),
                        is_expensive: false,
                        is_constrained: false,
                        supports_ipv4: false,
                        supports_ipv6: false,
                        supports_dns: false,
                        offline_reason: None,
                        observed_at_ms: 0,
                    }
                }

                pub fn is_online(&self) -> bool {
                    self.reachability == Reachability::Online
                }

                /// The interface traffic goes over first, if online.
                pub fn primary_interface(&self) -> Option<NetworkInterface> {
                    if self.is_online() {
                        self.interfaces.first().copied()
                    } else {
                        None
                    }
                }

                /// Online, not metered and not in Low Data Mode.
                pub fn is_unmetered(&self) -> bool {
                    self.is_online() && !self.is_expensive && !self.is_constrained
                }

                /// True when a transfer under `policy` may start now.
                pub fn allows(&self, policy: TransferPolicy) -> bool {
                    match policy {
                        TransferPolicy::Any => self.is_online(),
                        TransferPolicy::Unmetered => self.is_unmetered(),
                    }
                }
            }

            /// Where the host sends connectivity changes.
            #[derive(::uniffi::Object)]
            pub struct ConnectivitySink {
                feed: Weak<$crate::Feed<ConnectivityStatus>>,
            }

            #[::uniffi::export]
            impl ConnectivitySink {
                pub fn send(&self, status: ConnectivityStatus) {
                    if let Some(feed) = self.feed.upgrade() {
                        feed.publish(status);
                    }
                }
            }

            /// Implemented by the host (iOS: `NWPathMonitor`).
            #[::uniffi::export(with_foreign)]
            pub trait ConnectivityMonitor: Send + Sync {
                /// The status now.
                fn current(&self) -> ConnectivityStatus;
                /// Send every change to `sink` (and the current status
                /// straight away) until `stop`.
                fn start(&self, sink: Arc<ConnectivitySink>);
                fn stop(&self);
            }

            /// Called by the host at launch (`ForgeServices.install()`).
            #[::uniffi::export]
            pub fn install_native_connectivity(monitor: Arc<dyn ConnectivityMonitor>) {
                ConnectivityService::new(monitor).install();
            }

            /// Connectivity for Rust code. Cheap to clone.
            #[derive(Clone)]
            pub struct ConnectivityService {
                monitor: Arc<dyn ConnectivityMonitor>,
                feed: Arc<$crate::Feed<ConnectivityStatus>>,
            }

            $crate::__native_slot!(ConnectivityService, "connectivity");

            impl ConnectivityService {
                pub fn new(monitor: Arc<dyn ConnectivityMonitor>) -> Self {
                    Self {
                        monitor,
                        feed: $crate::Feed::new(),
                    }
                }

                /// Fresh from the host.
                pub fn current(&self) -> ConnectivityStatus {
                    self.monitor.current()
                }

                /// Calls `listener` with the current status and every change,
                /// on the host's thread, until the watch is dropped.
                pub fn watch(
                    &self,
                    listener: impl Fn(&ConnectivityStatus) + Send + Sync + 'static,
                ) -> $crate::Watch {
                    let monitor = self.monitor.clone();
                    let stopper = self.monitor.clone();
                    let sink = Arc::new(ConnectivitySink {
                        feed: Arc::downgrade(&self.feed),
                    });
                    self.feed
                        .watch(
                            listener,
                            move || {
                                monitor.start(sink);
                                Ok::<(), ::std::convert::Infallible>(())
                            },
                            move || stopper.stop(),
                        )
                        .unwrap_or_else(|never| match never {})
                }

                /// A service over [`testing::FakeConnectivity`].
                pub fn for_tests() -> (Self, Arc<testing::FakeConnectivity>) {
                    let fake = Arc::new(testing::FakeConnectivity::default());
                    (Self::new(fake.clone()), fake)
                }
            }

            pub mod testing {
                use super::*;
                use ::std::sync::Mutex;

                /// A monitor whose status the test sets.
                pub struct FakeConnectivity {
                    status: Mutex<ConnectivityStatus>,
                    sink: Mutex<Option<Arc<ConnectivitySink>>>,
                }

                impl Default for FakeConnectivity {
                    fn default() -> Self {
                        Self {
                            status: Mutex::new(ConnectivityStatus::unknown()),
                            sink: Mutex::new(None),
                        }
                    }
                }

                impl FakeConnectivity {
                    /// Changes the status and tells the listeners.
                    pub fn set(&self, status: ConnectivityStatus) {
                        *self.status.lock().unwrap() = status.clone();
                        let sink = self.sink.lock().unwrap().clone();
                        if let Some(sink) = sink {
                            sink.send(status);
                        }
                    }

                    pub fn is_started(&self) -> bool {
                        self.sink.lock().unwrap().is_some()
                    }
                }

                impl ConnectivityMonitor for FakeConnectivity {
                    fn current(&self) -> ConnectivityStatus {
                        self.status.lock().unwrap().clone()
                    }
                    fn start(&self, sink: Arc<ConnectivitySink>) {
                        sink.send(self.current());
                        *self.sink.lock().unwrap() = Some(sink);
                    }
                    fn stop(&self) {
                        *self.sink.lock().unwrap() = None;
                    }
                }
            }
        }
    };
}

#[doc(hidden)]
#[macro_export]
macro_rules! __native_device_status {
    () => {
        /// Battery, power, thermal state, storage, memory and what device
        /// this is. iOS: `UIDevice`, `ProcessInfo`, file-system resource
        /// values, `os_proc_available_memory`.
        pub mod device_status {
            use ::std::sync::{Arc, Weak};

            #[derive(::uniffi::Enum, Clone, Copy, Debug, PartialEq, Eq, Hash)]
            pub enum BatteryState {
                Unknown,
                Unplugged,
                Charging,
                /// Plugged in and full.
                Full,
            }

            /// How hot the device is, as the system judges it. At
            /// `Serious` the system slows the CPU; at `Critical` apps
            /// should stop optional work.
            #[derive(::uniffi::Enum, Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
            pub enum ThermalState {
                Nominal,
                Fair,
                Serious,
                Critical,
            }

            /// Why heavy work should wait (see
            /// [`DeviceStatus::defer_heavy_work`]).
            #[derive(::uniffi::Enum, Clone, Copy, Debug, PartialEq, Eq, Hash)]
            pub enum DeferReason {
                Thermal,
                LowPowerMode,
                LowBattery,
                LowMemory,
            }

            /// What the device is. Does not change while the app runs.
            #[derive(::uniffi::Record, Clone, Debug, PartialEq)]
            pub struct DeviceInfo {
                /// Hardware model, e.g. `iPhone16,2`.
                pub model_identifier: String,
                /// e.g. `iPhone`, `iPad`.
                pub model: String,
                /// e.g. `iOS`.
                pub system_name: String,
                /// e.g. `26.3.1`.
                pub system_version: String,
                /// Since iOS 16 this is the generic model name ("iPhone"):
                /// the user's own device name needs an entitlement Apple
                /// grants on request.
                pub device_name: String,
                pub processor_count: u32,
                pub active_processor_count: u32,
                pub physical_memory_bytes: u64,
                pub screen_width_points: f64,
                pub screen_height_points: f64,
                pub screen_scale: f64,
                pub is_simulator: bool,
                pub is_ios_app_on_mac: bool,
            }

            /// The device's state now. Battery, power and thermal changes
            /// are pushed; storage and memory are read on each `status()`.
            #[derive(::uniffi::Record, Clone, Debug, PartialEq)]
            pub struct DeviceStatus {
                /// 0.0 to 1.0, `None` where unknown (the simulator).
                pub battery_level: Option<f32>,
                pub battery_state: BatteryState,
                pub low_power_mode: bool,
                pub thermal_state: ThermalState,
                pub storage_total_bytes: Option<u64>,
                /// Space available for things the user asked for (iOS
                /// "important usage": counts purgeable space).
                pub storage_available_bytes: Option<u64>,
                /// How much more memory this app may use before the system
                /// ends it (`os_proc_available_memory`); `None` off iOS.
                pub memory_available_bytes: Option<u64>,
                /// Memory this app uses now (physical footprint).
                pub memory_footprint_bytes: Option<u64>,
                /// Seconds since the device started.
                pub uptime_seconds: f64,
                /// Screen brightness 0.0 to 1.0, `None` off iOS.
                pub screen_brightness: Option<f32>,
                /// When the host read this (Unix milliseconds).
                pub observed_at_ms: i64,
            }

            impl DeviceStatus {
                pub fn is_on_power(&self) -> bool {
                    matches!(
                        self.battery_state,
                        BatteryState::Charging | BatteryState::Full
                    )
                }

                /// The default policy for heavy work (large downloads,
                /// imports, on-device inference): wait when the device is
                /// hot, in Low Power Mode, under 20% and unplugged, or short
                /// of `memory_needed` bytes. `None` means go ahead.
                pub fn defer_heavy_work(&self, memory_needed: Option<u64>) -> Option<DeferReason> {
                    use $crate::sensors::{Blocker, Thermal};
                    let thermal = match self.thermal_state {
                        ThermalState::Nominal => Thermal::Nominal,
                        ThermalState::Fair => Thermal::Fair,
                        ThermalState::Serious => Thermal::Serious,
                        ThermalState::Critical => Thermal::Critical,
                    };
                    $crate::sensors::heavy_work_blocker(
                        thermal,
                        self.low_power_mode,
                        self.battery_level,
                        self.is_on_power(),
                        self.memory_available_bytes,
                        memory_needed,
                    )
                    .map(|blocker| match blocker {
                        Blocker::Thermal => DeferReason::Thermal,
                        Blocker::LowPowerMode => DeferReason::LowPowerMode,
                        Blocker::LowBattery => DeferReason::LowBattery,
                        Blocker::LowMemory => DeferReason::LowMemory,
                    })
                }
            }

            /// Where the host sends battery, power and thermal changes.
            #[derive(::uniffi::Object)]
            pub struct DeviceStatusSink {
                feed: Weak<$crate::Feed<DeviceStatus>>,
            }

            #[::uniffi::export]
            impl DeviceStatusSink {
                pub fn send(&self, status: DeviceStatus) {
                    if let Some(feed) = self.feed.upgrade() {
                        feed.publish(status);
                    }
                }
            }

            /// Implemented by the host.
            #[::uniffi::export(with_foreign)]
            pub trait DeviceStatusProvider: Send + Sync {
                fn info(&self) -> DeviceInfo;
                /// Read fresh, including storage and memory.
                fn status(&self) -> DeviceStatus;
                /// Send the status to `sink` now and whenever battery, power
                /// or thermal state changes, until `stop`.
                fn start(&self, sink: Arc<DeviceStatusSink>);
                fn stop(&self);
            }

            #[::uniffi::export]
            pub fn install_native_device_status(provider: Arc<dyn DeviceStatusProvider>) {
                DeviceStatusService::new(provider).install();
            }

            /// Device status for Rust code. Cheap to clone.
            #[derive(Clone)]
            pub struct DeviceStatusService {
                provider: Arc<dyn DeviceStatusProvider>,
                feed: Arc<$crate::Feed<DeviceStatus>>,
            }

            $crate::__native_slot!(DeviceStatusService, "device status");

            impl DeviceStatusService {
                pub fn new(provider: Arc<dyn DeviceStatusProvider>) -> Self {
                    Self {
                        provider,
                        feed: $crate::Feed::new(),
                    }
                }

                pub fn info(&self) -> DeviceInfo {
                    self.provider.info()
                }

                /// Fresh from the host. Reading storage takes a file-system
                /// call, so poll it no more than about once a second.
                pub fn status(&self) -> DeviceStatus {
                    self.provider.status()
                }

                /// Calls `listener` with the status now and on every battery,
                /// power or thermal change, until the watch is dropped.
                pub fn watch(
                    &self,
                    listener: impl Fn(&DeviceStatus) + Send + Sync + 'static,
                ) -> $crate::Watch {
                    let provider = self.provider.clone();
                    let stopper = self.provider.clone();
                    let sink = Arc::new(DeviceStatusSink {
                        feed: Arc::downgrade(&self.feed),
                    });
                    self.feed
                        .watch(
                            listener,
                            move || {
                                provider.start(sink);
                                Ok::<(), ::std::convert::Infallible>(())
                            },
                            move || stopper.stop(),
                        )
                        .unwrap_or_else(|never| match never {})
                }

                pub fn for_tests() -> (Self, Arc<testing::FakeDevice>) {
                    let fake = Arc::new(testing::FakeDevice::default());
                    (Self::new(fake.clone()), fake)
                }
            }

            pub mod testing {
                use super::*;
                use ::std::sync::Mutex;

                /// A device whose status the test sets.
                pub struct FakeDevice {
                    pub info: Mutex<DeviceInfo>,
                    status: Mutex<DeviceStatus>,
                    sink: Mutex<Option<Arc<DeviceStatusSink>>>,
                }

                impl Default for FakeDevice {
                    fn default() -> Self {
                        Self {
                            info: Mutex::new(DeviceInfo {
                                model_identifier: "iPhone16,2".into(),
                                model: "iPhone".into(),
                                system_name: "iOS".into(),
                                system_version: "26.0".into(),
                                device_name: "iPhone".into(),
                                processor_count: 6,
                                active_processor_count: 6,
                                physical_memory_bytes: 8 << 30,
                                screen_width_points: 430.0,
                                screen_height_points: 932.0,
                                screen_scale: 3.0,
                                is_simulator: false,
                                is_ios_app_on_mac: false,
                            }),
                            status: Mutex::new(DeviceStatus {
                                battery_level: Some(0.8),
                                battery_state: BatteryState::Unplugged,
                                low_power_mode: false,
                                thermal_state: ThermalState::Nominal,
                                storage_total_bytes: Some(256 << 30),
                                storage_available_bytes: Some(100 << 30),
                                memory_available_bytes: Some(3 << 30),
                                memory_footprint_bytes: Some(50 << 20),
                                uptime_seconds: 3600.0,
                                screen_brightness: Some(0.5),
                                observed_at_ms: 0,
                            }),
                            sink: Mutex::new(None),
                        }
                    }
                }

                impl FakeDevice {
                    /// Changes the status and tells the listeners.
                    pub fn set(&self, status: DeviceStatus) {
                        *self.status.lock().unwrap() = status.clone();
                        let sink = self.sink.lock().unwrap().clone();
                        if let Some(sink) = sink {
                            sink.send(status);
                        }
                    }

                    pub fn is_started(&self) -> bool {
                        self.sink.lock().unwrap().is_some()
                    }
                }

                impl DeviceStatusProvider for FakeDevice {
                    fn info(&self) -> DeviceInfo {
                        self.info.lock().unwrap().clone()
                    }
                    fn status(&self) -> DeviceStatus {
                        self.status.lock().unwrap().clone()
                    }
                    fn start(&self, sink: Arc<DeviceStatusSink>) {
                        sink.send(self.status());
                        *self.sink.lock().unwrap() = Some(sink);
                    }
                    fn stop(&self) {
                        *self.sink.lock().unwrap() = None;
                    }
                }
            }
        }
    };
}

#[doc(hidden)]
#[macro_export]
macro_rules! __native_motion {
    () => {
        /// Accelerometer, gyroscope, magnetometer, fused device motion and
        /// the barometer. iOS: Core Motion (`CMMotionManager`,
        /// `CMAltimeter`). The barometer needs the motion permission
        /// (`[ios.permissions] motion` in forge.toml); the rest need none.
        pub mod motion {
            use ::std::collections::HashMap;
            use ::std::sync::{Arc, Weak};

            use super::NativeServiceError;

            #[derive(::uniffi::Enum, Clone, Copy, Debug, PartialEq, Eq, Hash)]
            pub enum MotionSensor {
                /// Raw acceleration including gravity, in g.
                Accelerometer,
                /// Raw rotation rate, in radians per second.
                Gyroscope,
                /// Raw magnetic field, in microtesla (includes the device's
                /// own bias).
                Magnetometer,
                /// The sensors fused: attitude, gravity, user acceleration,
                /// unbiased rotation, calibrated magnetic field and heading.
                DeviceMotion,
                /// Barometric pressure and the change in altitude since
                /// the updates started.
                Altimeter,
            }

            impl MotionSensor {
                pub const ALL: [MotionSensor; 5] = [
                    MotionSensor::Accelerometer,
                    MotionSensor::Gyroscope,
                    MotionSensor::Magnetometer,
                    MotionSensor::DeviceMotion,
                    MotionSensor::Altimeter,
                ];
            }

            /// Which sensors this device has.
            #[derive(::uniffi::Record, Clone, Copy, Debug, PartialEq, Eq)]
            pub struct MotionAvailability {
                pub accelerometer: bool,
                pub gyroscope: bool,
                pub magnetometer: bool,
                pub device_motion: bool,
                pub altimeter: bool,
            }

            impl MotionAvailability {
                pub fn has(&self, sensor: MotionSensor) -> bool {
                    match sensor {
                        MotionSensor::Accelerometer => self.accelerometer,
                        MotionSensor::Gyroscope => self.gyroscope,
                        MotionSensor::Magnetometer => self.magnetometer,
                        MotionSensor::DeviceMotion => self.device_motion,
                        MotionSensor::Altimeter => self.altimeter,
                    }
                }
            }

            #[derive(::uniffi::Record, Clone, Copy, Debug, PartialEq)]
            pub struct Vector3 {
                pub x: f64,
                pub y: f64,
                pub z: f64,
            }

            impl Vector3 {
                pub fn magnitude(&self) -> f64 {
                    (self.x * self.x + self.y * self.y + self.z * self.z).sqrt()
                }
            }

            /// The device's orientation, in radians.
            #[derive(::uniffi::Record, Clone, Copy, Debug, PartialEq)]
            pub struct Attitude {
                /// Rotation about the long axis (tilting left and right).
                pub roll: f64,
                /// Rotation about the short axis (tilting towards you).
                pub pitch: f64,
                /// Rotation about the vertical axis.
                pub yaw: f64,
            }

            #[derive(::uniffi::Enum, Clone, Copy, Debug, PartialEq, Eq, Hash)]
            pub enum MagneticAccuracy {
                Uncalibrated,
                Low,
                Medium,
                High,
            }

            #[derive(::uniffi::Record, Clone, Copy, Debug, PartialEq)]
            pub struct DeviceMotionSample {
                pub attitude: Attitude,
                /// Gravity in the device's frame, in g.
                pub gravity: Vector3,
                /// Acceleration the user gives the device, gravity removed, in g.
                pub user_acceleration: Vector3,
                /// Rotation rate with the gyroscope's bias removed, rad/s.
                pub rotation_rate: Vector3,
                /// Calibrated magnetic field, microtesla; `None` without a
                /// magnetometer.
                pub magnetic_field: Option<Vector3>,
                pub magnetic_accuracy: MagneticAccuracy,
                /// Degrees from magnetic north (0 to 360), when the
                /// magnetometer is calibrated enough.
                pub heading_degrees: Option<f64>,
            }

            /// One update. `timestamp_s` is seconds since the device
            /// started, on the sensors' own clock: use differences.
            #[derive(::uniffi::Enum, Clone, Debug, PartialEq)]
            pub enum MotionReading {
                Acceleration {
                    value: Vector3,
                    timestamp_s: f64,
                },
                RotationRate {
                    value: Vector3,
                    timestamp_s: f64,
                },
                MagneticField {
                    value: Vector3,
                    timestamp_s: f64,
                },
                DeviceMotion {
                    sample: DeviceMotionSample,
                    timestamp_s: f64,
                },
                Altitude {
                    /// Metres up (or down) since the updates started.
                    relative_altitude_m: f64,
                    pressure_kpa: f64,
                    timestamp_s: f64,
                },
                /// The sensor stopped with an error (for example the motion
                /// permission was refused). No more updates follow.
                Failed {
                    sensor: MotionSensor,
                    reason: String,
                },
            }

            impl MotionReading {
                pub fn sensor(&self) -> MotionSensor {
                    match self {
                        Self::Acceleration { .. } => MotionSensor::Accelerometer,
                        Self::RotationRate { .. } => MotionSensor::Gyroscope,
                        Self::MagneticField { .. } => MotionSensor::Magnetometer,
                        Self::DeviceMotion { .. } => MotionSensor::DeviceMotion,
                        Self::Altitude { .. } => MotionSensor::Altimeter,
                        Self::Failed { sensor, .. } => *sensor,
                    }
                }

                pub fn timestamp_s(&self) -> Option<f64> {
                    match self {
                        Self::Acceleration { timestamp_s, .. }
                        | Self::RotationRate { timestamp_s, .. }
                        | Self::MagneticField { timestamp_s, .. }
                        | Self::DeviceMotion { timestamp_s, .. }
                        | Self::Altitude { timestamp_s, .. } => Some(*timestamp_s),
                        Self::Failed { .. } => None,
                    }
                }
            }

            /// Where the host sends one sensor's updates.
            #[derive(::uniffi::Object)]
            pub struct MotionSink {
                feed: Weak<$crate::Feed<MotionReading>>,
            }

            #[::uniffi::export]
            impl MotionSink {
                pub fn send(&self, reading: MotionReading) {
                    if let Some(feed) = self.feed.upgrade() {
                        feed.publish(reading);
                    }
                }
            }

            /// Implemented by the host (iOS: Core Motion).
            #[::uniffi::export(with_foreign)]
            pub trait MotionSensors: Send + Sync {
                fn availability(&self) -> MotionAvailability;
                /// Send `sensor`'s updates to `sink` every `interval_ms`
                /// until `stop`. Fails with `Unavailable` when the device
                /// has no such sensor.
                fn start(
                    &self,
                    sensor: MotionSensor,
                    interval_ms: u32,
                    sink: Arc<MotionSink>,
                ) -> Result<(), NativeServiceError>;
                fn stop(&self, sensor: MotionSensor);
            }

            #[::uniffi::export]
            pub fn install_native_motion(sensors: Arc<dyn MotionSensors>) {
                MotionService::new(sensors).install();
            }

            /// Motion sensors for Rust code. Cheap to clone.
            #[derive(Clone)]
            pub struct MotionService {
                sensors: Arc<dyn MotionSensors>,
                feeds: Arc<HashMap<MotionSensor, Arc<$crate::Feed<MotionReading>>>>,
            }

            $crate::__native_slot!(MotionService, "motion");

            impl MotionService {
                pub fn new(sensors: Arc<dyn MotionSensors>) -> Self {
                    let feeds = MotionSensor::ALL
                        .iter()
                        .map(|sensor| (*sensor, $crate::Feed::new()))
                        .collect();
                    Self {
                        sensors,
                        feeds: Arc::new(feeds),
                    }
                }

                pub fn availability(&self) -> MotionAvailability {
                    self.sensors.availability()
                }

                /// The last update from `sensor`, if it has run.
                pub fn latest(&self, sensor: MotionSensor) -> Option<MotionReading> {
                    self.feeds[&sensor].latest()
                }

                /// Calls `listener` with `sensor`'s updates, on the host's
                /// thread, until the watch is dropped. The first watcher of
                /// a sensor sets its rate (`interval_ms`, 10 to 10,000).
                pub fn watch(
                    &self,
                    sensor: MotionSensor,
                    interval_ms: u32,
                    listener: impl Fn(&MotionReading) + Send + Sync + 'static,
                ) -> Result<$crate::Watch, NativeServiceError> {
                    $crate::sensors::validate_motion_interval(interval_ms)
                        .map_err(|reason| NativeServiceError::InvalidInput { reason })?;
                    let feed = &self.feeds[&sensor];
                    let sensors = self.sensors.clone();
                    let stopper = self.sensors.clone();
                    let sink = Arc::new(MotionSink {
                        feed: Arc::downgrade(feed),
                    });
                    feed.watch(
                        listener,
                        move || sensors.start(sensor, interval_ms, sink),
                        move || stopper.stop(sensor),
                    )
                }

                pub fn for_tests() -> (Self, Arc<testing::FakeMotion>) {
                    let fake = Arc::new(testing::FakeMotion::default());
                    (Self::new(fake.clone()), fake)
                }
            }

            pub mod testing {
                use super::*;
                use ::std::sync::Mutex;

                /// Sensors whose readings the test sends.
                pub struct FakeMotion {
                    pub availability: Mutex<MotionAvailability>,
                    running: Mutex<HashMap<MotionSensor, (u32, Arc<MotionSink>)>>,
                }

                impl Default for FakeMotion {
                    fn default() -> Self {
                        Self {
                            availability: Mutex::new(MotionAvailability {
                                accelerometer: true,
                                gyroscope: true,
                                magnetometer: true,
                                device_motion: true,
                                altimeter: true,
                            }),
                            running: Mutex::new(HashMap::new()),
                        }
                    }
                }

                impl FakeMotion {
                    /// Sends `reading` to its sensor's listeners, if it runs.
                    pub fn send(&self, reading: MotionReading) {
                        let sink = self
                            .running
                            .lock()
                            .unwrap()
                            .get(&reading.sensor())
                            .map(|(_, sink)| sink.clone());
                        if let Some(sink) = sink {
                            sink.send(reading);
                        }
                    }

                    /// The interval `sensor` runs at, if it runs.
                    pub fn interval_ms(&self, sensor: MotionSensor) -> Option<u32> {
                        self.running.lock().unwrap().get(&sensor).map(|(i, _)| *i)
                    }
                }

                impl MotionSensors for FakeMotion {
                    fn availability(&self) -> MotionAvailability {
                        *self.availability.lock().unwrap()
                    }
                    fn start(
                        &self,
                        sensor: MotionSensor,
                        interval_ms: u32,
                        sink: Arc<MotionSink>,
                    ) -> Result<(), NativeServiceError> {
                        if !self.availability().has(sensor) {
                            return Err(NativeServiceError::Unavailable {
                                reason: format!("no {sensor:?} on this device"),
                            });
                        }
                        self.running
                            .lock()
                            .unwrap()
                            .insert(sensor, (interval_ms, sink));
                        Ok(())
                    }
                    fn stop(&self, sensor: MotionSensor) {
                        self.running.lock().unwrap().remove(&sensor);
                    }
                }
            }
        }
    };
}

#[doc(hidden)]
#[macro_export]
macro_rules! __native_location {
    () => {
        /// Location and compass heading. iOS: Core Location, "when in use"
        /// only. Needs `[ios.permissions] location_when_in_use` in
        /// forge.toml and `NativePermission::Location` granted.
        pub mod location {
            use ::std::sync::{Arc, Weak};

            use super::NativeServiceError;

            /// How precise fixes should be. Finer costs more battery.
            #[derive(::uniffi::Enum, Clone, Copy, Debug, PartialEq, Eq, Hash)]
            pub enum LocationAccuracy {
                /// The best the device can do (GPS when outdoors).
                Best,
                NearestTenMeters,
                HundredMeters,
                Kilometer,
                /// City-level: what the user gets when they turned off
                /// Precise Location.
                Reduced,
            }

            #[derive(::uniffi::Record, Clone, Copy, Debug, PartialEq)]
            pub struct LocationOptions {
                pub accuracy: LocationAccuracy,
                /// Only report after moving this many metres; `None` for
                /// every update.
                pub distance_filter_m: Option<f64>,
                /// Also report compass headings (where the top of the phone
                /// points).
                pub include_heading: bool,
            }

            /// One position fix. Optional fields are `None` when the device
            /// could not measure them.
            #[derive(::uniffi::Record, Clone, Debug, PartialEq)]
            pub struct LocationFix {
                pub latitude: f64,
                pub longitude: f64,
                /// Radius of uncertainty, metres.
                pub horizontal_accuracy_m: f64,
                /// Metres above sea level.
                pub altitude_m: Option<f64>,
                pub vertical_accuracy_m: Option<f64>,
                /// Metres per second.
                pub speed_mps: Option<f64>,
                pub speed_accuracy_mps: Option<f64>,
                /// Direction of travel, degrees from true north.
                pub course_degrees: Option<f64>,
                pub course_accuracy_degrees: Option<f64>,
                /// Building floor, where the venue provides it.
                pub floor: Option<i32>,
                /// When the fix was taken (Unix milliseconds).
                pub timestamp_ms: i64,
            }

            impl LocationFix {
                /// Great-circle distance to `other`, metres.
                pub fn distance_to(&self, other: &LocationFix) -> f64 {
                    $crate::sensors::distance_m(
                        self.latitude,
                        self.longitude,
                        other.latitude,
                        other.longitude,
                    )
                }
            }

            /// Where the top of the device points.
            #[derive(::uniffi::Record, Clone, Copy, Debug, PartialEq)]
            pub struct Heading {
                /// Degrees from magnetic north, 0 to 360.
                pub magnetic_degrees: f64,
                /// Degrees from true north; `None` until a location is known.
                pub true_degrees: Option<f64>,
                /// Maximum error, degrees; `None` when the heading is
                /// unreliable (calibrate by moving the phone in a figure 8).
                pub accuracy_degrees: Option<f64>,
                pub timestamp_ms: i64,
            }

            #[derive(::uniffi::Enum, Clone, Debug, PartialEq)]
            pub enum LocationEvent {
                Fix {
                    fix: LocationFix,
                },
                Heading {
                    heading: Heading,
                },
                /// Updates failed. With `permission_denied`, no more follow
                /// until the user allows location again.
                Failed {
                    reason: String,
                    permission_denied: bool,
                },
            }

            /// Where the host sends fixes and headings.
            #[derive(::uniffi::Object)]
            pub struct LocationSink {
                feed: Weak<$crate::Feed<LocationEvent>>,
            }

            #[::uniffi::export]
            impl LocationSink {
                pub fn send(&self, event: LocationEvent) {
                    if let Some(feed) = self.feed.upgrade() {
                        feed.publish(event);
                    }
                }
            }

            /// Implemented by the host (iOS: `CLLocationManager`).
            #[::uniffi::export(with_foreign)]
            pub trait LocationSource: Send + Sync {
                /// The last fix the system has, possibly old; check its
                /// timestamp.
                fn last_known(&self) -> Option<LocationFix>;
                /// False when the user turned off Precise Location for the
                /// app: fixes are then a few kilometres off on purpose.
                fn is_precise(&self) -> bool;
                fn heading_available(&self) -> bool;
                /// Send fixes (and headings, if asked) to `sink` until
                /// `stop`. Fails with `PermissionDenied` when location is
                /// not allowed.
                fn start(
                    &self,
                    options: LocationOptions,
                    sink: Arc<LocationSink>,
                ) -> Result<(), NativeServiceError>;
                fn stop(&self);
            }

            #[::uniffi::export]
            pub fn install_native_location(source: Arc<dyn LocationSource>) {
                LocationService::new(source).install();
            }

            /// Location for Rust code. Cheap to clone.
            #[derive(Clone)]
            pub struct LocationService {
                source: Arc<dyn LocationSource>,
                feed: Arc<$crate::Feed<LocationEvent>>,
            }

            $crate::__native_slot!(LocationService, "location");

            impl LocationService {
                pub fn new(source: Arc<dyn LocationSource>) -> Self {
                    Self {
                        source,
                        feed: $crate::Feed::new(),
                    }
                }

                pub fn last_known(&self) -> Option<LocationFix> {
                    self.source.last_known()
                }

                pub fn is_precise(&self) -> bool {
                    self.source.is_precise()
                }

                pub fn heading_available(&self) -> bool {
                    self.source.heading_available()
                }

                /// Calls `listener` with each fix and heading, on the host's
                /// thread, until the watch is dropped. The first watcher's
                /// options apply. Request `NativePermission::Location` first.
                pub fn watch(
                    &self,
                    options: LocationOptions,
                    listener: impl Fn(&LocationEvent) + Send + Sync + 'static,
                ) -> Result<$crate::Watch, NativeServiceError> {
                    $crate::sensors::validate_distance_filter(options.distance_filter_m)
                        .map_err(|reason| NativeServiceError::InvalidInput { reason })?;
                    let source = self.source.clone();
                    let stopper = self.source.clone();
                    let sink = Arc::new(LocationSink {
                        feed: Arc::downgrade(&self.feed),
                    });
                    self.feed.watch(
                        listener,
                        move || source.start(options, sink),
                        move || stopper.stop(),
                    )
                }

                pub fn for_tests() -> (Self, Arc<testing::FakeLocation>) {
                    let fake = Arc::new(testing::FakeLocation::default());
                    (Self::new(fake.clone()), fake)
                }
            }

            pub mod testing {
                use super::*;
                use ::std::sync::Mutex;

                /// A location source driven by the test.
                #[derive(Default)]
                pub struct FakeLocation {
                    pub denied: ::std::sync::atomic::AtomicBool,
                    running: Mutex<Option<(LocationOptions, Arc<LocationSink>)>>,
                    last: Mutex<Option<LocationFix>>,
                }

                impl FakeLocation {
                    pub fn send(&self, event: LocationEvent) {
                        if let LocationEvent::Fix { fix } = &event {
                            *self.last.lock().unwrap() = Some(fix.clone());
                        }
                        let sink = self
                            .running
                            .lock()
                            .unwrap()
                            .as_ref()
                            .map(|(_, s)| s.clone());
                        if let Some(sink) = sink {
                            sink.send(event);
                        }
                    }

                    pub fn options(&self) -> Option<LocationOptions> {
                        self.running.lock().unwrap().as_ref().map(|(o, _)| *o)
                    }
                }

                impl LocationSource for FakeLocation {
                    fn last_known(&self) -> Option<LocationFix> {
                        self.last.lock().unwrap().clone()
                    }
                    fn is_precise(&self) -> bool {
                        true
                    }
                    fn heading_available(&self) -> bool {
                        true
                    }
                    fn start(
                        &self,
                        options: LocationOptions,
                        sink: Arc<LocationSink>,
                    ) -> Result<(), NativeServiceError> {
                        if self.denied.load(::std::sync::atomic::Ordering::SeqCst) {
                            return Err(NativeServiceError::PermissionDenied {
                                reason: "location is not allowed".into(),
                            });
                        }
                        *self.running.lock().unwrap() = Some((options, sink));
                        Ok(())
                    }
                    fn stop(&self) {
                        *self.running.lock().unwrap() = None;
                    }
                }
            }
        }
    };
}

#[doc(hidden)]
#[macro_export]
macro_rules! __native_bluetooth {
    () => {
        /// Whether Bluetooth is on, and whether the app may use it. iOS:
        /// Core Bluetooth. Needs `[ios.permissions] bluetooth` in
        /// forge.toml. Watching (or requesting `NativePermission::Bluetooth`)
        /// shows the system prompt the first time.
        pub mod bluetooth {
            use ::std::sync::{Arc, Weak};

            #[derive(::uniffi::Enum, Clone, Copy, Debug, PartialEq, Eq, Hash)]
            pub enum BluetoothState {
                /// Not known yet (nothing has asked the system).
                Unknown,
                /// The connection to the system service was lost; an update
                /// follows.
                Resetting,
                /// This device has no Bluetooth Low Energy.
                Unsupported,
                /// The user has not allowed Bluetooth for this app.
                Unauthorized,
                PoweredOff,
                PoweredOn,
            }

            /// Where the host sends state changes.
            #[derive(::uniffi::Object)]
            pub struct BluetoothSink {
                feed: Weak<$crate::Feed<BluetoothState>>,
            }

            #[::uniffi::export]
            impl BluetoothSink {
                pub fn send(&self, state: BluetoothState) {
                    if let Some(feed) = self.feed.upgrade() {
                        feed.publish(state);
                    }
                }
            }

            /// Implemented by the host (iOS: `CBCentralManager`).
            #[::uniffi::export(with_foreign)]
            pub trait BluetoothMonitor: Send + Sync {
                /// The last state the system reported, `Unknown` before
                /// anything started the monitor.
                fn state(&self) -> BluetoothState;
                fn start(&self, sink: Arc<BluetoothSink>);
                fn stop(&self);
            }

            #[::uniffi::export]
            pub fn install_native_bluetooth(monitor: Arc<dyn BluetoothMonitor>) {
                BluetoothService::new(monitor).install();
            }

            /// Bluetooth state for Rust code. Cheap to clone.
            #[derive(Clone)]
            pub struct BluetoothService {
                monitor: Arc<dyn BluetoothMonitor>,
                feed: Arc<$crate::Feed<BluetoothState>>,
            }

            $crate::__native_slot!(BluetoothService, "bluetooth");

            impl BluetoothService {
                pub fn new(monitor: Arc<dyn BluetoothMonitor>) -> Self {
                    Self {
                        monitor,
                        feed: $crate::Feed::new(),
                    }
                }

                pub fn state(&self) -> BluetoothState {
                    self.monitor.state()
                }

                /// Calls `listener` with the state now and on every change.
                pub fn watch(
                    &self,
                    listener: impl Fn(&BluetoothState) + Send + Sync + 'static,
                ) -> $crate::Watch {
                    let monitor = self.monitor.clone();
                    let stopper = self.monitor.clone();
                    let sink = Arc::new(BluetoothSink {
                        feed: Arc::downgrade(&self.feed),
                    });
                    self.feed
                        .watch(
                            listener,
                            move || {
                                monitor.start(sink);
                                Ok::<(), ::std::convert::Infallible>(())
                            },
                            move || stopper.stop(),
                        )
                        .unwrap_or_else(|never| match never {})
                }

                pub fn for_tests() -> (Self, Arc<testing::FakeBluetooth>) {
                    let fake = Arc::new(testing::FakeBluetooth::default());
                    (Self::new(fake.clone()), fake)
                }
            }

            pub mod testing {
                use super::*;
                use ::std::sync::Mutex;

                pub struct FakeBluetooth {
                    state: Mutex<BluetoothState>,
                    sink: Mutex<Option<Arc<BluetoothSink>>>,
                }

                impl Default for FakeBluetooth {
                    fn default() -> Self {
                        Self {
                            state: Mutex::new(BluetoothState::Unknown),
                            sink: Mutex::new(None),
                        }
                    }
                }

                impl FakeBluetooth {
                    pub fn set(&self, state: BluetoothState) {
                        *self.state.lock().unwrap() = state;
                        let sink = self.sink.lock().unwrap().clone();
                        if let Some(sink) = sink {
                            sink.send(state);
                        }
                    }
                }

                impl BluetoothMonitor for FakeBluetooth {
                    fn state(&self) -> BluetoothState {
                        *self.state.lock().unwrap()
                    }
                    fn start(&self, sink: Arc<BluetoothSink>) {
                        sink.send(self.state());
                        *self.sink.lock().unwrap() = Some(sink);
                    }
                    fn stop(&self) {
                        *self.sink.lock().unwrap() = None;
                    }
                }
            }
        }
    };
}

#[doc(hidden)]
#[macro_export]
macro_rules! __native_audio {
    () => {
        /// Audio files. Recording and playing are driven by the UI, so they
        /// are SwiftUI-side classes in the iOS adapter (`ForgeAudioRecorder`,
        /// `ForgeAudioPlayer`, `ForgeAudioSession`, `ForgeAudio.waveform`);
        /// Rust asks for the microphone (`NativePermission::Microphone`) and
        /// measures files. Needs `[ios.permissions] microphone` in forge.toml.
        pub mod audio {
            use ::std::sync::Arc;

            /// Implemented by the host (iOS: AVFoundation).
            #[::uniffi::export(with_foreign)]
            pub trait AudioFiles: Send + Sync {
                /// The length in seconds of the audio file at `path`.
                fn duration(&self, path: String) -> Result<f64, super::NativeServiceError>;
            }

            #[::uniffi::export]
            pub fn install_native_audio(files: Arc<dyn AudioFiles>) {
                AudioService::new(files).install();
            }

            /// Audio files for Rust code. Cheap to clone.
            #[derive(Clone)]
            pub struct AudioService {
                files: Arc<dyn AudioFiles>,
            }

            $crate::__native_slot!(AudioService, "audio");

            impl AudioService {
                pub fn new(files: Arc<dyn AudioFiles>) -> Self {
                    Self { files }
                }

                /// The length of the audio file at `path` (absolute).
                pub fn duration(
                    &self,
                    path: &::std::path::Path,
                ) -> Result<::std::time::Duration, super::NativeServiceError> {
                    if !path.is_absolute() {
                        return Err(super::NativeServiceError::InvalidInput {
                            reason: format!("an audio path must be absolute, got {path:?}"),
                        });
                    }
                    let seconds = self.files.duration(path.to_string_lossy().into_owned())?;
                    ::std::time::Duration::try_from_secs_f64(seconds).map_err(|_| {
                        super::NativeServiceError::Platform {
                            code: -1,
                            reason: format!("the host reported a length of {seconds} s"),
                        }
                    })
                }

                pub fn for_tests() -> (Self, Arc<testing::FakeAudioFiles>) {
                    let fake = Arc::new(testing::FakeAudioFiles::default());
                    (Self::new(fake.clone()), fake)
                }
            }

            pub mod testing {
                use super::*;
                use ::std::collections::HashMap;
                use ::std::sync::Mutex;

                /// Lengths set by the test; other paths are `Platform` errors.
                #[derive(Default)]
                pub struct FakeAudioFiles {
                    lengths: Mutex<HashMap<String, f64>>,
                }

                impl FakeAudioFiles {
                    pub fn set(&self, path: impl Into<String>, seconds: f64) {
                        self.lengths
                            .lock()
                            .unwrap_or_else(|p| p.into_inner())
                            .insert(path.into(), seconds);
                    }
                }

                impl AudioFiles for FakeAudioFiles {
                    fn duration(
                        &self,
                        path: String,
                    ) -> Result<f64, super::super::NativeServiceError> {
                        self.lengths
                            .lock()
                            .unwrap_or_else(|p| p.into_inner())
                            .get(&path)
                            .copied()
                            .ok_or(super::super::NativeServiceError::Platform {
                                code: 2,
                                reason: format!("no audio file at {path}"),
                            })
                    }
                }
            }
        }
    };
}
