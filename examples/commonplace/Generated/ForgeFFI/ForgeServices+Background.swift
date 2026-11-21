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
        /// Registers each permitted identifier once per process; a second
        /// registration of the same identifier would crash. Patterns
        /// (`prefix.*`, continued-processing tasks) are left to the code that
        /// submits them.
        ///
        /// Several apps can share one process (Device Lab hosts them), each
        /// with its own copy of this adapter and its own Rust handlers. The
        /// copies share one table (`ForgeProcessTable`): the first registers
        /// an identifier with iOS, and each task goes to the copy whose Rust
        /// has a handler for it, or, if none has one yet (iOS launched the
        /// app for it before any core opened), to the first copy, whose Rust
        /// waits for a handler until the window ends.
        func registerPermitted() {
            lock.lock()
            defer { lock.unlock() }
            guard !registered else { return }
            registered = true
            let offer: (BGTask, Bool) -> Bool = { task, evenWithoutHandler in
                guard evenWithoutHandler || nativeBackgroundHandles(identifier: task.identifier) else {
                    return false
                }
                let run = runNativeBackgroundTask(
                    identifier: task.identifier, completion: BackgroundCompletion(task: task))
                task.expirationHandler = { run.expire() }
                return true
            }
            let scheduler = BGTaskScheduler.shared
            ForgeProcessTable.with(scheduler) { table in
                let takers = ForgeProcessTable.section(table, "background")
                for identifier in permitted() where !identifier.contains("*") {
                    if let existing = takers[identifier] as? NSMutableArray {
                        existing.add(offer)
                        continue
                    }
                    takers[identifier] = NSMutableArray(object: offer)
                    let ok = scheduler.register(forTaskWithIdentifier: identifier, using: nil) { task in
                        let candidates = ForgeProcessTable.with(BGTaskScheduler.shared) { table in
                            (ForgeProcessTable.section(table, "background")[identifier] as? NSArray)?
                                .compactMap { $0 as? (BGTask, Bool) -> Bool } ?? []
                        }
                        if candidates.contains(where: { $0(task, false) }) { return }
                        if let first = candidates.first, first(task, true) { return }
                        task.setTaskCompleted(success: false)
                    }
                    if !ok {
                        forgeServicesWarn("background task \(identifier) could not be registered")
                    }
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
