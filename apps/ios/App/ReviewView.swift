import SwiftUI

struct ReviewView: View {
    let model: SessionModel, key: EditorKey, turns: [ChatTurn]
    @Environment(\.dismiss) private var dismiss
    @State private var session = false
    @State var turn: String?
    var line: String? = nil
    @State private var review: ChangeReview?
    @State private var error = ""
    var body: some View {
        let entries = model.rows[key.host]?.groups["entry"] ?? []
        NavigationStack {
            List {
                if line == nil { Section {
                    Picker("範囲", selection: $session) {
                        Text("このターン").tag(false); Text("セッション全体").tag(true)
                    }.pickerStyle(.segmented)
                    if !session && !turns.isEmpty {
                        Picker("ターン", selection: $turn) {
                            Text("直近の変更").tag(String?.none)
                            ForEach(turns.reversed()) { t in Text(t.id == ChatTurn.beginning ? "最初の入力より前" : t.prompt.isEmpty ? "画像の依頼" : String(t.prompt.prefix(80))).tag(Optional(t.id)) }
                        }
                    }
                }
                }
                if let review {
                    let files = review.files.compactMap { file -> ChangeFile? in
                        let pieces = file.pieces.filter { line == nil || $0.lineID == line }; return pieces.isEmpty ? nil : ChangeFile(path: file.path, pieces: pieces)
                    }
                    let clipboard = files.flatMap(\.pieces).compactMap(\.patch).filter { !$0.isEmpty }.map { $0.hasSuffix("\n") ? $0 : $0 + "\n" }.joined()
                    if !clipboard.isEmpty { CopyText(text: clipboard, label: "差分をコピー") }
                    Section("\(files.count) ファイル · 読み込み済みの履歴") {
                        if files.isEmpty { Text("変更はありません").foregroundStyle(.secondary) }
                        ForEach(files.indices, id: \.self) { n in
                            DiffFile(file: files[n], showOutput: line != nil, expanded: n == 0).id(Data(files[n].path.utf8))
                        }
                    }
                } else if error.isEmpty { ProgressView("変更を集計しています…") }
                if !error.isEmpty { Text(error).foregroundStyle(PiTheme.failure) }
            }.listStyle(.plain).navigationTitle("変更").navigationBarTitleDisplayMode(.inline)
                .toolbar { ToolbarItem(placement: .confirmationAction) { Button("閉じる") { dismiss() } } }
                .safeAreaInset(edge: .bottom) {
                    if line == nil, let review, !review.files.isEmpty {
                        Button("この変更について依頼") {
                            do { try model.askAbout(review); dismiss() } catch { self.error = "下書きに追加できません。元の会話を確認してください。" }
                        }.buttonStyle(PiPrimary()).padding().background(.bar)
                    }
                }
                .task(id: Query(entries: entries, session: session, turn: turn)) {
                    review = nil; error = ""
                    do {
                        let all = try await model.review(key, turn: nil)
                        if !session, let id = turn ?? all.latestChangedTurn { review = try await model.review(key, turn: id) }
                        else { review = all }
                    } catch { if !Task.isCancelled { self.error = "変更を読み込めません。会話を確認してください。" } }
                }
        }.tint(PiTheme.accent)
    }
    private struct Query: Equatable { let entries: [Item], session: Bool, turn: String? }
}

private struct DiffFile: View {
    let file: ChangeFile, showOutput: Bool
    @State var expanded: Bool
    var body: some View {
        let diffs = file.pieces.compactMap(\.diff)
        DisclosureGroup(isExpanded: $expanded) {
            ForEach(file.pieces, id: \.lineID) { piece in
                if showOutput && !piece.output.isEmpty {
                    DisclosureGroup("ツールの出力") { ChangeBody(text: piece.output, truncated: piece.truncated) }
                }
                if let hunks = piece.diff?.hunks, !hunks.isEmpty {
                    ForEach(hunks.indices, id: \.self) { n in DiffHunk(hunk: hunks[n]) }
                } else {
                    Text(["binary": "バイナリファイル", "too_large": "大きいため差分を省略しました", "unknown_before": "変更前の内容は不明です", "concurrent": "同時に変更されたため差分は不明です"][piece.omitted ?? ""] ?? "差分情報はありません").foregroundStyle(.secondary)
                    if let body = piece.content ?? piece.patch { ChangeBody(text: body) }
                }
            }
        } label: {
            HStack {
                let added = file.pieces.first?.created == true
                Text(added ? "A" : "M").modifier(PiFont(.caption)).fontWeight(.semibold).padding(5).background(PiTheme.surface, in: RoundedRectangle(cornerRadius: 5)).accessibilityLabel(added ? "作成" : "変更")
                Text(verbatim: file.path).modifier(PiFont(.lead)).fontDesign(.monospaced); Spacer()
                if !diffs.isEmpty { Text("+\(diffs.reduce(0) { $0 + $1.added }) −\(diffs.reduce(0) { $0 + $1.removed })").modifier(PiFont(.caption)).monospacedDigit() }
            }.frame(minHeight: 44).accessibilityElement(children: .combine)
        }
    }
}
private struct ChangeBody: View {
    let text: String
    var truncated = false
    @State private var limit = 6000
    var body: some View {
        if truncated { Text("出力の末尾は省略されています。").foregroundStyle(.secondary) }
        Text(verbatim: String(text.prefix(limit))).modifier(PiFont(.caption)).fontDesign(.monospaced).textSelection(.enabled)
        if text.dropFirst(limit).first != nil { Button("続きを表示") { limit += 6000 } }
        CopyText(text: text, label: truncated ? "取得済みの出力をコピー" : "全文をコピー")
    }
}
private struct DiffHunk: View {
    let hunk: ChangeHunk
    @State private var limit = 120
    var body: some View {
        ScrollView(.horizontal) {
            VStack(alignment: .leading, spacing: 0) {
                Text(verbatim: hunk.header).foregroundStyle(.secondary).padding(.vertical, 6)
                ForEach(0..<min(limit, hunk.lines.count), id: \.self) { n in
                    let l = hunk.lines[n], color = l.old == nil ? PiTheme.accent : l.new == nil ? PiTheme.failure : PiTheme.text
                    Text(verbatim: "\(l.old.map(String.init) ?? "·")\t\(l.new.map(String.init) ?? "·")  \(l.old == nil ? "+" : l.new == nil ? "−" : " ") \(l.text)")
                        .foregroundStyle(color).frame(maxWidth: .infinity, alignment: .leading).background(color.opacity(l.old == nil || l.new == nil ? 0.09 : 0))
                    if l.noNewline { Text("末尾に改行なし").foregroundStyle(.secondary) }
                }
            }.modifier(PiFont(.caption)).fontDesign(.monospaced).textSelection(.enabled)
        }
        if hunk.lines.count > limit { Button("続きを表示（残り \(hunk.lines.count - limit) 行）") { limit += 120 } }
    }
}
