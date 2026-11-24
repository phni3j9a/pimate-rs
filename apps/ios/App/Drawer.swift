import SwiftUI

struct Drawer: View {
    let model: SessionModel
    let select: (String, String) -> Void
    @State private var query = ""
    @State private var pair = false
    @State private var settings = false
    @AppStorage("appearance") private var appearance = Appearance.system
    @AppStorage("typeSize") private var size = TypeSize.standard
    private func items(_ host: Host, _ section: String) -> [Item] {
        (model.rows[host.id]?.groups["instance"] ?? []).filter {
            let o = $0.object
            return o.str("section") == section && (query.isEmpty || [o.piTitle, o.str("cwd"), host.name].contains { $0.localizedCaseInsensitiveContains(query) })
        }
    }
    var body: some View {
        List {
            if model.hosts.contains(where: { !items($0, "waiting").isEmpty }) {
                Section("要対応") { ForEach(model.hosts, id: \.id) { host in rows(host, "waiting") } }
            }
            ForEach(model.hosts, id: \.id) { host in
                Section {
                    rows(host, "normal")
                    if !items(host, "bot").isEmpty { Label("Grok Bot", systemImage: "terminal").font(.caption); rows(host, "bot") }
                    if (model.rows[host.id]?.groups["instance"] ?? []).isEmpty {
                        Text(connection(host) == "接続中" ? "起動中の Pi はありません" : connection(host)).foregroundStyle(PiTheme.secondary)
                    }
                } header: { HStack { Label(host.name, systemImage: "desktopcomputer"); Spacer(); Text(connection(host)) }.textCase(nil) }
            }
            if !model.message.isEmpty { Text(model.message).foregroundStyle(PiTheme.failure) }
        }.listStyle(.plain).scrollContentBackground(.hidden).background(PiTheme.background)
            .navigationTitle("Pi 一覧").searchable(text: $query, prompt: "名前・フォルダで検索")
            .toolbar {
                ToolbarItem(placement: .topBarLeading) { Button("表示設定", systemImage: "slider.horizontal.3") { settings = true } }
                ToolbarItem(placement: .topBarTrailing) { Button("マシンを追加", systemImage: "plus") { pair = true } }
            }.sheet(isPresented: $pair) {
                NavigationStack { PairScreen(model: model) { pair = false }.toolbar { ToolbarItem(placement: .cancellationAction) { Button("閉じる") { pair = false } } } }
            }.sheet(isPresented: $settings) {
                NavigationStack { Form {
                    Picker("配色", selection: $appearance) { ForEach(Appearance.allCases, id: \.self) { Text($0.rawValue) } }
                    Picker("文字サイズ", selection: $size) { ForEach(TypeSize.allCases, id: \.self) { Text($0.rawValue) } }
                }.navigationTitle("表示設定").toolbar { ToolbarItem(placement: .confirmationAction) { Button("完了") { settings = false } } } }.presentationDetents([.medium, .large]).preferredColorScheme(appearance.scheme)
            }
    }
    private func connection(_ host: Host) -> String {
        if host.needsPairing { return "再ペアリングが必要" }
        switch model.rows[host.id]?.groups["host"]?.first(where: { $0.id == "connection" })?.string {
        case "connected": return "接続中"
        case "offline": return "オフライン"
        case "revoked", "invalid_token", "auth_failed": return "再ペアリングが必要"
        case "protocol_mismatch": return "バージョンが一致しません"
        default: return "接続しています…"
        }
    }
    private func rows(_ host: Host, _ section: String) -> some View {
        ForEach(items(host, section), id: \.id) { item in
            let o = item.object, waiting = section == "waiting", failed = !o.str("failureText").isEmpty
            Button { select(host.id, item.id) } label: {
                HStack(alignment: .top, spacing: 12) {
                    Circle().fill(failed ? PiTheme.failure : waiting ? Color.orange : o.str("status") == "running" ? PiTheme.accent : PiTheme.secondary).frame(width: 7, height: 7).padding(.top, 8)
                    VStack(alignment: .leading, spacing: 4) {
                        Text(o.piTitle.isEmpty ? item.id : o.piTitle).fontWeight(.semibold).foregroundStyle(PiTheme.text)
                        if waiting { Text(failed ? o.str("failureText") : o.str("questionTitle")).foregroundStyle(PiTheme.secondary) }
                        Text(o.str("cwd")).font(.system(.caption, design: .monospaced)).foregroundStyle(PiTheme.secondary).lineLimit(2)
                        Text([failed ? "失敗" : waiting ? "入力待ち" : o.str("status") == "running" ? "作業中" : "待機", o.str("origin") == "mobile" ? "スマホで起動" : o.str("origin") == "grok-bot" ? "Grok Bot" : "PC で起動"].joined(separator: " · ")).modifier(PiFont(.caption)).foregroundStyle(PiTheme.secondary)
                    }; Spacer(); SwiftUI.Image(systemName: "chevron.right").font(.caption).foregroundStyle(.tertiary).accessibilityHidden(true)
                }.padding(.vertical, 5).frame(minHeight: 52).contentShape(Rectangle())
            }.accessibilityIdentifier("pi-\(item.id)").listRowSeparator(.hidden).listRowBackground(PiTheme.background)
        }
    }
}
