import Foundation
import Observation

struct AgentChild: Equatable { let run: String, child: String }
struct StopTarget: Equatable {
    let editor: EditorKey, generation: UUID, startedAt: String, run: String, viewedChild: String?, action: String
    var child: String? { action == "stop_child" ? viewedChild : nil }
}
extension Rows {
    var runs: [Object] { (groups["run"] ?? []).map(\.object) }
    func originalRun(toolCallId: String) -> String? {
        guard !toolCallId.isEmpty else { return nil }; return runs.first { $0.str("toolCallId") == toolCallId }?["id"] as? String
    }
    func resumedChild(run: String, index: Int?) -> AgentChild? {
        let children = groups["run"]?.first { $0.id == run }?.object.list("children") ?? []
        guard let id = (children.first { index != nil && $0["index"] as? Int == index } ?? children.first)?["id"] as? String else { return nil }
        return AgentChild(run: run, child: id)
    }
    var subagentCounts: [String: Int] {
        guard !(groups["runWatch"] ?? []).isEmpty else { return [:] }
        var counts: [String: Int] = [:]
        for run in runs {
            var units = run.list("children"); if units.isEmpty { units = [run] }
            let started = units.contains { $0.str("state") != "queued" }
            for unit in units {
                switch unit.str("state") {
                case "queued" where started: break
                case "queued", "running", "stopping": counts["running", default: 0] += 1
                case "complete", "failed", "waiting", "paused", "stopped": counts[unit.str("state"), default: 0] += 1
                default: break
                }
            }
        }
        return counts
    }
    func stopTarget(_ key: EditorKey, run id: String, viewedChild: String?) -> StopTarget? {
        guard let startedAt = field("startedAt"), UInt64(startedAt) != nil,
              let run = runs.first(where: { $0.str("id") == id }), run["active"] as? Bool == true else { return nil }
        var action = run.str("stopAction")
        if let viewedChild {
            guard let c = run.list("children").first(where: { $0.str("id") == viewedChild }), ["running", "waiting", "queued"].contains(c.str("state")) else { return nil }
            if (c["actions"] as? [String])?.contains("stop_child") == true { action = "stop_child" }
        }
        guard ["stop", "stop_child", "interrupt", "abort_parent"].contains(action) else { return nil }
        return .init(editor: key, generation: viewGeneration, startedAt: startedAt, run: id, viewedChild: viewedChild, action: action)
    }
}

@MainActor @Observable final class WorkRecord {
    private(set) var items: [Object] = []
    private(set) var task = ""
    private(set) var truncated = false
    private(set) var more = false
    private(set) var loading = false
    private var cursor: String?
    private var again = false
    private var indices: [String: Int] = [:]
    private let page: (String?) async throws -> Object
    init(page: @escaping (String?) async throws -> Object) { self.page = page }
    func load() async throws -> Bool {
        guard !loading else { again = true; return false }
        loading = true; again = false; defer { loading = false }
        for _ in 0..<8 {
            try Task.checkCancellation(); let result = try await page(cursor); try Task.checkCancellation()
            if cursor == nil { indices.removeAll(); items.removeAll() }
            for item in result.list("items") {
                guard let id = item["entryId"] as? String else { throw CoreError.BadRequest }
                if let i = indices[id] { items[i] = item } else { indices[id] = items.count; items.append(item) }
            }
            if let title = result["task"] as? String { task = title }; truncated = truncated || result["truncated"] as? Bool == true
            let next = result.str("cursor"); more = !next.isEmpty && next != cursor
            if !next.isEmpty { cursor = next }; if !more { break }
        }
        return again
    }
}

