//! `ForgeWeatherApp`: the Weather example's Rust side.
//!
//! Concurrency contract (compare `crate::app` for the Counter):
//!
//! * There is no serial command thread. Weather has no state to mutate except
//!   the cache, and HTTP can take seconds, so each refresh and each search runs
//!   on its own short-lived worker thread. Callers are never blocked.
//! * **One refresh at a time.** Starting a refresh cancels the one in flight
//!   (pull-to-refresh twice, or switching places, never shows stale results).
//! * **Cancellation is immediate for the caller.** `cancel()` resolves
//!   `result()` with `Cancelled` at once. The blocking HTTP call cannot be
//!   interrupted, so the worker finishes in the background and its response
//!   is discarded: it neither resolves the operation nor touches the cache.
//!   (The one wait: if the worker is already writing the cache, `cancel()`
//!   waits for that write, and the refresh counts as having succeeded.)
//! * A successful refresh writes the cache *before* resolving, so
//!   `cached_report()` reflects it as soon as `result()` returns.
//! * **Offline fallback.** When a refresh fails for a network reason and the
//!   cache holds a report for (about) the same place that is not too old, the
//!   refresh *succeeds* with that report marked `Freshness::Cached`, carrying
//!   the problem. Otherwise it fails with the typed error.
//! * The cache holds the newest report for each of up to
//!   [`MAX_CACHED_REPORTS`] places, written atomically after every live
//!   refresh, so every saved place has something to show at launch. It is
//!   disposable: unreadable cache files are ignored, and cache write failures
//!   never fail a refresh.
//! * Saved places (see `places.rs`) are a separate small file; they are
//!   state the user made, so failing to write them is an error.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicU8, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use forge_net::{HttpClient, Request, UreqClient};
use serde::{Deserialize, Serialize};

use super::error::WeatherError;
use super::open_meteo;
use super::types::{
    Coordinate, FetchProblem, Freshness, Place, RefreshPhase, WeatherConfig, WeatherReport,
};
use crate::signal::{lock, Completion};

const CACHE_FILENAME: &str = "forge-weather-cache-v1.json";
/// 1: one report. 2: a list of reports, newest first. Both are read.
const CACHE_SCHEMA_VERSION: u32 = 2;
/// Reports kept in the cache, one per place.
pub(crate) const MAX_CACHED_REPORTS: usize = 16;
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(15);
const DEFAULT_MAX_CACHE_AGE_SECONDS: u64 = 24 * 60 * 60;
/// A cached report is reused for a place within this distance of it.
pub(crate) const CACHE_MATCH_RADIUS_KM: f64 = 5.0;
const USER_AGENT: &str = concat!(
    "forge-weather-example/",
    env!("CARGO_PKG_VERSION"),
    " (+https://github.com/GeneralKnowledge/ixOS)"
);

type RefreshResult = Result<WeatherReport, WeatherError>;

struct Settings {
    forecast_base: String,
    geocoding_base: String,
    timeout: Duration,
    max_cache_age_seconds: u64,
}

pub(crate) struct Shared {
    client: Arc<dyn HttpClient>,
    settings: Settings,
    cache_path: Option<PathBuf>,
    /// Newest first, at most one per place.
    cache: Mutex<Vec<WeatherReport>>,
    pub(crate) places_path: Option<PathBuf>,
    pub(crate) places: Mutex<Vec<Place>>,
    active: Mutex<Option<Arc<RefreshState>>>,
    next_id: AtomicU64,
    shut_down: AtomicBool,
}

struct RefreshState {
    id: u64,
    place: Place,
    phase: AtomicU8,
    cancel_requested: AtomicBool,
    completion: Arc<Completion<RefreshResult>>,
    /// Serialises "cancel" against "write the cache, then resolve", so a
    /// cancelled refresh never touches the cache and a successful one has
    /// written it before any caller sees the result.
    commit: Mutex<()>,
}

const PHASE_FETCHING: u8 = 0;
const PHASE_DECODING: u8 = 1;

impl RefreshState {
    fn cancel(&self) {
        self.cancel_requested.store(true, Ordering::SeqCst);
        let _commit = lock(&self.commit);
        self.completion.complete(Err(WeatherError::Cancelled));
    }

    fn phase(&self) -> RefreshPhase {
        if self.completion.get().is_some() {
            return RefreshPhase::Finished;
        }
        match self.phase.load(Ordering::SeqCst) {
            PHASE_FETCHING => RefreshPhase::Fetching,
            _ => RefreshPhase::Decoding,
        }
    }
}

