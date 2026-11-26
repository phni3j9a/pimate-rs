import Foundation
import Observation

func jsonString(_ object: Object) throws -> String {
    String(decoding: try JSONSerialization.data(withJSONObject: object), as: UTF8.self)
}
func decodeObject(_ raw: String) throws -> Object {
    guard let result = try JSONSerialization.jsonObject(with: Data(raw.utf8)) as? Object else { throw CoreError.BadRequest }; return result
}

enum FirstStep { case loading, offline, empty, list }
extension Rows {
    var firstStep: FirstStep {
        let host = groups["host"] ?? []
        switch host.first(where: { $0.id == "connection" })?.string {
        case "connected":
            guard host.first(where: { $0.id == "listKnown" })?.json == "true" else { return .loading }
            return (groups["instance"] ?? []).isEmpty ? .empty : .list
        case "offline", "invalid_token", "auth_failed", "revoked", "protocol_mismatch": return .offline
        default: return .loading
        }
    }
}

struct SpawnForm {
    let host: String
    private(set) var cwd = ""
    var trust = false, askTool: Bool, message = ""
    var model: Object?, thinking: String?
    init(host: String, askTool: Bool) { self.host = host; self.askTool = askTool }
    mutating func folder(_ path: String, knownTrusted: Bool = false) { cwd = path; trust = knownTrusted }
    func operation() throws -> Operation {
        guard !cwd.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty else { throw CoreError.BadRequest }
        var fields: Object = ["cwd": cwd, "trust": trust, "askTool": askTool]
        if let model { fields["model"] = model; fields["thinking"] = thinking }
        let first = message.trimmingCharacters(in: .whitespacesAndNewlines); if !first.isEmpty { fields["message"] = first }
        return .spawn(params: try jsonString(fields))
    }
}
enum SpawnOutcome: Equatable { case ready(String), notListed(String), messageUnconfirmed(String, attach: String?) }
extension SessionModel {
    func hostModels(_ id: String) async throws -> Object? {
        let recent = (rows[id]?.groups["instance"] ?? []).sorted { ($0.object["updatedAt"] as? Int64 ?? 0) > ($1.object["updatedAt"] as? Int64 ?? 0) }
        for pi in recent.prefix(3) {
            try Task.checkCancellation()
            do { return try await machineRequest(id, .hostModels(instance: pi.id)) }
            catch CoreError.Blocked {} catch CoreError.Rejected {} catch CoreError.Unsupported {} catch CoreError.StaleTarget {}
        }
        return nil
    }
    func spawn(_ form: SpawnForm) async throws -> SpawnOutcome {
        try Task.checkCancellation()
        let value = try await machineRequest(form.host, form.operation())
        try Task.checkCancellation()
        guard let id = value["instanceId"] as? String, !id.isEmpty else { throw CoreError.OutcomeUnknown }
        guard value.str("messageError").isEmpty else { return .messageUnconfirmed(id, attach: Rows.attachCommand(value["abducoSession"] as? String)) }
        let deadline = ContinuousClock.now + .seconds(20)
        repeat {
            if rows[form.host]?.groups["instance"]?.contains(where: { $0.id == id }) == true { return .ready(id) }
            try await Task.sleep(for: .milliseconds(200))
        } while ContinuousClock.now < deadline
        return .notListed(id)
    }
}

@MainActor struct LoginSession {
    private let connection: Connection, valid: () -> Bool
    let target: Target
    init(connection: Connection, target: Target, valid: @escaping () -> Bool) {
        self.connection = connection; self.target = target; self.valid = valid
    }
    func check() throws { try Task.checkCancellation(); guard valid() else { throw CoreError.StaleTarget } }
    func request(_ method: RcMethod, _ fields: Object = [:]) async throws -> Object {
        switch method { case .authList, .authBegin, .authRespond, .authCancel, .authLogout: break; default: throw CoreError.BadRequest }
        try check(); let raw = try await connection.perform(operation: .rc(target: target, method: method, params: try jsonString(fields)))
        try check(); return try decodeObject(raw)
    }
    func followOnce() async throws -> Bool {
        try check(); let more = try await connection.followLoginOnce(target: target); try check(); return more
    }
}