struct SentInstruction {
    let key: EditorKey, unit: AgentChild, action: String, text: String
    let newRun: String?, startedAt: UInt64?, sourceGeneration: UUID?, sourceIndex: Int?
    var destinationChild: String?
    var units: Int { [newRun ?? "", destinationChild ?? "", key.host, key.target.instance, key.target.session ?? "", unit.run, unit.child, action, text].reduce(0) { $0 + $1.utf16.count } }
}
struct ChildInstructions {
    private var sent: [SentInstruction] = []
    mutating func accept(_ value: SentInstruction) {
        guard value.units <= 65536 else { return }; sent.append(value); trim()
    }
    private mutating func trim() {
        var total = sent.reduce(0) { $0 + $1.units }
        while sent.count > 128 || total > 65536 { total -= sent.removeFirst().units }
    }
    mutating func forget(_ host: String) { sent.removeAll { $0.key.host == host } }
    mutating func resolveResumes(host: String, rows: Rows) {
        guard !sent.isEmpty, let start = rows.field("startedAt").flatMap(UInt64.init),
              let instance = rows.groups["view"]?.first(where: { $0.id == "instanceId" })?.string,
              let target = rows.ready(instance) else { return }
        sent.removeAll { $0.key.host == host && $0.key.target.instance == instance && $0.startedAt != nil && $0.startedAt != start }
        var bound = false
        for i in sent.indices.reversed() {
            let s = sent[i]
            guard s.key.host == host, s.key.target == target, s.startedAt == start,
                  target.session != nil || s.sourceGeneration == rows.viewGeneration,
                  s.destinationChild == nil, let run = s.newRun,
                  let destination = rows.resumedChild(run: run, index: s.sourceIndex) else { continue }
            sent[i].destinationChild = destination.child; bound = true
            if sent[i].units > 65536 { sent.remove(at: i) }
        }
        if bound { trim() }
    }
    func resume(_ key: EditorKey, from source: AgentChild, run: String) -> SentInstruction? {
        sent.last { $0.key == key && $0.unit == source && $0.newRun == run }
    }
    func forChild(_ key: EditorKey, _ unit: AgentChild) -> [SentInstruction] {
        let matching = sent.filter { $0.key == key }
        let origin = matching.last { $0.newRun == unit.run && $0.destinationChild == unit.child }?.unit
        return matching.filter { $0.unit == unit } + matching.filter { $0.unit != unit && $0.unit == origin }
    }
}
extension SessionModel {
    func resumedChild(_ key: EditorKey, from source: AgentChild, run: String, index: Int?) -> AgentChild? {
        guard key == editor else { return nil }
        if let child = instructions.resume(key, from: source, run: run)?.destinationChild { return AgentChild(run: run, child: child) }
        // Even when an oversized instruction was not retained, its open detail can navigate.
        return rows[key.host]?.resumedChild(run: run, index: index)
    }
    func workRecord(_ key: EditorKey, _ unit: AgentChild) throws -> WorkRecord {
        guard editor == key, let generation = rows[key.host]?.viewGeneration else { throw CoreError.StaleTarget }
        return WorkRecord { cursor in
            guard self.rows[key.host]?.viewGeneration == generation else { throw CoreError.StaleTarget }
            var fields: Object = ["runId": unit.run, "childId": unit.child]; fields["cursor"] = cursor
            return try decodeObject(await self.request(key, .subagentTranscript, fields))
        }
    }
    func control(_ target: StopTarget) async throws -> Object {
        let key = target.editor
        guard key == editor, rows[key.host]?.stopTarget(key, run: target.run, viewedChild: target.viewedChild) == target else { throw CoreError.StaleTarget }
        // The Host can finish the viewed child before its delta drains onto the main actor.
        let snapshot = try currentRows(key.host)
        guard snapshot.ready(key.target.instance) == key.target, snapshot.field("startedAt") == target.startedAt,
              snapshot.stopTarget(key, run: target.run, viewedChild: target.viewedChild)?.action == target.action else { throw CoreError.StaleTarget }
        var fields: Object = ["action": target.action, "runId": target.run]; fields["childId"] = target.child
        return try decodeObject(await request(key, .subagentControl, fields))
    }
    func control(_ key: EditorKey, _ unit: AgentChild, action: String, text: String) async throws -> Object {
        let body = text.trimmingCharacters(in: .whitespacesAndNewlines)
        guard ["steer", "follow_up", "resume"].contains(action), !body.isEmpty else { throw CoreError.BadRequest }
        let start = rows[key.host]?.field("startedAt").flatMap(UInt64.init), generation = rows[key.host]?.viewGeneration
        let index = action == "resume" ? rows[key.host]?.groups["run"]?.first { $0.id == unit.run }?.object.list("children").first { $0.str("id") == unit.child }?["index"] as? Int : nil
        let result = try decodeObject(await request(key, .subagentControl, ["runId": unit.run, "childId": unit.child, "action": action, "text": body]))
        instructions.accept(.init(key: key, unit: unit, action: action, text: body,
                                  newRun: action == "resume" ? result["newRunId"] as? String : nil,
                                  startedAt: start, sourceGeneration: generation, sourceIndex: index))
        if action == "resume", let rows = rows[key.host] { instructions.resolveResumes(host: key.host, rows: rows) }
        return result
    }
}
