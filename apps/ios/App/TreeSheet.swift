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
    @State private var truncated = false
    @State private var loaded = false
    @State private var attempt = 0
    var body: some View {
        List {
            Section {
                if truncated { Text("大きなツリーのため一部を省略しています。全体は PC の /tree で確認できます。").font(.footnote) }
                if loaded && nodes.isEmpty { Text("まだ会話がありません").foregroundStyle(.secondary) }
                ForEach(nodes) { row in
                    NavigationLink { TreeDestination(panel: panel, row: row) } label: {
                        HStack(spacing: 10) {
                            Capsule().fill(row.node["onBranch"] as? Bool == true ? PiTheme.accent : Color.secondary).frame(width: 3).accessibilityHidden(true)
                            VStack(alignment: .leading, spacing: 4) {
                                Text(verbatim: row.detail).font(.caption).foregroundStyle(.secondary)
                                if let label = row.node["label"] as? String { Text(verbatim: label).font(.headline) }
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
    @State private var summarize = false
    var body: some View {
        let idle = panel.rows.field("status") == "\"idle\"" && panel.rows.field("agentActive") != "true"
        List {
            Section(row.detail) { Text(verbatim: row.preview) }
            Section {
                Toggle("離れる枝を要約する", isOn: $summarize)
            } footer: { Text("要約には時間と利用料金がかかります。要約しなくても元の枝は残ります。") }
            Section {
                Button("ここへ移動") { panel.act(.move(entry: row.id, summarize: summarize), closeOnAck: true) }.disabled(!idle || !panel.ready)
            } footer: { Text(idle ? "ユーザーのメッセージを選ぶと、その直前へ戻ります。入力欄が空なら、選んだ文が入ります。" : "Pi の処理が終わるまで移動できません。") }
        }.navigationTitle("移動先を確認").navigationBarTitleDisplayMode(.inline)
    }
}
