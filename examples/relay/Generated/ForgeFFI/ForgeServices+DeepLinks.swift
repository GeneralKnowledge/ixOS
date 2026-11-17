// Forge native services: deep links. Source of truth: crates/forge-services/swift/.

import Foundation
import SwiftUI

/// Where the URLs the app is opened with arrive: custom schemes (forge.toml
/// `[ios] url_schemes`), universal links, and taps on notifications that
/// carry a `deepLink`. Each is parsed by Rust (`parseDeepLink`) and handed
/// to the views that called `onForgeDeepLink`. Links that arrive while no
/// handler is on screen (a cold launch from a link) wait for the first one.
@MainActor
public final class ForgeDeepLinks: ObservableObject {
    public static let shared = ForgeDeepLinks()

    /// Kept while no handler is registered; the oldest go first.
    public static let maxPending = 16

    /// The last link opened.
    @Published public private(set) var latest: DeepLink?

    private var handlers: [UUID: (DeepLink) -> Void] = [:]
    private var pending: [DeepLink] = []

    public init() {}

    /// Parses `url` and delivers it. Returns false (and logs) if Rust
    /// rejects it.
    @discardableResult
    public func open(_ url: URL) -> Bool {
        let link: DeepLink
        do {
            link = try parseDeepLink(url: url.absoluteString)
        } catch {
            print("[forge] ignoring deep link \(url.absoluteString): \(error)")
            return false
        }
        latest = link
        if handlers.isEmpty {
            pending.append(link)
            if pending.count > Self.maxPending { pending.removeFirst(pending.count - Self.maxPending) }
        } else {
            for handler in handlers.values { handler(link) }
        }
        return true
    }

    /// Registers `handler` and gives it any links that were waiting. Keep the
    /// token and pass it to `removeHandler` when done.
    public func addHandler(_ handler: @escaping (DeepLink) -> Void) -> UUID {
        let token = UUID()
        handlers[token] = handler
        let waiting = pending
        pending.removeAll()
        for link in waiting { handler(link) }
        return token
    }

    public func removeHandler(_ token: UUID) {
        handlers[token] = nil
    }
}

@MainActor
private struct ForgeDeepLinkHandler: ViewModifier {
    let perform: (DeepLink) -> Void
    @State private var token: UUID?

    func body(content: Content) -> some View {
        content
            .onAppear {
                if token == nil { token = ForgeDeepLinks.shared.addHandler(perform) }
            }
            .onDisappear {
                if let token { ForgeDeepLinks.shared.removeHandler(token) }
                token = nil
            }
    }
}

extension View {
    /// Feeds the URLs this app is opened with (custom schemes and universal
    /// links) into `ForgeDeepLinks`. Attach once, to the root view.
    @MainActor
    public func forgeOpenURLs() -> some View {
        onOpenURL { url in ForgeDeepLinks.shared.open(url) }
    }

    /// Calls `perform` with each deep link while this view is on screen.
    @MainActor
    public func onForgeDeepLink(perform: @escaping (DeepLink) -> Void) -> some View {
        modifier(ForgeDeepLinkHandler(perform: perform))
    }
}
