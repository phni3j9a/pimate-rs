import Foundation

struct Rows {
    var revision: UInt64 = 0
    var groups: [String: [Item]] = [:]
    var notices = ChatNotices()
    private(set) var viewGeneration = UUID()
    private(set) var connectionGeneration = UUID()
    init() {}
    init(_ snapshot: Snapshot) {
        revision = snapshot.revision
        groups = Dictionary(grouping: snapshot.items, by: \.collection).mapValues { $0.sorted { $0.position < $1.position } }
    }
    mutating func apply(_ rev: UInt64, _ changes: [Change], snapshot: () -> Snapshot) {
        guard rev > revision else { return }
        if changes.count > 64 || changes.contains(.resyncRequired) { replace(snapshot()); return }
        for change in changes {
            let group: String, id: String
            switch change {
            case .upsert(let item): (group, id) = (item.collection, item.id)
            case .remove(let g, let i): (group, id) = (g, i)
            case .resyncRequired: continue
            }
            if group == "host" && id == "connection" { connectionGeneration = UUID() }
            let tracksTarget = group == "view" && ["phase", "instanceId", "session", "startedAt"].contains(id)
            let before = tracksTarget ? ready(pi) : nil
            let start = tracksTarget ? field("startedAt") : nil
            switch change {
            case .upsert(let item):
                notices.update(item.collection, item.id, item, instance: pi)
                var items = groups[item.collection] ?? []
                items.removeAll { $0.id == item.id }
                items.insert(item, at: min(Int(clamping: item.position), items.count))
                groups[item.collection] = items
            case .remove(let collection, let id): notices.update(collection, id, nil, instance: pi); groups[collection]?.removeAll { $0.id == id }
            case .resyncRequired: break
            }
            if tracksTarget && (before != ready(pi) || start != field("startedAt")) { viewGeneration = UUID() }
        }
        revision = rev
        reconcileMove()
    }
    private mutating func reconcileMove() {
        guard notices.move == nil, let target = ready(pi), let dest = target.session,
              let item = groups["notice"]?.first(where: { $0.id == "sessionChanged" }),
              item.object.obj("to")["id"] as? String == dest else { return }
        notices.update("notice", "sessionChanged", item, instance: target.instance, snapshot: true)
    }
    private var pi: String? { groups["view"]?.first { $0.id == "instanceId" }?.string }
    mutating func replace(_ snapshot: Snapshot) {
        guard snapshot.revision >= revision else { return }
        let previous = revision; var tracked = notices
        let generation = viewGeneration, target = ready(pi), start = field("startedAt")
        let connection = connectionGeneration
        self = Rows(snapshot)
        if revision == previous { connectionGeneration = connection }
        if target == ready(pi), start == field("startedAt") { viewGeneration = generation }
        for (group, id) in [("operationResult", "latest"), ("busy", "operation"), ("notice", "sessionChanged")] {
            if group != "notice" || previous != revision { tracked.update(group, id, groups[group]?.first { $0.id == id }, instance: pi, snapshot: true) }
        }
        notices = tracked
        reconcileMove()
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
