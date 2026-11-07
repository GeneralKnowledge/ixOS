//! The real phone: Developer Disk Image, the WebDriverAgent runner, and an
//! HTTP transport to it, all over usbmuxd with the `idevice` crate that
//! Forge Device uses.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use idevice::provider::{IdeviceProvider, UsbmuxdProvider};
use idevice::services::dvt::xctest::{TestConfig, WdaRunHandle, XCUITestService};
use idevice::services::installation_proxy::InstallationProxyClient;
use idevice::services::lockdown::LockdownClient;
use idevice::services::mobile_image_mounter::ImageMounter;
use idevice::usbmuxd::{UsbmuxdAddr, UsbmuxdConnection};
use idevice::{IdeviceError, IdeviceService};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use crate::http::{encode_request, is_complete, parse_response, HttpTransport};
use crate::{Result, UiError};

/// Label idevice sends with each request; shows up in the phone's logs.
const LABEL: &str = "forge-uitest";

/// The port WebDriverAgent listens on, on the phone.
pub const WDA_PORT: u16 = 8100;

/// How the WebDriverAgent runner's bundle id ends once xtool has signed it
/// (xtool prefixes every free-account bundle id with `XTL-<id>.`).
pub const RUNNER_SUFFIX: &str = "WebDriverAgentRunner.xctrunner";

/// How long a WebDriverAgent request may take. Finding elements on a busy
/// screen can take seconds; screenshots are a few hundred kilobytes.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(60);

fn device_error(context: &str, err: IdeviceError) -> UiError {
    UiError::Device(format!("{context}: {err}"))
}

/// One connected phone.
pub struct Phone {
    runtime: Arc<tokio::runtime::Runtime>,
    provider: Arc<UsbmuxdProvider>,
    pub udid: String,
}

/// The parts of a personalised Developer Disk Image, from Xcode's
/// `Contents/Resources/CoreDeviceDDIs`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DdiFiles {
    pub image: PathBuf,
    pub trust_cache: PathBuf,
    pub build_manifest: PathBuf,
}

impl Phone {
    /// The phone with this UDID, or the only one connected.
    pub fn open(udid: Option<&str>) -> Result<Phone> {
        let backend =
            forge_device::UsbBackend::new().map_err(|e| UiError::Device(e.to_string()))?;
        let device = forge_device::pick_device(&backend, udid)
            .map_err(|e| UiError::Device(e.to_string()))?;
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()?;
        let provider = runtime.block_on(async {
            let mut mux = UsbmuxdConnection::default()
                .await
                .map_err(|e| device_error("cannot reach usbmuxd", e))?;
            let found = mux
                .get_device(&device.udid)
                .await
                .map_err(|e| device_error("phone went away", e))?;
            Ok::<_, UiError>(found.to_provider(UsbmuxdAddr::default(), LABEL))
        })?;
        Ok(Phone {
            runtime: Arc::new(runtime),
            provider: Arc::new(provider),
            udid: device.udid,
        })
    }

    async fn lockdown(&self) -> Result<LockdownClient> {
        let pairing = self
            .provider
            .get_pairing_file()
            .await
            .map_err(|e| device_error("this computer is not trusted by the phone", e))?;
        let mut lockdown = LockdownClient::connect(&*self.provider)
            .await
            .map_err(|e| device_error("lockdown", e))?;
        lockdown
            .start_session(&pairing)
            .await
            .map_err(|e| device_error("lockdown session", e))?;
        Ok(lockdown)
    }

    /// The iOS major version.
    pub fn ios_major(&self) -> Result<u32> {
        self.runtime.block_on(async {
            let mut lockdown = self.lockdown().await?;
            let version = lockdown
                .get_value(Some("ProductVersion"), None)
                .await
                .map_err(|e| device_error("iOS version", e))?;
            version
                .as_string()
                .and_then(|v| v.split('.').next())
                .and_then(|major| major.parse().ok())
                .ok_or_else(|| UiError::Device("unreadable iOS version".into()))
        })
    }

    /// Whether a Developer Disk Image is mounted (it is gone after a reboot).
    pub fn ddi_mounted(&self) -> Result<bool> {
        self.runtime.block_on(async {
            let mut mounter = ImageMounter::connect(&*self.provider)
                .await
                .map_err(|e| device_error("mobile_image_mounter", e))?;
            let devices = mounter
                .copy_devices()
                .await
                .map_err(|e| device_error("listing mounted images", e))?;
            Ok(!devices.is_empty())
        })
    }

