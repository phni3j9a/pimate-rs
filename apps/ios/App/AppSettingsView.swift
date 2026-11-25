import SwiftUI

struct AppSettingsView: View {
    let model: SessionModel
    private var settings = PiSettings()
    @State private var pair = false
    @State private var check: Task<Void, Never>?, update: AppUpdate?
    var body: some View {
        Form {
            Section("マシン") {
                ForEach(model.hosts, id: \.id) { host in
                    NavigationLink { MachineDetailView(model: model, host: host).id(host.id) } label: {
                        Label {
                            VStack(alignment: .leading, spacing: 4) {
                                Text(host.name).font(.headline); Text(MachineDetailView.summary(host, model.rows[host.id])).font(.caption).foregroundStyle(.secondary)
                            }.padding(.vertical, 2)
                        } icon: { SwiftUI.Image(systemName: "desktopcomputer") }
                    }.accessibilityIdentifier("machine-\(host.id)")
                }
                Button { pair = true } label: { Label("マシンを追加（ペアリング）", systemImage: "plus") }
            }
            Section("新しい Pi の既定") { Toggle("質問ツール（pimate_ask）", isOn: settings.$askTool) }
            Section("表示") {
                Picker("テーマ", selection: settings.$appearance) { ForEach(Appearance.allCases, id: \.self) { Text($0 == .system ? "システム" : $0.rawValue) } }
                Picker("文字サイズ", selection: settings.$typeSize) { ForEach(TypeSize.allCases, id: \.self) { Text($0.rawValue) } }
            }
            Section("情報") {
                LabeledContent("PiMate", value: PiSettings.version ?? "版は未確認")
                Button("更新を確認") {
                    guard check == nil else { return }; update = nil
                    check = Task { let result = await AppUpdate.check(); if !Task.isCancelled { update = result }; check = nil }
                }.disabled(check != nil)
                if check != nil { ProgressView("App Store を照会中…") }
                switch update {
                case .unknown: Text("公開版を確認できませんでした。")
                case .latest: Text("最新です")
                case .available(let version, let url): Link("\(version) に更新できます", destination: url)
                case nil: EmptyView()
                }
                Link("GitHub", destination: URL(string: "https://github.com/phni3j9a/pimate-rs")!); Link("不具合を報告", destination: PiSettings.issueURL)
            }
        }.navigationTitle("設定").navigationBarTitleDisplayMode(.inline).navigationDestination(isPresented: $pair) { PairScreen(model: model) { pair = false } }
        .onDisappear { check?.cancel() }
    }
}
