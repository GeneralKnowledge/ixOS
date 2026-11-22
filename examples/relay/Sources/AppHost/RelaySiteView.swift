@preconcurrency import RelayFFI
import ForgeUI
import SwiftUI
#if canImport(WebKit) && os(iOS)
import UIKit
import WebKit
#endif

/// A trusted device's site, opened over Relay only (N6,
/// docs/relay-web.md). The web view's own scheme, `relay-site://`, is
/// answered by Rust, which fetches each request from the device over
/// `relay/web/1`: nothing goes to the internet, and Safari can't open it.
struct RelaySiteView: View {
    let device: Device
    let model: RelayModel
    /// A page is on its way: over Relay that can take a few seconds, so
    /// the screen says so rather than staying blank.
    @State private var loading = true

    var body: some View {
        #if canImport(WebKit) && os(iOS)
        RelaySiteWebView(endpointID: device.endpointId, loading: $loading) {
            [core = model.core, id = device.endpointId] request in
            try await core.fetchSite(endpointId: id, request: request)
        }
        .overlay {
            if loading {
                ProgressView("Opening the site…")
                    .padding()
                    .background(.regularMaterial, in: RoundedRectangle(cornerRadius: 12, style: .continuous))
                    .accessibilityIdentifier("site.loading")
            }
        }
        .ignoresSafeArea(edges: .bottom)
        .navigationTitle("\(device.name)'s Site")
        .navigationBarTitleDisplayMode(.inline)
        #else
        ForgeStateView.empty("Sites Open on iPhone", systemImage: "globe")
        #endif
    }
}

#if canImport(WebKit) && os(iOS)
/// The site's address in the web view: one origin per device, so their
/// cookies and storage stay apart.
enum RelaySiteAddress {
    static let scheme = "relay-site"

    static func home(_ endpointID: String) -> URL? {
        URL(string: "\(scheme)://\(endpointID.lowercased())/")
    }

    /// The request Rust sends for `request`: its method, path with query,
    /// headers and body.
    static func siteRequest(_ request: URLRequest) -> SiteRequest? {
        guard let url = request.url else { return nil }
        var path = url.path(percentEncoded: true)
        if path.isEmpty { path = "/" }
        if let query = url.query(percentEncoded: true) { path += "?" + query }
        let headers = (request.allHTTPHeaderFields ?? [:])
            .map { SiteHeader(name: $0.key, value: $0.value) }
        return SiteRequest(
            method: request.httpMethod ?? "GET",
            path: path,
            headers: headers,
            body: request.httpBody ?? Data()
        )
    }
}

/// Answers `relay-site://` requests by asking Rust for them.
@MainActor
final class RelaySiteSchemeHandler: NSObject, WKURLSchemeHandler {
    typealias Fetch = @Sendable (SiteRequest) async throws -> SiteResponse

    private let fetch: Fetch
    /// The requests still running, so a stopped one is never answered.
    private var running: [ObjectIdentifier: Task<Void, Never>] = [:]

    init(fetch: @escaping Fetch) {
        self.fetch = fetch
    }

    func webView(_ webView: WKWebView, start urlSchemeTask: any WKURLSchemeTask) {
        guard let url = urlSchemeTask.request.url,
              let request = RelaySiteAddress.siteRequest(urlSchemeTask.request) else {
            urlSchemeTask.didFailWithError(URLError(.badURL))
            return
        }
        let id = ObjectIdentifier(urlSchemeTask)
        let fetch = fetch
        running[id] = Task { [weak self] in
            let fetched: SiteResponse
            do {
                fetched = try await fetch(request)
            } catch {
                fetched = Self.problem(error)
            }
            guard let self, self.running.removeValue(forKey: id) != nil else { return }
            let answer = Self.followable(fetched, from: url)
            var fields: [String: String] = [:]
            for header in answer.headers {
                fields[header.name] = fields[header.name].map { "\($0), \(header.value)" } ?? header.value
            }
            guard let response = HTTPURLResponse(
                url: url,
                statusCode: Int(answer.status),
                httpVersion: "HTTP/1.1",
                headerFields: fields
            ) else {
                urlSchemeTask.didFailWithError(URLError(.badServerResponse))
                return
            }
            urlSchemeTask.didReceive(response)
            urlSchemeTask.didReceive(answer.body)
            urlSchemeTask.didFinish()
        }
    }

    func webView(_ webView: WKWebView, stop urlSchemeTask: any WKURLSchemeTask) {
        running.removeValue(forKey: ObjectIdentifier(urlSchemeTask))?.cancel()
    }

