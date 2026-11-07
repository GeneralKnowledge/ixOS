//! The live connection to a development build of an app (Forge Device M1).
//!
//! The app (built with `forge run ios --devtools`) listens on the phone's
//! loopback interface; [`DeviceBackend::connect_port`] reaches it through
//! usbmuxd. The protocol is `forge_devproto`; this module adds what only
//! the computer needs: where pairing secrets are kept, and the connect
//! steps with errors in words.

use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

use forge_devproto::client::{self, format_secret, parse_secret, Client, From};
use forge_devproto::{new_secret, Secret};

use crate::{DeviceBackend, DeviceError, DeviceRef, Result};

/// A connection to a port on the phone.
pub trait LiveStream: Read + Write + Send {}
impl<T: Read + Write + Send> LiveStream for T {}

/// An open session with a development build.
pub type Session = Client<Box<dyn LiveStream>>;

/// Pairing secrets on this computer: one file per phone and app, readable
/// only by the user, under `$XDG_DATA_HOME/forge/devices` (normally
/// `~/.local/share/forge/devices`).
pub struct PairingStore {
    dir: PathBuf,
}

impl PairingStore {
    pub fn at(dir: impl Into<PathBuf>) -> Self {
        Self { dir: dir.into() }
    }

    /// The default location, from `XDG_DATA_HOME` or `HOME`.
    pub fn default_location() -> Result<Self> {
        let base = std::env::var_os("XDG_DATA_HOME")
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))
            .ok_or_else(|| {
                DeviceError::Pairings("set HOME or XDG_DATA_HOME to keep pairings".into())
            })?;
        Ok(Self::at(base.join("forge/devices")))
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Where the secret for `app` on the phone `udid` lives. Both names
    /// come from the phone, so anything but `[A-Za-z0-9._-]` is replaced.
    pub fn path(&self, udid: &str, app: &str) -> PathBuf {
        let clean = |s: &str| -> String {
            let s: String = s
                .chars()
                .map(|c| {
                    if c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-') {
                        c
                    } else {
                        '_'
                    }
                })
                .collect();
            // Never `.` or `..`.
            if s.trim_matches('.').is_empty() {
                "_".into()
            } else {
                s
            }
        };
        self.dir
            .join(clean(udid))
            .join(format!("{}.key", clean(app)))
    }

    pub fn load(&self, udid: &str, app: &str) -> Option<Secret> {
        parse_secret(&std::fs::read_to_string(self.path(udid, app)).ok()?)
    }

    /// Stores a secret, creating the folders private to the user.
    pub fn save(&self, udid: &str, app: &str, secret: &Secret) -> io::Result<PathBuf> {
        let path = self.path(udid, app);
        let dir = path.parent().expect("a key file is inside a folder");
        let mut builder = std::fs::DirBuilder::new();
        builder.recursive(true);
        #[cfg(unix)]
        std::os::unix::fs::DirBuilderExt::mode(&mut builder, 0o700);
        builder.create(dir)?;
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create(true).truncate(true);
        #[cfg(unix)]
        std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
        options
            .open(&path)?
            .write_all(format_secret(secret).as_bytes())?;
        Ok(path)
    }

    /// Deletes a secret. `Ok(false)` if there was none.
    pub fn remove(&self, udid: &str, app: &str) -> io::Result<bool> {
        match std::fs::remove_file(self.path(udid, app)) {
            Ok(()) => Ok(true),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(false),
            Err(e) => Err(e),
        }
    }
}

/// Pairs with the development build on screen and stores the secret.
/// Returns the app's name and where the secret was saved.
pub fn pair(
    backend: &dyn DeviceBackend,
    device: &DeviceRef,
    port: u16,
    store: &PairingStore,
) -> Result<(String, PathBuf)> {
    let stream = backend.connect_port(device, port)?;
    let secret = new_secret();
    let preamble = client::pair(stream, &secret)?;
    let path = store
        .save(&device.udid, &preamble.app, &secret)
        .map_err(|e| DeviceError::Pairings(format!("could not save the pairing: {e}")))?;
    Ok((preamble.app, path))
}

/// Opens a session with the development build on screen, using the stored
/// secret for whichever app answers.
pub fn open(
    backend: &dyn DeviceBackend,
    device: &DeviceRef,
    port: u16,
    store: &PairingStore,
    from: From,
) -> Result<Session> {
    let stream = backend.connect_port(device, port)?;
    let udid = device.udid.clone();
    Ok(Client::open(
        stream,
        |preamble| store.load(&udid, &preamble.app),
        from,
    )?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secrets_are_kept_per_phone_and_app_privately() {
        let dir = tempfile::tempdir().unwrap();
        let store = PairingStore::at(dir.path());
        let secret = new_secret();
        let path = store
            .save("00008130-AB", "org.example.chat", &secret)
            .unwrap();
        assert!(path.ends_with("00008130-AB/org.example.chat.key"));
        assert_eq!(store.load("00008130-AB", "org.example.chat"), Some(secret));
        assert_eq!(store.load("00008130-AB", "org.example.other"), None);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = |p: &Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode(&path), 0o600);
            assert_eq!(mode(path.parent().unwrap()), 0o700);
        }
        assert!(store.remove("00008130-AB", "org.example.chat").unwrap());
        assert!(!store.remove("00008130-AB", "org.example.chat").unwrap());
    }

    #[test]
    fn names_from_the_phone_cannot_escape_the_folder() {
        let store = PairingStore::at("/data");
        assert_eq!(
            store.path("../..", "../../etc/passwd"),
            PathBuf::from("/data/.._../.._.._etc_passwd.key")
        );
        assert_eq!(store.path("a/b", "x y"), PathBuf::from("/data/a_b/x_y.key"));
    }
}
