// Copied into every app's Generated/ForgeFFI by scripts/generate-bindings.sh.
// Do not edit the copy; the source of truth is
// crates/forge-services/swift/helpers/ForgeWeb.swift.

// WebKit and SafariServices views, iOS only. Elsewhere (Linux, where
// `swift test` runs an app's models, and macOS host tests) this file is empty.
#if os(iOS) && canImport(WebKit) && canImport(SafariServices)
import Foundation
import Observation
import SafariServices
import SwiftUI
import WebKit

// MARK: - What to show, and what it may do

/// What a web view shows: a web address, HTML the app already has, or a
/// page bundled with the app.
public enum ForgeWebSource: Sendable, Equatable {
    /// A page on the web.
    case url(URL)
    /// HTML the app has in memory (an article body, rich text from a feed).
    /// `baseURL` resolves relative links; nil means none.
    case html(String, baseURL: URL? = nil)
    /// An HTML file in the app (a bundled help page). It may read the other
    /// files in its own folder (images, styles), and nothing else.
    case file(URL)

    var displayURL: URL? {
        switch self {
        case .url(let url), .file(let url): url
        case .html(_, let base): base
        }
    }
}

/// What a `ForgeWebView` may do. The defaults are restrictive, and each
/// power is an explicit opt-in (docs/native-capability-roadmap.md, "Web
/// view"). It is a constrained web view, not a sandbox: an allowed page is
/// still a web page running its own code.
public struct ForgeWebPolicy: Sendable, Equatable {
    /// Hosts whose pages open in the view (a host also allows its
    /// subdomains). Any other web link opens in Safari. This controls where
    /// the view goes, not what an allowed page loads; see `remoteContent`.
    public var allowedHosts: Set<String>
    /// Non-web schemes (`mailto`, `tel`, another app's scheme) handed to the
    /// system when the person taps a link to them. Any other scheme is refused.
    public var externalSchemes: Set<String>
    /// Page scripts run. Turn off for content the app only displays.
    public var javaScript: Bool
    /// Cookies and site storage survive a relaunch. Off by default, so
    /// nothing is kept; turn on when a sign-in has to stick.
    public var persistentWebsiteData: Bool
    /// Pages may load scripts, images, styles and frames from the network.
    /// Off, nothing at an http or https address loads at all (a content rule
    /// list blocks it), which suits HTML the app supplies.
    public var remoteContent: Bool

    public init(
        allowedHosts: Set<String>,
        externalSchemes: Set<String> = [],
        javaScript: Bool = true,
        persistentWebsiteData: Bool = false,
        remoteContent: Bool = true
    ) {
        self.allowedHosts = Set(allowedHosts.map { $0.lowercased() })
        self.externalSchemes = Set(externalSchemes.map { $0.lowercased() })
        self.javaScript = javaScript
        self.persistentWebsiteData = persistentWebsiteData
        self.remoteContent = remoteContent
    }

    /// Only the host of `url` (and its subdomains) opens in place: a help
    /// page, a privacy policy, an article's original page.
    public static func only(_ url: URL) -> ForgeWebPolicy {
        ForgeWebPolicy(allowedHosts: Set([url.host].compactMap { $0 }))
    }

    /// For HTML the app supplies: no scripts, nothing from the network, and
    /// every link opens in Safari (or the system, for `mailto` and `tel`).
    public static let offline = ForgeWebPolicy(
        allowedHosts: [], externalSchemes: ["mailto", "tel"], javaScript: false, remoteContent: false)

    /// Whether a top-level navigation to `url` stays in the view.
    public func allows(_ url: URL) -> Bool {
        guard let scheme = url.scheme?.lowercased(), scheme == "https" || scheme == "http",
            let host = url.host?.lowercased()
        else { return false }
        return allowedHosts.contains { host == $0 || host.hasSuffix("." + $0) }
    }
}

/// Why a page could not be shown, as a value the screen can present.
public struct ForgeWebError: Error, Equatable, Sendable {
    public enum Kind: Sendable, Equatable {
        /// The page failed to load (offline, not found, a TLS failure).
        case loadFailed
        /// The link pointed at a file to download; downloads are off.
        case downloadRefused
        /// The page's web process kept crashing.
        case contentCrashed
    }

