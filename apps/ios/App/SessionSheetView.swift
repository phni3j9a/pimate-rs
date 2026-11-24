import SwiftUI

struct SessionSheetView: View {
    let panel: PiSheetPanel
    @State private var confirmExit = false
    var body: some View {
        let session = panel.field("session")?.object ?? [:], cwd = panel.field("cwd")?.string ?? ""
        let name = session.str("name").isEmpty ? (cwd as NSString).lastPathComponent : session.str("name")
        List {
            Section {
                Text(verbatim: name.isEmpty ? "このセッション" : name).font(.headline)
                if !cwd.isEmpty { Text(verbatim: cwd).font(.footnote.monospaced()).foregroundStyle(.secondary) }
                if let stats = panel.model.stats(panel.opened.key) {
                    Text("メッセージ \(stats.messages.formatted()) · \(stats.tokens.formatted()) トークン · \(stats.cost.formatted(.currency(code: "USD")))").font(.footnote).foregroundStyle(.secondary)
                }
            }
            panel.feedback
            Section("会話") {
                Button("新しいセッション", systemImage: "square.and.pencil") { panel.act(.new) }
                NavigationLink { ResumeSheet(panel: panel) } label: { Label("過去のセッションを開く", systemImage: "clock.arrow.circlepath") }
                NavigationLink { ForkSheet(panel: panel) } label: { Label("ここから分岐", systemImage: "arrow.triangle.branch") }
                Button("コンテキストを圧縮", systemImage: "arrow.down.right.and.arrow.up.left") { panel.act(.compact) }
            }.disabled(!panel.ready)
            Section("この Pi") {
                if let origin = panel.field("origin")?.string, let label = ["mobile": "スマホ", "pc": "PC", "grok-bot": "Grok Bot"][origin] { LabeledContent("起動元", value: label) }
                if let command = panel.rows.attachCommand {
                    CopyAttachCommand(command: command)
                    Text("PC のターミナルで実行すると、同じ Pi に接続できます。").font(.footnote).foregroundStyle(.secondary)
                }
                Button("この Pi を終了…", role: .destructive) { confirmExit = true }.disabled(!panel.ready)
                    .confirmationDialog("この Pi を終了しますか？", isPresented: $confirmExit, titleVisibility: .visible) {
                        Button("終了する", role: .destructive) { panel.act(.shutdown) }
                        Button("キャンセル", role: .cancel) {}
                    } message: { Text("\(name) の実行中の処理が止まります。会話はあとで開き直せます。") }
            }
        }.navigationTitle("セッション").navigationBarTitleDisplayMode(.inline)
    }
}

struct ResumeSheet: View {
    let panel: PiSheetPanel
    @State private var attempt = 0
    var body: some View {
        let sessions = panel.rows.groups["session"] ?? []
        List {
            panel.feedback
            if sessions.isEmpty && panel.live?.error == nil && panel.live?.loading == false { Text("保存されたセッションはありません").foregroundStyle(.secondary) }
            ForEach(sessions, id: \.id) { item in
                let o = item.object, path = o.str("path"), current = o["current"] as? Bool == true
                Button { panel.act(.resume(path: path)) } label: {
                    VStack(alignment: .leading, spacing: 4) {
                        Text(verbatim: [o.str("name"), o.str("firstMessage")].first { !$0.isEmpty } ?? "空のセッション")
                        if let count = o["messageCount"] as? Int { Text("メッセージ \(count) 件").font(.caption).foregroundStyle(.secondary) }
                        if current { Label("開いています", systemImage: "checkmark").font(.caption) }
                    }
                }.disabled(current || !panel.ready || path.isEmpty || panel.live?.error != nil)
            }
            if panel.live?.error != nil { Button("一覧を再取得") { attempt += 1 } }
        }.navigationTitle("過去のセッション").navigationBarTitleDisplayMode(.inline)
            .task(id: attempt) { try? await panel.model.run(.sessions, in: panel.opened) }
    }
}

struct ForkSheet: View {
    let panel: PiSheetPanel
    var body: some View {
        let entries = (panel.rows.groups["entry"] ?? []).reversed().filter { $0.object.str("kind") == "user" }
        List {
            Text("選んだメッセージの直前までを新しいセッションに写します。入力欄が空なら、選んだ文が入ります。").font(.footnote).foregroundStyle(.secondary)
            panel.feedback
            if entries.isEmpty { Text("分岐できるメッセージはありません") }
            ForEach(entries, id: \.id) { entry in
                let text = entry.object.str("text")
                Button { panel.act(.fork(entry: entry.id)) } label: { Text(verbatim: text.isEmpty ? "（画像のみ）" : text).lineLimit(4) }.disabled(!panel.ready)
            }
        }.navigationTitle("分岐元を選択").navigationBarTitleDisplayMode(.inline)
    }
}
