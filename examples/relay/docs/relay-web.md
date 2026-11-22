# N6: Sites only your devices can open

Status: design 2026-11-21, asked for by Reuben ("host a website via the
relay network … only accessible via relay, nothing on the normal
internet", and hosting on the phone "only when the app is open"). Parked
until after Relay 1.0, then started on 2026-11-22 as N6, the first
capability of [N5](relay-capabilities.md) ("start building N5 and N6 (a
small site)"). Built in the order below; nothing here changes delivery,
storage or pairing.

## What it is

A device can **host a site**: a folder of files, or a web server that
listens only on that device's localhost. Its trusted devices that are
allowed to can **open it**: in a browser on a computer, inside Relay on
the iPhone. The site never listens on a public port, so it isn't on the
normal internet at all: the only way in is Relay, and only for devices
the host has trusted and granted. Away from home the bytes go through our
relay (`relay.avedeus.ovh`), which forwards encrypted QUIC and can't read
them.

On the iPhone a hosted site exists **only while Relay is open**: iOS
suspends Relay in the background, and the site stops with it. That is the
intended behaviour, not a limitation to work around.

## The protocol: `relay/web/1`

A new ALPN on the device's one iroh endpoint, beside `relay/device/1`.

- **Who may connect.** The listener accepts a connection only from a
  trusted device granted **Sites** (below), and only while a site is
  shared; anything else is closed before a byte is read, as
  `relay/device/1` does. An open connection is checked every 250 ms and
  closed when that stops being true.
- **One HTTP/1.1 exchange per stream.** The viewer opens a bidirectional
  QUIC stream, writes one request, and reads one response, which ends
  when the host closes the stream (requests go with `Connection: close`).
  Many streams share one connection, so a page's images and scripts load
  in parallel. HTTP/1.1 is used as is on the stream: no new framing to
  design, and a browser's request passes through almost untouched.
- **What the host does with a request.** It answers from the shared
  folder, or forwards it to the configured `127.0.0.1:<port>` and streams
  the answer back. It rewrites nothing in the body.
- **Limits.** One target per site, always loopback (a site can't be used
  as a proxy to the host's network or the internet); request headers at
  most 64 KB; bodies streamed, not buffered; idle streams closed after 60 s;
  at most 32 streams per connection.
- **Not at first:** websockets, server-sent events, HTTP/2 to the target,
  uploads larger than a request body. They can follow once the basic path
  is proven.

## Permissions

Trust establishes identity, not permission (the review's rule). Hosting
uses N5's grant **Sites** ("Can Open My Site"), off by default, turned on
per device by the person, and cleared with the device's revocation. A
device without it is refused on every request (403), and its connection
is closed. Only a device with the grant sees "sites" among the offers in
the host's status answer.

The host chooses what to share: `relay listen --site <folder>`, or
`relay site <folder | 127.0.0.1:port>` while `relay listen` runs, on a
computer; **Share as a Site** on the phone. Nothing is shared until the
person does that, and `relay site --stop` (or stopping Relay) ends it.
One site per device for now.

## Opening a site

- **Computer:** `relay web <device> [--port N]` (in the listener when
  one runs, otherwise in its own process until Ctrl-C) listens on
  `127.0.0.1:N` (never on other interfaces) and tunnels each browser
  connection's requests over `relay/web/1`. Any browser on that computer
  then opens `http://localhost:N`.
- **iPhone:** a web view inside Relay (`WKWebView`) whose own URL scheme,
  `relay-web://<device>/…`, is answered by a `WKURLSchemeHandler` that
  asks Rust to fetch each request over iroh (`fetch_site`, which returns
  the whole response, up to 64 MB, chunked bodies joined). Safari can't
  open it: Safari
  doesn't speak iroh, and a localhost proxy for Safari would stop whenever
  iOS suspends Relay.

## Hosting on the phone

**Share as a Site** picks a folder (or a single HTML file) with the system
picker. While Relay is on screen, granted devices can open it; Relay shows
that it is sharing and who opened what. When Relay leaves the screen the
endpoint is suspended and the site is gone until Relay is opened again.

## Security notes

- Only loopback targets, so a site is never an open proxy.
- A folder site serves files under its folder only: paths are normalised
  and anything that escapes (`..`, symlinks out) is refused.
- The host's own name and addresses aren't exposed to the site; the
  viewer's endpoint id is passed to a proxied server as
  `X-Relay-Device: <endpoint id>` so a server can tell visitors apart, and
  any such header from the viewer is removed first.
- No logs of paths or content (rule 5); counts only.

## Order of work

1. This document.
2. Linux (with N5's grants, one PR): `relay/web/1`, `relay site`,
   `relay listen --site` and `relay web`, `fetch_site` for the iPhone, and
   tests over localhost (`tests/sites.rs`: a folder site, a proxied server
   told who asked, a device without the grant, the grant taken away, a
   revoked device, a stopped site, a path that escapes the folder, a
   browser through a local port). Then a real check: a page on the laptop
   reachable only over Relay; on Hetzner only with Reuben's go-ahead.
3. iPhone: the "Can Open My Site" switch and the in-app viewer, then
   Share as a Site, compiled with the iOS check; the phone test batched
   with the others.
