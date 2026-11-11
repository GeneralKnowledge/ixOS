//! Background downloads, for checking Forge's `transfers` service on a phone
//! (acceptance tests 1, 2, 3 and 6 in docs/native-capability-roadmap.md).
//! Only the Background Downloads screen calls it.
//!
//! The service does the work: it records each transfer before it starts,
//! verifies the file and puts it in this app's `downloads` folder, and
//! sorts out what happened while the app was closed. This module starts
//! test downloads and turns the ledger into rows for the screen.

use forge_storage::FileStore;

use crate::native::transfers::{
    Failure, TransferRecord, TransferRequest, TransferService, TransferState,
};
use crate::native::NativeServices;

/// Serves exactly the number of bytes asked for, so the size check has
/// something to check.
const TEST_URL: &str = "https://speed.cloudflare.com/__down?bytes=";
/// The store key test downloads go to.
const STORE: &str = "downloads";

#[derive(uniffi::Error, Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum TransferTestError {
    #[error("{reason}")]
    Unavailable { reason: String },
    #[error("{reason}")]
    Failed { reason: String },
}

/// One transfer, as the screen shows it.
#[derive(uniffi::Record, Debug, Clone, PartialEq)]
pub struct TransferRow {
    pub id: String,
    pub name: String,
    /// Queued, Running, Downloaded, Delivered, Failed or Cancelled.
    pub state: String,
    /// What went wrong, or where the file is.
    pub detail: Option<String>,
    /// 0 to 1 while running, when the size is known.
    pub fraction: Option<f64>,
    pub received_bytes: u64,
    pub attempts: u32,
    pub can_cancel: bool,
    pub can_retry: bool,
}

fn service() -> Result<TransferService, TransferTestError> {
    let service = TransferService::installed().map_err(|err| TransferTestError::Unavailable {
        reason: err.to_string(),
    })?;
    // Registered on every call: cheap, and the first call after a launch
    // delivers what finished while the app was closed.
    let dirs = NativeServices::installed()
        .map_err(|err| TransferTestError::Unavailable {
            reason: err.to_string(),
        })?
        .directories;
    service.deliver_to(
        STORE,
        FileStore::new(std::path::Path::new(&dirs.application_support).join("downloads")),
    );
    Ok(service)
}

/// Starts downloading `megabytes` MB (1 to 500) in the background.
#[uniffi::export]
pub fn transfer_test_start(megabytes: u32) -> Result<String, TransferTestError> {
    let megabytes = megabytes.clamp(1, 500);
    let bytes = u64::from(megabytes) * 1_000_000;
    let name = format!("test-{megabytes}mb-{}.bin", forge_runtime::now_ms());
    let request = TransferRequest::new(format!("{TEST_URL}{bytes}"), STORE, name)
        .expected_bytes(bytes)
        .max_bytes(bytes);
    service()?
        .start(request)
        .map_err(|err| TransferTestError::Failed {
            reason: err.to_string(),
        })
}

#[uniffi::export]
pub fn transfer_test_cancel(id: String) -> Result<(), TransferTestError> {
    service()?
        .cancel(&id)
        .map_err(|err| TransferTestError::Failed {
            reason: err.to_string(),
        })
}

#[uniffi::export]
pub fn transfer_test_retry(id: String) -> Result<(), TransferTestError> {
    service()?
        .retry(&id)
        .map_err(|err| TransferTestError::Failed {
            reason: err.to_string(),
        })
}

/// Forgets every finished transfer and deletes the test files.
#[uniffi::export]
pub fn transfer_test_clear() -> Result<(), TransferTestError> {
    let service = service()?;
    for record in service.list() {
        if !record.state.is_active() {
            let _ = service.remove(&record.id);
        }
    }
    if let Ok(dirs) = NativeServices::installed().map(|s| s.directories) {
        let _ = FileStore::new(std::path::Path::new(&dirs.application_support).join("downloads"))
            .clear();
    }
    Ok(())
}

/// Every transfer, newest first.
#[uniffi::export]
pub fn transfer_test_rows() -> Vec<TransferRow> {
    let Ok(service) = service() else {
        return Vec::new();
    };
    let mut rows: Vec<TransferRow> = service.list().iter().map(row).collect();
    rows.reverse();
    rows
}

pub(crate) fn row(record: &TransferRecord) -> TransferRow {
    let (state, detail, fraction, received) = match &record.state {
        TransferState::Queued => ("Queued", None, None, 0),
        TransferState::Running { received, expected } => (
            "Running",
            None,
            expected
                .or(record.request.expected_bytes)
                .filter(|e| *e > 0)
                .map(|e| (*received as f64 / e as f64).min(1.0)),
            *received,
        ),
        TransferState::Downloaded { size } => (
            "Downloaded",
            Some("Verified, waiting for its folder".to_string()),
            Some(1.0),
            *size,
        ),
        TransferState::Delivered { size } => (
            "Delivered",
            Some(format!("In downloads/{}", record.request.name)),
            Some(1.0),
            *size,
        ),
        TransferState::Failed(failure) => (
            match failure {
                Failure::Interrupted => "Interrupted",
                _ => "Failed",
            },
            Some(failure.to_string()),
            None,
            0,
        ),
        TransferState::Cancelled => ("Cancelled", None, None, 0),
    };
    TransferRow {
        id: record.id.clone(),
        name: record.request.name.clone(),
        state: state.into(),
        detail,
        fraction,
        received_bytes: received,
        attempts: record.attempts,
        can_cancel: record.state.is_active(),
        can_retry: matches!(
            record.state,
            TransferState::Failed(_) | TransferState::Cancelled
        ),
    }
}

/// Ends the app the way iOS does when it reclaims a suspended app, for
/// acceptance test 1. Apple suggests `exit` for testing background sessions;
/// swiping the app away is a different event (force quit, test 6).
#[uniffi::export]
pub fn transfer_test_end_app() {
    forge_runtime::logging::warn!(target: "transfer-test", "ending the app on purpose");
    std::process::exit(0);
}
