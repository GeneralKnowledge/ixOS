import CommonplaceFFI
import ForgeKit
import Foundation
import SoundscapeFFI

// How the A2UI Lab's app surfaces reach the apps Device Lab hosts: the same
// cores the Apps screen opens (`EmbeddedApps`), never a second copy of an
// app's store. Kept apart from the models so they need no app's bindings
// (docs/research/app-capability-audit.md, 4.2).

/// A refusal from an app, as a sentence for the notice.
struct A2uiAppRefusal: LocalizedError {
    var errorDescription: String?
}

@MainActor
enum A2uiAppHooks {
    /// Commonplace: its folders snapshot, and its own apply step, which
    /// checks the plan again against the library as it is now.
    static func commonplace() -> A2uiAppLink {
        A2uiAppLink(
            name: "Commonplace",
            snapshot: {
                let host = EmbeddedApps.commonplace
                await host.start()
                return host.core?.organiseSnapshotJson()
            },
            apply: { plan in
                guard let core = EmbeddedApps.commonplace.core else {
                    throw A2uiAppRefusal(errorDescription: "Commonplace isn't open.")
                }
                do {
                    let moved = try await core.applyFolderPlanJson(planJson: plan)
                    return moved == 1
                        ? "Moved 1 feed in Commonplace."
                        : "Moved \(moved) feeds in Commonplace."
                } catch {
                    throw A2uiAppRefusal(
                        errorDescription:
                            "Commonplace refused the plan: its feeds changed since the review. Start Over to see them as they are now.")
                }
            })
    }

    /// Soundscape: its library snapshot (no artwork), and its own apply
    /// step, which checks the songs again and makes the playlist.
    static func soundscape() -> A2uiAppLink {
        A2uiAppLink(
            name: "Soundscape",
            snapshot: {
                let host = EmbeddedApps.soundscape
                await host.start()
                return host.core?.playlistSnapshotJson()
            },
            apply: { plan in
                guard let core = EmbeddedApps.soundscape.core else {
                    throw A2uiAppRefusal(errorDescription: "Soundscape isn't open.")
                }
                do {
                    let made = try await core.applyPlaylistPlanJson(planJson: plan)
                    return "Made \u{201C}\(made.playlist.name)\u{201D} in Soundscape, with \(made.entries.count) songs."
                } catch {
                    throw A2uiAppRefusal(
                        errorDescription:
                            "Soundscape refused the plan: its library changed since the review. Start Over to see it as it is now.")
                }
            })
    }
}
