import Foundation

struct WorkLine: Identifiable, Equatable {
    let id: String, name: String, subject: String
    var output: String, state: String
    var images: HistoryImages?
    var callID: String?
    var changeCall = false
    var hasChange: Bool { changeCall && state == "完了" }
    var isThought: Bool { state.isEmpty }
    static func streamThought(_ stream: [Item], hasAnswer: Bool) -> (automatic: Bool, title: String) {
        func timestamp(_ id: String) -> UInt64? { stream.first { $0.id == id }.flatMap { UInt64($0.json) } }
        let end = timestamp("thinkingUntil")
        guard end != nil || hasAnswer else { return (true, "Thinking…") }
        // A snapshot can contain both thinking and answer text without a measured interval.
        let ms = end.flatMap { stop in timestamp("thinkingSince").map { start in Int(clamping: stop - min(start, stop)) } }
        return (false, thoughtTitle(ms))
    }
    static func thoughtTitle(_ ms: Int?) -> String {
        guard let ms, ms >= 1000 else { return "Thought" }
        let sec = ms / 1000 + (ms % 1000 >= 500 ? 1 : 0)
        return sec < 60 ? "Thought for \(sec)s" : "Thought for \(sec / 60)m \(sec % 60)s"
    }
}
struct ChatTurn: Identifiable, Equatable {
    static let beginning = "begin"
    let id: String
    var prompt = ""
    var images: HistoryImages?
    var imageCount: Int { images?.count ?? 0 }
    var lines: [Line] = []
    enum Line: Identifiable, Equatable {
        case reply(Reply), work(WorkLine)
        var id: String {
            switch self { case .reply(let r): "reply:" + r.id; case .work(let w): "work:" + w.id }
        }
    }
    var work: [WorkLine] { lines.compactMap { if case .work(let w) = $0 { w } else { nil } } }
    mutating func updateWork(_ index: Int, output: String, state: String, images: HistoryImages? = nil) {
        guard case .work(var w) = lines[index] else { return }; w.output = output; w.state = state; w.images = images; lines[index] = .work(w)
    }
    var start: Int?, end: Int?
    struct Reply: Identifiable, Equatable { let id: String, text: String; var notice = "" }
    var summary: String {
        let calls = work.filter { !$0.isThought }.count
        let elapsed = start.flatMap { a in end.map { max(0, ($0 - a) / 1000) } } ?? 0
        return [elapsed > 0 ? "\(elapsed)秒 作業" : "作業", calls > 0 ? "ツール \(calls) 回" : nil].compactMap { $0 }.joined(separator: " · ")
    }
    static func read(_ entries: [Item], live: [Item], timing: [Item] = []) -> [Self] {
        let thinkingMs = Dictionary(uniqueKeysWithValues: timing.map { ($0.id, Int($0.json)) })
        var turns = [Self(id: Self.beginning)], pending: [String: (Int, Int)] = [:]
        func line(_ id: String, _ o: Object, _ name: String, callID: String, changeCall: Bool = false) -> WorkLine {
            let args = o.obj("args"); return WorkLine(id: id, name: name, subject: args.str("path").isEmpty ? args.str("command") : args.str("path"), output: "", state: "未完了", callID: callID, changeCall: changeCall)
        }
        for item in entries {
            if Task.isCancelled { return [] }
            let o = item.object, kind = o.str("kind"), text = o.str("text"), ts = o["timestamp"] as? Int
            if kind == "user" {
                let images = HistoryImages(item)
                turns.append(Self(id: item.id, prompt: text, images: images, start: ts)); continue
            }
            let i = turns.count - 1; turns[i].end = ts
            if kind == "tool_result" {
                let id = o.str("toolCallId"), pos = pending.removeValue(forKey: id)
                if pos == nil { turns[i].lines.append(.work(line(item.id, o, o.str("toolName"), callID: id))) }
                let (t, w) = pos ?? (i, turns[i].lines.count - 1)
                turns[t].updateWork(w, output: text, state: o["isError"] as? Bool == true ? "失敗" : "完了", images: HistoryImages(item))
            } else {
                if !o.str("thinking").isEmpty { turns[i].lines.append(.work(WorkLine(id: item.id + ":thinking", name: WorkLine.thoughtTitle(thinkingMs[item.id] ?? nil), subject: "", output: o.str("thinking"), state: ""))) }
                let error = o.str("errorMessage")
                let stop = o.str("stopReason")
                // Older RCs may not have normalized the abort error yet. Match the RC's whole-message rule.
                let aborted = stop == "aborted" || ((stop.isEmpty || stop == "error") && !error.isEmpty &&
                    error.trimmingCharacters(in: .whitespacesAndNewlines).range(of: #"\A(?:AbortError:\s*)?(?:This operation was aborted|The operation was aborted|The user aborted a request|Request (?:was )?aborted|Aborted)[.!]?\z"#, options: [.regularExpression, .caseInsensitive]) != nil)
                let notice = aborted ? "中断しました" : error
                if !text.isEmpty || !notice.isEmpty { turns[i].lines.append(.reply(.init(id: item.id, text: text, notice: notice))) }
                for (n, c) in o.list("toolCalls").enumerated() {
                    pending[c.str("id")] = (i, turns[i].lines.count)
                    turns[i].lines.append(.work(line("\(item.id):\(n)", c, c.str("name"), callID: c.str("id"), changeCall: ["write", "edit"].contains(c.str("name")))))
                }
            }
        }
        for tool in live {
            let o = tool.object; guard o.str("status") == "running", o["parentToolCallId"] as? String == nil else { continue }
            let i = turns.count - 1, pos = pending[tool.id]
            if pos == nil { turns[i].lines.append(.work(line("live:" + tool.id, o, o.str("toolName"), callID: tool.id))) }
            let (t, w) = pos ?? (i, turns[i].lines.count - 1)
            turns[t].updateWork(w, output: o.obj("partial").list("content").map { $0.str("text") }.joined(), state: "実行中")
        }
        return turns.filter { !$0.prompt.isEmpty || $0.imageCount > 0 || !$0.lines.isEmpty }
    }
}
