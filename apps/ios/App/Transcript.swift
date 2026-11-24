import Foundation

struct WorkLine: Identifiable {
    let id: String, name: String, subject: String
    var output: String, state: String
}
struct ChatTurn: Identifiable {
    let id: String
    var prompt = "", imageCount = 0
    var lines: [Line] = []
    enum Line: Identifiable {
        case reply(Reply), work(WorkLine)
        var id: String {
            switch self { case .reply(let r): "reply:" + r.id; case .work(let w): "work:" + w.id }
        }
    }
    var work: [WorkLine] { lines.compactMap { if case .work(let w) = $0 { w } else { nil } } }
    mutating func updateWork(_ index: Int, output: String, state: String) {
        guard case .work(var w) = lines[index] else { return }; w.output = output; w.state = state; lines[index] = .work(w)
    }
    var start: Int?, end: Int?
    struct Reply: Identifiable { let id: String, text: String, meta: String }
    var summary: String {
        let calls = work.filter { $0.name != "考えた内容" }.count
        let elapsed = start.flatMap { a in end.map { max(0, ($0 - a) / 1000) } } ?? 0
        return [elapsed > 0 ? "\(elapsed)秒 作業" : "作業", calls > 0 ? "ツール \(calls) 回" : nil].compactMap { $0 }.joined(separator: " · ")
    }
    static func read(_ entries: [Item], live: [Item]) -> [Self] {
        var turns = [Self(id: "begin")], pending: [String: (Int, Int)] = [:]
        func line(_ id: String, _ o: Object, _ name: String) -> WorkLine {
            let args = o.obj("args"); return WorkLine(id: id, name: name, subject: args.str("path").isEmpty ? args.str("command") : args.str("path"), output: "", state: "未完了")
        }
        for item in entries {
            if Task.isCancelled { return [] }
            let o = item.object, kind = o.str("kind"), text = o.str("text"), ts = o["timestamp"] as? Int
            if kind == "user" {
                let count = max(o["imageCount"] as? Int ?? 0, o.list("images").count)
                turns.append(Self(id: item.id, prompt: text, imageCount: count, start: ts)); continue
            }
            let i = turns.count - 1; turns[i].end = ts
            if kind == "tool_result" {
                let id = o.str("toolCallId"), pos = pending.removeValue(forKey: id)
                if pos == nil { turns[i].lines.append(.work(line(item.id, o, o.str("toolName")))) }
                let (t, w) = pos ?? (i, turns[i].lines.count - 1)
                turns[t].updateWork(w, output: text, state: o["isError"] as? Bool == true ? "失敗" : "完了")
            } else {
                if !o.str("thinking").isEmpty { turns[i].lines.append(.work(WorkLine(id: item.id + ":thinking", name: "考えた内容", subject: "", output: o.str("thinking"), state: ""))) }
                let error = o.str("errorMessage"), usage = o.obj("usage")
                let meta = [o.obj("model").str("id"), (usage["input"] as? Int).map { "\($0.formatted()) in" } ?? "", (usage["output"] as? Int).map { "\($0.formatted()) out" } ?? "", (usage["cost"] as? Double).map { String(format: "$%.2f", $0) } ?? ""].filter { !$0.isEmpty }.joined(separator: " · ")
                if !text.isEmpty || !error.isEmpty { turns[i].lines.append(.reply(.init(id: item.id, text: [text, error].filter { !$0.isEmpty }.joined(separator: "\n"), meta: meta))) }
                for (n, c) in o.list("toolCalls").enumerated() {
                    pending[c.str("id")] = (i, turns[i].lines.count)
                    turns[i].lines.append(.work(line("\(item.id):\(n)", c, c.str("name"))))
                }
            }
        }
        for tool in live {
            let o = tool.object; guard o.str("status") == "running", o["parentToolCallId"] as? String == nil else { continue }
            let i = turns.count - 1, pos = pending[tool.id]
            if pos == nil { turns[i].lines.append(.work(line("live:" + tool.id, o, o.str("toolName")))) }
            let (t, w) = pos ?? (i, turns[i].lines.count - 1)
            turns[t].updateWork(w, output: o.obj("partial").list("content").map { $0.str("text") }.joined(), state: "実行中")
        }
        return turns.filter { !$0.prompt.isEmpty || $0.imageCount > 0 || !$0.lines.isEmpty }
    }
}
