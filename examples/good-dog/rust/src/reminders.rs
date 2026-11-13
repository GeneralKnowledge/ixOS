//! Calm reminders (GD5): one optional daily nudge at the owner's time. It
//! is skipped on a day the dog has already practised, or the owner said
//! "Not today". Its buttons are Start Now (opens Today) and Not Today.
//!
//! iOS can't skip one day of a repeating notification, so Rust schedules
//! one notification per day for the next week, by local day number, and
//! schedules them again whenever something that decides them changes:
//! the setting, a recorded session, "Not today", or the app coming back.

use serde::{Deserialize, Serialize};

use crate::training::{local_day, Stored};
use crate::{native, LocalTime};

use good_dog_core::coaching::DAY_MS;

/// The notification category with the two buttons.
pub(crate) const CATEGORY: &str = "gooddog.reminder";
/// Every reminder's id starts with this, then the local day number.
const ID_PREFIX: &str = "gooddog.reminder.";
/// How many days ahead are scheduled. A week covers anyone who opens the
/// app now and then; each time it opens, the week moves on.
const DAYS_AHEAD: i64 = 7;
pub(crate) const START: &str = "start";
pub(crate) const NOT_TODAY: &str = "not-today";

/// Saved with the rest of the data.
#[derive(Serialize, Deserialize, Default, Clone, Debug, PartialEq)]
pub(crate) struct ReminderSettings {
    /// Minutes after local midnight. None: reminders are off.
    pub at_minute: Option<u32>,
    /// The local day the owner said "Not today".
    pub skipped_day: Option<i64>,
}

impl Stored {
    /// Whether the dog has practised on `day`, by the offset in `now`.
    fn practised_on(&self, day: i64, now: LocalTime) -> bool {
        self.sessions.iter().any(|s| {
            local_day(LocalTime {
                ms: s.completed_at_ms,
                utc_offset_minutes: now.utc_offset_minutes,
            }) == day
        })
    }

    /// The reminders that should be pending now: one per day for the next
    /// week, from today if it's still to come and today is still open.
    pub(crate) fn reminders(&self, now: LocalTime) -> Vec<native::LocalNotification> {
        let (Some(at_minute), Some(dog)) = (self.reminder.at_minute, &self.dog) else {
            return Vec::new();
        };
        let today = local_day(now);
        let offset_ms = i64::from(now.utc_offset_minutes) * 60_000;
        (today..today + DAYS_AHEAD)
            .filter(|&day| {
                day != today
                    || (self.reminder.skipped_day != Some(today) && !self.practised_on(today, now))
            })
            .map(|day| {
                (
                    day,
                    day * DAY_MS + i64::from(at_minute) * 60_000 - offset_ms,
                )
            })
            .filter(|&(_, at)| at > now.ms)
            .map(|(day, at)| {
                let mut n = native::LocalNotification::now(
                    format!("{ID_PREFIX}{day}"),
                    format!("Time to practise with {}?", dog.name),
                    "A few minutes is plenty. Today's plan is ready.",
                );
                n.trigger = native::NotificationTrigger::At { unix_ms: at };
                n.category = Some(CATEGORY.into());
                n.thread_id = Some(CATEGORY.into());
                n
            })
            .collect()
    }
}

/// The local day a reminder is for, from its id.
pub(crate) fn day_of(notification_id: &str) -> Option<i64> {
    notification_id.strip_prefix(ID_PREFIX)?.parse().ok()
}

/// Replaces every pending reminder with `wanted`.
pub(crate) async fn schedule(
    services: &native::NativeServices,
    wanted: Vec<native::LocalNotification>,
    now: LocalTime,
) {
    // A day either side covers any time zone change since the last time.
    let today = local_day(now);
    for day in today - 2..today + DAYS_AHEAD + 2 {
        let id = format!("{ID_PREFIX}{day}");
        if !wanted.iter().any(|n| n.id == id) {
            services.cancel_notification(&id);
        }
    }
    for notification in wanted {
        if let Err(e) = services.schedule_notification(notification).await {
            // Turned off in Settings since, most likely.
            forge_runtime::logging::warn!("reminder not scheduled: {e}");
            return;
        }
    }
}

/// The two buttons.
pub(crate) fn category() -> native::NotificationCategory {
    native::NotificationCategory {
        id: CATEGORY.into(),
        actions: vec![
            native::NotificationAction::opens_app(START, "Start Now"),
            native::NotificationAction::button(NOT_TODAY, "Not Today"),
        ],
    }
}
