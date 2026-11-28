# A shared API layer: audit and proposal

Status: proposal (2026-11-28). Nothing here is built; each step below needs Reuben's approval.

The question: ixOS apps now talk to a lot of web APIs (Open-Meteo, Open Library, Jamendo, the Internet Archive, MusicBrainz, feeds, and soon Radio Browser and Podcast Index). Is a shared "API handler" worth having in the framework?

**Short answer: ixOS already has most of that layer, so a new one isn't worth building.** What is missing is small: one bug fix that is worth doing now, and three pieces that should wait for their second real use. Each service's own client stays in its app.

## What the framework already gives every app

| Need | Already there | Where |
|---|---|---|
| One request interface, test doubles | `HttpClient` trait, `Request`/`Response`, `StaticClient`, `NoopClient` | `crates/forge-net` |
| Native networking on iPhone (VPN, proxy, HTTP/2 and 3, Low Data Mode, system trust store) | The `http` service: `HttpService::prefer_installed` sends through `URLSession` once the host has installed its services, ureq before that and on Linux | `crates/forge-services/src/http.rs`, `swift/optional/ForgeServices+Http.swift` |
| The same on Android | `ForgeHttpTransport` on `HttpURLConnection` behind the same `HostHttp` trait (Pocket Library A4, #359) | Pocket Library's Android host |
| Timeouts | `Request::timeout`, 15 s default in both transports | `forge-net`, `http.rs` |
| Response size limits | `max_body_bytes` on both transports (8 MiB default), stopped as the body grows | same |
| Cancellation | `CancelHandle`, `send_cancellable`; cancelling stops the `URLSessionTask` | `forge-net`, `http.rs` |
| Retry spacing | `Backoff` (exponential, capped, jitter, cancel-aware wait) | `crates/forge-net/src/backoff.rs` |
| Offline and network errors | `NetError` (`Offline`, `Timeout`, `Certificate`, `Transport`, `Cancelled`), and `forge_app::app_error_conversions!(MyError: NetError)` | `forge-net`, `crates/forge-app/src/conversions.rs` |
| Streams (Server-Sent Events) | `open_stream_cancellable` on both transports | `forge-net`, `http.rs` |
| Pictures from the web, with HTTP caching and offline copies | `ForgeWebData` (URLCache, `Cache-Control`/`ETag`, shared downloads), used by `ForgeImages` | `crates/forge-services/swift/helpers/ForgeWebData.swift` |
| Keys and secrets | Keychain through `write_secret_string` / `read_secret_string` | `crates/forge-services/src/lib.rs` |
| Large downloads that outlive the app | The `transfers` service (background `URLSession`) | `crates/forge-services/src/transfers.rs` |

Swift barely calls web APIs directly. Apart from Device Lab's own model and package downloads, every app reaches the web from Rust through the `http` service, and pictures go through `ForgeWebData`. So there is no Swift-side duplication to fix, and the plumbing belongs in Rust, where iOS and Android both get it.

## How each app calls APIs today

| App | Services | User-Agent | Retries | Caching | Status handling |
|---|---|---|---|---|---|
| Weather | Open-Meteo forecast, air quality, pollen, geocoding | Set on the ureq fallback only (`app.rs:313`) | None | Its own report cache: last good forecast per place, 24 h, used when a fetch fails (`app.rs:440`) | `status_error` per source |
| Pocket Library | Open Library search and covers | Fallback and per request (`catalogue.rs:95`) | None; 429 and 5xx become "try later" | None | `check_status` (`catalogue.rs:109`) |
| Soundscape | Jamendo, Internet Archive, MusicBrainz, the take server | Fallback only, except MusicBrainz per request (`musicbrainz.rs:148`) | MusicBrainz only: up to 4 tries on 503, honouring `Retry-After`, capped at 3 s (`musicbrainz.rs:155`) | `JamendoAnswers`, a caching `HttpClient` in front of the app's client: 6 h fresh, 30 days kept for offline, client ID left out of the key (`jamendo_cache.rs`, 324 lines) | "X answered N" written four times (`jamendo.rs:390`, `archive.rs:143`, `listen.rs:545`, `musicbrainz.rs:179`) |
| Commonplace | Any feed, full-text pages | Per request (`lib.rs:1239`, `full.rs:336`) | None (refresh again later) | Conditional GET: `ETag` / `Last-Modified` kept per feed in SQLite, `304` means unchanged (`lib.rs:1229`) | Per fetch |
| Relay (chat path) | The chat server | Fallback only (`chat/mod.rs:1522`) | Outbox and reconnect on `Backoff` with jitter | None | Per call |
| Soundscape, planned (#460) | Radio Browser, Podcast Index, podcast feeds | | Radio Browser needs mirror failover | Station lists would suit the Jamendo-style cache; feeds the Commonplace-style conditional GET | |

## What is repeated, and what isn't

1. **The User-Agent doesn't reach the iPhone. This is a bug.** Weather, Soundscape (Jamendo, Internet Archive, the take server) and Relay's chat path set their User-Agent with `UreqClient::with_user_agent`, which is the fallback client. On iPhone, requests go through `URLSession` instead (`prefer_installed`), and neither `HostClient` nor `ForgeHTTPTransport` adds a User-Agent, so these requests go out with iOS's default (`<App>/<build> CFNetwork/… Darwin/…`), and on Android with `HttpURLConnection`'s. Only MusicBrainz, Commonplace and Pocket Library set it on each request, so theirs survives. The docs example in `native-services.md` shows the same pattern. Found by reading the code; not yet seen on a phone. It matters most for services that ask clients to identify themselves (MusicBrainz already does it right; Radio Browser asks for it too).
2. **Status classification is written by hand in four apps.** Weather, Pocket Library, Soundscape (four places) and Commonplace each turn a status into "worked", "try later" (429, 5xx), "not found" or "refused". Each app's words are its own and should stay so; the part that repeats is the classification and reading `Retry-After`. That is a few lines each, so it is worth a small helper only alongside step 3, not on its own.
3. **Retrying a busy server: one use.** Only MusicBrainz retries, and it does it well. Pocket Library gives up on Open Library's 429, which is fine for a search the person can repeat. The foundation review already lists this as N2 (`RetryPolicy` beside `Backoff`: attempt limit, `Retry-After`, which errors are worth retrying), deferred until a second use. Radio Browser's mirrors (L1 Live Radio in #460) would be that second use.
4. **Caching answers for offline: one generic use.** `JamendoAnswers` is already shaped like a framework piece (it wraps any `HttpClient`), but it is the only one. Weather's cache is about forecasts, not HTTP answers, and Commonplace's conditional GET is a different mechanism. Radio Browser station lists or podcast search would be the second use.
5. **Conditional GET (`ETag`, `Last-Modified`, `304`): one use,** Commonplace. Soundscape podcasts will need exactly this. `docs/app-plan.md` already gates feed handling: build it in Soundscape first, extract only after both apps pass on the phone. The validators travel with that extraction.
6. **Rate limiting: no use.** Every request today is one the person asked for, one at a time, so MusicBrainz's one-a-second rule holds by design. A limiter would only be needed for batch work, such as identifying a whole library against MusicBrainz in the background. Not planned.
7. **Keys: nothing missing.** Jamendo's client ID is a public identifier, kept in Soundscape's settings and left out of cache keys; that is fine. Podcast Index's secret goes in the Keychain through the existing secure storage, as #460 already says.
8. **A generic API client, endpoint descriptions or code generation (OpenAPI and the like): no.** Each service client is 100 to 400 lines and almost all of it is that service's quirks (Jamendo's empty answers, MusicBrainz ranking, Open Library's shapes). A shared abstraction would hide those without saving code.

One smaller finding: Soundscape's Internet Archive client accepts answers up to 16 MiB (`archive.rs:33`), but Soundscape's client stops bodies at 4 MiB (`lib.rs:112`), so a large Archive answer fails as a transport error before that check is reached. It belongs in a Soundscape PR, not this one.

## Proposal

### Step 1, now: one place for an app's network identity (a fix)

Give the `http` service the app's User-Agent (and the body limit it already takes) once, and have `HostClient` add the User-Agent to any request that doesn't set its own, so the same value goes out from ureq, `URLSession` and Android's transport. Roughly:

```rust
let http = native::http::HttpService::prefer_installed(
    Arc::new(UreqClient::with_user_agent(USER_AGENT)),
    |client| client.user_agent(USER_AGENT).max_body_bytes(MAX_BYTES),
);
```

The exact shape is for the PR; one option is a small settings value passed once instead of setting both clients. Then:

- Move Weather, Soundscape, Pocket Library, Commonplace and Relay's chat path onto it in the same PR (rule 12), and drop the per-request headers that become redundant. MusicBrainz keeps its own, since its User-Agent format is MusicBrainz's rule.
- Add a parity rule (`user-agent`) so an app that uses the `http` service names its User-Agent there.
- Fix the example in `native-services.md`.
- Test: a `FakeHost` test that a request without a User-Agent gets the app's, and one with its own keeps it. Phone check (short): one Weather refresh and one Jamendo search still work in Device Lab. The User-Agent itself can be confirmed from the take server's log on the laptop, which Soundscape already talks to.

Size: a few dozen lines in `forge-services` plus a line or two per app. Rust only, so both iOS and Android get it.

### Step 2, with Live Radio (L1 in #460): retry policy

When Radio Browser is built, add N2's `RetryPolicy` to `forge-net` beside `Backoff`, with the status classification from point 2: attempts, a deadline, `Retry-After` for 429 and 503 (capped), cancel-aware waits, and `NetError`s classified as worth retrying or not. Move MusicBrainz onto it in the same PR, so there are two users from day one. Mirror failover stays Radio Browser's own code in Soundscape.

### Step 3, after Live Radio passes on the phone: cached answers, if a second user appears

If Radio Browser (or podcast search) wants offline answers, move `JamendoAnswers` into `forge-net` as a generic caching client (fresh time, keep time, a key function that can leave secrets out, only successful non-empty answers kept) and put both on it. If nothing else needs it by then, it stays in Soundscape.

### Not proposed

A new crate, a separate "API handler" module, a rate limiter, a Swift-side API layer, new key storage, or generated API clients. Conditional GET waits for the feed extraction already planned in `docs/app-plan.md`.

## Decisions for Reuben

1. Do step 1 now as a small framework PR (recommended), or leave it until the next Soundscape work.
2. Agree that steps 2 and 3 wait for Live Radio, as gated above.

Related: [foundation review](../foundation-review.md) (N1, N2, F4), [native services: http](../native-services.md#http-through-urlsession-http), [framework candidates](framework-candidates.md), [app plan](../app-plan.md), Soundscape's podcasts and radio proposal (#460).