    public let kind: Kind
    /// A sentence for the person, from the system where it has one.
    public let message: String
    public let url: URL?
}

// MARK: - The web view

/// The navigation state of one `ForgeWebView`, for the screen around it:
/// title, progress, back and forward, and the error to show. Create one per
/// web view and keep it in `@State`.
@MainActor
@Observable
public final class ForgeWebState {
    public internal(set) var url: URL?
    public internal(set) var title: String?
    /// 0 to 1 while a page loads.
    public internal(set) var progress: Double = 0
    public internal(set) var isLoading = false
    public internal(set) var canGoBack = false
    public internal(set) var canGoForward = false
    /// Set when a page fails; cleared when the next load starts.
    public internal(set) var error: ForgeWebError?

    @ObservationIgnored weak var webView: WKWebView?
    @ObservationIgnored var source: ForgeWebSource?

    public init() {}

    public func goBack() { webView?.goBack() }
    public func goForward() { webView?.goForward() }
    public func stop() { webView?.stopLoading() }

    /// Reloads the page, or loads the source again after a failure.
    public func reload() {
        guard let webView else { return }
        error = nil
        if webView.url != nil, webView.url?.absoluteString != "about:blank" {
            webView.reload()
        } else if let source {
            ForgeWebView.Coordinator.start(source, in: webView)
        }
    }
}

/// A web page shown inside the app, on WebKit, under a `ForgeWebPolicy`.
/// Most screens want `ForgeWebScreen` or `.forgeWebSheet`, which add the
/// progress bar, back button and error view around it.
///
/// ```swift
/// @State private var web = ForgeWebState()
/// ForgeWebView(.url(article), policy: .only(article), state: web)
/// ```
///
/// Links to hosts outside the policy open in Safari, other schemes are
/// refused unless listed, downloads are refused, page script cannot reach
/// Swift or Rust (there is no message bridge), and a crashed web process
/// reloads the page once before reporting `contentCrashed`. The web view is
/// released when the view leaves the screen.
public struct ForgeWebView: UIViewRepresentable {
    private let source: ForgeWebSource
    private let policy: ForgeWebPolicy
    private let state: ForgeWebState

    public init(_ source: ForgeWebSource, policy: ForgeWebPolicy, state: ForgeWebState) {
        self.source = source
        self.policy = policy
        self.state = state
    }

    public func makeCoordinator() -> Coordinator {
        Coordinator(policy: policy, state: state)
    }

    public func makeUIView(context: Context) -> WKWebView {
        let configuration = WKWebViewConfiguration()
        configuration.websiteDataStore =
            policy.persistentWebsiteData ? .default() : .nonPersistent()
        configuration.defaultWebpagePreferences.allowsContentJavaScript = policy.javaScript
        configuration.preferences.javaScriptCanOpenWindowsAutomatically = false
        let webView = WKWebView(frame: .zero, configuration: configuration)
        webView.navigationDelegate = context.coordinator
        webView.uiDelegate = context.coordinator
        webView.allowsBackForwardNavigationGestures = true
        context.coordinator.attach(webView)
        context.coordinator.load(source, in: webView)
        return webView
    }

    public func updateUIView(_ webView: WKWebView, context: Context) {
        context.coordinator.policy = policy
        if context.coordinator.requested != source {
            context.coordinator.load(source, in: webView)
        }
    }

    public static func dismantleUIView(_ webView: WKWebView, coordinator: Coordinator) {
        webView.stopLoading()
        coordinator.detach(webView)
    }

    @MainActor
    public final class Coordinator: NSObject, WKNavigationDelegate, WKUIDelegate {
        var policy: ForgeWebPolicy
        let state: ForgeWebState
        var requested: ForgeWebSource?
        private var observations: [NSKeyValueObservation] = []
        private var crashReloads = 0

        /// Blocks every http and https load, for `remoteContent: false`.
        private static let blockRemote = """
            [{"trigger": {"url-filter": "^https?://"}, "action": {"type": "block"}}]
            """

        init(policy: ForgeWebPolicy, state: ForgeWebState) {
            self.policy = policy
            self.state = state
        }

