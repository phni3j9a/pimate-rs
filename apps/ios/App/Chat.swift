import SwiftUI

struct ChatScreen: View {
    let model: SessionModel, host: String, instance: String
    private var rows: Rows { model.rows[host] ?? Rows() }
    var body: some View {
        let r = rows, info = r.groups["instance"]?.first { $0.id == instance }?.object ?? [:]
        Group {
            if let key = model.editor, key.host == host, key.target.instance == instance { Conversation(model: model, key: key, rows: r).id(key) }
            else {
                let phase = r.groups["view"]?.first { $0.id == "phase" }?.string ?? "", conn = r.groups["host"]?.first { $0.id == "connection" }?.string ?? ""
                if ["revoked", "invalid_token", "auth_failed", "offline", "protocol_mismatch"].contains(conn) || phase == "instance_gone" { ContentUnavailableView("Pi に接続できません", systemImage: "wifi.slash", description: Text("一覧でマシンの接続と起動状態を確認してください。")) }
                else { ProgressView("同期しています…") }
            }
        }.navigationTitle(info.piTitle.isEmpty ? instance : info.piTitle).navigationBarTitleDisplayMode(.inline)
    }
}
private struct Conversation: View {
    let model: SessionModel, key: EditorKey, rows: Rows
    @State private var turns: [ChatTurn] = []
    @State private var loaded = false
    private var running: Bool { rows.field("status") == "\"running\"" }
    var body: some View {
        let groups = rows.groups, questions = groups["question"] ?? [], stream = groups["stream"] ?? []
        let entries = groups["entry"] ?? [], tools = groups["tool"] ?? [], pc = rows.field("pcPrompt").map { $0 != "null" } ?? false
        let text = stream.first { $0.id == "text" }?.string ?? "", thinking = stream.first { $0.id == "thinking" }?.string ?? ""
        VStack(spacing: 0) {
            HStack(spacing: 6) {
                Circle().fill(questions.isEmpty && !pc ? PiTheme.accent : PiTheme.waiting).frame(width: 6, height: 6)
                Text(!questions.isEmpty || pc ? "入力待ち" : running ? "作業中" : "待機").font(.caption)
            }.foregroundStyle(PiTheme.secondary).padding(.bottom, 6).accessibilityElement(children: .combine)
            ScrollViewReader { proxy in
                ScrollView {
                    LazyVStack(alignment: .leading, spacing: 22) {
                        if loaded && turns.isEmpty && stream.isEmpty && questions.isEmpty && !pc { Text("まだメッセージはありません").foregroundStyle(PiTheme.secondary) }
                        ForEach(turns) { turn in TurnView(turn: turn, active: running && turn.id == turns.last?.id) }
                        if !thinking.isEmpty { DisclosureGroup("考えています…") { Text(verbatim: thinking) } }
                        if !text.isEmpty { MarkdownBody(source: text) }
                        if pc { Label("PC 側で操作を完了してください", systemImage: "desktopcomputer") }
                        ForEach(questions, id: \.id) { q in QuestionCard(model: model, key: QuestionKey(editor: key, requestId: q.id), item: q).id(q.id) }
                        Color.clear.frame(height: 1).id("tail")
                    }.padding(.horizontal, 20).padding(.vertical, 12)
                }.defaultScrollAnchor(.bottom).scrollDismissesKeyboard(.interactively)
                    .onChange(of: questions.map(\.id)) { _, ids in if !ids.isEmpty { proxy.scrollTo("tail", anchor: .bottom); UIAccessibility.post(notification: .announcement, argument: "Pi からの質問が届きました") } }
            }
        }.background(PiTheme.background).foregroundStyle(PiTheme.text)
            .safeAreaInset(edge: .bottom) { Composer(model: model, key: key, rows: rows, running: running, blocked: !questions.isEmpty || pc) }
            .task(id: [entries, tools]) {
                let task = Task.detached(priority: .userInitiated) { ChatTurn.read(entries, live: tools) }
                let result = await withTaskCancellationHandler { await task.value } onCancel: { task.cancel() }
                if !Task.isCancelled { turns = result; loaded = true }
            }
    }
}
private struct TurnView: View {
    let turn: ChatTurn, active: Bool
    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            if !turn.prompt.isEmpty { Text(verbatim: turn.prompt).textSelection(.enabled).padding(.horizontal, 14).padding(.vertical, 10).background(PiTheme.surface, in: RoundedRectangle(cornerRadius: 16)).padding(.leading, 30).frame(maxWidth: .infinity, alignment: .trailing) }
            if !turn.work.isEmpty {
                if active { work } else { DisclosureGroup(turn.summary) { work }.font(.subheadline).foregroundStyle(PiTheme.secondary) }
            }
            ForEach(turn.replies) { reply in
                MarkdownBody(source: reply.text); if !reply.meta.isEmpty { Text(reply.meta).modifier(PiFont(.meta)).foregroundStyle(PiTheme.secondary) }
            }
        }
    }
    private var work: some View {
        VStack(spacing: 0) { ForEach(turn.work) { line in
            DisclosureGroup {
                Text(verbatim: line.output.isEmpty ? "出力はまだありません" : line.output).font(.system(.caption, design: .monospaced)).textSelection(.enabled).padding(10).frame(maxWidth: .infinity, alignment: .leading).background(PiTheme.surface, in: RoundedRectangle(cornerRadius: 10))
            } label: {
                HStack(spacing: 8) { SwiftUI.Image(systemName: line.state == "失敗" ? "exclamationmark.circle" : line.state == "実行中" ? "clock" : line.name == "考えた内容" ? "text.bubble" : "checkmark.circle"); Text(line.name).fontWeight(.semibold); Text(line.subject).lineLimit(2); Spacer(); Text(line.state).font(.caption2) }.font(.system(.caption, design: .monospaced)).frame(minHeight: 44)
            }.foregroundStyle(line.state == "失敗" ? PiTheme.failure : PiTheme.secondary)
        } }
    }
}
