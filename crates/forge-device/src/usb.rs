//! [`DeviceBackend`] over usbmuxd with the `idevice` crate.

use idevice::provider::{IdeviceProvider, UsbmuxdProvider};
use idevice::services::crashreportcopymobile::CrashReportCopyMobileClient;
use idevice::services::lockdown::LockdownClient;
use idevice::services::syslog_relay::SyslogRelayClient;
use idevice::usbmuxd::{UsbmuxdAddr, UsbmuxdConnection};
use idevice::{IdeviceError, IdeviceService};

use crate::{Connection, DeviceBackend, DeviceError, DeviceInfo, DeviceRef, Result};

/// Label idevice sends with each request; shows up in the phone's logs.
const LABEL: &str = "forge-device";

/// Talks to phones through the local usbmuxd. Each call opens its own
/// connections and closes them when it returns.
pub struct UsbBackend {
    runtime: tokio::runtime::Runtime,
}

impl UsbBackend {
    pub fn new() -> Result<Self> {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_io()
            .enable_time()
            .build()
            .map_err(|err| DeviceError::Service(format!("async runtime: {err}")))?;
        Ok(Self { runtime })
    }

    fn provider(&self, device: &DeviceRef) -> Result<UsbmuxdProvider> {
        self.runtime.block_on(async {
            let mut mux = mux().await?;
            let found = mux
                .get_device(&device.udid)
                .await
                .map_err(|_| DeviceError::NotFound(device.udid.clone()))?;
            Ok(found.to_provider(UsbmuxdAddr::default(), LABEL))
        })
    }
}

async fn mux() -> Result<UsbmuxdConnection> {
    UsbmuxdConnection::default()
        .await
        .map_err(|err| DeviceError::Usbmuxd {
            reason: describe(&err),
        })
}

/// The error text plus its source, which idevice keeps separately.
fn describe(err: &IdeviceError) -> String {
    match std::error::Error::source(err) {
        Some(source) => format!("{err}: {source}"),
        None => err.to_string(),
    }
}

fn service_error(err: IdeviceError) -> DeviceError {
    match err {
        IdeviceError::InvalidHostID => DeviceError::NotPaired {
            reason: describe(&err),
        },
        IdeviceError::DeviceLocked => DeviceError::NotPaired {
            reason: "the phone is locked".into(),
        },
        other => DeviceError::Service(describe(&other)),
    }
}

async fn pairing(provider: &UsbmuxdProvider) -> Result<idevice::pairing_file::PairingFile> {
    provider
        .get_pairing_file()
        .await
        .map_err(|err| DeviceError::NotPaired {
            reason: describe(&err),
        })
}

async fn connect<S: IdeviceService>(provider: &UsbmuxdProvider) -> Result<S> {
    pairing(provider).await?;
    S::connect(provider).await.map_err(service_error)
}

fn string_value(value: Option<plist::Value>) -> String {
    value
        .and_then(|v| v.as_string().map(str::to_string))
        .unwrap_or_default()
}

impl DeviceBackend for UsbBackend {
    fn devices(&self) -> Result<Vec<DeviceRef>> {
        self.runtime.block_on(async {
            let mut mux = mux().await?;
            let devices = mux
                .get_devices()
                .await
                .map_err(|err| DeviceError::Usbmuxd {
                    reason: describe(&err),
                })?;
            Ok(devices
                .into_iter()
                .map(|d| DeviceRef {
                    udid: d.udid,
                    connection: match d.connection_type {
                        idevice::usbmuxd::Connection::Usb => Connection::Usb,
                        idevice::usbmuxd::Connection::Network(ip) => {
                            Connection::Network(ip.to_string())
                        }
                        idevice::usbmuxd::Connection::Unknown(kind) => Connection::Other(kind),
                    },
                })
                .collect())
        })
    }

    fn info(&self, device: &DeviceRef) -> Result<DeviceInfo> {
        let provider = self.provider(device)?;
        self.runtime.block_on(async {
            let pairing = pairing(&provider).await?;
            let mut lockdown = LockdownClient::connect(&provider)
                .await
                .map_err(service_error)?;
            lockdown
                .start_session(&pairing)
                .await
                .map_err(service_error)?;
            let mut values = Vec::new();
            for (key, domain) in [
                ("DeviceName", None),
                ("ProductType", None),
                ("ProductVersion", None),
                ("BuildVersion", None),
                ("DeveloperModeStatus", Some("com.apple.security.mac.amfi")),
            ] {
                values.push(lockdown.get_value(Some(key), domain).await.ok());
            }
            let mut values = values.into_iter();
            let mut next = || values.next().flatten();
            let name = string_value(next());
            let product_type = string_value(next());
            let ios_version = string_value(next());
            let build_version = string_value(next());
            let developer_mode = next().and_then(|v| v.as_boolean());
            Ok(DeviceInfo {
                udid: device.udid.clone(),
                connection: device.connection.clone(),
                name,
                product_type,
                ios_version,
                build_version,
                developer_mode,
            })
        })
    }

    fn syslog(&self, device: &DeviceRef, on_line: &mut dyn FnMut(&str) -> bool) -> Result<()> {
        let provider = self.provider(device)?;
        self.runtime.block_on(async {
            let mut relay: SyslogRelayClient = connect(&provider).await?;
            loop {
                let line = match relay.next().await {
                    Ok(line) => line,
                    // The phone was unplugged or locked down the service.
                    Err(IdeviceError::UnexpectedResponse(_)) => return Ok(()),
                    Err(err) => return Err(service_error(err)),
                };
                if !on_line(line.trim_end_matches(['\n', '\0'])) {
                    return Ok(());
                }
            }
        })
    }

    fn crash_reports(&self, device: &DeviceRef) -> Result<Vec<String>> {
        let provider = self.provider(device)?;
        self.runtime.block_on(async {
            let mut client: CrashReportCopyMobileClient = connect(&provider).await?;
            let mut names = Vec::new();
            let mut folders = vec![None::<String>];
            // The crash folder has a few subfolders (Retired, Analytics);
            // one level down is enough for app crashes.
            while let Some(folder) = folders.pop() {
                let entries = client.ls(folder.as_deref()).await.map_err(service_error)?;
                for entry in entries {
                    if entry == "." || entry == ".." {
                        continue;
                    }
                    let path = match &folder {
                        Some(parent) => format!("{parent}/{entry}"),
                        None => entry.clone(),
                    };
                    if folder.is_none() && !entry.contains('.') {
                        folders.push(Some(path));
                    } else {
                        names.push(path);
                    }
                }
            }
            Ok(names)
        })
    }

    fn pull_crash_report(&self, device: &DeviceRef, name: &str) -> Result<Vec<u8>> {
        let provider = self.provider(device)?;
        self.runtime.block_on(async {
            let mut client: CrashReportCopyMobileClient = connect(&provider).await?;
            client.pull(name).await.map_err(service_error)
        })
    }
}
