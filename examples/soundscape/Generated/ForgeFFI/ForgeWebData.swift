// Copied into every app's Generated/ForgeFFI by scripts/generate-bindings.sh.
// Do not edit the copy; the source of truth is
// crates/forge-services/swift/helpers/ForgeWebData.swift. Its tests are in
// crates/forge-services/swift/Tests (`swift test --package-path
// crates/forge-services/swift`).

import Foundation
#if canImport(FoundationNetworking)
import FoundationNetworking
#endif

/// Small files from the web that a screen shows, such as pictures, fetched
/// the way a browser would: HTTP cache headers decide when a copy on disk is
/// still good, the copy is used when there is no connection, and a response
/// larger than `maxBytes` is refused before it is all in memory.
/// `ForgeImages.thumbnail(from:maxPixels:)` uses `ForgeWebData.shared`.
///
/// - Freshness: requests use `URLRequest.CachePolicy.useProtocolCachePolicy`,
///   so `Cache-Control`, `Expires`, `ETag` and `Last-Modified` decide whether
///   the disk copy is used, revalidated or fetched again. Nothing assumes a
///   file at an address never changes.
/// - Offline: when the network can't be reached, the request is made again
///   from the disk copy alone (stale or not) before reporting `.offline`.
/// - Refresh: `data(from:reload: true)` ignores the disk copy; `forget(_:)`
///   removes it.
/// - Sharing: callers asking for the same address while it downloads share
///   that download. A caller whose task is cancelled gets `.cancelled` at
///   once; the download stops when nobody is waiting for it any more.
///
/// The disk cache is `URLCache`, in its own directory, so these files never
/// crowd out the app's other responses. `URLCache` keeps no single response
/// larger than about 5% of its disk capacity.
public final class ForgeWebData: NSObject, @unchecked Sendable {
    /// Why there is no data.
    public enum Failure: Error, Equatable, Sendable {
        /// No connection, and no copy on disk.
        case offline
        /// The server answered with this status (not 2xx).
        case http(Int)
        /// The response was larger than `maxBytes`.
        case tooLarge
        /// The caller's task was cancelled.
        case cancelled
        /// Anything else: a bad address, a TLS failure, a refused connection.
        case failed
    }

    /// Pictures for the app's screens: Caches/ForgeImages, 200 MB on disk,
    /// responses up to 20 MB.
    public static let shared = ForgeWebData(
        cacheDirectory: FileManager.default.urls(for: .cachesDirectory, in: .userDomainMask)
            .first?.appendingPathComponent("ForgeImages", isDirectory: true))

    /// The largest response accepted, in bytes.
    public let maxBytes: Int

    private let cache: URLCache
    private var session: URLSession!
    private let lock = NSLock()
    private var downloads: [URL: Download] = [:]
    private var byTask: [Int: Download] = [:]
    /// Waiters cancelled before they were registered (the cancellation
    /// handler can run first).
    private var cancelledEarly: Set<UUID> = []

    /// - Parameters:
    ///   - configuration: the session's configuration; its `urlCache` and
    ///     `requestCachePolicy` are replaced. Tests pass one with their own
    ///     `protocolClasses`.
    ///   - cacheDirectory: where the disk cache lives; nil for memory only.
    ///   - diskCapacity: the disk cache's size in bytes.
    ///   - maxBytes: the largest response accepted.
    public init(
        configuration: URLSessionConfiguration = .default,
        cacheDirectory: URL?,
        diskCapacity: Int = 200 * 1024 * 1024,
        maxBytes: Int = 20 * 1024 * 1024
    ) {
        #if canImport(FoundationNetworking)
            cache = URLCache(memoryCapacity: 0, diskCapacity: diskCapacity, diskPath: cacheDirectory?.path)
        #else
            cache = URLCache(memoryCapacity: 0, diskCapacity: diskCapacity, directory: cacheDirectory)
        #endif
        self.maxBytes = maxBytes
        super.init()
        configuration.urlCache = cache
        configuration.requestCachePolicy = .useProtocolCachePolicy
        let queue = OperationQueue()
        queue.maxConcurrentOperationCount = 1
        session = URLSession(configuration: configuration, delegate: self, delegateQueue: queue)
    }

    /// The file at `url`. With `reload`, the disk copy is ignored and the
    /// server asked again (the new answer replaces the copy).
    public func data(from url: URL, reload: Bool = false) async -> Result<Data, Failure> {
        let id = UUID()
        return await withTaskCancellationHandler {
            await withCheckedContinuation { (continuation: CheckedContinuation<Result<Data, Failure>, Never>) in
                lock.lock()
                if cancelledEarly.remove(id) != nil {
                    lock.unlock()
                    continuation.resume(returning: .failure(.cancelled))
                    return
                }
                if let running = downloads[url] {
                    running.waiters[id] = continuation
                    lock.unlock()
                    return
                }
                let download = Download(url: url)
                download.waiters[id] = continuation
                downloads[url] = download
                start(download, policy: reload ? .reloadIgnoringLocalCacheData : .useProtocolCachePolicy)
                lock.unlock()
            }
        } onCancel: {
            cancelWaiter(id, for: url)
        }
    }

