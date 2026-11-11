// Forge native services: Face ID, Touch ID and the passcode (optional:
// `native_services!(biometrics)`). Source of truth:
// crates/forge-services/swift/optional/. Copied into an app's
// Generated/ForgeFFI only when its Rust crate selects the service.
//
// Needs NSFaceIDUsageDescription (forge.toml `[ios.permissions] face_id`):
// iOS ends an app that asks for Face ID without it.

import Foundation
import SwiftUI

#if canImport(LocalAuthentication)
    import LocalAuthentication
#endif

extension ForgeServices {
    static func installBiometrics() throws {
        installNativeBiometrics(authenticator: ForgeBiometrics.shared)
    }
}

/// `BiometricAuthenticator` over `LAContext`. Also usable from SwiftUI
/// directly: `await ForgeBiometrics.shared.authenticate(reason:)`.
public final class ForgeBiometrics: BiometricAuthenticator, @unchecked Sendable {
    public static let shared = ForgeBiometrics()

    private init() {}

    public func availability() -> BiometricAvailability {
        #if canImport(LocalAuthentication) && os(iOS)
            let context = LAContext()
            var error: NSError?
            let biometrics = context.canEvaluatePolicy(.deviceOwnerAuthenticationWithBiometrics, error: &error)
            let kind: BiometryKind
            switch context.biometryType {
            case .faceID: kind = .faceId
            case .touchID: kind = .touchId
            case .opticID: kind = .opticId
            default: kind = .none
            }
            let passcode = LAContext().canEvaluatePolicy(.deviceOwnerAuthentication, error: nil)
            return BiometricAvailability(kind: kind, biometricsUsable: biometrics, passcodeSet: passcode)
        #else
            return BiometricAvailability(kind: .none, biometricsUsable: false, passcodeSet: false)
        #endif
    }

    public func authenticate(reason: String) async -> AuthenticationResult {
        #if canImport(LocalAuthentication) && os(iOS)
            // A new context each time, so an earlier success is never reused.
            let context = LAContext()
            do {
                let ok = try await context.evaluatePolicy(.deviceOwnerAuthentication, localizedReason: reason)
                return ok ? .authenticated : .failed
            } catch let error as LAError {
                switch error.code {
                case .userCancel, .systemCancel, .appCancel, .userFallback: return .cancelled
                case .biometryLockout: return .lockedOut
                case .passcodeNotSet, .biometryNotAvailable, .biometryNotEnrolled, .notInteractive: return .unavailable
                default: return .failed
                }
            } catch {
                return .failed
            }
        #else
            return .unavailable
        #endif
    }
}

#if os(iOS)
    extension View {
        /// Locks the app behind Face ID, Touch ID or the passcode while
        /// `isEnabled`: at launch, and when it comes back after more than
        /// `graceSeconds` in the background. While locked, or while the app
        /// is not active (the app switcher), the content is covered, so its
        /// snapshot shows nothing private. Attach once, to the root view;
        /// keep `isEnabled` in the app's saved settings (Rust).
        ///
        /// The lock screen's button is `appLock.unlock`.
        public func forgeAppLock(
            isEnabled: Bool, graceSeconds: TimeInterval = 60, reason: String = "Unlock the app"
        ) -> some View {
            modifier(ForgeAppLock(isEnabled: isEnabled, graceSeconds: graceSeconds, reason: reason))
        }
    }

    private struct ForgeAppLock: ViewModifier {
        let isEnabled: Bool
        let graceSeconds: TimeInterval
        let reason: String

        @Environment(\.scenePhase) private var scenePhase
        @State private var locked = true
        @State private var leftAt: Date?
        @State private var authenticating = false
        @State private var message: String?

        func body(content: Content) -> some View {
            content
                .overlay {
                    if isEnabled && (locked || scenePhase != .active) {
                        lockScreen
                    }
                }
                .onChange(of: isEnabled) { _, _ in
                    // Turning the lock on doesn't lock the open app (the next
                    // return from the background does); turning it off unlocks.
                    locked = false
                }
                .onAppear {
                    if isEnabled { unlock() } else { locked = false }
                }
                .onChange(of: scenePhase) { _, phase in
                    guard isEnabled else { return }
                    switch phase {
                    case .background:
                        // Only the background counts: a Face ID prompt makes
                        // the app inactive, and must not lock it again.
                        if !locked { leftAt = Date() }
                    case .active:
                        if let leftAt, Date().timeIntervalSince(leftAt) > graceSeconds { locked = true }
                        leftAt = nil
                        if locked { unlock() }
                    default:
                        break
                    }
                }
        }

        private var lockScreen: some View {
            ZStack {
                Rectangle().fill(.background).ignoresSafeArea()
                VStack(spacing: 16) {
                    Image(systemName: "lock.fill")
                        .font(.largeTitle)
                        .foregroundStyle(.secondary)
                        .accessibilityHidden(true)
                    Text("Locked").font(.title2.bold())
                    if let message {
                        Text(message).font(.footnote).foregroundStyle(.secondary).multilineTextAlignment(.center)
                    }
                    if locked {
                        Button("Unlock with \(ForgeBiometrics.shared.availability().displayName)") { unlock() }
                            .buttonStyle(.borderedProminent)
                            .disabled(authenticating)
                            .accessibilityIdentifier("appLock.unlock")
                    }
                }
                .padding()
            }
        }

        private func unlock() {
            guard !authenticating else { return }
            authenticating = true
            Task { @MainActor in
                let result = await ForgeBiometrics.shared.authenticate(reason: reason)
                authenticating = false
                switch result {
                case .authenticated:
                    locked = false
                    message = nil
                case .unavailable:
                    // No passcode at all: a lock that can never open helps
                    // nobody. The setting should have been refused.
                    locked = false
                    message = nil
                case .lockedOut:
                    message = "Too many attempts. Unlock with your passcode."
                case .cancelled, .failed:
                    message = nil
                }
            }
        }
    }

    extension BiometricAvailability {
        /// "Face ID", "Touch ID", "Optic ID" or "Passcode", for buttons and
        /// settings.
        public var displayName: String {
            guard biometricsUsable else { return "Passcode" }
            switch kind {
            case .faceId: return "Face ID"
            case .touchId: return "Touch ID"
            case .opticId: return "Optic ID"
            case .none: return "Passcode"
            }
        }
    }
#endif
