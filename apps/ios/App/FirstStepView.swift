import SwiftUI

struct FirstStepView: View {
    let model: SessionModel, host: Host, close: () -> Void, select: (String, String) -> Void
    @State private var newPi = false
    private var phase: FirstStep { model.rows[host.id]?.firstStep ?? .loading }
    var body: some View {
        List {
            switch phase {
            case .loading: ProgressView("PC の Pi を確認しています…")
            case .offline: ContentUnavailableView("PC に接続していません", systemImage: "wifi.slash", description: Text("Tailscale と PC の Host を確認してください。"))
            case .empty, .list:
                Section {
                    Label("\(host.name) とつながりました", systemImage: "checkmark.circle").font(.title2.bold()).accessibilityAddTraits(.isHeader)
                    if phase == .empty {
                        DeviceBridge(); Text("まだ起動中の Pi はありません。このスマホから PC のフォルダを選んで、新しい Pi を起動できます。")
                        Text("$ pi").font(.body.monospaced()).textSelection(.enabled)
                        Text("PC のターミナルで起動した Pi も、ここに表示されます。").foregroundStyle(.secondary)
                    } else { Text("選ぶとチャットを開きます。").foregroundStyle(.secondary) }
                }.listRowSeparator(.hidden)
                if phase == .list {
                    Section("起動中の Pi") {
                        ForEach(model.rows[host.id]?.groups["instance"] ?? [], id: \.id) { item in
                            Button { select(host.id, item.id) } label: {
                                VStack(alignment: .leading, spacing: 4) {
                                    Text(item.object.piTitle.isEmpty ? item.id : item.object.piTitle).font(.headline)
                                    Text(item.object.str("cwd")).font(.caption.monospaced()).foregroundStyle(.secondary)
                                    if item.object.str("status") == "waiting_input" { Text("入力待ち").foregroundStyle(PiTheme.waiting) }
                                }.frame(minHeight: 44)
                            }.accessibilityIdentifier("pi-\(item.id)")
                        }
                    }
                }
            }
        }.listStyle(.plain).navigationTitle("最初の Pi").navigationBarTitleDisplayMode(.inline)
        .safeAreaInset(edge: .bottom) {
            VStack {
                if phase == .empty || phase == .list { Button("新しい Pi を起動") { newPi = true }.buttonStyle(PiPrimary()) }
                Button("一覧を開く", action: close).frame(minHeight: 44)
            }.padding(.horizontal, 20).padding(.vertical, 6).background(PiTheme.background)
        }.sheet(isPresented: $newPi) {
            NavigationStack {
                NewPiView(model: model, initial: host.id, select: select).toolbar {
                    ToolbarItem(placement: .cancellationAction) { Button("閉じる") { newPi = false } }
                }
            }
        }
    }
}
