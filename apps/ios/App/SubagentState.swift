import Foundation
import Observation

struct AgentChild: Equatable { let run: String, child: String }
struct StopTarget: Equatable {
    let editor: EditorKey, generation: UUID, run: String, viewedChild: String?, action: String
    var child: String? { action == "stop_child" ? viewedChild : nil }
}
extension Rows {
    var runs: [Object] { (groups["run"] ?? []).map(\.object) }
    func originalRun(toolCallId: String) -> String? {
        guard !toolCallId.isEmpty else { return nil }; return runs.first { $0.str("toolCallId") == toolCallId }?["id"] as? String
    }
    func resumedChild(run: String, index: Int?) -> AgentChild? {
        let children = runs.first { $0.str("id") == run }?.list("children") ?? []
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
        guard let run = runs.first(where: { $0.str("id") == id }), run["active"] as? Bool == true else { return nil }
        var action = run.str("stopAction")
        if let viewedChild {
            guard let c = run.list("children").first(where: { $0.str("id") == viewedChild }), ["running", "waiting", "queued"].contains(c.str("state")) else { return nil }
            if (c["actions"] as? [String])?.contains("stop_child") == true { action = "stop_child" }
        }
        guard ["stop", "stop_child", "interrupt", "abort_parent"].contains(action) else { return nil }
        return .init(editor: key, generation: viewGeneration, run: id, viewedChild: viewedChild, action: action)
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
    var units: Int { [key.host, key.target.instance, key.target.session ?? "", unit.run, unit.child, action, text].reduce(0) { $0 + $1.utf16.count } }
}
struct ChildInstructions {
    private var sent: [SentInstruction] = []
    mutating func accept(_ value: SentInstruction) {
        guard value.units <= 65536 else { return }; var total = sent.reduce(value.units) { $0 + $1.units }
        while sent.count >= 128 || total > 65536 { total -= sent.removeFirst().units }; sent.append(value)
    }
    mutating func forget(_ host: String) { sent.removeAll { $0.key.host == host } }
    func forChild(_ key: EditorKey, _ unit: AgentChild, resumedFrom: String?) -> [SentInstruction] {
        let matching = sent.filter { $0.key == key && $0.unit.child == unit.child }
        return matching.filter { $0.unit.run == unit.run } + matching.filter { $0.unit.run != unit.run && $0.unit.run == resumedFrom }
    }
}
extension SessionModel {
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
        guard snapshot.ready(key.target.instance) == key.target,
              snapshot.stopTarget(key, run: target.run, viewedChild: target.viewedChild)?.action == target.action else { throw CoreError.StaleTarget }
        var fields: Object = ["action": target.action, "runId": target.run]; fields["childId"] = target.child
        return try decodeObject(await request(key, .subagentControl, fields))
    }
    func control(_ key: EditorKey, _ unit: AgentChild, action: String, text: String) async throws -> Object {
        let body = text.trimmingCharacters(in: .whitespacesAndNewlines)
        guard ["steer", "follow_up", "resume"].contains(action), !body.isEmpty else { throw CoreError.BadRequest }
        let result = try decodeObject(await request(key, .subagentControl, ["runId": unit.run, "childId": unit.child, "action": action, "text": body]))
        instructions.accept(.init(key: key, unit: unit, action: action, text: body)); return result
    }
}
