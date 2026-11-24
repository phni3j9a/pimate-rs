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
    @AppStorage("appearance") private var appearance = Appearance.system
    @State private var compact = NavigationSplitViewColumn.sidebar
    @State private var selectionError = false
    var body: some View {
        Group {
            if model.hosts.isEmpty {
                NavigationStack { PairScreen(model: model) {} }
            } else {
                NavigationSplitView(preferredCompactColumn: $compact) {
                    Drawer(model: model) { host, instance in
                        do { try model.select(host: host, instance: instance); compact = .detail }
                        catch { selectionError = true }
                    }
                } detail: {
                    if let host = model.host, let id = model.instance {
                        ChatScreen(model: model, host: host, instance: id).id([host, id])
                    } else { ContentUnavailableView("Pi を選んでください", systemImage: "sidebar.left") }
                }
            }
        }.preferredColorScheme(appearance.scheme).tint(PiTheme.accent).modifier(PiFont(.lead))
            .alert("接続できません", isPresented: $selectionError) { Button("閉じる") {} }
    }
}

typealias Object = [String: Any]
extension Dictionary where Key == String, Value == Any {
    func str(_ key: String) -> String { self[key] as? String ?? "" }
    func obj(_ key: String) -> Object { self[key] as? Object ?? [:] }
    func list(_ key: String) -> [Object] { self[key] as? [Object] ?? [] }
    var piTitle: String {
        let name = obj("session").str("name")
        return name.isEmpty ? (str("cwd") as NSString).lastPathComponent : name
    }
}
extension Item {
    var object: Object { (try? JSONSerialization.jsonObject(with: Data(json.utf8))) as? Object ?? [:] }
    var string: String { (try? JSONDecoder().decode(String.self, from: Data(json.utf8))) ?? "" }
}