        func attach(_ webView: WKWebView) {
            state.webView = webView
            observations = [
                webView.observe(\.title) { [weak self] view, _ in
                    MainActor.assumeIsolated { self?.state.title = view.title }
                },
                webView.observe(\.url) { [weak self] view, _ in
                    MainActor.assumeIsolated {
                        if let url = view.url, url.scheme != "about" { self?.state.url = url }
                    }
                },
                webView.observe(\.estimatedProgress) { [weak self] view, _ in
                    MainActor.assumeIsolated { self?.state.progress = view.estimatedProgress }
                },
                webView.observe(\.isLoading) { [weak self] view, _ in
                    MainActor.assumeIsolated { self?.state.isLoading = view.isLoading }
                },
                webView.observe(\.canGoBack) { [weak self] view, _ in
                    MainActor.assumeIsolated { self?.state.canGoBack = view.canGoBack }
                },
                webView.observe(\.canGoForward) { [weak self] view, _ in
                    MainActor.assumeIsolated { self?.state.canGoForward = view.canGoForward }
                },
            ]
        }

        func detach(_ webView: WKWebView) {
            observations.forEach { $0.invalidate() }
            observations = []
            webView.navigationDelegate = nil
            webView.uiDelegate = nil
            if state.webView === webView { state.webView = nil }
        }

        func load(_ source: ForgeWebSource, in webView: WKWebView) {
            requested = source
            state.source = source
            state.url = source.displayURL
            state.error = nil
            crashReloads = 0
            guard !policy.remoteContent else {
                Self.start(source, in: webView)
                return
            }
            // The rule list must be in place before anything loads.
            WKContentRuleListStore.default().compileContentRuleList(
                forIdentifier: "forge.block-remote", encodedContentRuleList: Self.blockRemote
            ) { [weak webView] list, _ in
                MainActor.assumeIsolated {
                    guard let webView else { return }
                    if let list { webView.configuration.userContentController.add(list) }
                    // If the rules failed to compile, show nothing rather
                    // than load the network the app asked to keep out.
                    if list != nil { Self.start(source, in: webView) }
                }
            }
        }

        static func start(_ source: ForgeWebSource, in webView: WKWebView) {
            switch source {
            case .url(let url): webView.load(URLRequest(url: url))
            case .html(let html, let base): webView.loadHTMLString(html, baseURL: base)
            case .file(let url): webView.loadFileURL(url, allowingReadAccessTo: url.deletingLastPathComponent())
            }
        }

        /// A bundled file may open the other files in its own folder.
        private func allowsFile(_ url: URL) -> Bool {
            guard case .file(let root) = requested, url.isFileURL else { return false }
            let folder = root.deletingLastPathComponent().standardizedFileURL.path
            return url.standardizedFileURL.path.hasPrefix(folder + "/")
        }

        // MARK: Where a link may go

        public func webView(
            _ webView: WKWebView, decidePolicyFor action: WKNavigationAction,
            decisionHandler: @escaping (WKNavigationActionPolicy) -> Void
        ) {
            let url = action.request.url
            let scheme = url?.scheme?.lowercased() ?? ""
            // Subresources and frames inside an allowed page load as in Safari.
            if let frame = action.targetFrame, !frame.isMainFrame {
                decisionHandler(["http", "https", "about", "data", "blob"].contains(scheme) ? .allow : .cancel)
                return
            }
            guard let url else {
                decisionHandler(.cancel)
                return
            }
            switch scheme {
            case "http", "https":
                if policy.allows(url) {
                    if action.targetFrame == nil {
                        // target="_blank": open it here rather than in a new window.
                        decisionHandler(.cancel)
                        webView.load(action.request)
                    } else {
                        decisionHandler(.allow)
                    }
                } else {
                    decisionHandler(.cancel)
                    // Only a link the person tapped leaves the app; a page
                    // can't push someone to Safari by itself.
                    if action.navigationType == .linkActivated || action.targetFrame == nil {
                        UIApplication.shared.open(url)
                    }
                }
            case "about":
                decisionHandler(.allow)
            case "file":
                decisionHandler(allowsFile(url) ? .allow : .cancel)
            default:
                decisionHandler(.cancel)
                if policy.externalSchemes.contains(scheme), action.navigationType == .linkActivated {
                    UIApplication.shared.open(url)
                }
            }
        }