/// The Weather example's app instance. Thread-safe; share it freely.
#[derive(uniffi::Object)]
pub struct ForgeWeatherApp {
    pub(crate) shared: Arc<Shared>,
}

/// Handle to one refresh. Releasing it does not cancel the refresh.
#[derive(uniffi::Object)]
pub struct WeatherOperation {
    state: Arc<RefreshState>,
}

#[uniffi::export]
impl WeatherOperation {
    pub fn id(&self) -> u64 {
        self.state.id
    }

    /// The place this refresh was started for.
    pub fn place(&self) -> Place {
        self.state.place.clone()
    }

    pub fn phase(&self) -> RefreshPhase {
        self.state.phase()
    }

    pub fn is_cancel_requested(&self) -> bool {
        self.state.cancel_requested.load(Ordering::SeqCst)
    }

    /// Resolves `result()` with `Cancelled` now, unless it already resolved.
    /// Idempotent.
    pub fn cancel(&self) {
        self.state.cancel();
    }

    /// Resolves once with the report or the error. Can be awaited any number
    /// of times; every await returns the same value.
    pub async fn result(&self) -> Result<WeatherReport, WeatherError> {
        self.state.completion.wait().await
    }
}

#[uniffi::export]
impl ForgeWeatherApp {
    /// Opens the app with Open-Meteo defaults. `storage_directory` holds the
    /// offline cache; it is created if missing.
    #[uniffi::constructor]
    pub fn new(storage_directory: String) -> Self {
        Self::build(
            Arc::new(UreqClient::with_user_agent(USER_AGENT)),
            Some(PathBuf::from(storage_directory)),
            WeatherConfig::default(),
        )
        .expect("default config is valid")
    }

    /// Opens the app with overrides (alternative endpoints, timeouts). Tests
    /// use this to point at an unreachable host and exercise offline paths.
    #[uniffi::constructor]
    pub fn with_config(
        storage_directory: String,
        config: WeatherConfig,
    ) -> Result<Arc<Self>, WeatherError> {
        Self::build(
            Arc::new(UreqClient::with_user_agent(USER_AGENT)),
            Some(PathBuf::from(storage_directory)),
            config,
        )
        .map(Arc::new)
    }

    /// Starts fetching weather for `place`, cancelling any refresh already in
    /// flight. Returns at once; await `result()` on the returned handle.
    pub fn refresh(&self, place: Place) -> Result<Arc<WeatherOperation>, WeatherError> {
        let shared = &self.shared;
        if shared.shut_down.load(Ordering::SeqCst) {
            return Err(WeatherError::ShutDown);
        }
        open_meteo::validate_coordinate(place.coordinate)?;
        if place.name.trim().is_empty() {
            return Err(WeatherError::invalid("place name is empty"));
        }

        let state = Arc::new(RefreshState {
            id: shared.next_id.fetch_add(1, Ordering::SeqCst) + 1,
            place,
            phase: AtomicU8::new(PHASE_FETCHING),
            cancel_requested: AtomicBool::new(false),
            completion: Completion::new(),
            commit: Mutex::new(()),
        });
        if let Some(previous) = lock(&shared.active).replace(Arc::clone(&state)) {
            previous.cancel();
        }

        let worker_shared = Arc::clone(shared);
        let worker_state = Arc::clone(&state);
        let spawned = thread::Builder::new()
            .name(format!("forge-weather-refresh-{}", state.id))
            .spawn(move || run_refresh(&worker_shared, &worker_state));
        if let Err(err) = spawned {
            state.completion.complete(Err(WeatherError::Internal {
                reason: format!("could not start refresh thread: {err}"),
            }));
        }
        Ok(Arc::new(WeatherOperation { state }))
    }

    /// The refresh in flight, if any.
    pub fn active_refresh(&self) -> Option<Arc<WeatherOperation>> {
        lock(&self.shared.active)
            .as_ref()
            .filter(|s| s.completion.get().is_none())
            .map(|state| {
                Arc::new(WeatherOperation {
                    state: Arc::clone(state),
                })
            })
    }

