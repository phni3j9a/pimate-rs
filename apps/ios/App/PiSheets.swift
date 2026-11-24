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
                    Button("使用状況", systemImage: "gauge.with.dots.needle.33percent") { open(.usage) }
                    Button("セッション", systemImage: "bubble.left.and.bubble.right") { open(.session) }
                } label: { Label("会話の操作", systemImage: "ellipsis.circle") }.accessibilityIdentifier("chat-options")
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
    @State private var receiptAtStart: String? = nil
    @State private var tracksCompletion = false
    var rows: Rows { live != nil && model.editor == opened.key ? (model.rows[opened.key.host] ?? Rows()) : Rows() }
    func field(_ key: String) -> Item? { rows.groups["view"]?.first { $0.id == key } }
    var live: ChatSheet? { model.sheet?.id == opened.id ? model.sheet : nil }
    var ready: Bool { live?.loading == false && model.editor == opened.key && !model.busy && (rows.groups["busy"] ?? []).isEmpty }
    func act(_ action: SheetAction, closeOnAck: Bool = false) {
        guard ready else { return }
        acknowledged = false; receiptAtStart = model.receipt?.id
        switch action {
        case .compact, .new, .resume, .fork, .move: tracksCompletion = true
        default: tracksCompletion = false
        }
        Task {
            do {
                try await model.run(action, in: opened); acknowledged = true
                if closeOnAck { model.closeSheet(opened) }
            }
            catch { /* run publishes the error to this presentation only. */ }
        }
    }
    private func problem(_ error: CoreError) -> String {
        switch error {
        case .Rejected, .Blocked, .Unsupported, .BadRequest: return "操作は受け付けられませんでした。Pi の状態を確認してください。"
        default: return "応答を確認できません。もう一度操作する前に、現在の状態を確認してください。"
        }
    }
    @ViewBuilder private var feedback: some View {
        if tracksCompletion, let receipt = model.receipt, receipt.id != receiptAtStart {
            Text("この Pi の直近の操作結果").font(.caption).foregroundStyle(.secondary)
            ReceiptNote(receipt: receipt) { tracksCompletion = false; acknowledged = false; model.dismissReceipt(host: opened.key.host, receipt) }
        } else if let error = live?.error { Text(problem(error)).foregroundStyle(PiTheme.failure) }
        else if live?.loading == true { ProgressView("応答を待っています…") }
        else if acknowledged { Label(tracksCompletion ? "要求を受け付けました。完了は未確認です。" : "要求を受け付けました", systemImage: "tray.and.arrow.down") }
    }
    var body: some View {
        NavigationStack {
            Group {
                switch opened.page {
                case .model: ModelSettings(panel: self)
                case .usage: UsageSheet(panel: self)
                case .session: SessionSheetView(panel: self)
                case .resume: ResumeSheet(panel: self)
                case .fork: ForkSheet(panel: self)
                case .tree: TreeSheet(panel: self)
                case .commands: EmptyView()
                }
            }.toolbar { ToolbarItem(placement: .confirmationAction) { Button("閉じる") { model.closeSheet(opened) }.accessibilityIdentifier("sheet-close") } }
        }.safeAreaInset(edge: .bottom) {
            VStack(alignment: .leading, spacing: 0) { feedback }.font(.callout).padding(.horizontal)
                .frame(maxWidth: .infinity, alignment: .leading).background(.bar)
        }.tint(PiTheme.accent)
    }
}