        public func webView(
            _ webView: WKWebView, decidePolicyFor response: WKNavigationResponse,
            decisionHandler: @escaping (WKNavigationResponsePolicy) -> Void
        ) {
            if response.isForMainFrame, !response.canShowMIMEType {
                decisionHandler(.cancel)
                state.error = ForgeWebError(
                    kind: .downloadRefused,
                    message: "This link is a file to download, which this screen can't save.",
                    url: response.response.url)
                return
            }
            decisionHandler(.allow)
        }

        /// `window.open` and target="_blank" without a navigation: never a
        /// second web view.
        public func webView(
            _ webView: WKWebView, createWebViewWith configuration: WKWebViewConfiguration,
            for action: WKNavigationAction, windowFeatures: WKWindowFeatures
        ) -> WKWebView? {
            if let url = action.request.url, policy.allows(url) {
                webView.load(action.request)
            }
            return nil
        }

        // MARK: Loading and failures

        public func webView(_ webView: WKWebView, didStartProvisionalNavigation navigation: WKNavigation!) {
            state.error = nil
        }

        public func webView(_ webView: WKWebView, didFinish navigation: WKNavigation!) {
            crashReloads = 0
        }

        public func webView(
            _ webView: WKWebView, didFailProvisionalNavigation navigation: WKNavigation!, withError error: Error
        ) {
            fail(error, webView)
        }

        public func webView(_ webView: WKWebView, didFail navigation: WKNavigation!, withError error: Error) {
            fail(error, webView)
        }

        public func webViewWebContentProcessDidTerminate(_ webView: WKWebView) {
            if crashReloads < 1 {
                crashReloads += 1
                state.reload()
            } else {
                state.error = ForgeWebError(
                    kind: .contentCrashed, message: "This page stopped working.", url: state.url)
            }
        }

        private func fail(_ error: Error, _ webView: WKWebView) {
            let nsError = error as NSError
            // A load we cancelled ourselves (a policy decision or a new load)
            // is not a failure the person needs to see.
            if nsError.domain == NSURLErrorDomain, nsError.code == NSURLErrorCancelled { return }
            // WebKit's "frame load interrupted", sent when a policy decision cancels a load.
            if nsError.domain == "WebKitErrorDomain", nsError.code == 102 { return }
            if state.error?.kind == .downloadRefused { return }
            let failed = (nsError.userInfo[NSURLErrorFailingURLErrorKey] as? URL) ?? state.url
            state.error = ForgeWebError(kind: .loadFailed, message: nsError.localizedDescription, url: failed)
        }
    }
}

// MARK: - The ready-made screen

/// A page to show in a sheet: what to load, its policy, and a title for the
/// moment before the page names itself.
public struct ForgeWebPage: Identifiable, Sendable, Equatable {
    public let id = UUID()
    public var source: ForgeWebSource
    public var policy: ForgeWebPolicy
    public var title: String?

    public init(_ source: ForgeWebSource, policy: ForgeWebPolicy, title: String? = nil) {
        self.source = source
        self.policy = policy
        self.title = title
    }

    /// A web page whose own site opens in place and every other link in
    /// Safari: a help page, a privacy policy, terms, an article's original.
    public static func link(_ url: URL, title: String? = nil) -> ForgeWebPage {
        ForgeWebPage(.url(url), policy: .only(url), title: title)
    }

    /// HTML the app has, shown with no scripts and nothing from the network.
    public static func html(_ html: String, title: String? = nil) -> ForgeWebPage {
        ForgeWebPage(.html(html), policy: .offline, title: title)
    }

    /// An HTML file bundled with the app (`Bundle.main.url(forResource:...)`),
    /// shown like `html`.
    public static func bundled(_ file: URL, title: String? = nil) -> ForgeWebPage {
        ForgeWebPage(.file(file), policy: .offline, title: title)
    }
}

/// A full web screen: the page, a progress bar while it loads, Back,
/// Open in Safari (for web pages) and an error view with Try Again. Put it in
/// a `NavigationStack` or push it; `.forgeWebSheet` does that with a Done
/// button. Identifiers: `web.page`, `web.back`, `web.openInSafari`,
/// `web.retry`.
public struct ForgeWebScreen: View {
    private let page: ForgeWebPage
    @State private var web = ForgeWebState()