    /// Mounts the personalised Developer Disk Image (iOS 17 and later).
    /// Apple's TSS server signs it for this phone, so this needs internet.
    pub fn mount_ddi(&self, files: &DdiFiles) -> Result<()> {
        let image = std::fs::read(&files.image)?;
        let trust_cache = std::fs::read(&files.trust_cache)?;
        let manifest = std::fs::read(&files.build_manifest)?;
        self.runtime.block_on(async {
            let chip_id = {
                let mut lockdown = self.lockdown().await?;
                let value = lockdown
                    .get_value(Some("UniqueChipID"), None)
                    .await
                    .map_err(|e| device_error("chip id", e))?;
                value
                    .as_unsigned_integer()
                    .ok_or_else(|| UiError::Device("unreadable chip id".into()))?
            };
            let mut mounter = ImageMounter::connect(&*self.provider)
                .await
                .map_err(|e| device_error("mobile_image_mounter", e))?;
            mounter
                .mount_personalized(
                    &*self.provider,
                    image,
                    trust_cache,
                    &manifest,
                    None,
                    chip_id,
                )
                .await
                .map_err(|e| device_error("mounting the Developer Disk Image", e))
        })
    }

    /// Bundle ids of the apps the developer installed.
    pub fn user_apps(&self) -> Result<Vec<String>> {
        self.runtime.block_on(async {
            let mut proxy = InstallationProxyClient::connect(&*self.provider)
                .await
                .map_err(|e| device_error("installation_proxy", e))?;
            let apps = proxy
                .get_apps(Some("User"), None)
                .await
                .map_err(|e| device_error("listing apps", e))?;
            let mut ids: Vec<String> = apps.into_keys().collect();
            ids.sort();
            Ok(ids)
        })
    }

    /// Starts the WebDriverAgent runner through testmanagerd and waits until
    /// it answers. It runs until the returned handle is dropped.
    pub fn start_runner(&self, runner_id: &str) -> Result<Runner> {
        let provider: Arc<dyn IdeviceProvider> = self.provider.clone();
        let handle = self.runtime.block_on(async {
            let mut proxy = InstallationProxyClient::connect(&*provider)
                .await
                .map_err(|e| device_error("installation_proxy", e))?;
            let config = TestConfig::from_installation_proxy(&mut proxy, runner_id, None)
                .await
                .map_err(|e| device_error("reading the runner's install", e))?;
            XCUITestService::new(provider)
                .run_until_wda_ready(config, Duration::from_secs(90))
                .await
                .map_err(|e| device_error("starting WebDriverAgent", e))
        })?;
        Ok(Runner { handle })
    }

    /// An HTTP transport to WebDriverAgent on the phone.
    pub fn transport(&self) -> UsbHttp {
        UsbHttp {
            runtime: self.runtime.clone(),
            provider: self.provider.clone(),
            port: WDA_PORT,
        }
    }
}

/// A running WebDriverAgent; stops when dropped.
pub struct Runner {
    handle: WdaRunHandle,
}

impl Drop for Runner {
    fn drop(&mut self) {
        self.handle.abort();
    }
}

/// One usbmuxd connection per request to a port on the phone.
pub struct UsbHttp {
    runtime: Arc<tokio::runtime::Runtime>,
    provider: Arc<UsbmuxdProvider>,
    port: u16,
}

impl HttpTransport for UsbHttp {
    fn request(&mut self, method: &str, path: &str, body: Option<&[u8]>) -> Result<(u16, Vec<u8>)> {
        let request = encode_request(method, path, body);
        let raw = self.runtime.block_on(async {
            let device = self
                .provider
                .connect(self.port)
                .await
                .map_err(|e| device_error("connecting to WebDriverAgent", e))?;
            let mut socket = device
                .get_socket()
                .ok_or_else(|| UiError::Device("usbmuxd gave no socket".into()))?;
            socket.write_all(&request).await?;
            socket.flush().await?;
            // Read until the response is complete or the server closes;
            // never wait for a close that a kept-alive server won't send.
            let mut raw = Vec::new();
            let mut buf = vec![0u8; 64 * 1024];
            let read_all = async {
                loop {
                    let n = socket.read(&mut buf).await?;
                    if n == 0 {
                        break;
                    }
                    raw.extend_from_slice(&buf[..n]);
                    if is_complete(&raw) {
                        break;
                    }
                }
                Ok::<_, std::io::Error>(())
            };
            match tokio::time::timeout(REQUEST_TIMEOUT, read_all).await {
                Ok(read) => read?,
                Err(_) => {
                    return Err(UiError::Wda(format!(
                        "{method} {path}: no complete reply after {} s (is the phone locked, or WebDriverAgent stuck?)",
                        REQUEST_TIMEOUT.as_secs()
                    )))
                }
            };
            if raw.is_empty() {
                return Err(UiError::Wda(format!(
                    "{method} {path}: WebDriverAgent closed the connection without replying (did the runner stop?)"
                )));
            }
            Ok::<_, UiError>(raw)
        })?;
        parse_response(&raw)
    }
}

