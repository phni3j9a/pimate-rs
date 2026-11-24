import SwiftUI

struct ChatScreen: View {
    let model: SessionModel, host: String, instance: String
    private var rows: Rows { model.rows[host] ?? Rows() }
    var body: some View {
        let r = rows, info = r.groups["instance"]?.first { $0.id == instance }?.object ?? [:]
        Group {
            if let key = model.editor, key.host == host, key.target.instance == instance { Conversation(model: model, key: key, rows: r).id(r.viewGeneration).id(key) }
            else {
                VStack {
                    if model.edge?.connection == "offline" || model.edge?.connection == "connecting" {
                        ContentUnavailableView("再接続中", systemImage: "wifi.slash", description: Text("マシンとの接続を待っています。未確認のメッセージは自動で再送しません。"))
                    } else if let problem = Self.connectionProblem(r) { ContentUnavailableView("Pi に接続できません", systemImage: "wifi.slash", description: Text(problem)) }
                    else { ProgressView("同期しています…").frame(maxHeight: .infinity) }
                    DraftList(rows: r, instance: instance, model: model, key: nil)
                }
            }
        }.navigationTitle(info.piTitle.isEmpty ? instance : info.piTitle).navigationBarTitleDisplayMode(.inline)
    }
    static func connectionProblem(_ rows: Rows) -> String? {
        let phase = rows.field("phase")
        if phase == "\"subscribe_error\"" || phase == "\"instance_gone\"" {
            return "この Pi を開けません。一覧に戻り、起動状態を確認して選び直してください。"
        }
        let connection = rows.groups["host"]?.first { $0.id == "connection" }?.string ?? ""
        return ["revoked", "invalid_token", "auth_failed", "offline", "protocol_mismatch"].contains(connection) ? "一覧でマシンの接続と起動状態を確認してください。" : nil
    }
}
private struct Conversation: View {
    let model: SessionModel, key: EditorKey, rows: Rows
    @State private var turns: [ChatTurn] = []
    @State private var table: OpenTable?
    @State private var photo: OpenPhoto?
    private var photos: PhotoContext {
        PhotoContext(model: model, key: key, entries: rows.field("entryImages") == "true", files: rows.field("fileImages") == "true", open: { photo = $0 })
    }
    @State private var loaded = false
    @State private var position: String? = "tail"
    private var running: Bool { rows.field("status") == "\"running\"" }
    var body: some View {
        let groups = rows.groups, questions = groups["question"] ?? [], stream = groups["stream"] ?? []
        let entries = groups["entry"] ?? [], tools = groups["tool"] ?? [], edge = model.edge
        let pc = edge?.pcPrompt != nil
        let text = stream.first { $0.id == "text" }?.string ?? "", thinking = stream.first { $0.id == "thinking" }?.string ?? ""
        VStack(spacing: 0) {
            HStack(spacing: 6) {
                Circle().fill(questions.isEmpty && !pc ? PiTheme.accent : PiTheme.waiting).frame(width: 6, height: 6)
                Text(pc && questions.isEmpty ? "PC で操作待ち" : !questions.isEmpty ? "入力待ち" : running ? "作業中" : "待機").font(.caption)
            }.foregroundStyle(PiTheme.secondary).padding(.bottom, 6).accessibilityElement(children: .combine)
            ScrollView {
                    LazyVStack(alignment: .leading, spacing: 22) {
                        if loaded && turns.isEmpty && stream.isEmpty && questions.isEmpty && !pc { Text("まだメッセージはありません").foregroundStyle(PiTheme.secondary) }
                        ForEach(turns) { turn in TurnView(turn: turn, active: running && turn.id == turns.last?.id) }
                        if !thinking.isEmpty { DisclosureGroup("考えています…") { Text(verbatim: thinking) } }
                        if !text.isEmpty { MarkdownBody(source: text, answer: "stream:" + (entries.last?.id ?? "begin")) }
                        if let move = model.move { MoveNote(move: move) }
                        if let result = model.receipt { ReceiptNote(receipt: result) { model.dismissReceipt(host: key.host, result) } }
                        if let prompt = edge?.pcPrompt, questions.isEmpty { TerminalCard(title: prompt.str("title"), command: edge?.attachCommand) }
                        ForEach(questions, id: \.id) { q in QuestionCard(model: model, key: QuestionKey(editor: key, requestId: q.id), item: q).id(q.id) }
                        Color.clear.frame(height: 1).id("tail")
                    }.scrollTargetLayout().padding(.horizontal, 20).padding(.vertical, 12)
                }.modifier(PhotoViewport()).scrollPosition(id: $position, anchor: .bottom).defaultScrollAnchor(.bottom).scrollDismissesKeyboard(.interactively)
                    .onChange(of: questions.map(\.id)) { _, ids in if !ids.isEmpty { position = "tail"; UIAccessibility.post(notification: .announcement, argument: "Pi からの質問が届きました") } }
        }.background(PiTheme.background).foregroundStyle(PiTheme.text)
            .safeAreaInset(edge: .bottom, spacing: 8) { Composer(model: model, key: key, rows: rows, running: running, blocked: !questions.isEmpty || pc) }
            // A streaming block can disappear at message_end; the presented table must outlive it.
            .environment(\.photoContext, photos)
            .fullScreenCover(item: $photo) { shown in
                PhotoViewer(gallery: shown, context: photos)
            }
            .environment(\.openPiTable, { table = $0 })
            .fullScreenCover(item: $table) { TableViewer(shown: $0, context: photos) }
            .task(id: [entries, tools]) {
                let task = Task.detached(priority: .userInitiated) { ChatTurn.read(entries, live: tools) }
                let result = await withTaskCancellationHandler { await task.value } onCancel: { task.cancel() }
                if !Task.isCancelled { turns = result; loaded = true }
            }
    }
}
private struct TurnView: View {
    let turn: ChatTurn, active: Bool
    @State private var expanded = false
    var body: some View {
        let firstWork = turn.work.first?.id
        VStack(alignment: .leading, spacing: 12) {
            if !turn.prompt.isEmpty || turn.imageCount > 0 {
                VStack(alignment: .leading, spacing: 4) {
                    if !turn.prompt.isEmpty { Text(verbatim: turn.prompt).textSelection(.enabled) }
                    if let images = turn.images { HistoryPhotos(images: images) }
                }.padding(.horizontal, 14).padding(.vertical, 10).background(PiTheme.surface, in: RoundedRectangle(cornerRadius: 16)).padding(.leading, 30).frame(maxWidth: .infinity, alignment: .trailing)
            }
            ForEach(turn.lines) { line in
                switch line {
                case .reply(let reply):
                    MarkdownBody(source: reply.text, answer: reply.id); if !reply.meta.isEmpty { Text(reply.meta).modifier(PiFont(.meta)).foregroundStyle(PiTheme.secondary) }
                case .work(let w):
                    if !active && w.id == firstWork {
                        Button { expanded.toggle() } label: {
                            HStack { Text(turn.summary); Spacer(); SwiftUI.Image(systemName: expanded ? "chevron.down" : "chevron.right") }.frame(minHeight: 44)
                        }.font(.subheadline).foregroundStyle(PiTheme.secondary).accessibilityValue(expanded ? "展開中" : "折りたたみ")
                    }
                    if active || expanded { work(w) }
                    if let images = w.images { HistoryPhotos(images: images) }
                }
            }
        }
    }
    private func work(_ line: WorkLine) -> some View {
            DisclosureGroup {
                Text(verbatim: line.output.isEmpty ? "出力はまだありません" : line.output).font(.system(.caption, design: .monospaced)).textSelection(.enabled).padding(10).frame(maxWidth: .infinity, alignment: .leading).background(PiTheme.surface, in: RoundedRectangle(cornerRadius: 10))
            } label: {
                HStack(spacing: 8) { SwiftUI.Image(systemName: line.state == "失敗" ? "exclamationmark.circle" : line.state == "実行中" ? "clock" : line.name == "考えた内容" ? "text.bubble" : "checkmark.circle"); Text(line.name).fontWeight(.semibold); Text(line.subject).lineLimit(2); Spacer(); Text(line.state).font(.caption2) }.font(.system(.caption, design: .monospaced)).frame(minHeight: 44)
            }.foregroundStyle(line.state == "失敗" ? PiTheme.failure : PiTheme.secondary)
    }
}
