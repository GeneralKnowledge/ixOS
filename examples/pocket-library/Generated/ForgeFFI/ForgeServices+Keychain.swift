// Forge native services: Keychain. Source of truth: crates/forge-services/swift/.

import Foundation
import Security

/// `SecureStore` backed by the Keychain (generic passwords).
///
/// Items are readable after the device's first unlock following a restart,
/// so background work can use them, and are never synced to iCloud or moved
/// to another device (`AfterFirstUnlockThisDeviceOnly`). They survive app
/// updates and can survive deleting the app.
public final class KeychainSecureStore: SecureStore, @unchecked Sendable {
    public static var defaultService: String {
        (Bundle.main.bundleIdentifier ?? "forge.app") + ".forge-secure-store"
    }

    /// The Keychain service name (`kSecAttrService`); keys are accounts.
    public let service: String

    public init(service: String = KeychainSecureStore.defaultService) {
        self.service = service
    }

    public func get(key: String) throws -> Data? {
        var query = baseQuery(key)
        query[kSecReturnData as String] = true
        query[kSecMatchLimit as String] = kSecMatchLimitOne
        var item: CFTypeRef?
        let status = SecItemCopyMatching(query as CFDictionary, &item)
        switch status {
        case errSecSuccess:
            return item as? Data
        case errSecItemNotFound:
            return nil
        default:
            throw Self.failure(status, "reading \(key)")
        }
    }

    public func set(key: String, value: Data) throws {
        let query = baseQuery(key)
        let attributes: [String: Any] = [
            kSecValueData as String: value,
            kSecAttrAccessible as String: kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly,
        ]
        var status = SecItemUpdate(query as CFDictionary, attributes as CFDictionary)
        if status == errSecItemNotFound {
            let item = query.merging(attributes) { _, new in new }
            status = SecItemAdd(item as CFDictionary, nil)
        }
        guard status == errSecSuccess else {
            throw Self.failure(status, "writing \(key)")
        }
    }

    public func delete(key: String) throws {
        let status = SecItemDelete(baseQuery(key) as CFDictionary)
        guard status == errSecSuccess || status == errSecItemNotFound else {
            throw Self.failure(status, "deleting \(key)")
        }
    }

    private func baseQuery(_ key: String) -> [String: Any] {
        [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrService as String: service,
            kSecAttrAccount as String: key,
        ]
    }

    static func failure(_ status: OSStatus, _ what: String) -> NativeServiceError {
        let text = (SecCopyErrorMessageString(status, nil) as String?) ?? "OSStatus \(status)"
        return .Platform(code: Int64(status), reason: "Keychain \(what): \(text)")
    }
}
