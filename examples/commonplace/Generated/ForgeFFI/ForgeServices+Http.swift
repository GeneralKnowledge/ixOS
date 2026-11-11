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
/// the response is complete; `cancel` stops it from another thread.
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
        let done = DispatchSemaphore(value: 0)

        init(limit: UInt64) {
            self.limit = limit
        }
    }

    private let lock = NSLock()
    /// By `URLSessionTask.taskIdentifier`.
    private var pending: [Int: Pending] = [:]
    /// By Rust's request id, for `cancel`.
    private var tasks: [UInt64: URLSessionTask] = [:]
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
        guard let url = URL(string: request.url), url.scheme == "https" || url.scheme == "http"
        else {
            throw HttpTransportError.InvalidRequest(reason: "not an http(s) URL")
        }
        var urlRequest = URLRequest(
            url: url, timeoutInterval: max(0.001, Double(request.timeoutMs) / 1000))
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
        let state = Pending(limit: request.maxBodyBytes)
        lock.lock()
        if cancelledEarly.remove(request.id) != nil {
            lock.unlock()
            throw HttpTransportError.Cancelled
        }
        pending[task.taskIdentifier] = state
        tasks[request.id] = task
        let firstRequest = !announced
        announced = true
        lock.unlock()

        if firstRequest {
            // Once per launch, so a device log shows which transport ran.
            NSLog("[forge] http: requests go through URLSession")
        }
        task.resume()
        state.done.wait()

        lock.lock()
        tasks[request.id] = nil
        lock.unlock()

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
        var headers: [HttpHeader] = []
        for (name, value) in http.allHeaderFields {
            headers.append(HttpHeader(name: "\(name)", value: "\(value)"))
        }
        return HttpResponse(
            status: UInt16(clamping: http.statusCode), headers: headers, body: state.data)
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
        lock.unlock()
        guard let state else { return }
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
