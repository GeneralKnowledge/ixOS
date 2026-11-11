// Forge native services: background transfers (optional: `native_services!(transfers)`).
// Source of truth: crates/forge-services/swift/optional/. Copied into an app's
// Generated/ForgeFFI only when its Rust crate selects the service.

import Foundation

extension ForgeServices {
    static func installTransfers() throws {
        try installNativeTransfers(session: ForgeTransfers.shared)
    }
}

/// `TransferSession` backed by a background `URLSession`: downloads keep going
/// while the app is suspended or ended by the system, and iOS relaunches the
/// app in the background to deliver them. Rust (`forge_services::transfers`)
/// keeps the ledger, verifies each file and delivers it into the app's file
/// store; this class only runs the session and moves each finished file out
/// of the system's temporary folder before the delegate callback returns.
///
/// The app must pass the system's completion handler on, from its app
/// delegate:
///
/// ```swift
/// final class AppDelegate: NSObject, UIApplicationDelegate {
///     func application(_ application: UIApplication,
///                      handleEventsForBackgroundURLSession identifier: String,
///                      completionHandler: @escaping () -> Void) {
///         ForgeTransfers.handleEvents(forBackgroundURLSession: identifier,
///                                     completionHandler: completionHandler)
///     }
/// }
/// // In the App: @UIApplicationDelegateAdaptor(AppDelegate.self) var appDelegate
/// ```
public final class ForgeTransfers: NSObject, TransferSession, URLSessionDownloadDelegate,
    @unchecked Sendable
{
    public static let shared = ForgeTransfers()

    /// The session for transfers the person is waiting for, and the one for
    /// discretionary transfers (the system picks the time). Their identifiers
    /// stay the same between launches, so a relaunched app gets its tasks
    /// back.
    public static var sessionIdentifier: String {
        (Bundle.main.bundleIdentifier ?? "org.forgemobile.app") + ".forge-transfers"
    }
    public static var discretionarySessionIdentifier: String {
        sessionIdentifier + ".discretionary"
    }

    /// Passes the system's completion handler for a background session's
    /// events on. It is called once the session has delivered them all, and
    /// Rust has saved, verified and delivered each one.
    public static func handleEvents(
        forBackgroundURLSession identifier: String,
        completionHandler: @escaping () -> Void
    ) {
        guard identifier == sessionIdentifier || identifier == discretionarySessionIdentifier else {
            return
        }
        NSLog("[forge] transfers: woken for background session events")
        shared.lock.lock()
        shared.completionHandlers[identifier] = completionHandler
        let attached = shared.sink != nil
        shared.lock.unlock()
        if attached {
            // Recreating the session is what makes iOS deliver the events.
            _ = shared.session(discretionary: identifier == discretionarySessionIdentifier)
        }
    }

    private let lock = NSLock()
    private var sink: TransferSink?
    private var sessions: [Bool: URLSession] = [:]
    private var completionHandlers: [String: () -> Void] = [:]
    /// Ids whose file was moved and reported, so the task's completion
    /// callback that follows reports nothing more.
    private var reported: Set<String> = []
    private var lastProgress: [String: Date] = [:]
    /// The delegate queue: serial, so each callback finishes (and Rust has
    /// handled it) before the next, and before `urlSessionDidFinishEvents`.
    private let queue: OperationQueue = {
        let queue = OperationQueue()
        queue.name = "org.forgemobile.services.transfers"
        queue.maxConcurrentOperationCount = 1
        return queue
    }()

    private let directoryURL: URL = {
        let base =
            FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask).first
            ?? URL(fileURLWithPath: NSTemporaryDirectory())
        return base.appendingPathComponent("ForgeTransfers", isDirectory: true)
    }()

    private var inboxURL: URL {
        directoryURL.appendingPathComponent("inbox", isDirectory: true)
    }

    private override init() {
        super.init()
    }

    // MARK: TransferSession

    public func directory() -> String {
        directoryURL.path
    }

    public func attach(sink: TransferSink) {
        lock.lock()
        self.sink = sink
        lock.unlock()
        survey(sink: sink) { [weak self] live in
            // After a force quit iOS can keep listing a cancelled task for a
            // while and report its end only later. Look again once, so a dead
            // task doesn't stay Running until the next launch.
            guard live > 0 else { return }
            DispatchQueue.global().asyncAfter(deadline: .now() + 15) {
                self?.survey(sink: sink) { _ in }
            }
        }
    }

    /// Tells Rust which tasks the sessions still have. A task that already
    /// ended with an error (its delegate callback not yet delivered) is
    /// reported as failed rather than live.
    private func survey(sink: TransferSink, done: @escaping @Sendable (Int) -> Void) {
        let sessions = [session(discretionary: false), session(discretionary: true)]
        let group = DispatchGroup()
        let found = Locked<[SessionTaskInfo]>([])
        let ended = Locked<[(String, NSError)]>([])
        for session in sessions {
            group.enter()
            session.getAllTasks { tasks in
                for task in tasks {
                    guard let id = task.taskDescription else { continue }
                    if let error = task.error as NSError? {
                        ended.update { $0.append((id, error)) }
                        continue
                    }
                    guard task.state != .canceling, task.state != .completed else { continue }
                    let expected = task.countOfBytesExpectedToReceive
                    let info = SessionTaskInfo(
                        id: id,
                        received: UInt64(max(0, task.countOfBytesReceived)),
                        expected: expected > 0 ? UInt64(expected) : nil,
                        state: Self.describe(task.state))
                    found.update { $0.append(info) }
                }
                group.leave()
            }
        }
        // On the delegate queue, in order with the session's callbacks.
        group.notify(queue: .global()) { [queue] in
            queue.addOperation {
                for (id, error) in ended.value {
                    sink.failed(
                        id: id, code: Int64(error.code), reason: error.localizedDescription,
                        cancelled: error.domain == NSURLErrorDomain
                            && error.code == NSURLErrorCancelled)
                }
                sink.reconcile(tasks: found.value)
                done(found.value.count)
            }
        }
    }

    private static func describe(_ state: URLSessionTask.State) -> String {
        switch state {
        case .running: return "running"
        case .suspended: return "suspended"
        case .canceling: return "canceling"
        case .completed: return "completed"
        @unknown default: return "unknown"
        }
    }

    public func start(task: TransferTask) throws {
        guard let url = URL(string: task.url), url.scheme == "https" || url.scheme == "http" else {
            throw NativeServiceError.InvalidInput(reason: "not an http(s) URL")
        }
        var request = URLRequest(url: url)
        for header in task.headers {
            request.setValue(header.value, forHTTPHeaderField: header.name)
        }
        request.allowsCellularAccess = task.allowsCellular
        let download = session(discretionary: task.discretionary).downloadTask(with: request)
        download.taskDescription = task.id
        if let expected = task.expectedBytes {
            download.countOfBytesClientExpectsToReceive = Int64(clamping: expected)
        }
        download.resume()
    }

    public func cancel(id: String) {
        for session in [session(discretionary: false), session(discretionary: true)] {
            session.getAllTasks { tasks in
                tasks.filter { $0.taskDescription == id }.forEach { $0.cancel() }
            }
        }
    }

    private func session(discretionary: Bool) -> URLSession {
        lock.lock()
        defer { lock.unlock() }
        if let session = sessions[discretionary] {
            return session
        }
        let identifier = discretionary ? Self.discretionarySessionIdentifier : Self.sessionIdentifier
        let configuration = URLSessionConfiguration.background(withIdentifier: identifier)
        configuration.isDiscretionary = discretionary
        configuration.sessionSendsLaunchEvents = true
        let session = URLSession(configuration: configuration, delegate: self, delegateQueue: queue)
        sessions[discretionary] = session
        return session
    }

    private var currentSink: TransferSink? {
        lock.lock()
        defer { lock.unlock() }
        return sink
    }

    /// Ids are made by Rust: `t` and hex digits. Anything else could name a
    /// path outside the inbox, so its file is not moved.
    private static func isTransferId(_ id: String) -> Bool {
        id.count > 1 && id.count <= 64 && id.first == "t"
            && id.dropFirst().allSatisfy { "0123456789abcdef-".contains($0) }
    }

    // MARK: URLSessionDownloadDelegate

    public func urlSession(
        _ session: URLSession, downloadTask: URLSessionDownloadTask,
        didWriteData bytesWritten: Int64, totalBytesWritten: Int64,
        totalBytesExpectedToWrite: Int64
    ) {
        guard let id = downloadTask.taskDescription, let sink = currentSink else { return }
        // At most twice a second per transfer.
        let now = Date()
        lock.lock()
        let last = lastProgress[id]
        let due = last.map { now.timeIntervalSince($0) >= 0.5 } ?? true
        if due { lastProgress[id] = now }
        lock.unlock()
        guard due else { return }
        sink.progress(
            id: id, received: UInt64(max(0, totalBytesWritten)),
            expected: totalBytesExpectedToWrite > 0 ? UInt64(totalBytesExpectedToWrite) : nil)
    }

    public func urlSession(
        _ session: URLSession, downloadTask: URLSessionDownloadTask,
        didFinishDownloadingTo location: URL
    ) {
        guard let id = downloadTask.taskDescription, Self.isTransferId(id) else { return }
        // iOS deletes `location` when this method returns: move it first.
        let destination = inboxURL.appendingPathComponent(id)
        do {
            try FileManager.default.createDirectory(at: inboxURL, withIntermediateDirectories: true)
            try? FileManager.default.removeItem(at: destination)
            try FileManager.default.moveItem(at: location, to: destination)
        } catch {
            return  // reported as a failure when the task completes
        }
        let status = (downloadTask.response as? HTTPURLResponse)?.statusCode ?? 0
        lock.lock()
        reported.insert(id)
        lastProgress[id] = nil
        lock.unlock()
        // Returns once Rust has verified the file and saved the outcome.
        currentSink?.downloaded(id: id, httpStatus: UInt16(clamping: status))
    }

    public func urlSession(
        _ session: URLSession, task: URLSessionTask, didCompleteWithError error: Error?
    ) {
        guard let id = task.taskDescription else { return }
        lock.lock()
        let wasReported = reported.remove(id) != nil
        lastProgress[id] = nil
        lock.unlock()
        if wasReported && error == nil {
            return
        }
        let nsError = error.map { $0 as NSError }
        currentSink?.failed(
            id: id,
            code: Int64(nsError?.code ?? 0),
            reason: nsError?.localizedDescription ?? "the downloaded file could not be kept",
            cancelled: nsError?.domain == NSURLErrorDomain && nsError?.code == NSURLErrorCancelled)
    }

    public func urlSessionDidFinishEvents(forBackgroundURLSession session: URLSession) {
        // Every callback before this one ran on the serial delegate queue and
        // returned only after Rust handled it, so all of it is saved now.
        guard let identifier = session.configuration.identifier else { return }
        lock.lock()
        let handler = completionHandlers.removeValue(forKey: identifier)
        lock.unlock()
        if let handler {
            // Logged by Rust, so it reaches the device log.
            currentSink?.eventsHandled(session: identifier)
            DispatchQueue.main.async(execute: handler)
        }
    }
}

/// A value shared between callbacks on different queues.
private final class Locked<Value>: @unchecked Sendable {
    private let lock = NSLock()
    private var stored: Value

    init(_ value: Value) {
        stored = value
    }

    var value: Value {
        lock.lock()
        defer { lock.unlock() }
        return stored
    }

    func update(_ change: (inout Value) -> Void) {
        lock.lock()
        change(&stored)
        lock.unlock()
    }
}
