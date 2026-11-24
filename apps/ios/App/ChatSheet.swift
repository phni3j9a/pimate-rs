import Foundation

struct ModelRef: Equatable {
    let provider: String, id: String
    var fields: Object { ["provider": provider, "id": id] }
}

enum SheetAction {
    case models, select(ModelRef), favorites([ModelRef]), saveDefault(ModelRef, thinking: String?), thinking(String)
    case commands, sessions, tree, new, resume(path: String), fork(entry: String), move(entry: String, summarize: Bool)
    case label(entry: String, value: String?), compact, shutdown

    var request: (RcMethod, Object) {
        switch self {
        case .commands: return (.commands, [:])
        case .models: return (.models, [:])
        case .select(let ref): return (.setModel, ref.fields)
        case .favorites(let refs): return (.setScopedModels, ["models": refs.map(\.fields)])
        case .saveDefault(let ref, let thinking):
            var fields: Object = ["model": ref.fields]; fields["thinking"] = thinking; return (.setDefaultModel, fields)
        case .thinking(let level): return (.setThinking, ["level": level])
        case .sessions: return (.sessions, [:])
        case .tree: return (.sessionTree, [:])
        case .new: return (.sessionOp, ["op": "new"])
        case .resume(let path): return (.sessionOp, ["op": "switch", "arg": path])
        case .fork(let entry): return (.sessionOp, ["op": "fork", "arg": entry])
        case .move(let entry, let summarize): return (.sessionOp, ["op": "tree", "arg": entry, "summarize": summarize])
        case .label(let entry, let value):
            var fields: Object = ["entryId": entry]; fields["label"] = value; return (.setLabel, fields)
        case .compact: return (.compact, [:])
        case .shutdown: return (.shutdown, [:])
        }
    }
}

struct ChatSheet {
    enum Page { case model, usage, session, resume, fork, tree, commands }
    let id = UUID()
    let key: EditorKey, page: Page
    var loading = false, reply: Object?, error: CoreError?
}

extension Rows {
    var attachCommand: String? {
        guard let name = groups["view"]?.first(where: { $0.id == "abducoSession" })?.string,
              name.range(of: #"\A[A-Za-z0-9][A-Za-z0-9_.-]{0,99}\z"#, options: .regularExpression) != nil else { return nil }
        return "pimate attach \(name)"
    }
}
