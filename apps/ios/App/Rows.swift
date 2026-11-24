import Foundation

struct Rows {
    var revision: UInt64 = 0
    var groups: [String: [Item]] = [:]
    init() {}
    init(_ snapshot: Snapshot) {
        revision = snapshot.revision
        groups = Dictionary(grouping: snapshot.items, by: \.collection).mapValues { $0.sorted { $0.position < $1.position } }
    }
    mutating func apply(_ rev: UInt64, _ changes: [Change], snapshot: () -> Snapshot) {
        guard rev > revision else { return }
        if changes.count > 64 || changes.contains(.resyncRequired) { self = Rows(snapshot()); return }
        for change in changes {
            switch change {
            case .upsert(let item):
                var items = groups[item.collection] ?? []
                items.removeAll { $0.id == item.id }
                items.insert(item, at: min(Int(clamping: item.position), items.count))
                groups[item.collection] = items
            case .remove(let collection, let id): groups[collection]?.removeAll { $0.id == id }
            case .resyncRequired: break
            }
        }
        revision = rev
    }
    func field(_ id: String) -> String? { groups["view"]?.first { $0.id == id }?.json }
    func ready(_ selected: String?) -> Target? {
        guard let selected, field("phase") == "\"ready\"", let instance = field("instanceId"),
              (try? JSONDecoder().decode(String.self, from: Data(instance.utf8))) == selected else { return nil }
        var session: String?
        if let json = field("session"), json != "null" {
            struct Session: Decodable { let id: String }
            guard let s = try? JSONDecoder().decode(Session.self, from: Data(json.utf8)) else { return nil }
            session = s.id
        }
        return Target(instance: selected, session: session)
    }
}

// One queued main-thread drain per host. On overload only a snapshot can close the gap.
final class Updates: Listener, @unchecked Sendable {
    typealias Batch = (UInt64, [Change])
    private let lock = NSLock()
    private var pending: [Batch] = []
    private let deliver: @MainActor ([Batch]) -> Void
    init(_ deliver: @escaping @MainActor ([Batch]) -> Void) { self.deliver = deliver }
    func changed(revision: UInt64, changes: [Change]) {
        lock.withLock {
            if pending.isEmpty {
                DispatchQueue.main.async {
                    let batches = self.lock.withLock { let result = self.pending; self.pending.removeAll(); return result }
                    self.deliver(batches)
                }
            }
            if pending.count >= 8 { pending = [(revision, [.resyncRequired])] }
            else { pending.append((revision, changes.count > 64 ? [.resyncRequired] : changes)) }
        }
    }
}