    /// Looks places up by name ("Paris", "Springfield, IL"). Returns up to 10
    /// matches, best first; an empty list means no match.
    pub async fn search_places(&self, query: String) -> Result<Vec<Place>, WeatherError> {
        let shared = Arc::clone(&self.shared);
        if shared.shut_down.load(Ordering::SeqCst) {
            return Err(WeatherError::ShutDown);
        }
        let query = open_meteo::validate_query(&query)?.to_string();
        let completion: Arc<Completion<Result<Vec<Place>, WeatherError>>> = Completion::new();
        let worker_completion = Arc::clone(&completion);
        let spawned = thread::Builder::new()
            .name("forge-weather-search".into())
            .spawn(move || {
                worker_completion.complete(run_search(&shared, &query));
            });
        if let Err(err) = spawned {
            return Err(WeatherError::Internal {
                reason: format!("could not start search thread: {err}"),
            });
        }
        completion.wait().await
    }

    /// The last successfully fetched report, marked `Cached` with its age, or
    /// `None`. Never waits for the network; use it to fill the screen at launch.
    pub fn cached_report(&self) -> Option<WeatherReport> {
        let cached = lock(&self.shared.cache).first().cloned()?;
        Some(as_cached(cached, None))
    }

    /// The cached report for `place` (within 5 km), however old, marked
    /// `Cached`. `weather_overview` drops the hours and days already past.
    pub fn cached_report_for(&self, place: Place) -> Option<WeatherReport> {
        let cache = lock(&self.shared.cache);
        nearest(&cache, place.coordinate).map(|r| as_cached(r.clone(), None))
    }

    /// Every cached report, newest first, marked `Cached`.
    pub fn cached_reports(&self) -> Vec<WeatherReport> {
        lock(&self.shared.cache)
            .iter()
            .map(|r| as_cached(r.clone(), None))
            .collect()
    }

    /// Cancels the refresh in flight; later calls fail with `ShutDown`.
    /// Idempotent. Workers still running finish in the background without
    /// touching the cache.
    pub fn shutdown(&self) {
        self.shared.shut_down.store(true, Ordering::SeqCst);
        if let Some(active) = lock(&self.shared.active).take() {
            active.cancel();
        }
    }

    pub fn is_shut_down(&self) -> bool {
        self.shared.shut_down.load(Ordering::SeqCst)
    }
}

impl ForgeWeatherApp {
    pub(crate) fn build(
        client: Arc<dyn HttpClient>,
        storage_directory: Option<PathBuf>,
        config: WeatherConfig,
    ) -> Result<Self, WeatherError> {
        forge_runtime::logging::init();
        let base = |value: Option<String>, default: &str| -> Result<String, WeatherError> {
            let value = value.unwrap_or_else(|| default.to_string());
            Request::get(&value).validate().map_err(|_| {
                WeatherError::invalid(format!("base URL must be http(s): {value:?}"))
            })?;
            Ok(value.trim_end_matches('/').to_string())
        };
        let settings = Settings {
            forecast_base: base(config.forecast_base_url, open_meteo::DEFAULT_FORECAST_BASE)?,
            geocoding_base: base(
                config.geocoding_base_url,
                open_meteo::DEFAULT_GEOCODING_BASE,
            )?,
            timeout: match config.request_timeout_ms {
                Some(0) => return Err(WeatherError::invalid("request timeout must be > 0")),
                Some(ms) => Duration::from_millis(u64::from(ms)),
                None => DEFAULT_TIMEOUT,
            },
            max_cache_age_seconds: config
                .max_cache_age_seconds
                .unwrap_or(DEFAULT_MAX_CACHE_AGE_SECONDS),
        };
        let cache_path = storage_directory
            .as_ref()
            .map(|dir| dir.join(CACHE_FILENAME));
        let cache = cache_path.as_deref().map(load_cache).unwrap_or_default();
        let places_path = storage_directory
            .as_ref()
            .map(|dir| dir.join(super::places::PLACES_FILENAME));
        let places = places_path
            .as_deref()
            .map(super::places::load_places)
            .unwrap_or_default();
        Ok(Self {
            shared: Arc::new(Shared {
                client,
                settings,
                cache_path,
                cache: Mutex::new(cache),
                places_path,
                places: Mutex::new(places),
                active: Mutex::new(None),
                next_id: AtomicU64::new(0),
                shut_down: AtomicBool::new(false),
            }),
        })
    }
}

