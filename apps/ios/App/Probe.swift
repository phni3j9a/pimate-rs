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
    private struct RepoRoute: Identifiable { let key: EditorKey; var id: EditorKey { key } }
    @State private var repo: RepoRoute?
    @State private var firstHost: String?
    private func select(_ host: String, _ instance: String) {
        do { try model.select(host: host, instance: instance); firstHost = nil; compact = .detail }
        catch { selectionError = true }
    }
    var body: some View {
        Group {
            if model.hosts.isEmpty {
                NavigationStack { PairScreen(model: model) {} }
            } else if let id = firstHost, let host = model.hosts.first(where: { $0.id == id }) {
                NavigationStack { FirstStepView(model: model, host: host, close: { firstHost = nil }, select: select) }
            } else {
                NavigationSplitView(preferredCompactColumn: $compact) {
                    Drawer(model: model, select: select)
                } detail: {
                    if let host = model.host, let id = model.instance {
                        ChatScreen(model: model, host: host, instance: id).id([host, id])
                            .toolbar {
                                if let key = model.editor {
                                    ToolbarItem(placement: .topBarTrailing) {
                                        Button("GitHub", systemImage: "point.3.connected.trianglepath.dotted") { repo = RepoRoute(key: key) }
                                    }
                                }
                            }
                    } else { ContentUnavailableView("Pi を選んでください", systemImage: "sidebar.left") }
                }
            }
        }.sheet(item: $repo) { route in
            NavigationStack {
                GithubView(model: model, editor: route.key, close: { repo = nil })
                    .toolbar { ToolbarItem(placement: .cancellationAction) { Button("閉じる") { repo = nil } } }
            }
        }.onChange(of: model.pairedHostId, initial: true) { _, id in
            if let id { firstHost = id; model.consumePairedHost() }
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
