import SwiftUI

extension ChatSheet: Identifiable {}

struct PiSheetMenu: ViewModifier {
    let model: SessionModel, key: EditorKey
    @State private var openFailed = false
    func body(content: Content) -> some View {
        let shown = model.sheet.flatMap { $0.key == key && $0.page != .commands ? $0 : nil }
        content.toolbar {
            ToolbarItem(placement: .topBarTrailing) {
                Menu {
                    Button("モデル・Thinking", systemImage: "slider.horizontal.3") { open(.model) }
                } label: { Label("会話の操作", systemImage: "ellipsis.circle") }
            }
        }.sheet(item: Binding(get: { shown }, set: { if $0 == nil, let shown { model.closeSheet(shown) } })) { opened in
            PiSheetPanel(model: model, opened: opened).id(opened.id)
                .presentationDetents([.fraction(0.82), .large]).presentationDragIndicator(.visible)
        }.alert("会話が変わりました。操作を選び直してください。", isPresented: $openFailed) { Button("閉じる") {} }
    }
    private func open(_ page: ChatSheet.Page) {
        do { _ = try model.openSheet(key, page: page) } catch { openFailed = true }
    }
}

struct PiSheetPanel: View {
    let model: SessionModel, opened: ChatSheet
    @State private var acknowledged = false
    var rows: Rows { model.rows[opened.key.host] ?? Rows() }
    func field(_ key: String) -> Item? { rows.groups["view"]?.first { $0.id == key } }
    var live: ChatSheet? { model.sheet?.id == opened.id ? model.sheet : nil }
    var ready: Bool { live?.loading == false && model.editor == opened.key && !model.busy && (rows.groups["busy"] ?? []).isEmpty }
    func act(_ action: SheetAction) {
        guard ready else { return }
        acknowledged = false
        Task {
            do { try await model.run(action, in: opened); acknowledged = true }
            catch { /* run publishes the error to this presentation only. */ }
        }
    }
    private func problem(_ error: CoreError) -> String {
        switch error {
        case .Rejected, .Blocked: return "操作は受け付けられませんでした。Pi の状態を確認してください。"
        default: return "応答を確認できません。もう一度操作する前に、現在の状態を確認してください。"
        }
    }
    @ViewBuilder var feedback: some View {
        if let error = live?.error {
            Text(problem(error))
                .foregroundStyle(PiTheme.failure)
        } else if live?.loading == true { ProgressView("応答を待っています…") }
        else if acknowledged { Label("要求を受け付けました", systemImage: "tray.and.arrow.down") }
        if let receipt = model.receipt {
            Section("直近の操作結果") { ReceiptNote(receipt: receipt) { model.dismissReceipt(host: opened.key.host, receipt) } }
        }
    }
    var body: some View {
        NavigationStack {
            Group {
                switch opened.page {
                case .model: ModelSettings(panel: self)
                default: EmptyView()
                }
            }.toolbar { ToolbarItem(placement: .confirmationAction) { Button("閉じる") { model.closeSheet(opened) }.accessibilityIdentifier("sheet-close") } }
        }.tint(PiTheme.accent)
    }
}

private struct ModelSettings: View {
    let panel: PiSheetPanel
    @State private var favoritesOnly = false
    @State private var search = ""
    private func ref(_ row: Object) -> ModelRef { ModelRef(provider: row.str("provider"), id: row.str("id")) }
    var body: some View {
        let models = panel.rows.groups["model"] ?? [], current = ref(panel.field("model")?.object ?? [:])
        let favorites = models.filter { $0.object["favorite"] as? Bool == true }.map { ref($0.object) }
        let shown = models.filter { row in
            let o = row.object
            return (!favoritesOnly || favorites.contains(ref(o))) && (search.isEmpty || [o.str("name"), o.str("provider"), o.str("id")].joined(separator: " ").localizedCaseInsensitiveContains(search))
        }
        let active = models.first { ref($0.object) == current }, levels = active?.object["thinkingLevels"] as? [String] ?? []
        let thinking = panel.field("thinking")?.string ?? ""
        List {
            Section {
                Picker("表示するモデル", selection: $favoritesOnly) { Text("すべて").tag(false); Text("お気に入り").tag(true) }.pickerStyle(.segmented)
                if models.count > 6 { TextField("モデルを検索", text: $search).textInputAutocapitalization(.never) }
                ForEach(shown, id: \.id) { row in
                    let o = row.object, m = ref(o), name = o.str("name").isEmpty ? m.id : o.str("name"), favorite = favorites.contains(m)
                    HStack {
                        Button { panel.act(.select(m)) } label: {
                            HStack {
                                VStack(alignment: .leading) { Text(verbatim: name).foregroundStyle(.primary); Text(verbatim: "\(m.provider)/\(m.id)").font(.caption).foregroundStyle(.secondary) }
                                Spacer(); if current == m { SwiftUI.Image(systemName: "checkmark") }
                            }.frame(minHeight: 44).contentShape(Rectangle())
                        }.accessibilityAddTraits(current == m ? .isSelected : []).disabled(current == m)
                        Button { panel.act(.favorites(favorite ? favorites.filter { $0 != m } : favorites + [m])) } label: {
                            SwiftUI.Image(systemName: favorite ? "star.fill" : "star").frame(width: 44, height: 44)
                        }.accessibilityLabel("\(name)をお気に入り\(favorite ? "から外す" : "に追加")")
                    }.buttonStyle(.borderless).disabled(!panel.ready)
                }
                if shown.isEmpty && panel.live?.loading == false { Text(favoritesOnly ? "該当するお気に入りはありません" : "該当するモデルはありません").foregroundStyle(.secondary) }
            } header: { Text("この Pi のモデル") } footer: { Text("選択は PC の Pi にも反映されます。お気に入りは PC の /scoped-models と共通です。") }
            Section("Thinking") {
                if levels.count > 1 {
                    Picker("レベル", selection: Binding(get: { thinking }, set: { panel.act(.thinking($0)) })) {
                        if !levels.contains(thinking) { Text("不明").tag(thinking) }
                        ForEach(levels, id: \.self) { Text(verbatim: $0).tag($0) }
                    }.disabled(!panel.ready)
                } else { Text(levels.first ?? "利用できるレベルはありません").foregroundStyle(.secondary) }
            }
            if let saved = panel.field("defaultModel"), saved.json != "null" {
                let value = saved.object, previous = ref(value.obj("model")), level = levels.count > 1 && levels.contains(thinking) ? thinking : nil
                Section("新しい Pi の既定") {
                    Text(verbatim: previous.id.isEmpty ? "未設定（Pi が選択）" : [previous.provider + "/" + previous.id, value.str("thinking")].filter { !$0.isEmpty }.joined(separator: " · "))
                    Button("現在のモデルと Thinking を既定にする") { panel.act(.saveDefault(current, thinking: level)) }
                        .disabled(!panel.ready || active == nil || (previous == current && (level == nil || level == value.str("thinking"))))
                }
            }
            panel.feedback
            if panel.live?.error != nil { Button("一覧を再取得") { Task { try? await panel.model.run(.models, in: panel.opened) } } }
        }.navigationTitle("モデル・Thinking").navigationBarTitleDisplayMode(.inline)
        .task { if panel.live?.loading == false && panel.live?.reply == nil && panel.live?.error == nil { try? await panel.model.run(.models, in: panel.opened) } }
    }
}
