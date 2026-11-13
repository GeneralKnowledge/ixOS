// Forge native services: HTTP through URLSession (optional: `native_services!(http)`).
// Source of truth: crates/forge-services/swift/optional/. Copied into an app's
// Generated/ForgeFFI only when its Rust crate selects the service.

import Foundation

extension ForgeServices {
    static func installHttp() throws {
        installNativeHttp(transport: ForgeHTTPTransport.shared)
    }
}

/// `HttpTransport` backed by `URLSession`, so Rust's ordinary requests get
/// the system's proxy and VPN settings, HTTP/2 and HTTP/3, Low Data Mode and
/// trust store. Rust calls `send` from a worker thread and it blocks until
/// the response is complete; `openStream` blocks only until the headers
/// arrive and then hands the body to Rust as it comes in (Server-Sent
/// Events); `cancel` stops either from another thread.
public final class ForgeHTTPTransport: NSObject, HttpTransport, URLSessionDataDelegate,
    @unchecked Sendable
{
    public static let shared = ForgeHTTPTransport()

    /// What one request has received so far.
    private final class Pending: @unchecked Sendable {
        let limit: UInt64
        var data = Data()
        var response: URLResponse?
        var error: Error?
        var tooLarge = false
        /// For `send`: the whole response arrived. For `openStream`: the
        /// headers arrived, or the request failed before them.
        let done = DispatchSemaphore(value: 0)
        /// Set for a stream: where its body goes once the headers are in.
        let stream: HttpStreamBody?
        var headersDelivered = false

        init(limit: UInt64, stream: HttpStreamBody? = nil) {
            self.limit = limit
            self.stream = stream
        }
    }

    private let lock = NSLock()
    /// By `URLSessionTask.taskIdentifier`.
    private var pending: [Int: Pending] = [:]
    /// By Rust's request id, for `cancel`.
    private var tasks: [UInt64: URLSessionTask] = [:]
    /// Rust's request id by `taskIdentifier`, to unregister a finished stream.
    private var ids: [Int: UInt64] = [:]
    /// Ids cancelled before their task started.
    private var cancelledEarly: Set<UInt64> = []
    private var announced = false
    private let queue: OperationQueue = {
        let queue = OperationQueue()
        queue.name = "org.forgemobile.services.http"
        queue.maxConcurrentOperationCount = 1
        return queue
    }()
    private lazy var session = URLSession(
        configuration: .default, delegate: self, delegateQueue: queue)

    private override init() {
        super.init()
    }

    // MARK: HttpTransport

    public func send(request: HttpRequest) throws -> HttpResponse {
        let state = Pending(limit: request.maxBodyBytes)
        _ = try start(request, state: state, idleTimeout: nil)
        state.done.wait()
        finished(request.id)

        if state.tooLarge {
            throw HttpTransportError.Transport(
                reason: "the response body is larger than \(state.limit) bytes")
        }
        if let error = state.error {
            throw Self.transportError(error)
        }
        guard let http = state.response as? HTTPURLResponse else {
            throw HttpTransportError.Transport(reason: "not an HTTP response")
        }
        return HttpResponse(
            status: UInt16(clamping: http.statusCode), headers: Self.headers(of: http),
            body: state.data)
    }

    public func openStream(request: HttpRequest, body: HttpStreamBody) throws -> HttpStreamHead {
        let state = Pending(limit: .max, stream: body)
        // URLSession's timeout is an idle timeout, and a stream may be
        // quiet for long stretches: give the body a day, and bound the
        // wait for the headers here instead.
        let task = try start(request, state: state, idleTimeout: 24 * 60 * 60)
        let waited = state.done.wait(
            timeout: .now() + max(0.001, Double(request.timeoutMs) / 1000))
        if waited == .timedOut {
            task.cancel()
            state.done.wait()
            finished(request.id)
            throw HttpTransportError.Timeout
        }
        if let error = state.error {
            finished(request.id)
            throw Self.transportError(error)
        }
        guard let http = state.response as? HTTPURLResponse else {
            task.cancel()
            finished(request.id)
            throw HttpTransportError.Transport(reason: "not an HTTP response")
        }
        // The task stays registered for `cancel` until it completes.
        return HttpStreamHead(
            status: UInt16(clamping: http.statusCode), headers: Self.headers(of: http))
    }

    /// Builds and starts the task for `request`, registered for `cancel`.
    private func start(_ request: HttpRequest, state: Pending, idleTimeout: Double?) throws
        -> URLSessionTask
    {
        guard let url = URL(string: request.url), url.scheme == "https" || url.scheme == "http"
        else {
            throw HttpTransportError.InvalidRequest(reason: "not an http(s) URL")
        }
        var urlRequest = URLRequest(
            url: url,
            timeoutInterval: idleTimeout ?? max(0.001, Double(request.timeoutMs) / 1000))
        urlRequest.httpMethod = request.method
        for header in request.headers {
            urlRequest.addValue(header.value, forHTTPHeaderField: header.name)
        }
        let task: URLSessionTask
        if let file = request.bodyFile {
            task = session.uploadTask(with: urlRequest, fromFile: URL(fileURLWithPath: file))
        } else {
            if !request.body.isEmpty {
                urlRequest.httpBody = request.body
            }
            task = session.dataTask(with: urlRequest)
        }
        lock.lock()
        if cancelledEarly.remove(request.id) != nil {
            lock.unlock()
            throw HttpTransportError.Cancelled
        }
        pending[task.taskIdentifier] = state
        tasks[request.id] = task
        ids[task.taskIdentifier] = request.id
        let firstRequest = !announced
        announced = true
        lock.unlock()

        if firstRequest {
            // Once per launch, so a device log shows which transport ran.
            NSLog("[forge] http: requests go through URLSession")
        }
        task.resume()
        return task
    }

    private func finished(_ id: UInt64) {
        lock.lock()
        if let task = tasks.removeValue(forKey: id) {
            ids[task.taskIdentifier] = nil
        }
        lock.unlock()
    }

    private static func headers(of http: HTTPURLResponse) -> [HttpHeader] {
        http.allHeaderFields.map { name, value in
            HttpHeader(name: "\(name)", value: "\(value)")
        }
    }

    public func cancel(id: UInt64) {
        lock.lock()
        defer { lock.unlock() }
        if let task = tasks[id] {
            task.cancel()
        } else {
            cancelledEarly.insert(id)
        }
    }

    // MARK: URLSessionDataDelegate

    private func state(of task: URLSessionTask) -> Pending? {
        lock.lock()
        defer { lock.unlock() }
        return pending[task.taskIdentifier]
    }

    public func urlSession(
        _ session: URLSession, dataTask: URLSessionDataTask,
        didReceive response: URLResponse,
        completionHandler: @escaping (URLSession.ResponseDisposition) -> Void
    ) {
        guard let state = state(of: dataTask) else {
            completionHandler(.cancel)
            return
        }
        state.response = response
        if state.stream != nil {
            state.headersDelivered = true
            completionHandler(.allow)
            state.done.signal()
            return
        }
        if response.expectedContentLength > 0,
            UInt64(response.expectedContentLength) > state.limit
        {
            state.tooLarge = true
            completionHandler(.cancel)
            return
        }
        completionHandler(.allow)
    }

    public func urlSession(
        _ session: URLSession, dataTask: URLSessionDataTask, didReceive data: Data
    ) {
        guard let state = state(of: dataTask), !state.tooLarge else { return }
        if let stream = state.stream {
            stream.receive(data: data)
            return
        }
        state.data.append(data)
        if UInt64(state.data.count) > state.limit {
            state.tooLarge = true
            state.data = Data()
            dataTask.cancel()
        }
    }

    public func urlSession(
        _ session: URLSession, task: URLSessionTask, didCompleteWithError error: Error?
    ) {
        lock.lock()
        let state = pending.removeValue(forKey: task.taskIdentifier)
        if state?.stream != nil, let id = ids.removeValue(forKey: task.taskIdentifier) {
            tasks[id] = nil
        }
        lock.unlock()
        guard let state else { return }
        if let stream = state.stream, state.headersDelivered {
            // `openStream` has returned: the body's end goes to Rust.
            if let error {
                stream.fail(error: Self.transportError(error))
            } else {
                stream.finish()
            }
            return
        }
        if state.response == nil {
            state.response = task.response
        }
        state.error = error
        state.done.signal()
    }

    /// How `forge_net::NetError` classes an error from URLSession:
    /// "cancelled", "timeout", "offline", "certificate", "invalid" or
    /// "transport". Public so app tests can check the mapping.
    public static func kind(of error: Error) -> String {
        let error = error as NSError
        guard error.domain == NSURLErrorDomain else { return "transport" }
        switch error.code {
        case NSURLErrorCancelled:
            return "cancelled"
        case NSURLErrorTimedOut:
            return "timeout"
        case NSURLErrorNotConnectedToInternet, NSURLErrorCannotFindHost,
            NSURLErrorCannotConnectToHost, NSURLErrorNetworkConnectionLost,
            NSURLErrorDNSLookupFailed, NSURLErrorInternationalRoamingOff,
            NSURLErrorDataNotAllowed, NSURLErrorCallIsActive:
            return "offline"
        case NSURLErrorServerCertificateUntrusted, NSURLErrorServerCertificateHasBadDate,
            NSURLErrorServerCertificateNotYetValid, NSURLErrorServerCertificateHasUnknownRoot:
            return "certificate"
        case NSURLErrorBadURL, NSURLErrorUnsupportedURL:
            return "invalid"
        default:
            return "transport"
        }
    }

    static func transportError(_ error: Error) -> HttpTransportError {
        let reason = error.localizedDescription
        switch kind(of: error) {
        case "cancelled": return .Cancelled
        case "timeout": return .Timeout
        case "offline": return .Offline(reason: reason)
        case "certificate": return .Certificate(reason: reason)
        case "invalid": return .InvalidRequest(reason: reason)
        default: return .Transport(reason: reason)
        }
    }
}
