
// ---------------------------------------------------------------------------
// Added by scripts/make-services-check-app.sh: drives the native services from
// Rust on a device, so the Keychain, notification and permission calls go
// through the real Rust -> Swift path. Not part of the starter.

/// Notification responses seen so far, shown by `services_check_responses`.
static RESPONSES: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());

/// One line per notification response Rust received (a tap, an action).
#[uniffi::export]
pub fn services_check_responses() -> Vec<String> {
    RESPONSES.lock().map(|r| r.clone()).unwrap_or_default()
}

/// Runs the Rust-side checks and returns one line per check, starting with
/// PASS, FAIL or INFO.
#[uniffi::export]
pub async fn services_self_check() -> Vec<String> {
    use native::{LocalNotification, NativePermission, NativeServices, NotificationTrigger};
    let mut report = Vec::new();
    // The system log: `forge device logs --grep services-check` on the
    // laptop should show these three lines, one per level.
    forge_runtime::logging::init();
    forge_runtime::logging::info!(target: "services-check", "log probe info");
    forge_runtime::logging::warn!(target: "services-check", "log probe warn");
    forge_runtime::logging::error!(target: "services-check", "log probe error");
    report.push("INFO logging: wrote 3 lines tagged [services-check] to the system log".into());

    let mut check = |name: &str, result: Result<String, String>| match result {
        Ok(detail) => report.push(format!("PASS {name}: {detail}")),
        Err(detail) => report.push(format!("FAIL {name}: {detail}")),
    };

    let services = match NativeServices::installed() {
        Ok(services) => services,
        Err(err) => {
            check("installed", Err(err.to_string()));
            return report;
        }
    };
    check("installed", Ok(format!("{:?}", services.directories.application_support)));

    let key = "services-check";
    let secret = format!("secret-{}", forge_services::notify::now_ms());
    check(
        "keychain write/read",
        services
            .write_secret_string(key, &secret)
            .and_then(|_| services.read_secret_string(key))
            .map_err(|e| e.to_string())
            .and_then(|read| {
                if read.as_deref() == Some(secret.as_str()) {
                    Ok("value read back".into())
                } else {
                    Err(format!("read back {read:?}"))
                }
            }),
    );
    check(
        "keychain delete",
        services
            .delete_secret(key)
            .and_then(|_| services.read_secret_string(key))
            .map_err(|e| e.to_string())
            .and_then(|read| match read {
                None => Ok("gone".into()),
                Some(_) => Err("still there".into()),
            }),
    );

    let before = services.permission_status(NativePermission::Notifications).await;
    let after = services.request_permission(NativePermission::Notifications).await;
    check(
        "notification permission",
        if after.allows_use() {
            Ok(format!("{before:?} -> {after:?}"))
        } else {
            Err(format!("{before:?} -> {after:?} (allow notifications to finish the check)"))
        },
    );

    let mut reminder = LocalNotification::now(
        "services-check",
        "Forge services check",
        "Tap me: I carry a deep link",
    );
    reminder.trigger = NotificationTrigger::After {
        seconds: 5,
        repeats: false,
    };
    reminder.deep_link = Some("forgecheck://check/notification?from=tap".into());
    // Action buttons (long-press the banner): Reply, Done, Open.
    check(
        "notification actions",
        services
            .set_notification_categories(vec![native::NotificationCategory {
                id: "check".into(),
                actions: vec![
                    native::NotificationAction::reply("reply"),
                    native::NotificationAction::button("done", "Done"),
                    native::NotificationAction::opens_app("open", "Open"),
                ],
            }])
            .map(|_| "long-press the banner for Reply, Done and Open".into())
            .map_err(|e| e.to_string()),
    );
    reminder.category = Some("check".into());
    native::on_notification_response(|response| {
        // What the person typed stays out of the log; only its length.
        forge_runtime::logging::info!(
            target: "services-check",
            "notification response: {} (text: {} chars)",
            response.action_id,
            response.text.as_deref().map_or(0, str::len)
        );
        if let Ok(mut log) = RESPONSES.lock() {
            log.push(format!("PASS notification response: {}", response.action_id));
        }
    });
    check(
        "schedule notification",
        services
            .schedule_notification(reminder)
            .await
            .map(|_| "fires in 5 s; lock the phone or stay here".into())
            .map_err(|e| e.to_string()),
    );
    let pending = services.notifier.pending().await;
    check(
        "pending notifications",
        if pending.iter().any(|id| id == "services-check") {
            Ok(format!("{pending:?}"))
        } else {
            Err(format!("services-check missing from {pending:?}"))
        },
    );

    let mut invalid = LocalNotification::now("bad", "Every second", "");
    invalid.trigger = NotificationTrigger::After {
        seconds: 1,
        repeats: true,
    };
    check(
        "rust rejects invalid triggers",
        match services.schedule_notification(invalid).await {
            Err(native::NativeServiceError::InvalidInput { reason }) => Ok(reason),
            other => Err(format!("{other:?}")),
        },
    );

    let export = std::path::Path::new(&services.directories.documents).join("services-check.txt");
    check(
        "write a file to share",
        std::fs::write(&export, "Written by Rust for the share sheet.\n")
            .map(|_| export.display().to_string())
            .map_err(|e| e.to_string()),
    );
    report
}

/// A file Rust wrote, for the share button.
#[uniffi::export]
pub fn services_check_share_item() -> Option<native::ShareItem> {
    let services = native::NativeServices::installed().ok()?;
    let path = std::path::Path::new(&services.directories.documents).join("services-check.txt");
    path.exists().then(|| native::ShareItem::File {
        path: path.display().to_string(),
        title: Some("Forge services check".into()),
    })
}

/// Reads an imported file by path, as an app would after `forgeFileImporter`.
#[uniffi::export]
pub fn services_check_read_import(path: String) -> String {
    match std::fs::metadata(&path) {
        Ok(meta) => format!("PASS Rust read the imported file: {} bytes", meta.len()),
        Err(err) => format!("FAIL Rust could not read {path}: {err}"),
    }
}
