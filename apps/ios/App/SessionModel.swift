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
    private let images = ChatImages()
    func image(_ key: EditorKey, source: ChatImageSource, thumbnail: Bool) async throws -> Data {
        guard key == editor else { throw CoreError.StaleTarget }; let origin = selection
        let data = try await images.load(source, thumbnail: thumbnail) { method, fields in try await self.request(key, method, fields) }
        guard key == editor, origin == selection else { throw CoreError.StaleTarget }; return data
    }
    private(set) var sheet: ChatSheet?
    func openSheet(_ key: EditorKey, page: ChatSheet.Page) throws -> ChatSheet {
        guard key == editor else { throw CoreError.StaleTarget }
        let opened = ChatSheet(key: key, page: page); sheet = opened; return opened
    }
    func closeSheet(_ opened: ChatSheet) { if sheet?.id == opened.id { sheet = nil } }
    func run(_ action: SheetAction, in opened: ChatSheet) async throws {
        guard sheet?.id == opened.id, opened.key == editor else { throw CoreError.StaleTarget }
        guard sheet?.loading == false else { throw CoreError.Blocked }
        sheet?.loading = true; sheet?.error = nil; sheet?.reply = nil
        defer { if sheet?.id == opened.id { sheet?.loading = false } }
        let (method, fields) = action.request
        do {
            let raw = try await request(opened.key, method, fields)
            guard sheet?.id == opened.id else { throw CoreError.StaleTarget }
            sheet?.reply = try JSONSerialization.jsonObject(with: Data(raw.utf8)) as? Object
        } catch {
            if sheet?.id == opened.id { sheet?.error = (error as? CoreError) ?? .OutcomeUnknown }; throw error
        }
    }
    func request(_ key: EditorKey, _ method: RcMethod, _ fields: Object) async throws -> String {
        guard key == editor, let c = connections[key.host] else { throw CoreError.StaleTarget }
        let origin = selection, data = try JSONSerialization.data(withJSONObject: fields)
        let result = try await c.perform(operation: .rc(target: key.target, method: method, params: String(decoding: data, as: UTF8.self)))
        guard editor == key, selection == origin, connections[key.host] === c else { throw CoreError.StaleTarget }; return result
    }
    func stats(_ key: EditorKey) -> SessionStats? { editor == key ? connections[key.host]?.sessionStats() : nil }
    func allowances(_ key: EditorKey, nowMs: UInt64) -> [Allowance] {
        guard editor == key else { return [] }; return connections[key.host]?.allowances(nowMs: nowMs) ?? []
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
        if editor != old { images.clear(); sheet = nil; selection = UUID(); message = "" }
        if let key = editor, key.host == id {
            if let result = r.notices.receipt, !result.textHandled, result.instance == key.target.instance,
               result.outcome == .done, let dest = result.value["sessionId"] as? String, dest == key.target.session,
               let text = [result.value.str("forkText"), result.value.obj("event").str("editorText")].first(where: { !$0.isEmpty }) {
                rows[id]?.notices.receipt?.textHandled = true
                if body(key).text.isEmpty && body(key).images.isEmpty { self.text(text, for: key) }
            }
            let live = Dictionary(uniqueKeysWithValues: (r.groups["question"] ?? []).map { ($0.id, $0.json) })
            answers = answers.filter { $0.key.editor != key || live[$0.key.requestId] == $0.value.json }
        }
    }
    func pair(_ link: String) async throws {
        guard !busy else { throw CoreError.Blocked }; busy = true; defer { busy = false }
        let h = try await core.pair(link: link, name: "PiMate iOS")
        // Registration is committed. Retry only local reads, never the one-time pairing request.
        generations[h.id] = nil; connections[h.id] = nil; rows[h.id] = nil
        needsReload = true
        retryRegistrations()
    }
    func select(host id: String, instance next: String?) throws {
        guard let c = connections[id] else { throw CoreError.Offline }
        if host != id, let host { try connections[host]?.subscribe(instance: nil) }
        try c.subscribe(instance: next)
        images.clear(); sheet = nil; selection = UUID(); host = id; instance = next; message = ""
        var r = rows[id] ?? Rows(); r.replace(c.snapshot()); publish(r, id: id)
    }
    var edge: ChatEdge? {
        guard let host, let instance, let r = rows[host] else { return nil }; return ChatEdge(r, instance: instance)
    }
    var needsMoveConfirmation: Bool {
        guard let key = editor, let move, !move.local, !move.confirmed, let from = move.notice.obj("from")["id"] as? String else { return false }
        let previous = body(EditorKey(host: key.host, target: Target(instance: key.target.instance, session: from)))
        return !previous.text.isEmpty || !previous.images.isEmpty
    }
    var receipt: ChatReceipt? {
        guard let host, let r = rows[host]?.notices.receipt, r.instance == instance, !r.dismissed else { return nil }; return r
    }
    func dismissReceipt(host id: String, _ shown: ChatReceipt) {
        guard id == host, receipt?.id == shown.id, instance == shown.instance else { return }; rows[id]?.notices.receipt?.dismissed = true
    }
    var move: ChatMove? {
        guard let key = editor, let m = rows[key.host]?.notices.move, m.instance == key.target.instance,
              m.notice.obj("to")["id"] as? String == key.target.session else { return nil }; return m
    }
    func confirmMove(_ key: EditorKey, id: UUID) {
        guard key == editor, move?.id == id else { return }; rows[key.host]?.notices.move?.confirmed = true
    }
    func body(_ key: EditorKey) -> RestoredDraft { drafts[key] ?? RestoredDraft(text: "", images: []) }
    func text(_ value: String, for key: EditorKey) {
        guard editor == key else { return }; var d = body(key); d.text = value; drafts[key] = d
    }
    func send(_ key: EditorKey, _ sent: RestoredDraft) async throws {
        guard !needsMoveConfirmation, attaching == nil else { throw CoreError.Blocked }
        guard sent == body(key) else { throw CoreError.StaleTarget }
        try await perform(.send(target: key.target, text: sent.text, images: sent.images), key) {
            if self.body(key) == sent { self.drafts[key] = nil }
        }
    }
    func removeAttachment(_ key: EditorKey, shown: [Image], at index: Int) throws {
        guard editor == key, body(key).images == shown, shown.indices.contains(index) else { throw CoreError.StaleTarget }
        var d = body(key); d.images.remove(at: index); drafts[key] = d
    }

    func suggestions(_ key: EditorKey, text: String) -> [String] {
        guard editor == key else { return [] }; return connections[key.host]?.commandSuggestions(text: text) ?? []
    }

    // Returns the sheet to present, or nil after a command was accepted by the host.
    func builtin(_ key: EditorKey, _ sent: RestoredDraft) async throws -> String? {
        guard editor == key, body(key) == sent else { throw CoreError.StaleTarget }
        let line = sent.text.trimmingCharacters(in: .whitespacesAndNewlines), name = String(line.dropFirst())
        guard line.first == "/", sent.images.isEmpty,
              rows[key.host]?.groups["command"]?.contains(where: { $0.id == name && $0.object["source"] as? String == "builtin" }) == true else { throw CoreError.BadRequest }
        switch name {
        case "model", "resume", "fork", "tree": drafts[key] = nil; return name
        case "compact", "new", "reload":
            let op = Operation.rc(target: key.target, method: name == "compact" ? .compact : .sessionOp,
                                  params: name == "compact" ? "{}" : "{\"op\":\"\(name)\"}")
            try await perform(op, key) { if self.body(key) == sent { self.drafts[key] = nil } }; return nil
        default: throw CoreError.Unsupported
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
                switch error as? CoreError {
                case .Rejected, .Blocked, .BadRequest, .Unsupported, .BuiltinCommand:
                    message = "操作は拒否されました"
                case .Offline: message = "操作は送信されませんでした"
                case .StaleTarget: message = "操作対象が変わりました。現在の状態を確認してください"
                default: message = "操作を確認できません。再送前に履歴を確認してください"
                }
            }
            throw error
        }
        accepted() // The confirmed receipt belongs to the sender, not the currently visible editor.
        guard key == editor, origin == selection, connections[key.host] === c else { throw CoreError.StaleTarget }
    }
    // The picker passes the key captured at presentation and temporary local files. Never apply a late result to a new editor.
    private(set) var attaching: EditorKey?
    func attach(_ key: EditorKey, files: [URL]) async throws {
        guard key == editor, let r = rows[key.host], r.field("promptImages") == "true" else { throw CoreError.Blocked }
        guard !files.isEmpty else { return }
        guard attaching == nil, !busy else { throw CoreError.Blocked }
        attaching = key; defer { attaching = nil }
        let existing = body(key).images, origin = selection, count = files.count
        let limit = min(4, Int(r.field("promptImageLimit") ?? "1") ?? 1)
        guard count > 0, count <= limit - existing.count else { throw CoreError.Blocked }
        let used = existing.reduce(0) { $0 + $1.data.utf8.count / 4 * 3 - $1.data.suffix(2).filter { $0 == "=" }.count }
        let budget = min(512 * 1024, (600 * 1024 - used) / count)
        let work = Task.detached { try files.map { url in
            try Task.checkCancellation(); return try autoreleasepool { try encodePhoto(url, budget: budget) }
        } }
        let images = try await withTaskCancellationHandler { try await work.value } onCancel: { work.cancel() }
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
