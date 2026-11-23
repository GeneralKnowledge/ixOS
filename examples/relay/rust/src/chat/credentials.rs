//! Where the account (and its token) is kept: the iOS Keychain in the app,
//! memory in tests.

use std::sync::Mutex;

use serde::{Deserialize, Serialize};

use super::types::{ChatAccount, ChatError};
use crate::native::NativeServices;

/// The Keychain item holding the account.
pub const ACCOUNT_KEY: &str = "chat.account";

/// The account as stored: everything needed to talk to its server again.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredAccount {
    pub server_url: String,
    pub user_id: String,
    pub display_name: String,
    /// The old API's account token; empty once the account is a key on a
    /// room host (`room`).
    pub token: String,
    /// The room host this account lives on, over `relay/room/1` (N4): the
    /// account is this device's key there, and no token is sent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub room: Option<crate::rooms::RoomHost>,
}

impl StoredAccount {
    /// What the host may see: everything but the token.
    pub fn public(&self) -> ChatAccount {
        ChatAccount {
            user_id: self.user_id.clone(),
            display_name: self.display_name.clone(),
            server_url: self.server_url.clone(),
            on_relay: self.room.is_some(),
        }
    }
}

/// A place to keep one account.
pub trait Credentials: Send + Sync {
    fn load(&self) -> Result<Option<StoredAccount>, ChatError>;
    fn save(&self, account: &StoredAccount) -> Result<(), ChatError>;
    /// Removes the account; no error if there is none.
    fn delete(&self) -> Result<(), ChatError>;
}

/// The account in the platform's secure store (the iOS Keychain), through
/// `forge-services`. The item is readable after first unlock on this device
/// only and is not in backups (see the Swift adapter).
pub struct KeychainCredentials {
    services: NativeServices,
}

impl KeychainCredentials {
    /// The services the host installed with `ForgeServices.install()`.
    pub fn installed() -> Result<Self, ChatError> {
        let services = NativeServices::installed().map_err(|err| ChatError::UnsupportedOnHost {
            detail: format!("no Keychain: {err}"),
        })?;
        Ok(Self::new(services))
    }

    pub fn new(services: NativeServices) -> Self {
        Self { services }
    }
}

impl Credentials for KeychainCredentials {
    fn load(&self) -> Result<Option<StoredAccount>, ChatError> {
        let Some(json) = self.services.read_secret_string(ACCOUNT_KEY)? else {
            return Ok(None);
        };
        match serde_json::from_str(&json) {
            Ok(account) => Ok(Some(account)),
            // An unreadable item is useless: drop it, so the user can sign
            // up again instead of being stuck.
            Err(err) => {
                forge_runtime::logging::warn!(target: "chat", "ignoring an unreadable Keychain account: {err}");
                self.services.delete_secret(ACCOUNT_KEY)?;
                Ok(None)
            }
        }
    }

    fn save(&self, account: &StoredAccount) -> Result<(), ChatError> {
        let json = serde_json::to_string(account).map_err(|err| ChatError::Internal {
            reason: err.to_string(),
        })?;
        self.services
            .write_secret_string(ACCOUNT_KEY, &json)
            .map_err(Into::into)
    }

    fn delete(&self) -> Result<(), ChatError> {
        self.services.delete_secret(ACCOUNT_KEY).map_err(Into::into)
    }
}

/// Keeps the account in memory only (tests, `swift run` on a Mac).
#[derive(Default)]
pub struct MemoryCredentials {
    account: Mutex<Option<StoredAccount>>,
}

impl Credentials for MemoryCredentials {
    fn load(&self) -> Result<Option<StoredAccount>, ChatError> {
        Ok(self.lock().clone())
    }

    fn save(&self, account: &StoredAccount) -> Result<(), ChatError> {
        *self.lock() = Some(account.clone());
        Ok(())
    }

    fn delete(&self) -> Result<(), ChatError> {
        *self.lock() = None;
        Ok(())
    }
}

impl MemoryCredentials {
    fn lock(&self) -> std::sync::MutexGuard<'_, Option<StoredAccount>> {
        self.account.lock().unwrap_or_else(|p| p.into_inner())
    }
}