    /// A redirect, as a page that goes on to its target. WebKit doesn't
    /// follow a redirect answered through a scheme handler, so the page
    /// does it: a page of the site moves to the new address, and one that
    /// points off the site only links to it. A redirect for an image or a
    /// script isn't followed.
    static func followable(_ answer: SiteResponse, from url: URL) -> SiteResponse {
        guard [301, 302, 303, 307, 308].contains(answer.status),
              let location = answer.headers.first(where: { $0.name.lowercased() == "location" })?.value,
              let target = URL(string: location, relativeTo: url)?.absoluteURL else {
            return answer
        }
        let onSite = target.scheme == RelaySiteAddress.scheme && target.host == url.host
        let escaped = target.absoluteString
            .replacingOccurrences(of: "&", with: "&amp;")
            .replacingOccurrences(of: "\"", with: "&quot;")
            .replacingOccurrences(of: "<", with: "&lt;")
        let page = onSite
            ? """
              <!doctype html><html><head><meta charset="utf-8">
              <meta http-equiv="refresh" content="0;url=\(escaped)"></head>
              <body><a href="\(escaped)">Continue</a></body></html>
              """
            : """
              <!doctype html><html><head><meta charset="utf-8">
              <meta name="viewport" content="width=device-width, initial-scale=1"></head>
              <body><p>This page moved off the site, to <a href="\(escaped)">\(escaped)</a>.</p></body></html>
              """
        return SiteResponse(
            status: 200,
            headers: [SiteHeader(name: "Content-Type", value: "text/html; charset=utf-8")],
            body: Data(page.utf8)
        )
    }

    /// A page saying why the site didn't open.
    private static func problem(_ error: any Error) -> SiteResponse {
        let reason = String(describing: error)
            .replacingOccurrences(of: "&", with: "&amp;")
            .replacingOccurrences(of: "<", with: "&lt;")
        let page = """
        <!doctype html><html><head><meta charset="utf-8">
        <meta name="viewport" content="width=device-width, initial-scale=1">
        <style>body{font:-apple-system-body;margin:24px;color:#333}@media(prefers-color-scheme:dark){body{background:#000;color:#ddd}}</style>
        </head><body><h3>The site didn't open</h3>
        <p>The device may be offline, not sharing a site, or not letting this device open it.</p>
        <p><small>\(reason)</small></p></body></html>
        """
        return SiteResponse(
            status: 502,
            headers: [SiteHeader(name: "Content-Type", value: "text/html; charset=utf-8")],
            body: Data(page.utf8)
        )
    }
}

/// The web view. Pages of the site open in place; any other link opens
/// outside Relay (Safari, Mail), and nothing in a page loads from the
/// internet, so the view never shows or reaches it.
struct RelaySiteWebView: UIViewRepresentable {
    let endpointID: String
    @Binding var loading: Bool
    let fetch: RelaySiteSchemeHandler.Fetch

    func makeCoordinator() -> Coordinator { Coordinator(loading: $loading) }

    func makeUIView(context: Context) -> WKWebView {
        let configuration = WKWebViewConfiguration()
        configuration.setURLSchemeHandler(
            RelaySiteSchemeHandler(fetch: fetch),
            forURLScheme: RelaySiteAddress.scheme
        )
        // Nothing the site stores outlives the view.
        configuration.websiteDataStore = .nonPersistent()
        let view = WKWebView(frame: .zero, configuration: configuration)
        view.navigationDelegate = context.coordinator
        view.allowsBackForwardNavigationGestures = true
        view.accessibilityIdentifier = "site.web"
        let home = RelaySiteAddress.home(endpointID)
        // Images, scripts and the like don't ask the navigation delegate, so
        // a rule list blocks every load from the internet. It must be in
        // place before the page loads; if it fails, nothing loads.
        WKContentRuleListStore.default().compileContentRuleList(
            forIdentifier: "relay.site-only", encodedContentRuleList: Self.internetBlocked
        ) { [weak view, coordinator = context.coordinator] list, _ in
            MainActor.assumeIsolated {
                guard let view, let list, let home else {
                    // Nothing will load: don't leave the spinner up.
                    coordinator.loading.wrappedValue = false
                    return
                }
                view.configuration.userContentController.add(list)
                view.load(URLRequest(url: home))
            }
        }
        return view
    }

    /// Blocks every load from the internet: web pages and web sockets.
    private static let internetBlocked = """
        [{"trigger": {"url-filter": "^https?://"}, "action": {"type": "block"}},
         {"trigger": {"url-filter": "^wss?://"}, "action": {"type": "block"}}]
        """

    func updateUIView(_ view: WKWebView, context: Context) {
        context.coordinator.loading = $loading
    }

    @MainActor
    final class Coordinator: NSObject, WKNavigationDelegate {
        var loading: Binding<Bool>

        init(loading: Binding<Bool>) {
            self.loading = loading
        }

        func webView(_ webView: WKWebView, didStartProvisionalNavigation navigation: WKNavigation!) {
            loading.wrappedValue = true
        }

        func webView(_ webView: WKWebView, didFinish navigation: WKNavigation!) {
            loading.wrappedValue = false
        }

        func webView(_ webView: WKWebView, didFail navigation: WKNavigation!, withError error: any Error) {
            loading.wrappedValue = false
        }

        func webView(
            _ webView: WKWebView, didFailProvisionalNavigation navigation: WKNavigation!, withError error: any Error
        ) {
            loading.wrappedValue = false
        }
        func webView(
            _ webView: WKWebView,
            decidePolicyFor navigationAction: WKNavigationAction,
            decisionHandler: @escaping @MainActor (WKNavigationActionPolicy) -> Void
        ) {
            guard let url = navigationAction.request.url else {
                decisionHandler(.cancel)
                return
            }
            if url.scheme == RelaySiteAddress.scheme
                || url.scheme == "about" || url.scheme == "data" || url.scheme == "blob" {
                decisionHandler(.allow)
                return
            }
            decisionHandler(.cancel)
            if navigationAction.navigationType == .linkActivated {
                UIApplication.shared.open(url)
            }
        }
    }
}
#endif
