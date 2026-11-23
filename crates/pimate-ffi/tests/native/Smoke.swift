import Foundation

private final class Store: SecretStore, @unchecked Sendable {
    private let lock = NSLock()
    private var credential: Credential?
    func hosts() -> [Host] { lock.withLock { credential.map { [$0.host] } ?? [] } }
    func load(id: String) throws -> Credential {
        try lock.withLock {
            guard let credential, credential.host.id == id else { throw CallbackError.Failed }
            return credential
        }
    }
    func save(host: Host, token: String) { lock.withLock { credential = Credential(host: host, token: token) } }
    func invalidate(id: String, deviceId: String) {
        lock.withLock {
            if credential?.host.id == id && credential?.host.deviceId == deviceId { credential?.host.needsPairing = true }
        }
    }
    func forget(id: String) { lock.withLock { if credential?.host.id == id { credential = nil } } }
}
private final class Updates: Listener, @unchecked Sendable {
    private let lock = NSLock()
    private weak var connection: Connection?
    func attach(_ c: Connection) { lock.withLock { connection = c } }
    var detached: Bool { lock.withLock { connection == nil } }
    let (items, writer) = AsyncStream<Item>.makeStream()
    func changed(revision: UInt64, changes: [Change]) {
        if changes.contains(.resyncRequired), let c = lock.withLock({ connection }) {
            for item in c.snapshot().items { writer.yield(item) }
            return
        }
        for case let .upsert(item) in changes { writer.yield(item) }
    }
}
@main struct Smoke {
    static func main() async throws {
        let core = Core(store: Store()), updates = Updates()
        let host = try await core.pair(link: readLine()!, name: "Swift test")
        let hosts = try core.hosts()
        precondition(hosts == [host])
        try await receive(core: core, host: host, updates: updates)
        precondition(updates.detached)
    }
    static private func receive(core: Core, host: Host, updates: Updates) async throws {
        let connection = try core.connect(id: host.id, listener: updates)
        updates.attach(connection)
        let target = Target(instance: "stub", session: "s")
        do {
            try await Task {
                withUnsafeCurrentTask { $0?.cancel() }
                try await connection.followLogin(target: target)
            }.value
            preconditionFailure("cancelled observer returned normally")
        } catch is CancellationError {}
        defer { withExtendedLifetime(connection) {} }
        var items = updates.items.makeAsyncIterator()
        while let item = await items.next() { if item.collection == "instance" && item.id == "stub" { break } }
        try connection.subscribe(instance: "stub")
        while let item = await items.next() { if item.collection == "stream" && item.id == "text" && item.json == "\"hello\"" { break } }
        let doc = parse(source: "[あ😀](https://example.org)", fileImages: false)
        precondition(plainText(block: doc.blocks()[0]) == "あ😀")
        let image = parse(source: "![alt](https://example.org/img.png)", fileImages: false).images()[0]
        precondition(image.src.value() == "https://example.org/img.png")
        precondition(utf16Range(source: "あ😀", start: 3, end: 7) == Utf16Range(start: 1, end: 3))
    }
}
