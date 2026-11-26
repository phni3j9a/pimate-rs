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
            return o.str("section") == section && (query.isEmpty || [o.piTitle, o.str("cwd"), host.name, o.str("origin") == "grok-bot" ? "Grok Bot" : ""].contains { $0.localizedCaseInsensitiveContains(query) })
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
                    if !items(host, "bot").isEmpty { Label { Text("Grok Bot") } icon: { mark("grok-bot", 14).foregroundStyle(PiTheme.text) }.modifier(PiFont(.caption)).accessibilityLabel(host.name + ": Grok Bot").accessibilityAddTraits(.isHeader); rows(host, "bot") }
                    if (model.rows[host.id]?.groups["instance"] ?? []).isEmpty {
                        Text(Self.emptyMessage(connection: connection(host), rows: model.rows[host.id])).foregroundStyle(PiTheme.secondary)
                    }
                } header: { HStack { Label { Text(host.displayName(in: model.hosts)) } icon: { mark("lucide-monitor", 18) }; Spacer(); Text(connection(host)) }.textCase(nil) }
            }
            if !model.message.isEmpty { Text(model.message).foregroundStyle(PiTheme.failure) }
        }.listStyle(.plain).scrollContentBackground(.hidden).background(PiTheme.background)
            .navigationTitle("Pi 一覧").searchable(text: $query, placement: .navigationBarDrawer(displayMode: .always), prompt: "名前・フォルダで検索")
            .safeAreaInset(edge: .bottom) {
                HStack(spacing: 18) {
                    Button { settings = true } label: { mark("lucide-settings", 22).frame(width: 44, height: 44) }.accessibilityLabel("設定")
                    Spacer()
                    Button { newPi = true } label: { HStack(spacing: 4) { mark("lucide-plus", 16); Text("新しい Pi") }.modifier(PiFont(.body)).padding(.horizontal, 14).padding(.vertical, 7).foregroundStyle(PiTheme.background).background(PiTheme.accent, in: Capsule()).frame(minHeight: 44) }.accessibilityLabel("新しい Pi")
                }.padding(.horizontal, 20).padding(.bottom, 16).background(PiTheme.background)
            }.sheet(isPresented: $newPi) {
                NewPiView(model: model, initial: model.host ?? model.hosts.first?.id ?? "") { h, pi in newPi = false; select(h, pi) }
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
            Button { select(host.id, item.id) } label: {
                InstanceRowLabel(item: item, machine: host.displayName(in: model.hosts))
            }.accessibilityIdentifier("pi-\(item.id)").listRowSeparator(.hidden).listRowBackground(PiTheme.background)
        }
    }
}

struct InstanceRowLabel: View {
    let item: Item
    var machine: String? = nil
    var body: some View {
        let o = item.object, waiting = o.str("section") == "waiting", failed = !o.str("failureText").isEmpty
        let children = o.obj("subagents"), pending = children["waiting"] as? Int ?? 0
        let running = max(0, (children["active"] as? Int ?? 0) - pending)
        let working = o["working"] as? Bool == true
        let origin = o.str("origin") == "mobile" ? "スマホで起動" : o.str("origin") == "grok-bot" ? "Grok Bot" : "PC で起動"
        let state = failed ? "失敗" : waiting ? "入力待ち" : "待機"
        HStack(alignment: .top, spacing: 12) {
            if working { ProgressView().controlSize(.small).accessibilityHidden(true) } else {
                Circle().fill(failed ? PiTheme.failure : waiting ? Color.orange : PiTheme.secondary).frame(width: 7, height: 7).padding(.top, 8)
            }
            VStack(alignment: .leading, spacing: 4) {
                Text(o.piTitle.isEmpty ? item.id : o.piTitle).fontWeight(.semibold).foregroundStyle(PiTheme.text)
                if waiting, let machine {
                    Label { Text(machine) } icon: { mark("lucide-monitor", 14) }.modifier(PiFont(.caption)).foregroundStyle(PiTheme.secondary)
                }
                if waiting && (failed || !o.str("questionTitle").isEmpty) { Text(failed ? o.str("failureText") : o.str("questionTitle")).foregroundStyle(PiTheme.secondary) }
                Text(o.str("cwd")).modifier(PiFont(.caption)).fontDesign(.monospaced).foregroundStyle(PiTheme.secondary).lineLimit(2)
                if running > 0 { Text("サブエージェント \(running) 実行中").modifier(PiFont(.caption)) }
                if pending > 0 { Text("サブエージェント \(pending) 入力待ち").modifier(PiFont(.caption)).foregroundStyle(PiTheme.waiting) }
                Text(working ? origin : "\(state) · \(origin)").modifier(PiFont(.caption)).foregroundStyle(PiTheme.secondary)
            }
            Spacer()
            if working { Text("作業中").modifier(PiFont(.caption)).foregroundStyle(PiTheme.secondary) }
            mark("lucide-chevron-right", 14).foregroundStyle(.tertiary).accessibilityHidden(true)
        }.padding(.vertical, 5).frame(minHeight: 52).contentShape(Rectangle())
    }
}
