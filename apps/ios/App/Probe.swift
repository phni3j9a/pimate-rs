import SwiftUI

@main @MainActor struct PiMateApp: App {
    @Environment(\.scenePhase) private var phase
    @State private var model = SessionModel(core: Core(store: KeychainStore.forLaunch()))
    var body: some Scene {
        WindowGroup {
            Probe(model: model)
                .onReceive(NotificationCenter.default.publisher(for: UIApplication.protectedDataDidBecomeAvailableNotification)) { _ in model.retryRegistrations() }
                .onChange(of: phase) { _, next in if next == .active { model.retryRegistrations() } }
        }
    }
}

struct Probe: View {
    let model: SessionModel
    @State private var link = ""
    @State private var notice = ""
    @AppStorage("appearance") private var appearance = Appearance.system
    @AppStorage("typeSize") private var size = TypeSize.standard
    var body: some View {
        NavigationStack {
            List {
                Section("ペアリング") {
                    SecureField("リンクを貼り付ける", text: $link).textInputAutocapitalization(.never).autocorrectionDisabled()
                    Button("接続") {
                        let captured = link; link = ""
                        Task { do { try await model.pair(captured); notice = "" } catch { notice = "リンクと接続を確認してください" } }
                    }.disabled(link.isEmpty || model.busy)
                    if !notice.isEmpty { Text(notice) }
                }
                if model.hosts.isEmpty { Text("登録済みのマシンはありません").accessibilityIdentifier("emptyHosts") }
                ForEach(model.hosts, id: \.id) { host in
                    Section(host.name) {
                        ForEach(model.rows[host.id]?.groups["instance"] ?? [], id: \.id) { row in
                            Button(row.id) { do { try model.select(host: host.id, instance: row.id) } catch { notice = "選択できません" } }
                        }
                    }
                }
                if let key = model.editor {
                    Section(key.target.instance) {
                        TextField("メッセージ", text: Binding(get: { model.body(key).text }, set: { model.text($0, for: key) }), axis: .vertical).id(key)
                        Button("送信") {
                            let draft = model.body(key)
                            Task { try? await model.send(key, draft) }
                        }.disabled(model.busy || model.body(key).text.isEmpty)
                        if !model.message.isEmpty { Text(model.message).foregroundStyle(PiTheme.failure) }
                    }
                } else if !model.message.isEmpty { Text(model.message) }
                Picker("配色", selection: $appearance) { ForEach(Appearance.allCases, id: \.self) { Text($0.rawValue).tag($0) } }
                Picker("文字サイズ", selection: $size) { ForEach(TypeSize.allCases, id: \.self) { Text($0.rawValue).tag($0) } }
            }.modifier(PiFont(.body)).foregroundStyle(PiTheme.text).tint(PiTheme.accent)
                .scrollContentBackground(.hidden).background(PiTheme.background).navigationTitle("接続の確認")
        }.preferredColorScheme(appearance.scheme)
    }
}
