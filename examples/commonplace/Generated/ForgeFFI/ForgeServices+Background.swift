// Forge native services: background tasks (optional: `native_services!(background)`).
// Source of truth: crates/forge-services/swift/optional/. Copied into an app's
// Generated/ForgeFFI only when its Rust crate selects the service.
//
// iOS requires every task identifier to be registered before the app
// finishes launching, so installBackground() (called by
// ForgeServices.install() at launch) registers each identifier in
// BGTaskSchedulerPermittedIdentifiers (forge.toml `[ios] background_tasks`)
// and hands every task iOS starts to Rust. Rust registers the handlers and
// decides when to ask again (crates/forge-services/src/background.rs).

import Foundation

#if os(iOS)
    import BackgroundTasks
#endif

extension ForgeServices {
    static func installBackground() throws {
        installNativeBackground(scheduler: ForgeBackgroundScheduler.shared)
        #if os(iOS)
            ForgeBackgroundScheduler.shared.registerPermitted()
        #endif
    }
}

/// `BackgroundScheduler` over `BGTaskScheduler`.
public final class ForgeBackgroundScheduler: BackgroundScheduler, @unchecked Sendable {
    public static let shared = ForgeBackgroundScheduler()

    private let lock = NSLock()
    private var registered = false

    private init() {}

    public func permitted() -> [String] {
        Bundle.main.object(forInfoDictionaryKey: "BGTaskSchedulerPermittedIdentifiers") as? [String] ?? []
    }

    #if os(iOS)
        /// Registers each permitted identifier once per process. A second
        /// registration of the same identifier would crash.
        func registerPermitted() {
            lock.lock()
            defer { lock.unlock() }
            guard !registered else { return }
            registered = true
            for identifier in permitted() {
                let ok = BGTaskScheduler.shared.register(forTaskWithIdentifier: identifier, using: nil) { task in
                    let run = runNativeBackgroundTask(
                        identifier: identifier, completion: BackgroundCompletion(task: task))
                    task.expirationHandler = { run.expire() }
                }
                if !ok {
                    print("[forge] background task \(identifier) could not be registered")
                }
            }
        }
    #endif

    public func submit(request: BackgroundTaskRequest) throws {
        #if os(iOS)
            let taskRequest: BGTaskRequest
            switch request.kind {
            case .refresh:
                taskRequest = BGAppRefreshTaskRequest(identifier: request.identifier)
            case .processing(let requiresExternalPower, let requiresNetwork):
                let processing = BGProcessingTaskRequest(identifier: request.identifier)
                processing.requiresExternalPower = requiresExternalPower
                processing.requiresNetworkConnectivity = requiresNetwork
                taskRequest = processing
            }
            taskRequest.earliestBeginDate = Date(timeIntervalSinceNow: request.earliestBeginSeconds)
            do {
                try BGTaskScheduler.shared.submit(taskRequest)
            } catch let error as BGTaskScheduler.Error {
                switch error.code {
                case .unavailable:
                    // Background App Refresh is off for the app, or the
                    // simulator (which never runs tasks).
                    throw NativeServiceError.PermissionDenied(
                        reason: "background tasks unavailable: Background App Refresh is off for this app")
                case .notPermitted:
                    throw NativeServiceError.InvalidInput(
                        reason: "\(request.identifier) is not in BGTaskSchedulerPermittedIdentifiers")
                case .tooManyPendingTaskRequests:
                    throw NativeServiceError.Platform(
                        code: Int64(error.code.rawValue), reason: "too many pending background task requests")
                default:
                    throw NativeServiceError.Platform(code: Int64(error.code.rawValue), reason: "\(error)")
                }
            }
        #else
            throw NativeServiceError.Unavailable(reason: "background tasks need iOS")
        #endif
    }

    public func cancel(identifier: String) {
        #if os(iOS)
            BGTaskScheduler.shared.cancel(taskRequestWithIdentifier: identifier)
        #endif
    }
}

#if os(iOS)
    /// Reports Rust's result to iOS.
    private final class BackgroundCompletion: BackgroundTaskCompletion, @unchecked Sendable {
        private let task: BGTask

        init(task: BGTask) {
            self.task = task
        }

        func finished(success: Bool) {
            task.setTaskCompleted(success: success)
        }
    }
#endif
