import Foundation
import Security

// The single value holds the complete registration; no token in Keychain attributes or logs.
final class KeychainStore: SecretStore, @unchecked Sendable {
    private struct Record: Codable {
        var id, name, address, device, token: String
        var invalid: Bool
        var host: Host { Host(id: id, name: name, address: address, deviceId: device, needsPairing: invalid) }
    }
    private let service: String
    private static let lock = NSLock()
    init(service: String = "dev.pimate.app.rs.debug.hosts.v1") { self.service = service }

    static func forLaunch(environment: [String: String] = ProcessInfo.processInfo.environment) -> KeychainStore {
        if environment["PIMATE_TEST_STORE"] == "1" || environment["XCTestConfigurationFilePath"] != nil {
            return KeychainStore(service: "dev.pimate.tests.\(UUID())")
        }
        return KeychainStore()
    }

    private func query(_ id: String? = nil) -> [CFString: Any] {
        var q: [CFString: Any] = [kSecClass: kSecClassGenericPassword, kSecAttrService: service]
        if let id { q[kSecAttrAccount] = id }
        return q
    }
    private func record(_ id: String) throws -> Record? {
        var q = query(id); q[kSecReturnData] = true
        var value: CFTypeRef?
        let status = SecItemCopyMatching(q as CFDictionary, &value)
        if status == errSecItemNotFound { return nil }
        guard status == errSecSuccess, let data = value as? Data,
              let r = try? JSONDecoder().decode(Record.self, from: data), r.id == id else { throw CallbackError.Failed }
        return r
    }
    private func put(_ r: Record) throws {
        let q = query(r.id)
        let fields: [CFString: Any] = [kSecValueData: try JSONEncoder().encode(r),
                                      kSecAttrAccessible: kSecAttrAccessibleWhenUnlockedThisDeviceOnly]
        var status = SecItemUpdate(q as CFDictionary, fields as CFDictionary)
        if status == errSecItemNotFound { status = SecItemAdd(q.merging(fields) { _, v in v } as CFDictionary, nil) }
        guard status == errSecSuccess else { throw CallbackError.Failed }
    }
    func hosts() throws -> [Host] {
        try Self.lock.withLock {
            var q = query(); q[kSecReturnAttributes] = true; q[kSecMatchLimit] = kSecMatchLimitAll
            var result: CFTypeRef?
            let status = SecItemCopyMatching(q as CFDictionary, &result)
            if status == errSecItemNotFound { return [] }
            guard status == errSecSuccess, let rows = result as? [[CFString: Any]] else { throw CallbackError.Failed }
            return try rows.map {
                guard let id = $0[kSecAttrAccount] as? String, let r = try record(id) else { throw CallbackError.Failed }
                return r.host
            }.sorted { $0.id < $1.id }
        }
    }
    func load(id: String) throws -> Credential {
        try Self.lock.withLock {
            guard let r = try record(id) else { throw CallbackError.Failed }
            return Credential(host: r.host, token: r.token)
        }
    }
    func save(host: Host, token: String) throws {
        try Self.lock.withLock { try put(Record(id: host.id, name: host.name, address: host.address,
                                                device: host.deviceId, token: token, invalid: host.needsPairing)) }
    }
    func invalidate(id: String, deviceId: String) throws {
        try Self.lock.withLock {
            guard var r = try record(id), r.device == deviceId else { return }
            r.invalid = true; r.token = ""; try put(r)
        }
    }
    func forget(id: String) throws {
        try Self.lock.withLock {
            let status = SecItemDelete(query(id) as CFDictionary)
            guard status == errSecSuccess || status == errSecItemNotFound else { throw CallbackError.Failed }
        }
    }
}
