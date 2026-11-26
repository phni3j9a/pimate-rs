import SwiftUI

struct NewPiView: View {
    let model: SessionModel, select: (String, String) -> Void
    @State private var host: String
    @State private var busy = false
    @Environment(\.dismiss) private var dismiss
    @State private var loaded: SpawnCatalog?
    private var settings = PiSettings()
    private var rows: Rows? { model.rows[host] }
    private var generation: UUID? { rows?.catalogGeneration }
    private var loadingModels: Bool {
        if let generation { return loaded?.generation != generation }
        return rows?.machineOnline == true && rows?.firstStep == .loading
    }
    init(model: SessionModel, initial: String, select: @escaping (String, String) -> Void) {
        self.model = model; self.select = select; _host = State(initialValue: initial)
    }
    var body: some View {
        NavigationStack {
            SpawnFields(model: model, form: SpawnForm(host: host, askTool: settings.askTool), busy: $busy,
                        catalog: loaded?.value(for: generation),
                        loadingModels: loadingModels, select: select) {
                Section("マシン") {
                    Picker("起動先", selection: $host) {
                        ForEach(model.hosts, id: \.id) { Text($0.displayName(in: model.hosts)).tag($0.id) }
                    }.disabled(busy)
                }
            }.id(host).navigationTitle("新しい Pi").navigationBarTitleDisplayMode(.inline)
                .toolbar { ToolbarItem(placement: .cancellationAction) { Button("閉じる") { dismiss() }.disabled(busy) } }
        }.interactiveDismissDisabled(busy).task(id: generation) {
            loaded = nil
            guard let generation else { return }
            let result = try? await model.hostModels(host)
            guard !Task.isCancelled, generation == self.generation else { return }
            loaded = SpawnCatalog(generation: generation, payload: result)
        }
    }
}

