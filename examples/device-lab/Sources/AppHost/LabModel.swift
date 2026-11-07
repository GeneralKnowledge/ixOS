@preconcurrency import ForgeFFI
import Foundation
import Observation

/// The dashboard's view of the Rust core. It holds no rules of its own: it
/// copies `DeviceLab.snapshot()` for the views, and passes the user's taps
/// and the app's lifecycle to Rust.
@MainActor
@Observable
public final class LabModel {
    public private(set) var snapshot: LabSnapshot

    @ObservationIgnored private let lab: DeviceLab

    public init(lab: DeviceLab = DeviceLab()) {
        self.lab = lab
        snapshot = lab.snapshot()
    }

    /// Copies Rust's snapshot about ten times a second until the calling
    /// task is cancelled (attach with `.task`). A snapshot only copies
    /// memory, and views redraw only when it changed.
    public func follow(every interval: Duration = .milliseconds(100)) async {
        while !Task.isCancelled {
            refresh()
            try? await Task.sleep(for: interval)
        }
    }

    public func refresh() {
        let next = lab.snapshot()
        if next != snapshot { snapshot = next }
    }

    /// Sensors run only while the app is in use. Coming back also re-reads
    /// the permissions, which the user may have changed in Settings.
    public func setActive(_ active: Bool) {
        lab.setActive(active: active)
        refresh()
        if active {
            Task { await refreshPermissions() }
        }
    }

    public func refreshPermissions() async {
        _ = await lab.refreshPermissions()
        refresh()
    }

    /// Shows the system prompt (the first time) and starts what it allows.
    public func request(_ permission: NativePermission) async {
        _ = await lab.requestPermission(permission: permission)
        refresh()
    }

    /// Forgets peaks, ranges and the distance travelled.
    public func reset() {
        lab.reset()
        refresh()
    }
}
