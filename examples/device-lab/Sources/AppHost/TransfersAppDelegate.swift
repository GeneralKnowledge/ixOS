@preconcurrency import ForgeFFI

#if os(iOS)
    import UIKit

    /// Hands iOS's completion handler for background download events to
    /// Forge's transfers service, which calls it once Rust has saved,
    /// verified and delivered every finished download. The entry points
    /// attach it with `@UIApplicationDelegateAdaptor`.
    public final class TransfersAppDelegate: NSObject, UIApplicationDelegate {
        public func application(
            _ application: UIApplication,
            handleEventsForBackgroundURLSession identifier: String,
            completionHandler: @escaping () -> Void
        ) {
            ForgeTransfers.handleEvents(
                forBackgroundURLSession: identifier, completionHandler: completionHandler)
        }
    }
#endif