fn run_refresh(shared: &Shared, state: &RefreshState) {
    if state.completion.get().is_some() {
        return; // Cancelled before the thread got going.
    }
    let request = Request::get(open_meteo::forecast_url(
        &shared.settings.forecast_base,
        state.place.coordinate,
    ))
    .header("Accept", "application/json")
    .timeout(shared.settings.timeout);

    let fetched = shared.client.send(&request);
    if state.completion.get().is_some() {
        return; // Cancelled while waiting: discard the response.
    }
    state.phase.store(PHASE_DECODING, Ordering::SeqCst);
    let result = match fetched {
        Ok(response) if response.is_success() => {
            open_meteo::decode_forecast(&response.body, state.place.clone(), now_unix())
        }
        Ok(response) => Err(open_meteo::status_error(response.status, &response.body)),
        Err(err) => Err(err.into()),
    };

    match result {
        Ok(report) => {
            let _commit = lock(&state.commit);
            // A cancel that got here first wins: discard the report.
            if state.completion.get().is_none() {
                if !shared.shut_down.load(Ordering::SeqCst) {
                    store_cache(shared, report.clone());
                }
                state.completion.complete(Ok(report));
            }
        }
        Err(err) => {
            let fallback = err.as_fetch_problem().and_then(|problem| {
                usable_cache(shared, state.place.coordinate).map(|r| as_cached(r, Some(problem)))
            });
            state.completion.complete(fallback.ok_or(err));
        }
    }
}

fn run_search(shared: &Shared, query: &str) -> Result<Vec<Place>, WeatherError> {
    let request = Request::get(open_meteo::geocoding_url(
        &shared.settings.geocoding_base,
        query,
    ))
    .header("Accept", "application/json")
    .timeout(shared.settings.timeout);
    let response = shared.client.send(&request)?;
    if !response.is_success() {
        return Err(open_meteo::status_error(response.status, &response.body));
    }
    open_meteo::decode_search(&response.body)
}

fn now_unix() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX))
        .unwrap_or(0)
}

fn age_seconds(report: &WeatherReport) -> u64 {
    u64::try_from(now_unix().saturating_sub(report.fetched_at_unix)).unwrap_or(0)
}

fn as_cached(mut report: WeatherReport, problem: Option<FetchProblem>) -> WeatherReport {
    report.freshness = Freshness::Cached {
        age_seconds: age_seconds(&report),
        problem,
    };
    report
}

/// Great-circle distance in kilometres.
pub(crate) fn distance_km(a: Coordinate, b: Coordinate) -> f64 {
    let (lat1, lat2) = (a.latitude.to_radians(), b.latitude.to_radians());
    let dlat = lat2 - lat1;
    let dlon = (b.longitude - a.longitude).to_radians();
    let h = (dlat / 2.0).sin().powi(2) + lat1.cos() * lat2.cos() * (dlon / 2.0).sin().powi(2);
    2.0 * 6371.0 * h.sqrt().min(1.0).asin()
}

/// The cached report closest to `coordinate`, if one is within
/// [`CACHE_MATCH_RADIUS_KM`].
fn nearest(cache: &[WeatherReport], coordinate: Coordinate) -> Option<&WeatherReport> {
    cache
        .iter()
        .map(|r| (distance_km(r.place.coordinate, coordinate), r))
        .filter(|(d, _)| *d <= CACHE_MATCH_RADIUS_KM)
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(_, r)| r)
}

fn usable_cache(shared: &Shared, coordinate: Coordinate) -> Option<WeatherReport> {
    let cache = lock(&shared.cache);
    let cached = nearest(&cache, coordinate)?;
    (age_seconds(cached) <= shared.settings.max_cache_age_seconds).then(|| cached.clone())
}

#[derive(Serialize, Deserialize)]
struct CacheEnvelope {
    schema_version: u32,
    /// Schema 2.
    #[serde(default)]
    reports: Vec<WeatherReport>,
    /// Schema 1.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    report: Option<WeatherReport>,
}

fn load_cache(path: &Path) -> Vec<WeatherReport> {
    let Ok(bytes) = std::fs::read(path) else {
        return Vec::new();
    };
    match serde_json::from_slice::<CacheEnvelope>(&bytes) {
        Ok(CacheEnvelope {
            schema_version: CACHE_SCHEMA_VERSION,
            mut reports,
            ..
        }) => {
            reports.truncate(MAX_CACHED_REPORTS);
            reports
        }
        Ok(CacheEnvelope {
            schema_version: 1,
            report: Some(report),
            ..
        }) => vec![report],
        // Corrupt, or written by another build: the cache is disposable.
        _ => Vec::new(),
    }
}