private struct SpawnFields<HostPicker: View>: View {
    let model: SessionModel
    @State var form: SpawnForm
    @Binding var busy: Bool
    let catalog: Object?, loadingModels: Bool
    let select: (String, String) -> Void
    @ViewBuilder var hostPicker: () -> HostPicker
    @State private var folders: [Object] = []
    @State private var home = ""
    @State private var all = false
    @State private var query = ""
    @FocusState private var editingPath: Bool
    @AppStorage("askTool") private var savedAsk = false
    private var models: [Object] { catalog?.list("models") ?? [] }
    private func same(_ a: Object, _ b: Object?) -> Bool { a.str("provider") == b?.str("provider") && a.str("id") == b?.str("id") }
    private var split: FolderPrefix? { editingPath && rows?.machineOnline == true && rows?.machineInfo["dirs"] as? Bool == true ? splitFolderPath(text: form.cwd) : nil }
    private var defaultLabel: String {
        let saved = catalog?.obj("default") ?? [:], ref = saved.obj("model"), entry = models.first { same($0, ref) }
        let name = entry?.str("name") ?? "", thinking = saved.str("thinking")
        guard !ref.str("id").isEmpty else { return "Pi の既定" }
        let level = thinking.isEmpty || thinking == "off" || (entry?["thinkingLevels"] as? [String])?.count == 1 ? "" : " · " + thinking
        return "Pi の既定（" + (name.isEmpty ? ref.str("id") : name) + level + "）"
    }
    @State private var dirs: [String] = []
    @State private var fetchedParent = ""
    @State private var request: Task<Void, Never>?
    @State private var loading = true
    @State private var note = ""
    @State private var outcome = ""
    private var rows: Rows? { model.rows[form.host] }
    private func choose(_ path: String) {
        form.folder(path, knownTrusted: folders.contains { $0["trusted"] as? Bool == true && SpawnForm.sameFolder(path, known: $0.str("path"), home: home) })
    }
    var body: some View {
        Form {
            hostPicker()
            Section {
                TextField("フォルダのパス", text: Binding(get: { form.cwd }, set: choose)).focused($editingPath).submitLabel(.done).onSubmit { editingPath = false }.autocorrectionDisabled().textInputAutocapitalization(.never)
                if loading { ProgressView("候補を取得中…") }
                ForEach(folders.prefix(8).indices, id: \.self) { i in Button(folders[i].str("path")) { choose(folders[i].str("path")) } }
                if let split, fetchedParent.utf8.elementsEqual(split.parent.utf8) {
                    let matches = matchFolders(names: dirs, prefix: split.prefix)
                    ForEach(matches.indices, id: \.self) { i in
                        let name = matches[i]
                        Button(name + "/") { choose(split.parent + name + "/") }
                    }
                }
                Toggle("このフォルダを信頼する", isOn: $form.trust)
            } header: { Text("フォルダ") } footer: { Text("フォルダ内の設定と Extension を読み込みます。オンにすると PC の Pi にも記憶され、次回から確認されません。") }
            Section("モデル") {
                NavigationLink {
                    List {
                        let favorites = catalog?.list("scoped") ?? []
                        let term = models.count > 6 ? query.trimmingCharacters(in: .whitespacesAndNewlines) : ""
                        if !favorites.isEmpty {
                            Picker("表示するモデル", selection: $all) { Text("お気に入り").tag(false); Text("すべて").tag(true) }.pickerStyle(.segmented)
                        }
                        if models.count > 6 { TextField("モデルを検索", text: $query).textInputAutocapitalization(.never).autocorrectionDisabled() }
                        if term.isEmpty { Button(defaultLabel) { form.model = nil; form.thinking = nil } }
                        let shown = models.indices.filter { i in
                            let m = models[i]
                            return (favorites.isEmpty || all || favorites.contains { same($0, m) }) && (term.isEmpty || (m.str("provider") + "/" + m.str("id") + " " + m.str("name")).localizedCaseInsensitiveContains(term))
                        }
                        ForEach(shown, id: \.self) { i in
                            let m = models[i]
                            Button { form.model = ["id": m.str("id"), "provider": m.str("provider")]; form.thinking = nil } label: {
                                HStack { Text(m.str("provider") + "/" + m.str("id")); if same(m, form.model) { SwiftUI.Image(systemName: "checkmark") } }
                            }
                        }
                        if loadingModels { ProgressView("モデルを読み込み中…") }
                        else if catalog == nil { Text("モデル一覧を取得できません。起動時の設定: " + (form.model?.str("id") ?? defaultLabel)).foregroundStyle(.secondary) }
                        else if shown.isEmpty { Text("一致するモデルはありません").foregroundStyle(.secondary) }
                    }.navigationTitle("モデル").onChange(of: models.count > 6) { _, searchable in if !searchable { query = "" } }
                } label: { LabeledContent("モデル", value: form.model?.str("id") ?? defaultLabel) }.accessibilityIdentifier("spawn-model")
                if let levels = models.first(where: { $0.str("id") == form.model?.str("id") && $0.str("provider") == form.model?.str("provider") })?["thinkingLevels"] as? [String], levels.count > 1 {
                    Picker("Thinking", selection: $form.thinking) { Text("既定").tag(String?.none); ForEach(levels, id: \.self) { Text($0).tag(Optional($0)) } }
                }
                Toggle("質問ツール（pimate_ask）", isOn: $form.askTool).onChange(of: form.askTool) { _, value in savedAsk = value }
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
                home = result.str("home"); folders = result.list("folders"); if form.cwd.isEmpty { choose(home) }
            } catch { if !Task.isCancelled { note = "最近のフォルダを取得できません。パスを直接入力できます。" } }
        }.task(id: split.map { Data($0.parent.utf8) }) {
            guard let parent = split?.parent else { fetchedParent = ""; return }
            do {
                try await Task.sleep(for: .milliseconds(150))
                let result = try await model.machineRequest(form.host, .dirs(path: parent)); try Task.checkCancellation()
                dirs = result["dirs"] as? [String] ?? []; fetchedParent = parent
            } catch { if !Task.isCancelled { fetchedParent = "" } }
        }.onDisappear { request?.cancel() }
    }
}

struct SpawnCatalog {
    let generation: UUID, payload: Object?
    func value(for connection: UUID?) -> Object? { connection == generation ? payload : nil }
}

extension Rows {
    var catalogGeneration: UUID? {
        firstStep == .list ? connectionGeneration : nil
    }
    var machineInfo: Object { groups["host"]?.first { $0.id == "info" }?.object ?? [:] }
    var machineOnline: Bool { groups["host"]?.first { $0.id == "connection" }?.string == "connected" }
}
