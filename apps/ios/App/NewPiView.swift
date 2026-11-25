import SwiftUI

struct NewPiView: View {
    let model: SessionModel, select: (String, String) -> Void
    @State private var host: String
    @State private var busy = false
    private var settings = PiSettings()
    init(model: SessionModel, initial: String, select: @escaping (String, String) -> Void) {
        self.model = model; self.select = select; _host = State(initialValue: initial)
    }
    var body: some View {
        SpawnFields(model: model, form: SpawnForm(host: host, askTool: settings.askTool), busy: $busy, select: select) {
            Section("マシン") {
                Picker("起動先", selection: $host) {
                    ForEach(model.hosts, id: \.id) { Text($0.displayName(in: model.hosts)).tag($0.id) }
                }.disabled(busy)
            }
        }.id(host).navigationTitle("新しい Pi").navigationBarTitleDisplayMode(.inline).interactiveDismissDisabled(busy)
    }
}

private struct SpawnFields<HostPicker: View>: View {
    let model: SessionModel
    @State var form: SpawnForm
    @Binding var busy: Bool
    let select: (String, String) -> Void
    @ViewBuilder var hostPicker: () -> HostPicker
    @State private var folders: [Object] = []
    @State private var models: [Object] = []
    @State private var dirs: [String]?
    @State private var request: Task<Void, Never>?
    @State private var loading = true
    @State private var note = ""
    @State private var outcome = ""
    private var rows: Rows? { model.rows[form.host] }
    private func choose(_ path: String) {
        dirs = nil
        form.folder(path, knownTrusted: folders.contains { $0.str("path").utf8.elementsEqual(path.utf8) && $0["trusted"] as? Bool == true })
    }
    var body: some View {
        Form {
            hostPicker()
            Section {
                TextField("フォルダのパス", text: Binding(get: { form.cwd }, set: choose)).autocorrectionDisabled().textInputAutocapitalization(.never)
                if loading { ProgressView("候補を取得中…") }
                ForEach(folders.prefix(8).indices, id: \.self) { i in Button(folders[i].str("path")) { choose(folders[i].str("path")) } }
                if rows?.machineInfo["dirs"] as? Bool == true {
                    Button("子フォルダを表示") {
                        let path = form.cwd; request?.cancel()
                        request = Task { do {
                            let result = try await model.machineRequest(form.host, .dirs(path: path)); try Task.checkCancellation()
                            if path.utf8.elementsEqual(form.cwd.utf8) { dirs = result["dirs"] as? [String] }
                        } catch { if !Task.isCancelled { note = "子フォルダを取得できませんでした。" } } }
                    }.disabled(form.cwd.isEmpty)
                    if let dirs {
                        if dirs.isEmpty { Text("子フォルダはありません").foregroundStyle(.secondary) }
                        ForEach(dirs.indices, id: \.self) { i in Button(dirs[i]) { choose(form.cwd + (form.cwd.hasSuffix("/") ? "" : "/") + dirs[i]) } }
                    }
                }
                Toggle("このフォルダを信頼する", isOn: $form.trust)
            } header: { Text("フォルダ") } footer: { Text("信頼すると、このフォルダの設定と Extension を読み込みます。") }
            Section("モデル") {
                Picker("モデル", selection: Binding(get: { models.firstIndex { $0.str("id") == form.model?.str("id") && $0.str("provider") == form.model?.str("provider") } ?? -1 }, set: { i in
                    form.model = models.indices.contains(i) ? ["id": models[i].str("id"), "provider": models[i].str("provider")] : nil; form.thinking = nil
                })) {
                    Text("PC の既定").tag(-1)
                    ForEach(models.indices, id: \.self) { i in Text(models[i].str("id") + " · " + models[i].str("provider")).tag(i) }
                }
                if let levels = models.first(where: { $0.str("id") == form.model?.str("id") && $0.str("provider") == form.model?.str("provider") })?["thinkingLevels"] as? [String], !levels.isEmpty {
                    Picker("Thinking", selection: $form.thinking) { Text("既定").tag(String?.none); ForEach(levels, id: \.self) { Text($0).tag(Optional($0)) } }
                }
                Toggle("質問ツール（pimate_ask）", isOn: $form.askTool)
            }
            Section("最初のメッセージ（任意）") { TextField("Pi への依頼", text: $form.message, axis: .vertical).lineLimit(2...8) }
            if !note.isEmpty { Text(note).foregroundStyle(PiTheme.failure) }
            if !outcome.isEmpty { Text(outcome).textSelection(.enabled) }
        }.safeAreaInset(edge: .bottom) {
            Button(busy ? "起動を確認しています…" : "起動") {
                guard !busy else { return }; busy = true; note = ""; request?.cancel()
                let approved = form
                request = Task {
                    defer { busy = false }
                    do {
                        switch try await model.spawn(approved) {
                        case .ready(let pi): select(approved.host, pi)
                        case .notListed: outcome = "起動を受け付けましたが、一覧にはまだ表示されていません。"
                        case .messageUnconfirmed(_, let attach): outcome = "Pi は起動しましたが、最初のメッセージは未確認です。" + (attach.map { "\n" + $0 } ?? "")
                        }
                    } catch CoreError.OutcomeUnknown {
                        if !Task.isCancelled { outcome = "起動要求の結果は不明です。この画面を閉じ、Pi 一覧で確認してください。" }
                    } catch { if !Task.isCancelled { note = "起動を確認できません。再実行の前に Pi 一覧を確認してください。" } }
                }
            }.buttonStyle(PiPrimary()).padding(.horizontal, 20).padding(.vertical, 6).background(.bar).disabled(!outcome.isEmpty || form.cwd.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty || rows?.machineOnline != true)
        }.disabled(busy).task {
            defer { loading = false }
            do {
                let result = try await model.machineRequest(form.host, .folders); try Task.checkCancellation()
                folders = result.list("folders"); if form.cwd.isEmpty { choose(result.str("home")) }
            } catch { if !Task.isCancelled { note = "最近のフォルダを取得できません。パスを直接入力できます。" } }
            do { let catalog = try await model.hostModels(form.host); try Task.checkCancellation(); models = catalog?.list("models") ?? [] }
            catch { if !Task.isCancelled { note += " モデル一覧を取得できません。PC の既定を利用できます。" } }
        }.onDisappear { request?.cancel() }
    }
}

extension Rows {
    var machineInfo: Object { groups["host"]?.first { $0.id == "info" }?.object ?? [:] }
    var machineOnline: Bool { groups["host"]?.first { $0.id == "connection" }?.string == "connected" }
}
