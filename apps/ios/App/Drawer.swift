import SwiftUI

extension Host {
    func displayName(in hosts: [Host]) -> String {
        hosts.contains { $0.id != id && $0.name == name } ? "\(name) · \(address)" : name
    }
}

struct Drawer: View {
    let model: SessionModel
    let select: (String, String) -> Void
    @State private var query = ""
    @State private var newPi = false
    @State private var pair = false
    @State private var settings = false
    @AppStorage("appearance") private var appearance = Appearance.system
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
                        Text(Self.emptyMessage(connection: connection(host), rows: model.rows[host.id])).foregroundStyle(PiTheme.secondary)
                    }
                } header: { HStack { Label(host.displayName(in: model.hosts), systemImage: "desktopcomputer"); Spacer(); Text(connection(host)) }.textCase(nil) }
            }
            if !model.message.isEmpty { Text(model.message).foregroundStyle(PiTheme.failure) }
        }.listStyle(.plain).scrollContentBackground(.hidden).background(PiTheme.background)
            .navigationTitle("Pi 一覧").searchable(text: $query, prompt: "名前・フォルダで検索")
            .safeAreaInset(edge: .bottom) {
                HStack(spacing: 18) {
                    Button("設定", systemImage: "gearshape") { settings = true }.frame(minHeight: 44)
                    Button { newPi = true } label: { Label("新しい Pi", systemImage: "plus") }.buttonStyle(PiPrimary())
                }.padding(.horizontal, 20).padding(.vertical, 6).background(PiTheme.background)
            }.sheet(isPresented: $newPi) {
                NavigationStack {
                    NewPiView(model: model, initial: model.host ?? model.hosts.first?.id ?? "") { h, pi in newPi = false; select(h, pi) }
                        .toolbar { ToolbarItem(placement: .cancellationAction) { Button("閉じる") { newPi = false } } }
                }
            }.toolbar {
                ToolbarItem(placement: .topBarTrailing) { Button("マシンを追加", systemImage: "plus") { pair = true } }
            }.sheet(isPresented: $pair) {
                NavigationStack { PairScreen(model: model) { pair = false }.toolbar { ToolbarItem(placement: .cancellationAction) { Button("閉じる") { pair = false } } } }
            }.sheet(isPresented: $settings) {
                NavigationStack {
                    AppSettingsView(model: model).toolbar { ToolbarItem(placement: .confirmationAction) { Button("完了") { settings = false } } }
                }.preferredColorScheme(appearance.scheme)
            }
    }
    static func emptyMessage(connection: String, rows: Rows?) -> String {
        guard connection == "接続中" else { return connection }
        return rows?.groups["host"]?.first(where: { $0.id == "listKnown" })?.json == "true" ? "起動中の Pi はありません" : "Pi 一覧を取得しています…"
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
                        if waiting {
                            Label(host.displayName(in: model.hosts), systemImage: "desktopcomputer").modifier(PiFont(.caption)).foregroundStyle(PiTheme.secondary)
                        }
                        if waiting && (failed || !o.str("questionTitle").isEmpty) { Text(failed ? o.str("failureText") : o.str("questionTitle")).foregroundStyle(PiTheme.secondary) }
                        Text(o.str("cwd")).font(.system(.caption, design: .monospaced)).foregroundStyle(PiTheme.secondary).lineLimit(2)
                        Text([failed ? "失敗" : waiting ? "入力待ち" : o.str("status") == "running" ? "作業中" : "待機", o.str("origin") == "mobile" ? "スマホで起動" : o.str("origin") == "grok-bot" ? "Grok Bot" : "PC で起動"].joined(separator: " · ")).modifier(PiFont(.caption)).foregroundStyle(PiTheme.secondary)
                    }; Spacer(); SwiftUI.Image(systemName: "chevron.right").font(.caption).foregroundStyle(.tertiary).accessibilityHidden(true)
                }.padding(.vertical, 5).frame(minHeight: 52).contentShape(Rectangle())
            }.accessibilityIdentifier("pi-\(item.id)").listRowSeparator(.hidden).listRowBackground(PiTheme.background)
        }
    }
}
