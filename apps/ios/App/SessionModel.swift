import Foundation
import Observation

struct EditorKey: Hashable { let host: String; let target: Target }
struct QuestionKey: Hashable { let editor: EditorKey; let requestId: String }

@MainActor @Observable final class SessionModel {
    private let core: Core
    private var connections: [String: Connection] = [:]
    private var generations: [String: UUID] = [:]
    private(set) var hosts: [Host] = []
    private(set) var rows: [String: Rows] = [:]
    private(set) var host: String?
    private(set) var instance: String?
    private(set) var busy = false
    private(set) var message = ""
    private var drafts: [EditorKey: RestoredDraft] = [:]
    private var answers: [QuestionKey: (json: String, text: String)] = [:]
    private var selection = UUID()
    var editor: EditorKey? {
        guard let host, let target = rows[host]?.ready(instance) else { return nil }
        return EditorKey(host: host, target: target)
    }
    private var needsReload = true
    init(core: Core) { self.core = core; retryRegistrations() }

    func retryRegistrations() {
        guard needsReload else { return }
        do { hosts = try core.hosts() } catch { message = "保存済みの接続を開けません"; return }
        needsReload = false
        message = ""
        for h in hosts where !h.needsPairing && connections[h.id] == nil {
            do { try connect(h) } catch { needsReload = true; message = "接続を開けません" }
        }
    }
    private func connect(_ h: Host) throws {
        let id = h.id, generation = UUID()
        let updates = Updates { [weak self] batches in
            guard let self, self.generations[id] == generation, let c = self.connections[id] else { return }
            var r = self.rows[id] ?? Rows()
            for (rev, changes) in batches { r.apply(rev, changes, snapshot: c.snapshot) }
            self.publish(r, id: id)
        }
        let c = try core.connect(id: id, listener: updates)
        generations[id] = generation; connections[id] = c; publish(Rows(c.snapshot()), id: id)
    }
    private func publish(_ r: Rows, id: String) {
        let old = editor
        rows[id] = r
        if editor != old { selection = UUID(); message = "" }
        if let key = editor, key.host == id {
            let live = Dictionary(uniqueKeysWithValues: (r.groups["question"] ?? []).map { ($0.id, $0.json) })
            answers = answers.filter { $0.key.editor != key || live[$0.key.requestId] == $0.value.json }
        }
    }
    func pair(_ link: String) async throws {
        guard !busy else { throw CoreError.Blocked }; busy = true; defer { busy = false }
        let h = try await core.pair(link: link, name: "PiMate iOS")
        hosts = try core.hosts(); try connect(h)
    }
    func select(host id: String, instance next: String?) throws {
        guard let c = connections[id] else { throw CoreError.Offline }
        if host != id, let host { try connections[host]?.subscribe(instance: nil) }
        try c.subscribe(instance: next)
        selection = UUID(); host = id; instance = next; message = ""; publish(Rows(c.snapshot()), id: id)
    }
    func body(_ key: EditorKey) -> RestoredDraft { drafts[key] ?? RestoredDraft(text: "", images: []) }
    func text(_ value: String, for key: EditorKey) {
        guard editor == key else { return }; var d = body(key); d.text = value; drafts[key] = d
    }
    func send(_ key: EditorKey, _ sent: RestoredDraft) async throws {
        guard sent == body(key) else { throw CoreError.StaleTarget }
        try await perform(.send(target: key.target, text: sent.text, images: sent.images), key) {
            if self.body(key) == sent { self.drafts[key] = nil }
        }
    }
    func abort(_ key: EditorKey) async throws { try await perform(.abort(target: key.target), key) }
    func answerText(_ q: QuestionKey) -> String { answers[q]?.text ?? "" }
    private func live(_ q: QuestionKey, shown: String) -> Bool {
        editor == q.editor && rows[q.editor.host]?.groups["question"]?.contains { $0.id == q.requestId && $0.json == shown } == true
    }
    func editAnswer(_ q: QuestionKey, shown: String, text: String) {
        if live(q, shown: shown) { answers[q] = (shown, text) }
    }
    func answer(_ q: QuestionKey, shown: String, fields: [String: Any]) async throws {
        guard live(q, shown: shown) else { throw CoreError.StaleTarget }
        var params = fields; params["requestId"] = q.requestId
        let json = try JSONSerialization.data(withJSONObject: params)
        try await perform(.answer(target: q.editor.target, params: String(decoding: json, as: UTF8.self)), q.editor)
    }
    private func perform(_ op: Operation, _ key: EditorKey, accepted: () -> Void = {}) async throws {
        guard key == editor, let c = connections[key.host] else { throw CoreError.StaleTarget }
        guard !busy else { throw CoreError.Blocked }
        let origin = selection
        busy = true; defer { busy = false }
        message = ""
        do { try c.check(operation: op) } catch { message = "操作は送信されませんでした"; throw error }
        do { _ = try await c.perform(operation: op) }
        catch {
            if editor == key, origin == selection, connections[key.host] === c {
                message = (error as? CoreError) == .Rejected ? "操作は拒否されました" : "操作を確認できません。再送前に履歴を確認してください"
            }
            throw error
        }
        accepted() // The confirmed receipt belongs to the sender, not the currently visible editor.
        guard key == editor, origin == selection, connections[key.host] === c else { throw CoreError.StaleTarget }
    }
    // The picker passes the key captured at presentation and temporary local files. Never apply a late result to a new editor.
    func attach(_ key: EditorKey, files: [URL]) async throws {
        guard key == editor, let r = rows[key.host], r.field("promptImages") == "true" else { throw CoreError.Blocked }
        let existing = body(key).images, origin = selection, count = files.count
        let limit = min(4, Int(r.field("promptImageLimit") ?? "1") ?? 1)
        guard count > 0, count <= limit - existing.count else { throw CoreError.Blocked }
        let used = existing.reduce(0) { $0 + $1.data.utf8.count / 4 * 3 - $1.data.suffix(2).filter { $0 == "=" }.count }
        let budget = min(512 * 1024, (600 * 1024 - used) / count)
        let images = try await Task.detached { try files.map { url in
            try autoreleasepool { try encodePhoto(url, budget: budget) }
        } }.value
        try Task.checkCancellation()
        guard editor == key, origin == selection, body(key).images == existing else { throw CoreError.StaleTarget }
        var d = body(key); d.images += images; drafts[key] = d
    }
    func restore(_ key: EditorKey, id: String) throws {
        guard key == editor, let c = connections[key.host], !busy,
              body(key).text.isEmpty, body(key).images.isEmpty else { throw CoreError.Blocked }
        drafts[key] = try c.takeUnconfirmed(target: key.target, id: id)
    }
}
