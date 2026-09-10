import Foundation
import LocalAuthentication
import Security

protocol SecretStore { func read() throws -> String?; func set(_ value: String) throws; func delete() throws }

enum KeychainError: Error { case status(OSStatus) }

struct KeychainStore: SecretStore, @unchecked Sendable {
    let service = "com.savagemechanic.damon"
    let account = "opencode-zen-api-key"

    private var query: [String: Any] { [kSecClass as String: kSecClassGenericPassword, kSecAttrService as String: service, kSecAttrAccount as String: account] }

    func read() throws -> String? {
        var request = query
        request[kSecReturnData as String] = true
        request[kSecMatchLimit as String] = kSecMatchLimitOne
        let context = LAContext()
        context.interactionNotAllowed = true
        request[kSecUseAuthenticationContext as String] = context
        var result: CFTypeRef?
        let status = SecItemCopyMatching(request as CFDictionary, &result)
        if status == errSecItemNotFound { return nil }
        guard status == errSecSuccess, let data = result as? Data else { throw KeychainError.status(status) }
        return String(data: data, encoding: .utf8)
    }

    func set(_ value: String) throws {
        try delete()
        var request = query
        request[kSecValueData as String] = Data(value.utf8)
        let status = SecItemAdd(request as CFDictionary, nil)
        guard status == errSecSuccess else { throw KeychainError.status(status) }
    }

    func delete() throws {
        let status = SecItemDelete(query as CFDictionary)
        guard status == errSecSuccess || status == errSecItemNotFound else { throw KeychainError.status(status) }
    }
}
