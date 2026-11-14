import Foundation
#if os(iOS) && canImport(BackgroundTasks)
@preconcurrency import BackgroundTasks
#endif

/// Keeps a whole-book run going after the person leaves Pocket Library:
/// an iOS 26 continued-processing task, which iOS shows with its own
/// progress (and a Stop button) while it runs. Rust's run carries on as
/// long as iOS keeps the app running; if iOS ends the task, or Apple's
/// model refuses to work in the background, the run stops and its saved
/// progress lets the next one carry on.
///
/// The task identifier must start with the app's bundle id, which xtool
/// changes when it signs (`XTL-<team>.`), so it is built at run time and
/// Info.plist permits both spellings (forge.toml `background_tasks`).
@MainActor
final class BookBackgroundTask {
    static let shared = BookBackgroundTask()

    /// "<bundle id>.booksummary"; the permitted identifier adds ".*".
    static var prefix: String { (Bundle.main.bundleIdentifier ?? "app") + ".booksummary" }

    private var registered = false
    private var running = false
    private var progress: (done: Int, total: Int, subtitle: String) = (0, 1, "")
    private var onExpire: (() -> Void)?
    #if os(iOS) && canImport(BackgroundTasks)
    /// The task iOS started (a BGContinuedProcessingTask).
    private var task: BGTask?
    #endif

    private init() {}

    /// Asks iOS to keep the run going in the background. Returns why not
    /// (an older iOS, an identifier Info.plist doesn't permit, iOS
    /// refusing), or nil once asked.
    @discardableResult
    func begin(title: String, onExpire: @escaping () -> Void) -> String? {
        self.onExpire = onExpire
        progress = (0, 1, "Starting")
        #if os(iOS) && canImport(BackgroundTasks)
        if #available(iOS 26.0, *) {
            let pattern = Self.prefix + ".*"
            let permitted = Bundle.main.object(forInfoDictionaryKey: "BGTaskSchedulerPermittedIdentifiers") as? [String] ?? []
            guard permitted.contains(pattern) else {
                return "Info.plist doesn't permit \(pattern)"
            }
            if !registered {
                registered = BGTaskScheduler.shared.register(forTaskWithIdentifier: pattern, using: nil) { task in
                    nonisolated(unsafe) let task = task
                    DispatchQueue.main.async {
                        MainActor.assumeIsolated { BookBackgroundTask.shared.attach(task) }
                    }
                }
                guard registered else { return "iOS refused to register \(pattern)" }
            }
            let request = BGContinuedProcessingTaskRequest(
                identifier: Self.prefix + ".\(UUID().uuidString)", title: title, subtitle: "Starting")
            request.strategy = .fail
            do {
                try BGTaskScheduler.shared.submit(request)
                running = true
                return nil
            } catch {
                return "iOS didn't start the background task (\(error.localizedDescription))"
            }
        }
        return "Reading in the background needs iOS 26."
        #else
        return "Reading in the background needs iOS."
        #endif
    }

    /// Chapters done of `total`, shown in iOS's progress.
    func update(done: Int, total: Int, subtitle: String) {
        progress = (done, max(total, 1), subtitle)
        apply()
    }

    /// The run ended; tells iOS whether it finished.
    func end(success: Bool) {
        running = false
        onExpire = nil
        #if os(iOS) && canImport(BackgroundTasks)
        task?.setTaskCompleted(success: success)
        task = nil
        #endif
    }

    #if os(iOS) && canImport(BackgroundTasks)
    private func attach(_ task: BGTask) {
        guard running else {
            task.setTaskCompleted(success: false)
            return
        }
        self.task = task
        task.expirationHandler = {
            DispatchQueue.main.async {
                MainActor.assumeIsolated { BookBackgroundTask.shared.expired() }
            }
        }
        apply()
    }

    private func expired() {
        onExpire?()
        end(success: false)
    }
    #endif

    private func apply() {
        #if os(iOS) && canImport(BackgroundTasks)
        guard let task else { return }
        if #available(iOS 26.0, *), let continued = task as? BGContinuedProcessingTask {
            // Only the continued-processing task reports progress.
            continued.progress.totalUnitCount = Int64(progress.total)
            continued.progress.completedUnitCount = Int64(progress.done)
            continued.updateTitle(continued.title, subtitle: progress.subtitle)
        }
        #endif
    }
}
