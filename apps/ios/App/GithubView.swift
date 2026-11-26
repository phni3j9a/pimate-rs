import SwiftUI

enum GithubText {
    static func review(_ item: Object) -> String {
        if item["isDraft"] as? Bool == true { return "ドラフト" }
        switch item.str("reviewDecision") {
        case "APPROVED": return "承認済み"
        case "CHANGES_REQUESTED": return "変更の依頼あり"
        default: return "レビュー待ち"
        }
    }
    static func ci(_ item: Object) -> String {
        let ci = item.obj("ci")
        guard let passed = ci["passed"] as? Int, let failed = ci["failed"] as? Int, let pending = ci["pending"] as? Int,
              passed > 0 || failed > 0 || pending > 0 else { return "CI チェックなし" }
        return "CI 成功 \(passed) · 失敗 \(failed) · 待ち \(pending)"
    }
    static func state(_ item: Object) -> String {
        let state = item.str("state")
        return ["OPEN": "オープン", "MERGED": "マージ済み", "CLOSED": "クローズ済み"][state] ?? state
    }
    static func guidance(_ code: String) -> String {
        switch code {
        case "gh_missing": return "PC に GitHub CLI（gh）をインストールし、gh auth login を実行してください。"
        case "gh_auth": return "PC のターミナルで gh auth login を実行してください。"
        case "not_github": return "この Pi のフォルダは GitHub のリポジトリではありません。GitHub の remote があるフォルダで Pi を開いてください。"
        case "not_found": return "対象の Pi、Issue または PR が見つかりません。アクセス権も確認してください。"
        default: return "接続を確認して、もう一度お試しください。"
        }
    }
}

struct GithubView: View {
    let model: SessionModel, editor: EditorKey, close: () -> Void
    var kind = "list", number: Int?
    @State private var data: Object?
    @State private var error = ""
    @State private var tab = "issue"
    @State private var retry = 0
    @State private var table: OpenTable?
    var body: some View {
        List {
            if !error.isEmpty { Text(error).foregroundStyle(PiTheme.failure); Button("再読み込み") { retry += 1 } }
            if let data {
                if kind == "list" {
                    Section {
                        Text(data.obj("repo").str("name")).modifier(PiFont(.sheet)).fontWeight(.semibold)
                        Picker("種類", selection: $tab) { Text("Issue").tag("issue"); Text("PR").tag("pr") }.pickerStyle(.segmented)
                    }
                    let items = data.list(tab == "issue" ? "issues" : "prs")
                    if items.isEmpty { Text("該当する項目はありません").foregroundStyle(.secondary) }
                    ForEach(items.indices, id: \.self) { i in
                        if let item = GithubItem(editor, items[i]) {
                            NavigationLink { GithubView(model: model, editor: editor, close: close, kind: tab, number: item.number) } label: {
                                VStack(alignment: .leading, spacing: 5) {
                                    Text(verbatim: item.title).modifier(PiFont(.sheet)).fontWeight(.semibold)
                                    Text("#\(item.number) · \(items[i].str("author")) · \(GithubText.state(items[i]))")
                                    if tab == "pr" { prStatus(items[i]) }
                                }.modifier(PiFont(.caption))
                            }
                        }
                    }
                } else { detail(data.obj("item")) }
            } else if error.isEmpty { ProgressView("PC から読み込み中…") }
        }.navigationTitle(number.map { "\(kind == "issue" ? "Issue" : "PR") #\($0)" } ?? "GitHub").navigationBarTitleDisplayMode(.inline)
        .environment(\.openPiTable, { table = $0 })
        .fullScreenCover(item: $table) { TableViewer(shown: $0, context: nil) }
        .task(id: retry) {
            data = nil; error = ""
            do {
                guard model.rows[editor.host]?.machineInfo["github"] as? Bool == true else { throw CoreError.Unsupported }
                var q: Object = ["kind": kind]; q["number"] = number
                let result = try await model.github(editor, query: q); try Task.checkCancellation()
                if result["ok"] as? Bool == false { error = GithubText.guidance(result.str("code")); return }
                guard let r = result["data"] as? Object, kind == "list" ? r["issues"] is [Object] && r["prs"] is [Object] : r.obj("item")["number"] as? Int == number else { throw CoreError.InvalidMessage }; data = r
            } catch { if !Task.isCancelled { self.error = "PC から取得できませんでした。接続と Pi の状態を確認してください。" } }
        }
    }
    private func prStatus(_ value: Object) -> some View {
        VStack(alignment: .leading, spacing: 3) { Text(GithubText.review(value)); Text(GithubText.ci(value)) }.foregroundStyle(.secondary)
    }
    @ViewBuilder private func detail(_ value: Object) -> some View {
        Section {
            Text(verbatim: value.str("title")).modifier(PiFont(.title)).bold(); Text("\(value.str("author")) · \(GithubText.state(value))").modifier(PiFont(.caption)).foregroundStyle(.secondary)
            if let labels = value["labels"] as? [String], !labels.isEmpty { Text("ラベル: \(labels.joined(separator: " · "))") }
            if kind == "pr" { prStatus(value) }
            if value.str("body").isEmpty { Text("本文はありません") } else { MarkdownBody(source: value.str("body"), answer: "github-body") }
            if value["bodyTruncated"] as? Bool == true { Text("本文の一部を省略しています") }
        }
        if kind == "pr" {
            if let head = value["headRefName"] as? String, let base = value["baseRefName"] as? String { Text("\(head) → \(base)").modifier(PiFont(.lead)).fontDesign(.monospaced).textSelection(.enabled) }
            if let n = value["changedFiles"] as? Int, let added = value["additions"] as? Int, let removed = value["deletions"] as? Int { Text("変更 \(n) ファイル · +\(added) / −\(removed)") }
            Section("CI チェック") {
                let checks = value.list("checks")
                ForEach(checks.indices, id: \.self) { i in LabeledContent(checks[i].str("name"), value: ["passed": "成功", "failed": "失敗", "pending": "待ち"][checks[i].str("state")] ?? checks[i].str("state")) }
                if value["checksTruncated"] as? Bool == true { Text("CI チェックの一部を省略しています。続きは GitHub で確認してください。") }
                else if !(value["checks"] is [Object]) { Text("CI チェックの内訳は未提供です") }
                else if checks.isEmpty { Text("チェックはありません") }
            }
        }
        Section("コメント") {
            let comments = value.list("comments")
            if comments.isEmpty { Text("コメントはありません").foregroundStyle(.secondary) }
            ForEach(comments.indices, id: \.self) { i in
                Text(verbatim: comments[i].str("author")).modifier(PiFont(.sheet)).fontWeight(.semibold); MarkdownBody(source: comments[i].str("body"), answer: "github-comment-\(i)")
                if comments[i]["bodyTruncated"] as? Bool == true { Text("一部を省略しています") }
            }
            if value["commentsTruncated"] as? Bool == true { Text("最新のコメントのみ表示しています") }
        }
        if let item = GithubItem(editor, value) {
            if let url = item.url { Link("GitHub で開く", destination: url) }
            Button("Pi に依頼") { do { try model.askAbout(item); close() } catch { self.error = "Pi が切り替わりました。下書きは変更していません。" } }
                .accessibilityHint("現在の下書きに参照を追加します。送信はしません。")
        }
    }
}
