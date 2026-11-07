//! The UniFFI shell every app crate repeats: the lifecycle phase enum, a
//! per-call cancellation object and the event stream object. The logic lives
//! in plain Rust ([`Lifecycle`](crate::Lifecycle), [`CancelHandle`](crate::CancelHandle),
//! [`Subscription`](crate::Subscription)); [`app_shell!`](crate::app_shell)
//! only emits the thin `#[uniffi::export]` wrappers, in the app's own crate,
//! with the names the app chooses.

use std::sync::{Mutex, MutexGuard};
use std::time::{SystemTime, UNIX_EPOCH};

/// Locks `mutex`, recovering the value if a panic poisoned it. Sound for app
/// state whose critical sections leave it consistent at every step (each
/// change is built aside and assigned at the end), which is the pattern the
/// app crates follow.
pub fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Unix time in milliseconds; 0 if the clock is before 1970.
pub fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// Emits the UniFFI types an app's API shares with every other Forge app,
/// in the calling crate (which must depend on `uniffi`). Each part is
/// optional and named by the app, so its Swift and Kotlin API keeps its own
/// names:
///
/// ```ignore
/// forge_runtime::app_shell! {
///     // `enum AppPhase` (Launching, Active, Inactive, Background,
///     // Terminated), convertible to and from `forge_runtime::Phase`.
///     phase: AppPhase,
///     // `Cancellation`: `new()` and `cancel()` for the host; Rust reads its
///     // `handle` field (a `CancelHandle`).
///     cancellation: Cancellation,
///     // `AppEvents`: `next()` and `cancel()` for the host over a
///     // `Subscription<AppEvent>`; a lag becomes `AppEvent::EventsDropped { count }`.
///     // Rust creates one with `AppEvents::subscribe(&hub)`.
///     events: AppEvents(AppEvent::EventsDropped),
/// }
/// ```
#[macro_export]
macro_rules! app_shell {
    () => {};
    (phase: $phase:ident $(, $($rest:tt)*)?) => {
        $crate::__app_phase!($phase);
        $crate::app_shell!($($($rest)*)?);
    };
    (cancellation: $cancellation:ident $(, $($rest:tt)*)?) => {
        $crate::__cancellation!($cancellation);
        $crate::app_shell!($($($rest)*)?);
    };
    (events: $stream:ident($event:ident :: $dropped:ident) $(, $($rest:tt)*)?) => {
        $crate::__event_stream!($stream, $event, $dropped);
        $crate::app_shell!($($($rest)*)?);
    };
}

#[doc(hidden)]
#[macro_export]
macro_rules! __app_phase {
    ($phase:ident) => {
        /// Where the app is in its lifecycle. The host reports it with
        /// `set_phase`: iOS from SwiftUI's `scenePhase` (and
        /// `willTerminate`), Android from the activity's `onStart`,
        /// `onResume`, `onPause` and `onStop`.
        #[derive(::uniffi::Enum, Clone, Copy, Debug, PartialEq, Eq)]
        pub enum $phase {
            /// Created, not on screen yet. The phase before the first report.
            Launching,
            /// On screen and receiving input.
            Active,
            /// On screen without input: an alert, the app switcher, a dialog.
            Inactive,
            /// Not visible; the OS may suspend or kill the process from here.
            /// Work started on screen is stopped.
            Background,
            /// Going away: same as `shutdown()`. iOS does not always report it.
            Terminated,
        }

        impl ::std::convert::From<$phase> for $crate::Phase {
            fn from(phase: $phase) -> Self {
                match phase {
                    $phase::Launching => Self::Launching,
                    $phase::Active => Self::Active,
                    $phase::Inactive => Self::Inactive,
                    $phase::Background => Self::Background,
                    $phase::Terminated => Self::Terminated,
                }
            }
        }

        impl ::std::convert::From<$crate::Phase> for $phase {
            fn from(phase: $crate::Phase) -> Self {
                match phase {
                    $crate::Phase::Launching => Self::Launching,
                    $crate::Phase::Active => Self::Active,
                    $crate::Phase::Inactive => Self::Inactive,
                    $crate::Phase::Background => Self::Background,
                    $crate::Phase::Terminated => Self::Terminated,
                }
            }
        }
    };
}

#[doc(hidden)]
#[macro_export]
macro_rules! __cancellation {
    ($cancellation:ident) => {
        /// Lets the host stop one request part way. Swift: create one per
        /// call and cancel it from `withTaskCancellationHandler`, because
        /// cancelling a Swift `Task` does not reach Rust on its own. Kotlin:
        /// cancelling the coroutine is enough; pass `null`.
        #[derive(::uniffi::Object, Default)]
        pub struct $cancellation {
            handle: $crate::CancelHandle,
        }

        #[::uniffi::export]
        impl $cancellation {
            #[uniffi::constructor]
            pub fn new() -> ::std::sync::Arc<Self> {
                ::std::sync::Arc::new(Self::default())
            }

            /// Stops the request: it fails with `Cancelled` at once, even
            /// while waiting for a slow server. Idempotent.
            pub fn cancel(&self) {
                self.handle.cancel();
            }
        }
    };
}

#[doc(hidden)]
#[macro_export]
macro_rules! __event_stream {
    ($stream:ident, $event:ident, $dropped:ident) => {
        /// One consumer's events. Await `next()` in a loop; `None` means it ended.
        #[derive(::uniffi::Object)]
        pub struct $stream {
            subscription: $crate::Subscription<$event>,
        }

        impl $stream {
            /// A new stream from `hub`, or `None` once the hub is closed
            /// (the app shut down).
            #[allow(dead_code)]
            pub(crate) fn subscribe(
                hub: &$crate::EventHub<$event>,
            ) -> ::std::option::Option<::std::sync::Arc<Self>> {
                hub.subscribe()
                    .map(|subscription| ::std::sync::Arc::new(Self { subscription }))
            }
        }

        #[::uniffi::export]
        impl $stream {
            /// The next event, or `None` once this stream has ended.
            pub async fn next(&self) -> ::std::option::Option<$event> {
                match self.subscription.next().await? {
                    $crate::Delivery::Event(event) => Some(event),
                    $crate::Delivery::Dropped(count) => Some($event::$dropped { count }),
                }
            }

            /// Ends this stream; a pending `next()` returns `None`.
            pub fn cancel(&self) {
                self.subscription.cancel();
            }
        }
    };
}