fn store_cache(shared: &Shared, mut report: WeatherReport) {
    report.freshness = Freshness::Live;
    let mut cache = lock(&shared.cache);
    // Two refreshes for one place can finish out of order; keep the newest.
    if nearest(&cache, report.place.coordinate)
        .is_some_and(|c| c.fetched_at_unix > report.fetched_at_unix)
    {
        return;
    }
    let here = report.place.coordinate;
    let mut updated: Vec<WeatherReport> = Vec::with_capacity(MAX_CACHED_REPORTS);
    updated.push(report);
    updated.extend(
        cache
            .iter()
            .filter(|r| distance_km(r.place.coordinate, here) > CACHE_MATCH_RADIUS_KM)
            .cloned(),
    );
    updated.truncate(MAX_CACHED_REPORTS);
    if let Some(path) = &shared.cache_path {
        let envelope = CacheEnvelope {
            schema_version: CACHE_SCHEMA_VERSION,
            reports: updated.clone(),
            report: None,
        };
        // Best effort: a failed write leaves the previous file intact.
        if let Ok(json) = serde_json::to_vec(&envelope) {
            if let Some(dir) = path.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            let _ = forge_storage::write_atomic(path, &json);
        }
    }
    *cache = updated;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::signal::block_on;
    use crate::weather::types::PlaceSource;
    use forge_net::{Method, NetError, Response, StaticClient};
    use std::sync::mpsc;
    use std::time::Instant;

    const FORECAST: &str = include_str!("fixtures/forecast_paris.json");
    const SEARCH: &str = include_str!("fixtures/search_paris.json");

    fn paris() -> Place {
        Place {
            name: "Paris".into(),
            region: None,
            country: Some("France".into()),
            country_code: Some("FR".into()),
            coordinate: Coordinate {
                latitude: 48.8534,
                longitude: 2.3488,
            },
            timezone: None,
            population: None,
            source: PlaceSource::Manual,
        }
    }

    fn lyon() -> Place {
        Place {
            name: "Lyon".into(),
            coordinate: Coordinate {
                latitude: 45.75,
                longitude: 4.85,
            },
            ..paris()
        }
    }

    fn forecast_url(place: &Place) -> String {
        open_meteo::forecast_url(open_meteo::DEFAULT_FORECAST_BASE, place.coordinate)
    }

    fn app(client: impl HttpClient + 'static, dir: Option<&Path>) -> ForgeWeatherApp {
        ForgeWeatherApp::build(
            Arc::new(client),
            dir.map(Path::to_path_buf),
            WeatherConfig::default(),
        )
        .unwrap()
    }

    /// A client whose every call blocks until the test releases it, then
    /// delegates to `inner`.
    struct GatedClient {
        inner: StaticClient,
        entered: Mutex<mpsc::Sender<()>>,
        release: Mutex<mpsc::Receiver<()>>,
    }

    impl HttpClient for GatedClient {
        fn send(&self, request: &Request) -> Result<Response, NetError> {
            let _ = lock(&self.entered).send(());
            let _ = lock(&self.release).recv();
            self.inner.send(request)
        }
    }

    fn gated(inner: StaticClient) -> (GatedClient, mpsc::Receiver<()>, mpsc::Sender<()>) {
        let (entered_tx, entered_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        (
            GatedClient {
                inner,
                entered: Mutex::new(entered_tx),
                release: Mutex::new(release_rx),
            },
            entered_rx,
            release_tx,
        )
    }

    fn wait_until(mut condition: impl FnMut() -> bool) {
        let deadline = Instant::now() + Duration::from_secs(5);
        while !condition() {
            assert!(Instant::now() < deadline, "condition not met in time");
            thread::sleep(Duration::from_millis(5));
        }
    }

    #[test]
    fn refresh_fetches_decodes_and_caches() {
        let dir = tempfile::tempdir().unwrap();
        let client = StaticClient::new().route(
            Method::Get,
            forecast_url(&paris()),
            Ok(Response::new(200, FORECAST)),
        );
        let app = app(client, Some(dir.path()));
        assert!(app.cached_report().is_none());

        let op = app.refresh(paris()).unwrap();
        let report = block_on(op.result()).unwrap();
        assert_eq!(report.freshness, Freshness::Live);
        assert_eq!(report.daily.len(), 7);
        assert_eq!(op.phase(), RefreshPhase::Finished);
        assert!(app.active_refresh().is_none());

        // Cached in memory and on disk, and a new instance restores it.
        assert!(matches!(
            app.cached_report().unwrap().freshness,
            Freshness::Cached { problem: None, .. }
        ));
        let reopened = self::app(NoopClientForTests, Some(dir.path()));
        let restored = reopened.cached_report().unwrap();
        assert_eq!(restored.current, report.current);
        assert_eq!(restored.hourly, report.hourly);
    }

    #[derive(Clone, Copy)]
    struct NoopClientForTests;
    impl HttpClient for NoopClientForTests {
        fn send(&self, request: &Request) -> Result<Response, NetError> {
            forge_net::NoopClient.send(request)
        }
    }

    #[test]
    fn offline_without_cache_is_a_typed_error() {
        let client = StaticClient::new().route(
            Method::Get,
            forecast_url(&paris()),
            Err(NetError::Offline {
                reason: "no route".into(),
            }),
        );
        let app = app(client, None);
        let err = block_on(app.refresh(paris()).unwrap().result()).unwrap_err();
        assert_eq!(
            err,
            WeatherError::Offline {
                reason: "no route".into()
            }
        );
    }

    #[test]
    fn offline_with_cache_returns_the_cached_report_and_the_problem() {
        let dir = tempfile::tempdir().unwrap();
        let online = StaticClient::new().route(
            Method::Get,
            forecast_url(&paris()),
            Ok(Response::new(200, FORECAST)),
        );
        block_on(
            app(online, Some(dir.path()))
                .refresh(paris())
                .unwrap()
                .result(),
        )
        .unwrap();

        let offline = StaticClient::new()
            .route(Method::Get, forecast_url(&paris()), Err(NetError::Timeout))
            .route(Method::Get, forecast_url(&lyon()), Err(NetError::Timeout));
        let app = app(offline, Some(dir.path()));
        let report = block_on(app.refresh(paris()).unwrap().result()).unwrap();
        assert!(matches!(
            report.freshness,
            Freshness::Cached {
                problem: Some(FetchProblem::TimedOut),
                ..
            }
        ));
        // The cache is for Paris; Lyon gets the error.
        let err = block_on(app.refresh(lyon()).unwrap().result()).unwrap_err();
        assert_eq!(err, WeatherError::Timeout);
    }

    #[test]
    fn stale_cache_and_client_errors_are_not_masked() {
        let dir = tempfile::tempdir().unwrap();
        let online = StaticClient::new().route(
            Method::Get,
            forecast_url(&paris()),
            Ok(Response::new(200, FORECAST)),
        );
        block_on(
            app(online, Some(dir.path()))
                .refresh(paris())
                .unwrap()
                .result(),
        )
        .unwrap();

        // A 400 means the request is wrong: show it, even with a cache.
        let bad_request = StaticClient::new().route(
            Method::Get,
            forecast_url(&paris()),
            Ok(Response::new(400, r#"{"error":true,"reason":"nope"}"#)),
        );
        let err = block_on(
            app(bad_request, Some(dir.path()))
                .refresh(paris())
                .unwrap()
                .result(),
        )
        .unwrap_err();
        assert_eq!(
            err,
            WeatherError::Server {
                status: 400,
                reason: Some("nope".into())
            }
        );

        // A 503 with a cache older than the limit is an error too.
        let unavailable = StaticClient::new().route(
            Method::Get,
            forecast_url(&paris()),
            Ok(Response::new(503, "")),
        );
        let strict = ForgeWeatherApp::build(
            Arc::new(unavailable),
            Some(dir.path().to_path_buf()),
            WeatherConfig {
                max_cache_age_seconds: Some(0),
                ..WeatherConfig::default()
            },
        )
        .unwrap();
        // Age is whole seconds; make sure the cache is at least 1 s old.
        {
            let mut cache = lock(&strict.shared.cache);
            cache[0].fetched_at_unix -= 10;
        }
        let err = block_on(strict.refresh(paris()).unwrap().result()).unwrap_err();
        assert!(matches!(err, WeatherError::Server { status: 503, .. }));
    }

    #[test]
    fn cancel_resolves_at_once_and_discards_the_late_response() {
        let dir = tempfile::tempdir().unwrap();
        let (client, entered, release) = gated(StaticClient::new().route(
            Method::Get,
            forecast_url(&paris()),
            Ok(Response::new(200, FORECAST)),
        ));
        let app = app(client, Some(dir.path()));
        let op = app.refresh(paris()).unwrap();
        entered.recv().unwrap();
        assert_eq!(op.phase(), RefreshPhase::Fetching);
        assert!(app.active_refresh().is_some());

        op.cancel();
        assert!(op.is_cancel_requested());
        assert_eq!(block_on(op.result()), Err(WeatherError::Cancelled));
        assert_eq!(op.phase(), RefreshPhase::Finished);

        release.send(()).unwrap();
        // Give the worker time to (not) write the cache.
        thread::sleep(Duration::from_millis(50));
        assert!(app.cached_report().is_none());
        assert!(!dir.path().join(CACHE_FILENAME).exists());
        // Cancelling again is harmless.
        op.cancel();
        assert_eq!(block_on(op.result()), Err(WeatherError::Cancelled));
    }

    #[test]
    fn a_new_refresh_supersedes_the_one_in_flight() {
        let (client, entered, release) = gated(
            StaticClient::new()
                .route(
                    Method::Get,
                    forecast_url(&paris()),
                    Ok(Response::new(200, FORECAST)),
                )
                .route(
                    Method::Get,
                    forecast_url(&lyon()),
                    Ok(Response::new(200, FORECAST)),
                ),
        );
        let app = app(client, None);
        let first = app.refresh(paris()).unwrap();
        entered.recv().unwrap();
        let second = app.refresh(lyon()).unwrap();
        assert_eq!(block_on(first.result()), Err(WeatherError::Cancelled));
        assert!(second.id() > first.id());
        assert_eq!(app.active_refresh().unwrap().id(), second.id());

        release.send(()).unwrap();
        release.send(()).unwrap();
        let report = block_on(second.result()).unwrap();
        assert_eq!(report.place.name, "Lyon");
        wait_until(|| app.cached_report().is_some());
        assert_eq!(app.cached_report().unwrap().place.name, "Lyon");
    }

    #[test]
    fn shutdown_cancels_and_refuses_new_work() {
        let (client, entered, release) = gated(StaticClient::new());
        let app = app(client, None);
        let op = app.refresh(paris()).unwrap();
        entered.recv().unwrap();
        app.shutdown();
        app.shutdown();
        assert!(app.is_shut_down());
        assert_eq!(block_on(op.result()), Err(WeatherError::Cancelled));
        assert!(matches!(app.refresh(paris()), Err(WeatherError::ShutDown)));
        assert_eq!(
            block_on(app.search_places("Paris".into())),
            Err(WeatherError::ShutDown)
        );
        release.send(()).unwrap();
    }

    #[test]
    fn refresh_validates_its_input() {
        let app = app(StaticClient::new(), None);
        let mut bad = paris();
        bad.coordinate.latitude = 91.0;
        assert!(matches!(
            app.refresh(bad),
            Err(WeatherError::InvalidInput { .. })
        ));
        let mut unnamed = paris();
        unnamed.name = "  ".into();
        assert!(matches!(
            app.refresh(unnamed),
            Err(WeatherError::InvalidInput { .. })
        ));
    }

    #[test]
    fn search_returns_places_and_maps_errors() {
        let url = open_meteo::geocoding_url(open_meteo::DEFAULT_GEOCODING_BASE, "Paris");
        let client = StaticClient::new()
            .route(Method::Get, url, Ok(Response::new(200, SEARCH)))
            .route(
                Method::Get,
                open_meteo::geocoding_url(open_meteo::DEFAULT_GEOCODING_BASE, "Nowhere"),
                Ok(Response::new(200, "{}")),
            );
        let app = app(client, None);
        let places = block_on(app.search_places("  Paris ".into())).unwrap();
        assert_eq!(places.len(), 2);
        assert!(block_on(app.search_places("Nowhere".into()))
            .unwrap()
            .is_empty());
        assert!(matches!(
            block_on(app.search_places("x".into())),
            Err(WeatherError::InvalidInput { .. })
        ));
        // Unrouted URL: StaticClient answers 404.
        assert!(matches!(
            block_on(app.search_places("Atlantis".into())),
            Err(WeatherError::Server { status: 404, .. })
        ));
    }

    #[test]
    fn the_cache_keeps_one_report_per_place() {
        let dir = tempfile::tempdir().unwrap();
        let client = StaticClient::new()
            .route(
                Method::Get,
                forecast_url(&paris()),
                Ok(Response::new(200, FORECAST)),
            )
            .route(
                Method::Get,
                forecast_url(&lyon()),
                Ok(Response::new(200, FORECAST)),
            );
        let app = app(client, Some(dir.path()));
        for place in [paris(), lyon(), paris()] {
            block_on(app.refresh(place).unwrap().result()).unwrap();
        }
        let names: Vec<_> = app
            .cached_reports()
            .into_iter()
            .map(|r| r.place.name)
            .collect();
        assert_eq!(names, ["Paris", "Lyon"], "newest first, one each");
        assert_eq!(app.cached_report().unwrap().place.name, "Paris");
        let lyon_report = app.cached_report_for(lyon()).unwrap();
        assert_eq!(lyon_report.place.name, "Lyon");
        assert!(matches!(lyon_report.freshness, Freshness::Cached { .. }));
        let mut far = lyon();
        far.coordinate.latitude += 1.0;
        assert!(app.cached_report_for(far).is_none());

        let restored = super::load_cache(&dir.path().join(CACHE_FILENAME));
        assert_eq!(restored.len(), 2);
    }

    #[test]
    fn a_schema_1_cache_is_still_read() {
        let dir = tempfile::tempdir().unwrap();
        let client = StaticClient::new().route(
            Method::Get,
            forecast_url(&paris()),
            Ok(Response::new(200, FORECAST)),
        );
        let report = block_on(app(client, None).refresh(paris()).unwrap().result()).unwrap();
        // Schema 1 had no is_day or gusts per hour.
        let mut json = serde_json::to_value(&report).unwrap();
        for hour in json["hourly"].as_array_mut().unwrap() {
            let hour = hour.as_object_mut().unwrap();
            hour.remove("is_day");
            hour.remove("wind_gusts_kmh");
        }
        let v1 = serde_json::json!({"schema_version": 1, "report": json});
        std::fs::write(dir.path().join(CACHE_FILENAME), v1.to_string()).unwrap();
        let cached = app(StaticClient::new(), Some(dir.path()))
            .cached_report()
            .unwrap();
        assert_eq!(cached.place.name, "Paris");
        assert!(cached.hourly[0].is_day);
    }

    #[test]
    fn corrupt_or_foreign_cache_files_are_ignored() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(CACHE_FILENAME);
        std::fs::write(&path, "{not json").unwrap();
        assert!(app(StaticClient::new(), Some(dir.path()))
            .cached_report()
            .is_none());
        std::fs::write(&path, r#"{"schema_version":99,"report":{}}"#).unwrap();
        assert!(app(StaticClient::new(), Some(dir.path()))
            .cached_report()
            .is_none());
    }

    #[test]
    fn config_is_validated() {
        let build = |config| {
            ForgeWeatherApp::build(Arc::new(StaticClient::new()), None, config).map(|_| ())
        };
        assert!(build(WeatherConfig {
            forecast_base_url: Some("ftp://x".into()),
            ..Default::default()
        })
        .is_err());
        assert!(build(WeatherConfig {
            request_timeout_ms: Some(0),
            ..Default::default()
        })
        .is_err());
        assert!(build(WeatherConfig {
            geocoding_base_url: Some("http://127.0.0.1:9/".into()),
            ..Default::default()
        })
        .is_ok());
    }

    #[test]
    fn distance_is_sane() {
        let p = paris().coordinate;
        assert!(distance_km(p, p) < 1e-9);
        let d = distance_km(p, lyon().coordinate);
        assert!((380.0..400.0).contains(&d), "{d}");
    }

    /// End to end against the real service and the real client:
    /// `cargo test --manifest-path examples/weather/rust/Cargo.toml -- --ignored live_open_meteo`.
    #[test]
    #[ignore = "needs network access to open-meteo.com"]
    fn live_open_meteo() {
        let dir = tempfile::tempdir().unwrap();
        let app = ForgeWeatherApp::new(dir.path().display().to_string());
        let places = block_on(app.search_places("Paris".into())).unwrap();
        let place = places.into_iter().next().expect("Paris exists");
        let report = block_on(app.refresh(place).unwrap().result()).unwrap();
        assert_eq!(report.freshness, Freshness::Live);
        assert_eq!(report.hourly.len(), open_meteo::HOURLY_POINTS);
        assert_eq!(report.daily.len(), open_meteo::DAILY_POINTS);
    }

    /// The offline path with the real client: nothing listens on port 9.
    #[test]
    fn real_client_offline_path() {
        std::env::set_var("NO_PROXY", "127.0.0.1,localhost");
        std::env::set_var("no_proxy", "127.0.0.1,localhost");
        let dir = tempfile::tempdir().unwrap();
        let app = ForgeWeatherApp::with_config(
            dir.path().display().to_string(),
            WeatherConfig {
                forecast_base_url: Some("http://127.0.0.1:9".into()),
                request_timeout_ms: Some(2_000),
                ..Default::default()
            },
        )
        .unwrap();
        let err = block_on(app.refresh(paris()).unwrap().result()).unwrap_err();
        assert!(
            matches!(err, WeatherError::Offline { .. } | WeatherError::Timeout),
            "{err:?}"
        );
    }
}