    /// Removes the disk copy of `url`, so the next request asks the server.
    public func forget(_ url: URL) {
        cache.removeCachedResponse(for: URLRequest(url: url))
    }

    // MARK: - Private

    private final class Download {
        let url: URL
        var task: URLSessionDataTask?
        var buffer = Data()
        var fromCacheOnly = false
        var failure: Failure?
        var waiters: [UUID: CheckedContinuation<Result<Data, Failure>, Never>] = [:]
        init(url: URL) { self.url = url }
    }

    /// Call with the lock held.
    private func start(_ download: Download, policy: URLRequest.CachePolicy) {
        var request = URLRequest(url: download.url)
        request.cachePolicy = policy
        download.buffer = Data()
        download.failure = nil
        let task = session.dataTask(with: request)
        download.task = task
        byTask[task.taskIdentifier] = download
        task.resume()
    }

    private func cancelWaiter(_ id: UUID, for url: URL) {
        lock.lock()
        guard let download = downloads[url], let continuation = download.waiters.removeValue(forKey: id) else {
            cancelledEarly.insert(id)
            lock.unlock()
            return
        }
        let task = download.waiters.isEmpty ? download.task : nil
        if task != nil { downloads[url] = nil }
        lock.unlock()
        continuation.resume(returning: .failure(.cancelled))
        task?.cancel()
    }

    /// Ends `download`. Call with the lock held; returns the waiters, to be
    /// resumed after unlocking.
    private func finish(_ download: Download) -> [CheckedContinuation<Result<Data, Failure>, Never>] {
        if downloads[download.url] === download { downloads[download.url] = nil }
        let waiting = Array(download.waiters.values)
        download.waiters = [:]
        return waiting
    }

    private static let offlineCodes: Set<URLError.Code> = [
        .notConnectedToInternet, .networkConnectionLost, .cannotConnectToHost, .cannotFindHost,
        .dnsLookupFailed, .timedOut, .dataNotAllowed, .internationalRoamingOff,
    ]
}

extension ForgeWebData: URLSessionDataDelegate {
    public func urlSession(
        _ session: URLSession, dataTask: URLSessionDataTask, didReceive response: URLResponse,
        completionHandler: @escaping (URLSession.ResponseDisposition) -> Void
    ) {
        lock.lock()
        let download = byTask[dataTask.taskIdentifier]
        if let download {
            if let status = (response as? HTTPURLResponse)?.statusCode, !(200..<300).contains(status) {
                download.failure = .http(status)
            } else if response.expectedContentLength > Int64(maxBytes) {
                download.failure = .tooLarge
            }
        }
        let refuse = download?.failure != nil
        lock.unlock()
        completionHandler(refuse ? .cancel : .allow)
    }

    public func urlSession(_ session: URLSession, dataTask: URLSessionDataTask, didReceive data: Data) {
        lock.lock()
        defer { lock.unlock() }
        guard let download = byTask[dataTask.taskIdentifier], download.failure == nil else { return }
        download.buffer.append(data)
        if download.buffer.count > maxBytes {
            download.failure = .tooLarge
            download.buffer = Data()
            dataTask.cancel()
        }
    }

    public func urlSession(_ session: URLSession, task: URLSessionTask, didCompleteWithError error: Error?) {
        lock.lock()
        guard let download = byTask.removeValue(forKey: task.taskIdentifier) else {
            lock.unlock()
            return
        }
        let result: Result<Data, Failure>
        if let failure = download.failure {
            result = .failure(failure)
        } else if let error {
            let code = (error as? URLError)?.code
            if code == .cancelled, download.waiters.isEmpty {
                // Everyone waiting was cancelled; they have their answer.
                lock.unlock()
                return
            }
            if let code, Self.offlineCodes.contains(code), !download.fromCacheOnly, !download.waiters.isEmpty {
                // No network: try again from the disk copy alone.
                download.fromCacheOnly = true
                start(download, policy: .returnCacheDataDontLoad)
                lock.unlock()
                return
            }
            let offline = download.fromCacheOnly || code.map { Self.offlineCodes.contains($0) } == true
            result = .failure(offline ? .offline : .failed)
        } else {
            result = .success(download.buffer)
        }
        let waiting = finish(download)
        lock.unlock()
        for continuation in waiting { continuation.resume(returning: result) }
    }
}
