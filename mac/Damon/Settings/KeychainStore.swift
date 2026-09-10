import Foundation
import LocalAuthentication
import Security

protocol SecretStore { func read() throws -> String?; func set(_ value: String) throws; func delete() throws }

enum KeychainError: Error { case status(OSStatus) }

struct KeychainStore: SecretStore, @unchecked Sendable {
    let service: String
    let account: String

    init(service: String? = nil, account: String = "opencode-zen-api-key") {
        self.service = service ?? ProcessInfo.processInfo.environment["DAMON_KEYCHAIN_SERVICE"] ?? "com.savagemechanic.damon"
        self.account = account
    }

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
        let data = Data(value.utf8)
        let updated = SecItemUpdate(query as CFDictionary, [kSecValueData as String: data] as CFDictionary)
        if updated == errSecSuccess { return }
        guard updated == errSecItemNotFound else { throw KeychainError.status(updated) }
        var item = query
        item[kSecValueData as String] = data
        let added = SecItemAdd(item as CFDictionary, nil)
        guard added == errSecSuccess else { throw KeychainError.status(added) }
    }

    func delete() throws {
        let status = SecItemDelete(query as CFDictionary)
        guard status == errSecSuccess || status == errSecItemNotFound else { throw KeychainError.status(status) }
    }

    func readWithTimeout(seconds: Double = 1) async -> String? {
        await withCheckedContinuation { continuation in
            let gate = KeychainContinuationGate(continuation)
            DispatchQueue.global(qos: .userInitiated).async { gate.resume((try? read()) ?? nil) }
            DispatchQueue.global().asyncAfter(deadline: .now() + seconds) { gate.resume(nil) }
        }
    }
}

private final class KeychainContinuationGate: @unchecked Sendable {
    private let lock = NSLock()
    private var continuation: CheckedContinuation<String?, Never>?
    init(_ continuation: CheckedContinuation<String?, Never>) { self.continuation = continuation }
    func resume(_ value: String?) {
        lock.lock()
        let pending = continuation
        continuation = nil
        lock.unlock()
        pending?.resume(returning: value)
    }
}