enum CredentialOutcome: Equatable {
    case active, saved, savedNeedsReload, storageUnknown, failed, expired, ended
    init(_ flow: Object) {
        switch flow.str("status") {
        case "working", "input", "saving": self = .active
        case "succeeded": self = flow["refreshRequired"] as? Bool == true ? .savedNeedsReload : .saved
        case "failed": self = flow.str("error") == "storage_failed" ? .storageUnknown : .failed
        case "expired": self = .expired
        default: self = .ended
        }
    }
}

struct GithubItem {
    let editor: EditorKey, number: Int, title: String, url: URL?
    init?(_ editor: EditorKey, _ item: Object) {
        self.editor = editor
        guard let n = item["number"] as? Int, n > 0 else { return nil }; number = n; title = item.str("title")
        let link = URL(string: item.str("url")); url = WebImage.valid(link, scheme: "https") || WebImage.valid(link, scheme: "http") ? link : nil
    }
    var reference: String { "#\(number)「\(title)」" + (url.map { " \($0.absoluteString)" } ?? "") }
}

enum RemovalError: Error { case notRevoked }

struct MachineConfirmation: Equatable {
    enum Action: Equatable { case update, restart, revoke(String), forget, forgetLocal }
    let host: String, device: String, generation: UUID?, action: Action
    var warning: String {
        switch action {
        case .update: "このマシンの PiMate を実行時の最新版に更新します。確認時に表示した版と異なる場合があります。"
        case .restart: "このマシンの Host を再起動します。接続が一時切断されます。"
        case .revoke: "選んだ端末の登録を PC から取り消します。"
        case .forget: "この端末から接続情報と下書きを削除し、PC 側の登録も取り消します。"
        case .forgetLocal: "この端末からだけ接続情報と下書きを削除しますか？PC 側に登録が残る場合は、PC の Pi の /pimate devices で取り消してください。"
        }
    }
}
extension SessionModel {
    func confirmation(host id: String, _ action: MachineConfirmation.Action) throws -> MachineConfirmation {
        guard let registration = hosts.first(where: { $0.id == id }) else { throw CoreError.StaleTarget }
        return .init(host: id, device: registration.deviceId, generation: [.forget, .forgetLocal].contains(action) ? nil : rows[id]?.connectionGeneration, action: action)
    }
    func confirm(_ approved: MachineConfirmation) async throws -> Object {
        guard try confirmation(host: approved.host, approved.action) == approved else { throw CoreError.StaleTarget }
        switch approved.action {
        case .update: return try await machineRequest(approved.host, .update)
        case .restart: return try await machineRequest(approved.host, .restart)
        case .revoke(let device): return try await machineRequest(approved.host, .revoke(deviceId: device))
        case .forget, .forgetLocal: try await removeMachine(approved, localOnly: approved.action == .forgetLocal); return [:]
        }
    }
}


enum GithubStatus: Equatable {
    case unknown, notInstalled, loggedOut, loggedIn(String)
    init(_ status: Object?) {
        guard let status, let installed = status["installed"] as? Bool else { self = .unknown; return }
        if !installed { self = .notInstalled }
        else if let loggedIn = status["loggedIn"] as? Bool { self = loggedIn ? .loggedIn(status.str("login")) : .loggedOut }
        else { self = .unknown }
    }
}
extension SessionModel {
    func askAbout(_ item: GithubItem) throws { try appendReference(item.reference, to: item.editor) }
}

// Own this state for one provider sheet. SwiftUI cancels hold() when the sheet or generation changes.
@MainActor @Observable final class ProviderState {
    let host: String
    private(set) var login: LoginSession?
    private var owner: UUID?
    init(host: String) { self.host = host }
    func hold(_ model: SessionModel, generation: UUID) async throws {
        let ticket = UUID(); owner = ticket; login = nil
        defer { if owner == ticket { login = nil; owner = nil } }
        guard let rows = model.rows[host], rows.connectionGeneration == generation,
              rows.groups["host"]?.first(where: { $0.id == "connection" })?.string == "connected" else { return }
        try await model.withLogin(host) { session in
            guard owner == ticket else { throw CoreError.StaleTarget }
            login = session
            // Cancellation ends the stream and releases the dedicated Pi, including on navigation away.
            let (lifetime, end) = AsyncStream<Void>.makeStream(); defer { end.finish() }
            for await _ in lifetime {}
        }
    }
}
