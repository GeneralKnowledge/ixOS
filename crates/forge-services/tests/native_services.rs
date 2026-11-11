//! Expands `native_services!()` the way an app crate does and checks the
//! Rust-side API against the in-memory fakes.

use std::sync::Arc;

use forge_runtime::block_on;

uniffi::setup_scaffolding!("services_test");
forge_services::native_services!();

use native::*;

fn services() -> (tempfile::TempDir, NativeServices, testing::Fakes) {
    let dir = tempfile::tempdir().unwrap();
    let (services, fakes) = NativeServices::for_tests(dir.path());
    (dir, services, fakes)
}

#[test]
fn install_checks_directories_and_replaces() {
    let (_dir, services, _) = services();
    let mut relative = services.clone();
    relative.directories.caches = "Caches".into();
    assert!(matches!(
        relative.install(),
        Err(NativeServiceError::InvalidInput { .. })
    ));

    install_native_services(
        services.directories.clone(),
        services.secure_store.clone(),
        services.notifier.clone(),
        services.permissions.clone(),
    )
    .unwrap();
    assert!(native_services_installed());
    let installed = NativeServices::installed().unwrap();
    assert_eq!(installed.directories, services.directories);
    assert!(std::path::Path::new(&installed.directories.application_support).is_dir());

    NativeServices::uninstall();
    assert!(!native_services_installed());
    assert!(matches!(
        NativeServices::installed(),
        Err(NativeServiceError::NotInstalled { .. })
    ));
    assert_eq!(native_services_version(), forge_services::VERSION);
}

#[test]
fn secrets_round_trip_and_keys_are_checked() {
    let (_dir, services, fakes) = services();
    assert_eq!(services.read_secret_string("token").unwrap(), None);
    services.write_secret_string("token", "s3cret").unwrap();
    assert_eq!(
        services.read_secret_string("token").unwrap().as_deref(),
        Some("s3cret")
    );
    fakes
        .secure_store
        .set("binary".into(), vec![0xff, 0xfe])
        .unwrap();
    assert!(matches!(
        services.read_secret_string("binary"),
        Err(NativeServiceError::InvalidInput { .. })
    ));
    services.delete_secret("token").unwrap();
    services.delete_secret("token").unwrap(); // missing is fine
    assert_eq!(services.read_secret_string("token").unwrap(), None);
    assert!(matches!(
        services.write_secret_string("", "x"),
        Err(NativeServiceError::InvalidInput { .. })
    ));
}

#[test]
fn notifications_are_checked_then_scheduled() {
    let (_dir, services, fakes) = services();
    let mut reminder = LocalNotification::now("r1", "Read", "Chapter 3");
    reminder.deep_link = Some("myapp://book/7".into());
    block_on(services.schedule_notification(reminder.clone())).unwrap();
    // Same id replaces.
    reminder.body = "Chapter 4".into();
    block_on(services.schedule_notification(reminder.clone())).unwrap();
    assert_eq!(fakes.notifier.scheduled(), vec![reminder]);
    assert_eq!(
        block_on(services.notifier.pending()),
        vec!["r1".to_string()]
    );

    let mut bad = LocalNotification::now("r2", "Every", "second");
    bad.trigger = NotificationTrigger::After {
        seconds: 1,
        repeats: true,
    };
    assert!(matches!(
        block_on(services.schedule_notification(bad)),
        Err(NativeServiceError::InvalidInput { .. })
    ));

    services.cancel_notification("r1");
    assert!(fakes.notifier.scheduled().is_empty());

    fakes.notifier.deny();
    assert!(matches!(
        block_on(services.schedule_notification(LocalNotification::now("r3", "T", "B"))),
        Err(NativeServiceError::PermissionDenied { .. })
    ));
}

