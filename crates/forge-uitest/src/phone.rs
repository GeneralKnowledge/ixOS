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

/// The parts of a personalised Developer Disk Image (Xcode 26.0's xip has
/// none; the doronz88/DeveloperDiskImage mirror does).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DdiFiles {
    pub image: PathBuf,
    pub trust_cache: PathBuf,
    pub build_manifest: PathBuf,
}

impl Phone {
    /// The phone with this UDID, or the only one connected.
    pub fn open(udid: Option<&str>) -> Result<Phone> {
        debug_log_from_env();
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

    /// Runs one exchange with the phone, and gives up after `limit` rather
    /// than waiting forever on a socket the phone never answers.
    fn within<T>(
        &self,
        what: &str,
        limit: Duration,
        work: impl std::future::Future<Output = Result<T>>,
    ) -> Result<T> {
        within_on(&self.runtime, what, limit, work)
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
        self.within("reading the iOS version", Duration::from_secs(30), async {
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
        self.within(
            "checking the Developer Disk Image",
            Duration::from_secs(30),
            async {
                let mut mounter = ImageMounter::connect(&*self.provider)
                    .await
                    .map_err(|e| device_error("mobile_image_mounter", e))?;
                let devices = mounter
                    .copy_devices()
                    .await
                    .map_err(|e| device_error("listing mounted images", e))?;
                Ok(!devices.is_empty())
            },
        )
    }

    /// Mounts the personalised Developer Disk Image (iOS 17 and later).
    /// Apple's TSS server signs it for this phone, so this needs internet.
    pub fn mount_ddi(&self, files: &DdiFiles) -> Result<()> {
        let image = std::fs::read(&files.image)?;
        let trust_cache = std::fs::read(&files.trust_cache)?;
        let manifest = std::fs::read(&files.build_manifest)?;
        self.within(
            "mounting the Developer Disk Image",
            Duration::from_secs(300),
            async {
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
            },
        )
    }

    /// Bundle ids of the apps the developer installed.
    pub fn user_apps(&self) -> Result<Vec<String>> {
        self.within("listing apps", Duration::from_secs(60), async {
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
        self.stop_stale_wda();
        let provider: Arc<dyn IdeviceProvider> = self.provider.clone();
        let runner_id = runner_id.to_owned();
        // On its own task, so the deadline below holds even when a step
        // inside idevice waits on a phone socket that never answers (seen on
        // iOS 26.3 when the runner's developer profile wasn't trusted yet):
        // idevice's own limit only covers waiting for WDA's port.
        let task = self.runtime.spawn(async move {
            let mut proxy = InstallationProxyClient::connect(&*provider)
                .await
                .map_err(|e| device_error("installation_proxy", e))?;
            let config = TestConfig::from_installation_proxy(&mut proxy, &runner_id, None)
                .await
                .map_err(|e| device_error("reading the runner's install", e))?;
            XCUITestService::new(provider)
                .run_until_wda_ready(config, WDA_READY)
                .await
                .map_err(|e| device_error("starting WebDriverAgent", e))
        });
        let abort = task.abort_handle();
        let handle = self
            .runtime
            .block_on(async { tokio::time::timeout(RUNNER_START, task).await })
            .map_err(|_| {
                abort.abort();
                UiError::Device(format!(
                    "WebDriverAgent did not start within {} s. {RUNNER_HINT}",
                    RUNNER_START.as_secs()
                ))
            })?
            .map_err(|e| UiError::Device(format!("the runner start task failed: {e}")))??;
        Ok(Runner { handle })
    }

    /// Shuts down a WebDriverAgent left from an earlier command. When a
    /// `forge ui` command exits, its XCTest session ends and the phone tears
    /// that runner down, but not at once: a new start would see the old WDA
    /// answer `/status`, report ready, and then lose it mid-command when the
    /// phone kills it (seen on Cati as "closed the connection" on every
    /// request after the first). Asking it to shut down first avoids that.
    fn stop_stale_wda(&self) {
        let mut http = self.transport_with_timeout(PROBE_TIMEOUT);
        if http.request("GET", "/status", None).is_err() {
            return;
        }
        let _ = http.request("GET", "/wda/shutdown", None);
        let deadline = std::time::Instant::now() + STALE_WDA_GONE;
        while std::time::Instant::now() < deadline {
            if http.request("GET", "/status", None).is_err() {
                return;
            }
            std::thread::sleep(Duration::from_millis(500));
        }
    }

    /// An HTTP transport to WebDriverAgent on the phone.
    pub fn transport(&self) -> UsbHttp {
        self.transport_with_timeout(REQUEST_TIMEOUT)
    }

    fn transport_with_timeout(&self, timeout: Duration) -> UsbHttp {
        UsbHttp {
            runtime: self.runtime.clone(),
            provider: self.provider.clone(),
            port: WDA_PORT,
            timeout,
        }
    }
}

/// Runs `work` on `runtime` with a deadline. The timeout is created inside
/// the runtime: `tokio::time::timeout` registers with the timer of the
/// runtime it is created in, and panics ("no reactor running") when it is
/// created on a plain thread, which is where the CLI calls from.
fn within_on<T>(
    runtime: &tokio::runtime::Runtime,
    what: &str,
    limit: Duration,
    work: impl std::future::Future<Output = Result<T>>,
) -> Result<T> {
    runtime
        .block_on(async move { tokio::time::timeout(limit, work).await })
        .unwrap_or_else(|_| {
            Err(UiError::Device(format!(
                "{what}: the phone did not answer within {} s (is it unlocked and still plugged in?)",
                limit.as_secs()
            )))
        })
}

/// A quick "is anything answering?" request.
const PROBE_TIMEOUT: Duration = Duration::from_secs(5);
/// How long a stale WDA gets to go away after `/wda/shutdown`.
const STALE_WDA_GONE: Duration = Duration::from_secs(10);
/// How long idevice waits for WDA's port once the test runs.
const WDA_READY: Duration = Duration::from_secs(90);
/// The whole runner start, from asking testmanagerd to WDA answering.
const RUNNER_START: Duration = Duration::from_secs(120);
const RUNNER_HINT: &str = "Unlock the phone and keep it on the home screen. If this is the first start, trust the developer in Settings > General > VPN & Device Management (the runner is signed by your Apple ID), then try again";

/// Apps that xtool sideloaded (`XTL-<team>.<id>`): the ones a free Apple
/// account's 3-app limit counts. App Store apps don't count.
pub fn sideloaded(apps: &[String]) -> Vec<String> {
    apps.iter()
        .filter(|id| id.starts_with("XTL-"))
        .cloned()
        .collect()
}

/// With `FORGE_UI_DEBUG=1`, prints idevice's debug log (lockdown,
/// testmanagerd and XCTest replies, e.g. the answer to the runner's UI
/// testing authorisation) to stderr. Off by default: it is noisy.
fn debug_log_from_env() {
    use tracing_subscriber::prelude::*;
    if std::env::var_os("FORGE_UI_DEBUG").is_none_or(|v| v.is_empty() || v == "0") {
        return;
    }
    use tracing_subscriber::filter::LevelFilter;
    // The crate root logs every plist sent and received (installation_proxy
    // replies are whole Info.plists of every app): info only there. The
    // services, tunnel and XCTest modules are what's worth reading.
    let only_idevice = tracing_subscriber::filter::Targets::new()
        .with_target("idevice", LevelFilter::INFO)
        .with_target("idevice::services", LevelFilter::DEBUG)
        .with_target("idevice::services::installation_proxy", LevelFilter::INFO)
        .with_target("idevice::tcp", LevelFilter::DEBUG);
    let _ = tracing_subscriber::registry()
        .with(
            tracing_subscriber::fmt::layer()
                .with_writer(std::io::stderr)
                .with_filter(only_idevice),
        )
        .try_init();
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
    /// How long one request may take, connect to last byte.
    timeout: Duration,
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
            match tokio::time::timeout(self.timeout, read_all).await {
                Ok(read) => read?,
                Err(_) => {
                    return Err(UiError::Wda(format!(
                        "{method} {path}: no complete reply after {} s (is the phone locked, or WebDriverAgent stuck?)",
                        self.timeout.as_secs()
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
             e.g. PersonalizedImages/Xcode_iOS_DDI_Personalized from github.com/doronz88/DeveloperDiskImage)",
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

    fn runtime() -> tokio::runtime::Runtime {
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1)
            .enable_all()
            .build()
            .unwrap()
    }

    // Called from a plain thread, as the CLI does; this panicked with "no
    // reactor running" when the timeout was built outside the runtime.
    #[test]
    fn within_runs_from_a_plain_thread() {
        let rt = runtime();
        let got = std::thread::spawn(move || {
            within_on(&rt, "test", Duration::from_secs(5), async { Ok(7) })
        })
        .join()
        .unwrap();
        assert_eq!(got.unwrap(), 7);
    }

    #[test]
    fn within_gives_up_after_the_limit() {
        let rt = runtime();
        let got: Result<()> = within_on(&rt, "waiting", Duration::from_millis(50), async {
            tokio::time::sleep(Duration::from_secs(10)).await;
            Ok(())
        });
        let err = got.unwrap_err().to_string();
        assert!(err.contains("waiting: the phone did not answer"), "{err}");
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
    fn only_sideloaded_apps_count() {
        let apps = ids(&["XTL-AB12.a", "com.burbn.instagram", "XTL-AB12.b"]);
        assert_eq!(sideloaded(&apps), ["XTL-AB12.a", "XTL-AB12.b"]);
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
            .contains("Xcode_iOS_DDI_Personalized"));
    }
}
