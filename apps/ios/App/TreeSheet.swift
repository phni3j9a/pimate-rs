import SwiftUI

struct BranchRow: Identifiable {
    let id: String, depth: Int, node: Object
    // RC sends preorder. Only a fork, not every consecutive message, increases the indent.
    static func read(_ nodes: [Object]) -> [BranchRow] {
        var children: [String: Int] = [:], depths: [String: Int] = [:]
        for node in nodes { if let parent = node["parentId"] as? String { children[parent, default: 0] += 1 } }
        return nodes.compactMap { node in
            let id = node.str("id"), parent = node.str("parentId")
            guard !id.isEmpty, depths[id] == nil else { return nil }
            let depth = (depths[parent] ?? 0) + (children[parent, default: 0] > 1 ? 1 : 0)
            depths[id] = depth
            return BranchRow(id: id, depth: depth, node: node)
        }
    }
    var preview: String { node.str("preview").isEmpty ? "（本文なし）" : node.str("preview") }
    var detail: String {
        let kind = node.str("kind")
        let speaker = ["user": "あなた", "assistant": "Pi", "tool_result": "ツール", "compaction": "圧縮", "branch_summary": "枝の要約"][kind] ?? kind
        let position = node["isCurrent"] as? Bool == true ? "現在位置" : node["onBranch"] as? Bool == true ? "現在の枝" : "別の枝"
        return speaker + " · " + position
    }
}

struct TreeSheet: View {
    let panel: PiSheetPanel
    @State private var nodes: [BranchRow] = []
    @State private var usersOnly = true
    @State private var truncated = false
    @State private var loaded = false
    @State private var attempt = 0
    var body: some View {
        List {
            Section {
                Picker("表示", selection: $usersOnly) { Text("ユーザーのみ").tag(true); Text("すべて").tag(false) }.pickerStyle(.segmented)
                if truncated { Text("大きなツリーのため一部を省略しています。全体は PC の /tree で確認できます。").modifier(PiFont(.caption)) }
                if loaded && nodes.isEmpty { Text("まだ会話がありません").foregroundStyle(.secondary) }
                ForEach(usersOnly ? nodes.filter { $0.node.str("kind") == "user" } : nodes) { row in
                    NavigationLink { TreeDestination(panel: panel, row: row) } label: {
                        HStack(spacing: 10) {
                            Capsule().fill(row.node["onBranch"] as? Bool == true ? PiTheme.accent : Color.secondary).frame(width: 3).accessibilityHidden(true)
                            VStack(alignment: .leading, spacing: 4) {
                                Text(verbatim: row.detail).modifier(PiFont(.caption)).foregroundStyle(.secondary)
                                if let label = row.node["label"] as? String { Text(verbatim: label).modifier(PiFont(.sheet)).fontWeight(.semibold) }
                                Text(verbatim: row.preview).lineLimit(3)
                            }
                        }.padding(.leading, CGFloat(min(row.depth, 5)) * 10).padding(.vertical, 3)
                    }.accessibilityIdentifier("branch-" + row.id).disabled(!panel.ready || !loaded)
                }
            } header: { Text("移動先のメッセージを選択") }
        }.navigationTitle("セッションツリー").navigationBarTitleDisplayMode(.inline)
            .toolbar { ToolbarItem(placement: .topBarTrailing) { Button("更新", systemImage: "arrow.clockwise") { attempt += 1 }.disabled(!panel.ready) } }
            .task(id: attempt) {
                loaded = false
                do {
                    try await panel.model.run(.tree, in: panel.opened)
                    guard !Task.isCancelled, let reply = panel.live?.reply else { return }
                    nodes = BranchRow.read(reply.list("nodes")); truncated = reply["truncated"] as? Bool == true; loaded = true
                } catch { nodes = []; truncated = false }
            }
    }
}

private struct TreeDestination: View {
    let panel: PiSheetPanel, row: BranchRow
    @Environment(\.dismiss) private var dismiss
    @State private var label: String
    @State private var summarize = false
    init(panel: PiSheetPanel, row: BranchRow) {
        self.panel = panel; self.row = row; _label = State(initialValue: row.node.str("label"))
    }
    private func save(_ value: String?) {
        panel.act(.label(entry: row.id, value: value)) { dismiss() }
    }
    var body: some View {
        let idle = panel.idle, trimmed = label.trimmingCharacters(in: .whitespacesAndNewlines)
        List {
            Section(row.detail) { Text(verbatim: row.preview) }
            Section {
                TextField("ラベル", text: $label).submitLabel(.done).disabled(!panel.ready) // edits during the save would be dropped by dismiss()
                // RC limits labels by JavaScript .length, i.e. UTF-16 units.
                Button("ラベルを保存") { save(trimmed) }
                    .disabled(!panel.ready || trimmed.isEmpty || trimmed.utf16.count > 200 || trimmed == row.node.str("label"))
                if !row.node.str("label").isEmpty { Button("ラベルを外す") { save(nil) }.disabled(!panel.ready) }
            } header: { Text("ラベル") } footer: { if trimmed.utf16.count > 200 { Text("200文字以内で入力してください。") } }
            Section {
                Toggle("離れる枝を要約する", isOn: $summarize)
            } footer: { Text("要約には時間と利用料金がかかります。要約しなくても元の枝は残ります。") }
            Section {
                Button("ここへ移動") { panel.act(.move(entry: row.id, summarize: summarize), closeOnAck: true) }.disabled(!idle || !panel.ready)
            } footer: { Text(idle ? "ユーザーのメッセージを選ぶと、その直前へ戻ります。入力欄が空なら、選んだ文が入ります。" : "Pi の処理が終わるまで移動できません。") }
        }.navigationTitle("移動先を確認").navigationBarTitleDisplayMode(.inline)
    }
}