private struct ModelSettings: View {
    let panel: PiSheetPanel
    @State private var favoriteFilter: Bool? = nil
    @State private var search = ""
    private func ref(_ row: Object) -> ModelRef { ModelRef(provider: row.str("provider"), id: row.str("id")) }
    var body: some View {
        let models = panel.rows.groups["model"] ?? [], current = ref(panel.field("model")?.object ?? [:])
        let favorites = models.filter { $0.object["favorite"] as? Bool == true }.map { ref($0.object) }
        let favoritesOnly = favoriteFilter ?? !favorites.isEmpty
        let shown = models.filter { row in
            let o = row.object
            return (!favoritesOnly || favorites.contains(ref(o))) && (search.isEmpty || [o.str("name"), o.str("provider"), o.str("id")].joined(separator: " ").localizedCaseInsensitiveContains(search))
        }
        let active = models.first { ref($0.object) == current }, levels = active?.object["thinkingLevels"] as? [String] ?? []
        let thinking = panel.field("thinking")?.string ?? ""
        let editable = panel.ready && panel.live?.reply != nil && panel.live?.error == nil
        List {
            Section {
                Picker("表示するモデル", selection: Binding(get: { favoritesOnly }, set: { favoriteFilter = $0 })) { Text("すべて").tag(false); Text("お気に入り").tag(true) }.pickerStyle(.segmented)
                if (favoritesOnly ? favorites.count : models.count) > 6 { TextField("モデルを検索", text: $search).textInputAutocapitalization(.never) }
                ForEach(shown, id: \.id) { row in
                    let o = row.object, m = ref(o), name = o.str("name").isEmpty ? m.id : o.str("name"), favorite = favorites.contains(m)
                    HStack {
                        Button { if current != m { panel.act(.select(m)) } } label: {
                            HStack {
                                VStack(alignment: .leading) { Text(verbatim: name).foregroundStyle(.primary); Text(verbatim: "\(m.provider)/\(m.id)").font(.caption).foregroundStyle(.secondary) }
                                Spacer(); if current == m { SwiftUI.Image(systemName: "checkmark") }
                            }.frame(minHeight: 44).contentShape(Rectangle())
                        }.accessibilityAddTraits(current == m ? .isSelected : [])
                        Button { panel.act(.favorites(favorite ? favorites.filter { $0 != m } : favorites + [m])) } label: {
                            SwiftUI.Image(systemName: favorite ? "star.fill" : "star").frame(width: 44, height: 44)
                        }.accessibilityLabel("\(name)をお気に入り\(favorite ? "から外す" : "に追加")")
                    }.buttonStyle(.borderless).disabled(!editable)
                }
                if shown.isEmpty && panel.live?.loading == false { Text(favoritesOnly ? "該当するお気に入りはありません" : "該当するモデルはありません").foregroundStyle(.secondary) }
            } header: { Text("この Pi のモデル") } footer: { Text("選択は PC の Pi にも反映されます。お気に入りは PC の /scoped-models と共通です。") }
            Section("Thinking") {
                if levels.count > 1 {
                    Picker("レベル", selection: Binding(get: { thinking }, set: { panel.act(.thinking($0)) })) {
                        if !levels.contains(thinking) { Text("不明").tag(thinking) }
                        ForEach(levels, id: \.self) { Text(verbatim: $0).tag($0) }
                    }.disabled(!editable)
                } else { Text(levels.first ?? "利用できるレベルはありません").foregroundStyle(.secondary) }
            }
            if let saved = panel.field("defaultModel"), saved.json != "null" {
                let value = saved.object, previous = ref(value.obj("model")), level = levels.count > 1 && levels.contains(thinking) ? thinking : nil
                Section("新しい Pi の既定") {
                    Text(verbatim: previous.id.isEmpty ? "未設定（Pi が選択）" : [previous.provider + "/" + previous.id, value.str("thinking")].filter { !$0.isEmpty }.joined(separator: " · "))
                    Button("現在のモデルと Thinking を既定にする") { panel.act(.saveDefault(current, thinking: level)) }
                        .disabled(!editable || active == nil || (previous == current && (level == nil || level == value.str("thinking"))))
                }
            }
            if panel.live?.error != nil { Button("一覧を再取得") { Task { try? await panel.model.run(.models, in: panel.opened) } } }
        }.navigationTitle("モデル・Thinking").navigationBarTitleDisplayMode(.inline)
        .task { if panel.live?.loading == false && panel.live?.reply == nil && panel.live?.error == nil { try? await panel.model.run(.models, in: panel.opened) } }
    }
}