/// The installed runner, if any.
pub fn find_runner(apps: &[String]) -> Option<&str> {
    apps.iter().map(String::as_str).find(|id| {
        *id == "com.facebook.WebDriverAgentRunner.xctrunner"
            || id.ends_with(&format!(".{RUNNER_SUFFIX}"))
    })
}

/// The installed bundle id for an app's forge.toml bundle id: the same id,
/// or xtool's `XTL-<id>.` prefixed copy of it.
pub fn resolve_app<'a>(
    apps: &'a [String],
    bundle_id: &str,
) -> std::result::Result<&'a str, String> {
    if let Some(exact) = apps.iter().find(|id| *id == bundle_id) {
        return Ok(exact);
    }
    let matches: Vec<&String> = apps
        .iter()
        .filter(|id| {
            id.strip_prefix("XTL-")
                .and_then(|rest| rest.split_once('.'))
                .is_some_and(|(_, original)| original == bundle_id)
        })
        .collect();
    match matches.as_slice() {
        [one] => Ok(one),
        [] => Err(format!("{bundle_id} is not installed on the phone")),
        many => Err(format!(
            "several installs match {bundle_id}: {}",
            many.iter()
                .map(|s| s.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        )),
    }
}

/// Finds the image, trust cache and build manifest under `dir` (Xcode's
/// `CoreDeviceDDIs` folder, or any folder holding the three). Prefers the
/// iOS image when the folder has images for several platforms.
pub fn find_ddi(dir: &Path) -> std::result::Result<DdiFiles, String> {
    let mut candidates = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(next) = stack.pop() {
        let entries = std::fs::read_dir(&next).map_err(|e| format!("{}: {e}", next.display()))?;
        let mut files = Vec::new();
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else {
                files.push(path);
            }
        }
        let named = |pred: &dyn Fn(&str) -> bool| {
            files
                .iter()
                .find(|p| p.file_name().and_then(|n| n.to_str()).is_some_and(pred))
                .cloned()
        };
        let manifest = named(&|n| n == "BuildManifest.plist");
        let trust = named(&|n| n.ends_with(".trustcache"));
        let image = named(&|n| n.ends_with(".dmg"));
        if let (Some(build_manifest), Some(trust_cache), Some(image)) = (manifest, trust, image) {
            candidates.push(DdiFiles {
                image,
                trust_cache,
                build_manifest,
            });
        }
    }
    candidates.sort_by_key(|c| !c.image.to_string_lossy().contains("iOS"));
    candidates.into_iter().next().ok_or_else(|| {
        format!(
            "no Developer Disk Image under {} (looking for BuildManifest.plist, a .dmg and its .trustcache; \
             they are in Xcode.app/Contents/Resources/CoreDeviceDDIs)",
            dir.display()
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ids(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn finds_the_xtool_signed_runner() {
        let apps = ids(&[
            "XTL-AB12.org.forgemobile.pocketlibrary",
            "XTL-AB12.com.facebook.WebDriverAgentRunner.xctrunner",
        ]);
        assert_eq!(
            find_runner(&apps),
            Some("XTL-AB12.com.facebook.WebDriverAgentRunner.xctrunner")
        );
        assert_eq!(find_runner(&ids(&["a.b"])), None);
    }

    #[test]
    fn resolves_xtool_bundle_ids() {
        let apps = ids(&[
            "XTL-AB12.org.forgemobile.pocketlibrary",
            "org.example.direct",
        ]);
        assert_eq!(
            resolve_app(&apps, "org.example.direct"),
            Ok("org.example.direct")
        );
        assert_eq!(
            resolve_app(&apps, "org.forgemobile.pocketlibrary"),
            Ok("XTL-AB12.org.forgemobile.pocketlibrary")
        );
        assert_eq!(
            resolve_app(&apps, "forgemobile.pocketlibrary"),
            Err("forgemobile.pocketlibrary is not installed on the phone".into())
        );
    }

    #[test]
    fn finds_the_ios_ddi() {
        let dir = tempfile::tempdir().unwrap();
        for platform in ["tvOS_DDI", "iOS_DDI"] {
            let restore = dir.path().join(platform).join("Restore");
            std::fs::create_dir_all(&restore).unwrap();
            for name in ["BuildManifest.plist", "090-1.dmg", "090-1.dmg.trustcache"] {
                std::fs::write(restore.join(name), b"x").unwrap();
            }
        }
        let found = find_ddi(dir.path()).unwrap();
        assert!(
            found.image.ends_with("iOS_DDI/Restore/090-1.dmg"),
            "{found:?}"
        );
        assert!(found.trust_cache.ends_with("090-1.dmg.trustcache"));
        assert!(find_ddi(&dir.path().join("iOS_DDI/Restore/..")).is_ok());
        let empty = tempfile::tempdir().unwrap();
        assert!(find_ddi(empty.path())
            .unwrap_err()
            .contains("CoreDeviceDDIs"));
    }
}
