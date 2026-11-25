import SwiftUI

struct AgentDetailView: View {
    let model: SessionModel, key: EditorKey, route: AgentRoute, navigate: (AgentRoute) -> Void
    @State private var record: WorkRecord?, turns: [ChatTurn] = [], reload = 0
    @State private var readError = "", sendError = "", text = "", busy = false
    @State private var nextRun: String?, table: OpenTable?
    private var unit: AgentChild { .init(run: route.run, child: route.child) }
    var body: some View {
        let rows = model.rows[key.host] ?? Rows(), row = rows.groups["run"]?.first { $0.id == route.run }
        let run = row?.object ?? [:], child = run.list("children").first { $0.str("id") == route.child }
        let destination = nextRun.flatMap { model.instructions.resume(key, from: unit, run: $0)?.destinationChild }
        ScrollView {
            LazyVStack(alignment: .leading, spacing: 16) {
                if nextRun != nil { ProgressView("再開先の作業記録を待っています…") }
                else if let child {
                    let has = child["hasTranscript"] as? Bool == true
                    Text(verbatim: [agentState(child.str("state")), child.str("agent"), child.str("model")].filter { !$0.isEmpty }.joined(separator: " · ")).font(.subheadline).foregroundStyle(.secondary)
                    if let i = run.obj("stage")["index"] as? Int, let count = run.obj("stage")["count"] as? Int { Text("チェーン \(i + 1) / \(count)").font(.caption) }
                    heading("親の Pi からの依頼")
                    Text(verbatim: record?.task.isEmpty == false ? record!.task : has && (record == nil || record?.loading == true) ? "読み込み中…" : "依頼文は取得されていません").textSelection(.enabled)
                    Divider(); heading("子の作業記録")
                    let captions = captions(model.instructions.forChild(key, unit))
                    ForEach(turns) { t in TurnView(turn: t, active: true, runForCall: { _ in nil }, openRun: { _ in }, caption: captions[t.id]) }
                    if turns.isEmpty && readError.isEmpty {
                        if has && (record == nil || record?.loading == true) { ProgressView("読み込み中…") }
                        else { Text(has ? "作業記録はまだありません" : "作業記録は提供されていません").foregroundStyle(.secondary) }
                    }
                    if !child.str("question").isEmpty { heading("親の Pi に確認中"); Text(verbatim: child.str("question")) }
                    if !child.str("error").isEmpty { Text(verbatim: child.str("error")).foregroundStyle(PiTheme.failure) }
                    if record?.truncated == true { Text("記録の一部を省略しています").font(.caption) }
                    if !readError.isEmpty { Text(readError).foregroundStyle(PiTheme.failure) }
                    if has && (record?.more == true || !readError.isEmpty) { Button("記録を読み込む") { reload += 1 }.disabled(record?.loading == true) }
                    Divider()
                    controls(child, rows: rows)
                } else { ContentUnavailableView("この実行は一覧にありません", systemImage: "tray") }
            }.padding(20)
        }.navigationTitle(child?.str("title") ?? "サブエージェント").navigationBarTitleDisplayMode(.inline)
            .scrollDismissesKeyboard(.interactively)
            .environment(\.photoContext, nil) // Child entry/file IDs never belong to the parent.
            .environment(\.openPiTable, { table = $0 })
            .sheet(item: $table) { shown in ScrollView { MarkdownTable(table: shown.table, answer: shown.answer, full: true).padding() }.environment(\.photoContext, nil).presentationDragIndicator(.visible) }
            .onChange(of: destination, initial: true) { _, child in if let child, let nextRun { navigate(.init(run: nextRun, child: child)) } }
            .task(id: Load(row: row, retry: reload)) {
                guard child?["hasTranscript"] as? Bool == true else { return }
                do {
                    if record == nil { record = try model.workRecord(key, unit) }; guard let record else { return }
                    while record.loading { try await Task.sleep(for: .milliseconds(100)) }; readError = ""
                    while try await record.load() { try await Task.sleep(for: .milliseconds(100)) }
                    try Task.checkCancellation()
                    let items = try record.items.map { Item(collection: "entry", id: $0.str("entryId"), position: 0, json: try jsonString($0)) }
                    turns = ChatTurn.read(items, live: [])
                } catch { if !Task.isCancelled { readError = "作業記録を取得できません。接続を確認してください。" } }
            }
    }
    private struct Load: Equatable { let row: Item?, retry: Int }
    private func heading(_ title: String) -> some View { Text(title).font(.headline).accessibilityAddTraits(.isHeader) }
    private func captions(_ sent: [SentInstruction]) -> [String: String] {
        var remaining = sent, result: [String: String] = [:]
        for turn in turns where !turn.prompt.isEmpty {
            if let i = remaining.firstIndex(where: { $0.text == turn.prompt.trimmingCharacters(in: .whitespacesAndNewlines) }) { result[turn.id] = "あなた · " + agentAction(remaining.remove(at: i).action) }
            else { result[turn.id] = "追加の指示" }
        }; return result
    }
    @ViewBuilder private func controls(_ child: Object, rows: Rows) -> some View {
        let actions = child["actions"] as? [String] ?? []
        if let action = actions.contains("steer") ? "steer" : actions.contains("resume") ? "resume" : nil {
            heading("子への指示")
            TextField("サブエージェントへの指示", text: $text, axis: .vertical).lineLimit(1...5).textFieldStyle(.roundedBorder).disabled(busy)
            Button(agentAction(action)) {
                guard !busy else { return }; busy = true; sendError = ""
                Task { defer { busy = false }; do {
                    let reply = try await model.control(key, unit, action: action, text: text)
                    text = ""; reload += 1; if action == "resume" { nextRun = reply["newRunId"] as? String }
                } catch { sendError = "指示の受付を確認できません。再送する前に記録を確認してください。" } }
            }.buttonStyle(PiPrimary()).disabled(busy || text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
        } else { Label(actions.contains("follow_up") ? "割り込み未対応のため、作業記録の閲覧のみです。" : "この子の作業記録は閲覧のみです。", systemImage: "eye").font(.callout).foregroundStyle(.secondary) }
        if !sendError.isEmpty { Text(sendError).foregroundStyle(PiTheme.failure) }
        AgentStop(model: model, target: rows.stopTarget(key, run: route.run, viewedChild: route.child))
    }
}
