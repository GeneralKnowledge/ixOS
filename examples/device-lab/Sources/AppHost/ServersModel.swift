import Foundation
import Observation

/// The Servers screen (Tools tab): whether the computers the hosted apps
/// talk to answer from this phone right now, before a phone check starts.
/// Each server's address is the one its app already uses, so this checks
/// exactly what the app would reach. No UI imports, so it tests on Linux.
@MainActor
@Observable
public final class ServersModel {
    public struct Server: Identifiable, Equatable, Sendable {
        /// Which app uses it ("Relay Chats").
        public let app: String
        /// What it is ("Chat server").
        public let role: String
        /// The address the app uses, or nil when the app has none yet.
        public let address: String?
        /// What to do when there is no address.
        public let setUp: String
        public var id: String { app }
    }

    public enum Check: Equatable, Sendable {
        case notChecked
        case checking
        /// It answered (any HTTP status) after this many milliseconds.
        case answered(milliseconds: Int)
        case unreachable(String)
    }

    public private(set) var servers: [Server]
    public private(set) var checks: [String: Check] = [:]

    /// Asks a URL and returns how long the answer took, or throws.
    public typealias Probe = @Sendable (URL) async throws -> Duration

    @ObservationIgnored private let probe: Probe

    public init(servers: [Server], probe: @escaping Probe = ServersModel.httpProbe) {
        self.servers = servers
        self.probe = probe
    }

    public func check(_ server: Server) -> Check { checks[server.id] ?? .notChecked }

    /// Checks every server with an address at once.
    public func checkAll() async {
        await withTaskGroup(of: Void.self) { group in
            for server in servers where server.address != nil {
                group.addTask { await self.checkOne(server) }
            }
        }
    }

    private func checkOne(_ server: Server) async {
        guard let address = server.address, let url = Self.url(address) else {
            checks[server.id] = .unreachable("Not a web address")
            return
        }
        checks[server.id] = .checking
        do {
            let took = try await probe(url)
            let ms = Int(took.components.seconds * 1000 + took.components.attoseconds / 1_000_000_000_000_000)
            checks[server.id] = .answered(milliseconds: ms)
        } catch {
            checks[server.id] = .unreachable(Self.reason(error))
        }
    }

    /// `100.72.135.124:8642` becomes `http://100.72.135.124:8642`; a full
    /// URL stays as it is.
    nonisolated static func url(_ address: String) -> URL? {
        let trimmed = address.trimmingCharacters(in: .whitespaces)
        guard !trimmed.isEmpty else { return nil }
        let full = trimmed.contains("://") ? trimmed : "http://\(trimmed)"
        guard let url = URL(string: full), url.host != nil else { return nil }
        return url
    }

    /// Whether an address is a Tailscale one (100.64.0.0/10), so the screen
    /// can say that Tailscale must be on.
    public nonisolated static func isTailscale(_ address: String) -> Bool {
        guard let host = url(address)?.host else { return false }
        let parts = host.split(separator: ".").compactMap { Int($0) }
        return parts.count == 4 && parts[0] == 100 && (64...127).contains(parts[1])
    }

    nonisolated static func reason(_ error: Error) -> String {
        guard let error = error as? URLError else { return error.localizedDescription }
        switch error.code {
        case .timedOut: return "No answer within 3 seconds"
        case .cannotConnectToHost: return "Nothing is listening there: is the server running?"
        case .notConnectedToInternet: return "This phone is offline"
        case .cannotFindHost, .dnsLookupFailed: return "Can't find that computer"
        default: return error.localizedDescription
        }
    }

    /// A GET with a 3-second limit; any HTTP answer counts, since the
    /// question is only whether the server is up.
    public static let httpProbe: Probe = { url in
        var request = URLRequest(url: url, timeoutInterval: 3)
        request.cachePolicy = .reloadIgnoringLocalCacheData
        let clock = ContinuousClock()
        let start = clock.now
        _ = try await URLSession.shared.data(for: request)
        return clock.now - start
    }
}