    public init(_ page: ForgeWebPage) {
        self.page = page
    }

    public var body: some View {
        Group {
            if let error = web.error {
                ContentUnavailableView {
                    Label("Can't Show Page", systemImage: "exclamationmark.triangle")
                } description: {
                    Text(error.message)
                } actions: {
                    if error.kind != .downloadRefused {
                        Button("Try Again") { web.reload() }
                            .accessibilityIdentifier("web.retry")
                    }
                }
            } else {
                ForgeWebView(page.source, policy: page.policy, state: web)
                    .accessibilityIdentifier("web.page")
                    .overlay(alignment: .top) {
                        if web.isLoading {
                            ProgressView(value: web.progress)
                                .accessibilityLabel("Loading page")
                        }
                    }
            }
        }
        .navigationTitle(web.title?.isEmpty == false ? web.title! : (page.title ?? ""))
        .navigationBarTitleDisplayMode(.inline)
        .toolbar {
            ToolbarItemGroup(placement: .primaryAction) {
                if web.canGoBack {
                    Button("Back", systemImage: "chevron.backward") { web.goBack() }
                        .accessibilityIdentifier("web.back")
                }
                if let url = web.url, ["http", "https"].contains(url.scheme?.lowercased() ?? "") {
                    Link(destination: url) {
                        Label("Open in Safari", systemImage: "safari")
                    }
                    .accessibilityIdentifier("web.openInSafari")
                }
            }
        }
    }
}

extension View {
    /// Shows `page` in a sheet with `ForgeWebScreen` and a Done button
    /// (`web.done`). Set it to show a page; it goes back to nil on Done.
    ///
    /// ```swift
    /// @State private var page: ForgeWebPage?
    /// Button("Privacy Policy") { page = .link(privacyURL, title: "Privacy") }
    /// ...
    /// .forgeWebSheet($page)
    /// ```
    public func forgeWebSheet(_ page: Binding<ForgeWebPage?>) -> some View {
        sheet(item: page) { page in
            ForgeWebSheet(page: page)
        }
    }

    /// Opens `url` in Safari inside the app (`ForgeSafariView`), full screen.
    /// Goes back to nil when the person closes it. Only http and https.
    public func forgeSafari(_ url: Binding<URL?>) -> some View {
        fullScreenCover(item: Binding(
            get: { url.wrappedValue.map(ForgeSafariItem.init) },
            set: { url.wrappedValue = $0?.url }
        )) { item in
            ForgeSafariView(url: item.url).ignoresSafeArea()
        }
    }
}

private struct ForgeSafariItem: Identifiable {
    let url: URL
    var id: URL { url }
}

private struct ForgeWebSheet: View {
    let page: ForgeWebPage
    @Environment(\.dismiss) private var dismiss

    var body: some View {
        NavigationStack {
            ForgeWebScreen(page)
                .toolbar {
                    ToolbarItem(placement: .cancellationAction) {
                        Button("Done") { dismiss() }
                            .accessibilityIdentifier("web.done")
                    }
                }
        }
    }
}

// MARK: - Safari

/// Safari inside the app (`SFSafariViewController`): the person's own
/// Safari, with its Reader, sign-ins and content blockers, and nothing the
/// app can see or script. The simplest way to show a web page the app does
/// not control; use `ForgeWebScreen` when the page belongs to the app's
/// flow. `.forgeSafari($url)` presents it.
public struct ForgeSafariView: UIViewControllerRepresentable {
    private let url: URL
    private let readerMode: Bool

    /// `url` must be http or https (Safari refuses anything else). `readerMode`
    /// opens straight into Reader where Safari can offer it.
    public init(url: URL, readerMode: Bool = false) {
        self.url = url
        self.readerMode = readerMode
    }

    public func makeUIViewController(context: Context) -> SFSafariViewController {
        let configuration = SFSafariViewController.Configuration()
        configuration.entersReaderIfAvailable = readerMode
        return SFSafariViewController(url: url, configuration: configuration)
    }

    public func updateUIViewController(_ controller: SFSafariViewController, context: Context) {}
}
#endif
