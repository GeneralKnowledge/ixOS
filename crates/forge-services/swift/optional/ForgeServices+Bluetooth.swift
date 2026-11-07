// Forge native services: Bluetooth state (optional: `native_services!(bluetooth)`).
// Source of truth: crates/forge-services/swift/optional/. Copied into an app's
// Generated/ForgeFFI only when its Rust crate selects the service.
//
// Needs NSBluetoothAlwaysUsageDescription (forge.toml `[ios.permissions]
// bluetooth`): iOS ends an app that touches Core Bluetooth without it, so
// nothing here creates a central manager until the key is present.

import Foundation

#if os(iOS)
    import CoreBluetooth
#endif

extension ForgeServices {
    static func installBluetooth() throws {
        installNativeBluetooth(monitor: ForgeBluetoothMonitor.shared)
        #if os(iOS)
            ForgePermissions.shared.register(BluetoothPermissionAdapter(), for: .bluetooth)
        #endif
    }
}

#if os(iOS)

    /// `BluetoothMonitor` over a `CBCentralManager`, created the first time
    /// something starts the monitor or asks for the permission (which is
    /// when iOS shows its prompt).
    public final class ForgeBluetoothMonitor: NSObject, BluetoothMonitor, CBCentralManagerDelegate,
        @unchecked Sendable
    {
        public static let shared = ForgeBluetoothMonitor()

        private let lock = NSLock()
        private var central: CBCentralManager?
        private var latest: BluetoothState = .unknown
        private var sink: BluetoothSink?
        private var waiters: [CheckedContinuation<Void, Never>] = []

        private override init() {
            super.init()
        }

        static var hasUsageDescription: Bool {
            Bundle.main.object(forInfoDictionaryKey: "NSBluetoothAlwaysUsageDescription") != nil
        }

        // MARK: BluetoothMonitor

        public func state() -> BluetoothState {
            lock.lock()
            defer { lock.unlock() }
            if central == nil {
                switch CBManager.authorization {
                case .denied, .restricted: return .unauthorized
                default: return .unknown
                }
            }
            return latest
        }

        public func start(sink: BluetoothSink) {
            lock.lock()
            self.sink = sink
            let started = central != nil
            let current = latest
            lock.unlock()
            if started {
                sink.send(state: current)
            } else {
                sink.send(state: state())
                ensureCentral()
            }
        }

        public func stop() {
            lock.lock()
            sink = nil
            lock.unlock()
        }

        // MARK: Permission

        /// Creates the central (showing the prompt if never asked) and waits
        /// for the system's answer.
        func requestAccess() async {
            guard Self.hasUsageDescription else {
                print("[forge] Bluetooth permission: add [ios.permissions] bluetooth to forge.toml first")
                return
            }
            for _ in 0..<3 where CBManager.authorization == .notDetermined {
                await withCheckedContinuation { (done: CheckedContinuation<Void, Never>) in
                    lock.lock()
                    waiters.append(done)
                    lock.unlock()
                    ensureCentral()
                }
            }
        }

        private func ensureCentral() {
            guard Self.hasUsageDescription else {
                print("[forge] Bluetooth: add [ios.permissions] bluetooth to forge.toml first")
                return
            }
            let create = {
                self.lock.lock()
                let exists = self.central != nil
                self.lock.unlock()
                guard !exists else { return }
                let central = CBCentralManager(
                    delegate: self, queue: nil,
                    options: [CBCentralManagerOptionShowPowerAlertKey: false])
                self.lock.lock()
                self.central = central
                self.lock.unlock()
            }
            if Thread.isMainThread { create() } else { DispatchQueue.main.async(execute: create) }
        }

        // MARK: CBCentralManagerDelegate

        public func centralManagerDidUpdateState(_ central: CBCentralManager) {
            let state: BluetoothState
            switch central.state {
            case .resetting: state = .resetting
            case .unsupported: state = .unsupported
            case .unauthorized: state = .unauthorized
            case .poweredOff: state = .poweredOff
            case .poweredOn: state = .poweredOn
            default: state = .unknown
            }
            lock.lock()
            latest = state
            let sink = self.sink
            let waiters = self.waiters
            self.waiters = []
            lock.unlock()
            sink?.send(state: state)
            for waiter in waiters { waiter.resume() }
        }
    }

    struct BluetoothPermissionAdapter: ForgePermissionAdapter {
        func status() async -> PermissionStatus {
            switch CBManager.authorization {
            case .notDetermined: return .notDetermined
            case .restricted: return .restricted
            case .denied: return .denied
            case .allowedAlways: return .granted
            @unknown default: return .unsupported
            }
        }

        func request() async -> PermissionStatus {
            let current = await status()
            guard current == .notDetermined else { return current }
            guard ForgeBluetoothMonitor.hasUsageDescription else { return .unsupported }
            await ForgeBluetoothMonitor.shared.requestAccess()
            return await status()
        }
    }

#else

    /// Bluetooth is wired up on iOS only; elsewhere it reports unsupported.
    public final class ForgeBluetoothMonitor: BluetoothMonitor, @unchecked Sendable {
        public static let shared = ForgeBluetoothMonitor()

        private init() {}

        public func state() -> BluetoothState { .unsupported }

        public func start(sink: BluetoothSink) { sink.send(state: .unsupported) }

        public func stop() {}
    }

#endif
