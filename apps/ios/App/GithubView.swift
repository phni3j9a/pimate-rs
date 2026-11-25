import SwiftUI

struct GithubView: View {
    let model: SessionModel, editor: EditorKey, close: () -> Void
    var kind = "list", number: Int?
    @State private var data: Object?, error = "", tab = "issue", retry = 0
    var body: some View {
        List {
            if !error.isEmpty { Text(error).foregroundStyle(PiTheme.failure); Button("再読み込み") { retry += 1 } }
            if let data {
                if kind == "list" {
                    Section {
                        Text(data.obj("repo").str("name")).font(.headline)
                        Picker("種類", selection: $tab) { Text("Issue").tag("issue"); Text("PR").tag("pr") }.pickerStyle(.segmented)
                    }
                    let items = data.list(tab == "issue" ? "issues" : "prs")
                    if items.isEmpty { Text("該当する項目はありません").foregroundStyle(.secondary) }
                    ForEach(items.indices, id: \.self) { i in
                        if let item = GithubItem(editor, items[i]) {
                            NavigationLink { GithubView(model: model, editor: editor, close: close, kind: tab, number: item.number) } label: {
                                VStack(alignment: .leading, spacing: 5) { Text(verbatim: item.title).font(.headline); Text("#\(item.number) · \(items[i].str("state"))").font(.caption).foregroundStyle(.secondary) }
                            }
                        }
                    }
                } else { detail(data.obj("item")) }
            } else if error.isEmpty { ProgressView("PC から読み込み中…") }
        }.navigationTitle(number.map { "\(kind == "issue" ? "Issue" : "PR") #\($0)" } ?? "GitHub").navigationBarTitleDisplayMode(.inline)
        .task(id: retry) {
            data = nil; error = ""
            do {
                var q: Object = ["kind": kind]; q["number"] = number
                let result = try await model.github(editor, query: q); try Task.checkCancellation()
                guard let r = result["data"] as? Object, kind == "list" ? r["issues"] is [Object] && r["prs"] is [Object] : r.obj("item")["number"] as? Int == number else { throw CoreError.InvalidMessage }; data = r
            } catch { if !Task.isCancelled { self.error = "GitHub から取得できませんでした。PC の接続と gh のログイン状態を確認してください。" } }
        }
    }
    @ViewBuilder private func detail(_ value: Object) -> some View {
        Section {
            Text(verbatim: value.str("title")).font(.title2.bold()); Text("\(value.str("author")) · \(value.str("state"))").font(.caption).foregroundStyle(.secondary)
            if value.str("body").isEmpty { Text("本文はありません") } else { MarkdownBody(source: value.str("body"), answer: "github-body") }
            if value["bodyTruncated"] as? Bool == true { Text("本文の一部を省略しています") }
        }
        if kind == "pr" {
            Section("CI チェック") {
                let checks = value.list("checks")
                ForEach(checks.indices, id: \.self) { i in LabeledContent(checks[i].str("name"), value: checks[i].str("state")) }
                if value["checksTruncated"] as? Bool == true { Text("CI チェックの一部を省略しています。続きは GitHub で確認してください。") }
                else if checks.isEmpty { Text("チェックはありません") }
            }
        }
        Section("コメント") {
            let comments = value.list("comments")
            if comments.isEmpty { Text("コメントはありません").foregroundStyle(.secondary) }
            ForEach(comments.indices, id: \.self) { i in
                Text(verbatim: comments[i].str("author")).font(.headline); MarkdownBody(source: comments[i].str("body"), answer: "github-comment-\(i)")
                if comments[i]["bodyTruncated"] as? Bool == true { Text("一部を省略しています") }
            }
            if value["commentsTruncated"] as? Bool == true { Text("最新のコメントのみ表示しています") }
        }
        if let item = GithubItem(editor, value) {
            if let url = item.url { Link("GitHub で開く", destination: url) }
            Button("Pi に依頼") { do { try model.askAbout(item); close() } catch { error = "Pi が切り替わりました。下書きは変更していません。" } }
                .accessibilityHint("現在の下書きに参照を追加します。送信はしません。")
        }
    }
}
