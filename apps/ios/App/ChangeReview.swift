import Foundation

struct ChangeLine {
    let old: Int?, new: Int?, text: String
    var noNewline = false
}
struct ChangeHunk { let header: String; var lines: [ChangeLine] = [] }
struct ChangePatch {
    var path: String?, hunks: [ChangeHunk] = []
    var added: Int { hunks.reduce(0) { $0 + $1.lines.filter { $0.old == nil }.count } }
    var removed: Int { hunks.reduce(0) { $0 + $1.lines.filter { $0.new == nil }.count } }
    private static let header = try! NSRegularExpression(pattern: #"^@@ -([0-9]+)(?:,([0-9]+))? \+([0-9]+)(?:,([0-9]+))? @@"#)
    init(_ patch: String) {
        let lines = patch.components(separatedBy: "\n"); var i = 0, git = false
        while i < lines.count {
            let line = lines[i]; i += 1
            if line.hasPrefix("diff --git ") { git = true }
            if line.hasPrefix("--- "), i < lines.count, lines[i].hasPrefix("+++ ") {
                let new = lines[i].dropFirst(4).prefix { $0 != "\t" }, old = line.dropFirst(4).prefix { $0 != "\t" }; i += 1
                let name = new == "/dev/null" ? old : new
                if path == nil { path = String(git && name.hasPrefix(new == "/dev/null" ? "a/" : "b/") ? name.dropFirst(2) : name) }
            }
            let ns = line as NSString
            guard let m = Self.header.firstMatch(in: line, range: NSRange(location: 0, length: ns.length)) else { continue }
            func number(_ index: Int) -> Int? { let r = m.range(at: index); return r.location == NSNotFound ? 1 : Int(ns.substring(with: r)) }
            guard var old = number(1), var left = number(2), var new = number(3), var right = number(4),
                  old <= Int.max - left, new <= Int.max - right else { continue }
            var h = ChangeHunk(header: line)
            while i < lines.count {
                let raw = lines[i], text = String(raw.dropFirst())
                if raw.hasPrefix("\\"), !h.lines.isEmpty { h.lines[h.lines.count - 1].noNewline = true }
                else if raw.hasPrefix("+"), right > 0 { h.lines.append(.init(old: nil, new: new, text: text)); new += 1; right -= 1 }
                else if raw.hasPrefix("-"), left > 0 { h.lines.append(.init(old: old, new: nil, text: text)); old += 1; left -= 1 }
                else if (raw.hasPrefix(" ") || raw.isEmpty), left > 0, right > 0 {
                    h.lines.append(.init(old: old, new: new, text: text)); old += 1; new += 1; left -= 1; right -= 1
                } else { break }; i += 1
            }
            hunks.append(h)
        }
    }
}
struct ChangePiece { let call: String, lineID: String, output: String, created: Bool, patch: String?, diff: ChangePatch?, content: String?, omitted: String? }
struct ChangeFile { let path: String; var pieces: [ChangePiece] }
struct ChangeReview {
    let key: EditorKey, turn: String?
    var latestChangedTurn: String?, files: [ChangeFile] = []
    var reference: String {
        let scope = turn == nil ? "このセッションの変更" : "このターンの変更"
        let label = files.first.map { "（" + $0.path + (files.count > 1 ? " ほか \(files.count - 1) ファイル" : "") + "）" } ?? ""
        return scope + label + "について："
    }
    init(key: EditorKey, entries: [Item], cwd: String, turn: String?) {
        self.key = key; self.turn = turn
        let history = entries.map(\.object); var results: [String: Object] = [:], pending: [String: String] = [:], indices: [Data: Int] = [:], current = ChatTurn.beginning
        for (i, entry) in history.enumerated() {
            if entry.str("kind") == "assistant" {
                for (j, call) in entry.list("toolCalls").enumerated() { pending[call.str("id")] = "\(i):\(j)" }
            } else if entry.str("kind") == "tool_result", let position = pending.removeValue(forKey: entry.str("toolCallId")) { results[position] = entry }
        }
        for (i, entry) in history.enumerated() {
            if entry.str("kind") == "user" { current = entry.str("entryId") }
            guard entry.str("kind") == "assistant", turn == nil || turn == current else { continue }
            for (j, call) in entry.list("toolCalls").enumerated() {
                let id = call.str("id"), name = call.str("name"), args = call.obj("args")
                guard ["write", "edit"].contains(name), let r = results["\(i):\(j)"], r["isError"] as? Bool != true else { continue }
                latestChangedTurn = current
                let details = r.obj("details"), w = details.obj("pimate")
                let write: Object = name == "write" && w.str("kind") == "write_diff" ? w : [:]
                let patch = (name == "edit" ? details : write)["patch"] as? String, diff = patch.map(ChangePatch.init)
                var path = [args.str("path"), args.str("file_path"), diff?.path ?? ""].first { !$0.isEmpty } ?? "（不明なファイル）"
                if path.hasPrefix("@") { path.removeFirst() }
                let root = String(cwd.reversed().drop(while: { $0 == "/" }).reversed()) + "/"
                let full = !cwd.isEmpty && !path.hasPrefix("/") ? root + path : path
                var parts: [Substring] = []
                for p in full.split(separator: "/") where p != "." {
                    if p == "..", let last = parts.last, last != ".." { parts.removeLast() }
                    else if p != ".." || !full.hasPrefix("/") { parts.append(p) }
                }
                let k = path == "~" || path.hasPrefix("~/") ? path : (full.hasPrefix("/") ? "/" : "./") + parts.joined(separator: "/")
                let pathBytes = Data(k.utf8)
                let index = indices[pathBytes] ?? files.count
                if index == files.count { indices[pathBytes] = index; files.append(.init(path: !cwd.isEmpty && path.hasPrefix(root) ? String(path.dropFirst(root.count)) : path, pieces: [])) }
                let reason = r["detailsOmitted"] as? Bool == true ? "too_large" : write.str("omitted")
                let content = name == "write" && patch == nil && !["too_large", "binary"].contains(reason) ? args["content"] as? String : nil
                files[index].pieces.append(.init(call: id, lineID: "\(entries[i].id):\(j)", output: r.str("text"), created: write["existed"] as? Bool == false && reason != "concurrent", patch: patch, diff: diff,
                    content: content, omitted: patch != nil ? nil : !reason.isEmpty ? reason : content == nil ? "no_diff" : "unknown_before"))
            }
        }
    }
}

extension SessionModel {
    func review(_ key: EditorKey, turn: String?) async throws -> ChangeReview {
        guard editor == key, let r = rows[key.host] else { throw CoreError.StaleTarget }
        let entries = r.groups["entry"] ?? [], cwd = r.groups["instance"]?.first { $0.id == key.target.instance }?.object.str("cwd") ?? ""
        let result = await Task.detached { ChangeReview(key: key, entries: entries, cwd: cwd, turn: turn) }.value
        try Task.checkCancellation(); guard editor == key, rows[key.host]?.viewGeneration == r.viewGeneration else { throw CoreError.StaleTarget }; return result
    }
    func askAbout(_ review: ChangeReview) throws { try appendReference(review.reference, to: review.key) }
}
