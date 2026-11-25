import SwiftUI

struct ReviewView: View {
    let model: SessionModel, key: EditorKey, turns: [ChatTurn]
    @Environment(\.dismiss) private var dismiss
    @State private var session = false
    @State private var turn: String?
    @State private var review: ChangeReview?
    @State private var error = ""
    var body: some View {
        let entries = model.rows[key.host]?.groups["entry"] ?? []
        NavigationStack {
            List {
                Section {
                    Picker("範囲", selection: $session) {
                        Text("このターン").tag(false); Text("セッション全体").tag(true)
                    }.pickerStyle(.segmented)
                    if !session && !turns.isEmpty {
                        Picker("ターン", selection: $turn) {
                            Text("直近の変更").tag(String?.none)
                            ForEach(turns.reversed()) { t in Text(t.prompt.isEmpty ? "画像の依頼" : String(t.prompt.prefix(80))).tag(Optional(t.id)) }
                        }
                    }
                }
                if let review {
                    Section("\(review.files.count) ファイル · 読み込み済みの履歴") {
                        if review.files.isEmpty { Text("変更はありません").foregroundStyle(.secondary) }
                        ForEach(review.files.indices, id: \.self) { n in
                            DiffFile(file: review.files[n], expanded: n == 0).id(Data(review.files[n].path.utf8))
                        }
                    }
                } else if error.isEmpty { ProgressView("変更を集計しています…") }
                if !error.isEmpty { Text(error).foregroundStyle(PiTheme.failure) }
            }.listStyle(.plain).navigationTitle("変更").navigationBarTitleDisplayMode(.inline)
                .toolbar { ToolbarItem(placement: .confirmationAction) { Button("閉じる") { dismiss() } } }
                .safeAreaInset(edge: .bottom) {
                    if let review, !review.files.isEmpty {
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
    let file: ChangeFile
    @State var expanded: Bool
    var body: some View {
        let diffs = file.pieces.compactMap(\.diff)
        DisclosureGroup(isExpanded: $expanded) {
            ForEach(file.pieces, id: \.call) { piece in
                if let hunks = piece.diff?.hunks, !hunks.isEmpty {
                    ForEach(hunks.indices, id: \.self) { n in DiffHunk(hunk: hunks[n]) }
                } else {
                    Text(["binary": "バイナリファイル", "too_large": "大きいため差分を省略しました", "unknown_before": "変更前の内容は不明です", "concurrent": "同時に変更されたため差分は不明です"][piece.omitted ?? ""] ?? "差分情報はありません").foregroundStyle(.secondary)
                    if let body = piece.content ?? piece.patch { Text(verbatim: body).font(.system(.caption, design: .monospaced)).textSelection(.enabled) }
                }
            }
        } label: {
            HStack {
                let added = file.pieces.first?.created == true
                Text(added ? "A" : "M").font(.caption.bold()).padding(5).background(PiTheme.surface, in: RoundedRectangle(cornerRadius: 5)).accessibilityLabel(added ? "作成" : "変更")
                Text(verbatim: file.path).font(.system(.subheadline, design: .monospaced)); Spacer()
                if !diffs.isEmpty { Text("+\(diffs.reduce(0) { $0 + $1.added }) −\(diffs.reduce(0) { $0 + $1.removed })").font(.caption.monospacedDigit()) }
            }.frame(minHeight: 44).accessibilityElement(children: .combine)
        }
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
            }.font(.system(.caption, design: .monospaced)).textSelection(.enabled)
        }
        if hunk.lines.count > limit { Button("続きを表示（残り \(hunk.lines.count - limit) 行）") { limit += 120 } }
    }
}
