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
enum SpawnOutcome: Equatable { case ready(String), notListed(String), messageUnconfirmed(String) }
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
        let value = try await machineRequest(form.host, form.operation())
        guard let id = value["instanceId"] as? String, !id.isEmpty else { throw CoreError.OutcomeUnknown }
        guard value.str("messageError").isEmpty else { return .messageUnconfirmed(id) }
        let deadline = ContinuousClock.now + .seconds(20)
        repeat {
            if rows[form.host]?.groups["instance"]?.contains(where: { $0.id == id }) == true { return .ready(id) }
            try await Task.sleep(for: .milliseconds(200))
        } while ContinuousClock.now < deadline
        return .notListed(id)
    }
}

@MainActor struct LoginSession {
    let connection: Connection, target: Target
    let valid: () -> Bool
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
    let number: Int, title: String, url: URL?
    init?(_ item: Object) {
        guard let n = item["number"] as? Int, n > 0 else { return nil }; number = n; title = item.str("title")
        let link = URL(string: item.str("url")); url = WebImage.valid(link, scheme: "https") || WebImage.valid(link, scheme: "http") ? link : nil
    }
    var reference: String { "#\(number)「\(title)」" + (url.map { " \($0.absoluteString)" } ?? "") }
}
