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

    var body: some View {
        #if canImport(WebKit) && os(iOS)
        RelaySiteWebView(endpointID: device.endpointId) { [core = model.core, id = device.endpointId] request in
            try await core.fetchSite(endpointId: id, request: request)
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
            let answer: SiteResponse
            do {
                answer = try await fetch(request)
            } catch {
                answer = Self.problem(error)
            }
            guard let self, self.running.removeValue(forKey: id) != nil else { return }
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
/// outside Relay (Safari, Mail), so the view never shows the internet.
struct RelaySiteWebView: UIViewRepresentable {
    let endpointID: String
    let fetch: RelaySiteSchemeHandler.Fetch

    func makeCoordinator() -> Coordinator { Coordinator() }

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
        if let home = RelaySiteAddress.home(endpointID) {
            view.load(URLRequest(url: home))
        }
        return view
    }

    func updateUIView(_ view: WKWebView, context: Context) {}

    @MainActor
    final class Coordinator: NSObject, WKNavigationDelegate {
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
