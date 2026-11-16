//! "My Location": where the device is, through the `location` service
//! (Core Location on iOS). Swift no longer drives `CLLocationManager`; it
//! only names the place it gets back (the reverse geocoder).

use std::sync::mpsc;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use super::error::WeatherError;
use super::types::Coordinate;
use super::ForgeWeatherApp;
use crate::native::location::{LocationAccuracy, LocationEvent, LocationOptions, LocationService};
use crate::native::{NativePermission, NativeServices, PermissionStatus};

/// A last-known fix younger than this is good enough for a forecast.
const RECENT_ENOUGH: Duration = Duration::from_secs(10 * 60);
/// How long to wait for a fix before saying location isn't available.
const FIX_TIMEOUT: Duration = Duration::from_secs(20);

/// One position for "My Location".
#[derive(uniffi::Record, Clone, Copy, Debug, PartialEq)]
pub struct DeviceFix {
    pub coordinate: Coordinate,
    /// Radius of uncertainty, metres.
    pub horizontal_accuracy_m: f64,
}

#[uniffi::export]
impl ForgeWeatherApp {
    /// Where the device is. Asks for "when in use" permission the first
    /// time; `LocationDenied` when the person said no, `LocationUnavailable`
    /// when no fix arrives. City-level accuracy: fast and cheap on battery.
    /// On-screen work: leaving the screen cancels it.
    pub async fn current_location(&self) -> Result<DeviceFix, WeatherError> {
        let unavailable = |reason: &str| WeatherError::LocationUnavailable {
            reason: reason.into(),
        };
        let native =
            NativeServices::installed().map_err(|_| unavailable("no native services installed"))?;
        let service =
            LocationService::installed().map_err(|_| unavailable("location isn't installed"))?;
        let mut status = native.permission_status(NativePermission::Location).await;
        if status == PermissionStatus::NotDetermined {
            status = native.request_permission(NativePermission::Location).await;
        }
        match status {
            s if s.allows_use() => {}
            PermissionStatus::Denied | PermissionStatus::Restricted => {
                return Err(WeatherError::LocationDenied)
            }
            _ => return Err(unavailable("location is not supported here")),
        }
        if let Some(fix) = service.last_known() {
            let age_ms = forge_runtime::now_ms().saturating_sub(fix.timestamp_ms);
            if u128::try_from(age_ms).unwrap_or(u128::MAX) <= RECENT_ENOUGH.as_millis() {
                return Ok(device_fix(
                    fix.latitude,
                    fix.longitude,
                    fix.horizontal_accuracy_m,
                ));
            }
        }
        self.rt
            .run("weather-locate", self.rt.foreground(None), move |cancel| {
                first_fix(&service, cancel)
            })
            .await
    }
}

fn device_fix(latitude: f64, longitude: f64, accuracy: f64) -> DeviceFix {
    DeviceFix {
        coordinate: Coordinate {
            latitude,
            longitude,
        },
        horizontal_accuracy_m: accuracy,
    }
}

/// Watches until the first fix, a failure, cancellation or the timeout.
fn first_fix(
    service: &LocationService,
    cancel: &forge_net::CancelHandle,
) -> Result<DeviceFix, WeatherError> {
    let (tx, rx) = mpsc::channel();
    let tx = Mutex::new(tx);
    let options = LocationOptions {
        accuracy: LocationAccuracy::Kilometer,
        distance_filter_m: None,
        include_heading: false,
    };
    // Dropping the watch stops Core Location.
    let _watch = service
        .watch(options, move |event| {
            if let Ok(tx) = tx.lock() {
                let _ = tx.send(event.clone());
            }
        })
        .map_err(|err| match err {
            crate::native::NativeServiceError::PermissionDenied { .. } => {
                WeatherError::LocationDenied
            }
            other => WeatherError::LocationUnavailable {
                reason: other.to_string(),
            },
        })?;
    let started = Instant::now();
    loop {
        if cancel.is_cancelled() {
            return Err(WeatherError::Cancelled);
        }
        match rx.recv_timeout(Duration::from_millis(200)) {
            Ok(LocationEvent::Fix { fix }) => {
                return Ok(device_fix(
                    fix.latitude,
                    fix.longitude,
                    fix.horizontal_accuracy_m,
                ))
            }
            Ok(LocationEvent::Failed {
                permission_denied: true,
                ..
            }) => return Err(WeatherError::LocationDenied),
            Ok(LocationEvent::Failed { reason, .. }) => {
                return Err(WeatherError::LocationUnavailable { reason })
            }
            Ok(LocationEvent::Heading { .. }) | Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                return Err(WeatherError::LocationUnavailable {
                    reason: "location updates stopped".into(),
                })
            }
        }
        if started.elapsed() >= FIX_TIMEOUT {
            return Err(WeatherError::LocationUnavailable {
                reason: "no fix in time".into(),
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_first_fix_is_the_answer_and_a_denial_is_typed() {
        let (service, fake) = LocationService::for_tests();
        let cancel = forge_net::CancelHandle::new();
        let sender = std::thread::spawn({
            let fake = fake.clone();
            move || {
                std::thread::sleep(Duration::from_millis(50));
                fake.send(LocationEvent::Fix {
                    fix: crate::native::location::LocationFix {
                        latitude: 55.9,
                        longitude: -3.6,
                        horizontal_accuracy_m: 900.0,
                        altitude_m: None,
                        vertical_accuracy_m: None,
                        speed_mps: None,
                        speed_accuracy_mps: None,
                        course_degrees: None,
                        course_accuracy_degrees: None,
                        floor: None,
                        timestamp_ms: 0,
                    },
                });
            }
        });
        let fix = first_fix(&service, &cancel).unwrap();
        sender.join().unwrap();
        assert_eq!(fix.coordinate.latitude, 55.9);

        let sender = std::thread::spawn({
            let fake = fake.clone();
            move || {
                std::thread::sleep(Duration::from_millis(50));
                fake.send(LocationEvent::Failed {
                    reason: "denied".into(),
                    permission_denied: true,
                });
            }
        });
        assert_eq!(
            first_fix(&service, &cancel),
            Err(WeatherError::LocationDenied)
        );
        sender.join().unwrap();
    }

    #[test]
    fn cancelling_stops_the_wait() {
        let (service, _fake) = LocationService::for_tests();
        let cancel = forge_net::CancelHandle::new();
        cancel.cancel();
        assert_eq!(first_fix(&service, &cancel), Err(WeatherError::Cancelled));
    }
}
