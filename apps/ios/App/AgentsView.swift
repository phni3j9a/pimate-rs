import SwiftUI

struct AgentSheet: Identifiable { let id = UUID(); let run: String? }
struct AgentRoute: Hashable { let run: String, child: String }
private let agentStates = ["running": "実行中", "queued": "待機中", "stopping": "停止中", "paused": "一時停止", "waiting": "入力待ち", "complete": "完了", "failed": "失敗", "stopped": "停止済み"]
func agentState(_ state: String) -> String { agentStates[state] ?? "状態不明" }
func agentAction(_ action: String) -> String {
    ["steer": "割り込む", "resume": "指示して再開", "follow_up": "完了後に指示", "stop_child": "この子を停止", "abort_parent": "親の Pi ごと停止", "interrupt": "実行全体を一時停止"][action] ?? "実行全体を停止"
}
struct AgentSummary: View {
    let counts: [String: Int], open: () -> Void
    var body: some View {
        if !counts.isEmpty {
            Button(action: open) {
                HStack { Label("サブエージェント", systemImage: "person.2"); Spacer()
                    Text(["running", "waiting", "paused", "failed", "complete", "stopped"].compactMap { s in counts[s].map { "\($0) \(agentState(s))" } }.joined(separator: " · ")).foregroundStyle(.secondary)
                    SwiftUI.Image(systemName: "chevron.forward").accessibilityHidden(true)
                }.font(.subheadline).frame(minHeight: 44).padding(.horizontal).contentShape(Rectangle())
            }.buttonStyle(.plain).accessibilityIdentifier("agents-summary")
        }
    }
}
struct AgentsView: View {
    let model: SessionModel, key: EditorKey, sheet: AgentSheet
    @Environment(\.dismiss) private var dismiss
    @State private var path: [AgentRoute] = []
    var body: some View {
        let rows = model.rows[key.host] ?? Rows(), runs = rows.runs.filter { sheet.run == nil || sheet.run == $0.str("id") }
        NavigationStack(path: $path) {
            List {
                if runs.isEmpty { Text("実行の記録はありません").foregroundStyle(.secondary) }
                ForEach([true, false], id: \.self) { open in
                    let group = runs.filter { ($0["open"] as? Bool == true) == open }
                    if !group.isEmpty {
                        Section(open ? "実行中・再開可能" : "以前の実行") {
                            ForEach(group, id: \.selfID) { run in
                                VStack(alignment: .leading, spacing: 4) {
                                    Text(verbatim: [run.str("backend"), run.str("mode")].filter { !$0.isEmpty }.joined(separator: " · ")).font(.caption).foregroundStyle(.secondary)
                                    if run.list("children").isEmpty { Text(agentState(run.str("state"))) }
                                }
                                ForEach(run.list("children"), id: \.selfID) { c in
                                    NavigationLink(value: AgentRoute(run: run.str("id"), child: c.str("id"))) {
                                        VStack(alignment: .leading, spacing: 4) {
                                            HStack(alignment: .firstTextBaseline) {
                                                Text(verbatim: c.str("title").isEmpty ? c.str("agent") : c.str("title")).font(.headline)
                                                if let ms = c["startedAt"] as? Double, ms > 0 {
                                                    Spacer(); let start = Date(timeIntervalSince1970: ms / 1000)
                                                    if let end = c["endedAt"] as? Double { Text(Duration.seconds(max(0, (end - ms) / 1000)).formatted(.time(pattern: .minuteSecond))).font(.caption.monospacedDigit()) }
                                                    else { Text(start, style: .timer).font(.caption.monospacedDigit()).multilineTextAlignment(.trailing) }
                                                }
                                            }
                                            Text(verbatim: [agentState(c.str("state")), c.str("agent"), c.obj("activity").str("tool"), c.obj("activity").str("args")].filter { !$0.isEmpty }.joined(separator: " · ")).font(.caption).foregroundStyle(.secondary)
                                        }.padding(.vertical, 4)
                                    }.accessibilityIdentifier("agent-\(run.str("id"))-\(c.str("id"))")
                                }
                                AgentStop(model: model, target: rows.stopTarget(key, run: run.str("id"), viewedChild: nil))
                            }
                        }
                    }
                }
            }.listStyle(.insetGrouped).navigationTitle("サブエージェント").navigationBarTitleDisplayMode(.inline)
                .toolbar { ToolbarItem(placement: .confirmationAction) { Button("閉じる") { dismiss() } } }
                .navigationDestination(for: AgentRoute.self) { route in AgentDetailView(model: model, key: key, route: route) { path = [$0] }.id(route) }
        }.onAppear {
            if let run = sheet.run, let children = runs.first?.list("children"), children.count == 1 { path = [.init(run: run, child: children[0].str("id"))] }
        }.tint(PiTheme.accent).presentationDetents([.large]).presentationDragIndicator(.visible)
    }
}
private extension Dictionary where Key == String, Value == Any { var selfID: String { str("id") } }
struct AgentStop: View {
    let model: SessionModel, target: StopTarget?
    @State private var approved: StopTarget?
    @State private var busy = false
    @State private var error = ""
    var body: some View {
        Group { if let target {
            Button(role: .destructive) { approved = target } label: { Label(agentAction(target.action) + "…", systemImage: "stop.circle") }.foregroundStyle(PiTheme.failure).frame(minHeight: 44).disabled(busy)
                .confirmationDialog(agentAction(target.action), isPresented: Binding(get: { approved != nil }, set: { if !$0 { approved = nil } }), titleVisibility: .visible, presenting: approved) { fixed in
                    Button(agentAction(fixed.action), role: .destructive) {
                        guard fixed == target, !busy else { return }; busy = true
                        Task { defer { busy = false }; do { _ = try await model.control(fixed) } catch { self.error = "停止の結果を確認できません。現在の状態を確認してください。" } }
                    }; Button("キャンセル", role: .cancel) {}
                } message: { fixed in Text(fixed.action == "stop_child" ? "選択した子だけが対象です。" : fixed.action == "abort_parent" ? "親の Pi のターンも停止します。" : "この実行に含まれるすべての子が対象です。") }
        } }.onChange(of: target) { approved = nil }
        if !error.isEmpty { Text(error).foregroundStyle(PiTheme.failure) }
    }
}