#[test]
fn permissions_report_and_request() {
    let (_dir, services, fakes) = services();
    let p = NativePermission::Notifications;
    assert_eq!(
        block_on(services.permission_status(p)),
        PermissionStatus::NotDetermined
    );
    assert_eq!(
        block_on(services.request_permission(p)),
        PermissionStatus::Granted
    );
    fakes.permissions.set(p, PermissionStatus::Denied);
    assert_eq!(
        block_on(services.request_permission(p)),
        PermissionStatus::Denied
    );
    assert!(!PermissionStatus::Denied.allows_use());
    assert!(PermissionStatus::Provisional.allows_use());
}

#[test]
fn the_microphone_is_its_own_permission() {
    let (_dir, services, fakes) = services();
    fakes
        .permissions
        .set(NativePermission::Notifications, PermissionStatus::Denied);
    assert_eq!(
        block_on(services.permission_status(NativePermission::Microphone)),
        PermissionStatus::NotDetermined
    );
    assert_eq!(
        block_on(services.request_permission(NativePermission::Microphone)),
        PermissionStatus::Granted
    );
    assert_eq!(
        block_on(services.permission_status(NativePermission::Notifications)),
        PermissionStatus::Denied
    );
}

#[test]
fn deep_links_parse_through_the_export() {
    let link = parse_deep_link("myapp://book/7?from=notification".into()).unwrap();
    assert_eq!(link.route, vec!["book".to_string(), "7".to_string()]);
    assert_eq!(link.query_value("from"), Some("notification"));
    assert_eq!(link.url, "myapp://book/7?from=notification");
    assert!(matches!(
        parse_deep_link("not a link".into()),
        Err(NativeServiceError::InvalidInput { .. })
    ));
}

#[test]
fn rust_can_implement_the_host_interfaces() {
    struct Failing;
    impl SecureStore for Failing {
        fn get(&self, _: String) -> Result<Option<Vec<u8>>, NativeServiceError> {
            Err(NativeServiceError::Platform {
                code: -25308,
                reason: "interaction not allowed".into(),
            })
        }
        fn set(&self, _: String, _: Vec<u8>) -> Result<(), NativeServiceError> {
            Ok(())
        }
        fn delete(&self, _: String) -> Result<(), NativeServiceError> {
            Ok(())
        }
    }
    let (_dir, mut services, _) = services();
    services.secure_store = Arc::new(Failing);
    let err = services.read_secret_string("k").unwrap_err();
    assert_eq!(
        err.to_string(),
        "platform error -25308: interaction not allowed"
    );
}

#[test]
fn notification_categories_are_checked_and_responses_reach_the_handler() {
    let dir = tempfile::tempdir().unwrap();
    let (services, fakes) = NativeServices::for_tests(dir.path());
    let message = NotificationCategory {
        id: "message".into(),
        actions: vec![
            NotificationAction::reply("reply"),
            NotificationAction::button("read", "Mark as Read"),
            NotificationAction::destructive("delete", "Delete"),
        ],
    };
    services
        .set_notification_categories(vec![message.clone()])
        .unwrap();
    assert_eq!(fakes.notifier.categories(), vec![message]);
    let twice = NotificationCategory {
        id: "c".into(),
        actions: vec![
            NotificationAction::button("a", "A"),
            NotificationAction::button("a", "B"),
        ],
    };
    assert!(matches!(
        services.set_notification_categories(vec![twice]),
        Err(NativeServiceError::InvalidInput { .. })
    ));

    // A response that arrives before the handler (a cold launch) waits.
    let reply = NotificationResponse {
        notification_id: "m1".into(),
        action_id: "reply".into(),
        text: Some("On my way".into()),
        category: Some("message".into()),
        thread_id: Some("room-1".into()),
        deep_link: None,
    };
    deliver_notification_response(reply.clone());
    let seen = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let sink = seen.clone();
    on_notification_response(move |r| sink.lock().unwrap().push(r));
    assert_eq!(*seen.lock().unwrap(), vec![reply]);
}
