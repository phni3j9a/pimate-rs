import SwiftUI

struct ChatScreen: View {
    let model: SessionModel, host: String, instance: String
    let showList: () -> Void
    private var rows: Rows { model.rows[host] ?? Rows() }
    var body: some View {
        let r = rows, info = r.groups["instance"]?.first { $0.id == instance }?.object ?? [:]
        Group {
            if let key = model.editor, key.host == host, key.target.instance == instance { Conversation(model: model, key: key, rows: r, showList: showList).id(r.viewGeneration).id(key) }
            else {
                VStack {
                    if model.edge?.connection == "offline" || model.edge?.connection == "connecting" {
                        ContentUnavailableView("再接続中", systemImage: "wifi.slash", description: Text("マシンとの接続を待っています。未確認のメッセージは自動で再送しません。"))
                    } else if let problem = Self.connectionProblem(r) { ContentUnavailableView("Pi に接続できません", systemImage: "wifi.slash", description: Text(problem)) }
                    else { ProgressView("同期しています…").frame(maxHeight: .infinity) }
                    DraftList(rows: r, instance: instance, model: model, key: nil)
                }
            }
        }.navigationTitle(info.piTitle.isEmpty ? instance : info.piTitle).navigationBarTitleDisplayMode(.inline)
    }
    static func connectionProblem(_ rows: Rows) -> String? {
        let phase = rows.field("phase")
        if phase == "\"subscribe_error\"" || phase == "\"instance_gone\"" {
            return "この Pi を開けません。一覧に戻り、起動状態を確認して選び直してください。"
        }
        let connection = rows.groups["host"]?.first { $0.id == "connection" }?.string ?? ""
        return ["revoked", "invalid_token", "auth_failed", "offline", "protocol_mismatch"].contains(connection) ? "一覧でマシンの接続と起動状態を確認してください。" : nil
    }
}
// Match the rendered content, not a time window: a question can arrive in the same layout pass.
struct TailContent: Equatable {
    let turns: [ChatTurn], stream: [Item], questions: [Item], pc: String?, receipt: String?, move: UUID?
    let agentActive: Bool
    var activeTurnID: String? { agentActive ? turns.last?.id : nil }
}
struct TailMetrics: Equatable {
    let height: CGFloat, viewport: CGRect
    var nearTail: Bool { height - viewport.maxY < 48 }
    func shouldFollow(after old: Self?, dragging: Bool, content: TailContent? = nil, manualLayout: TailContent? = nil) -> Bool {
        guard !dragging, !viewport.isEmpty else { return false }
        guard let old, !old.viewport.isEmpty else { return true }
        return old.nearTail && (((manualLayout == nil || content != manualLayout) && height != old.height) || viewport.height != old.viewport.height)
    }
}
private struct Conversation: View {
    let model: SessionModel, key: EditorKey, rows: Rows
    let showList: () -> Void
    @Environment(\.horizontalSizeClass) private var widthClass
    @State private var horizontalRegions: [CGRect] = []
    @State private var turns: [ChatTurn] = []
    private struct ReviewRoute: Identifiable { let id = UUID(); var turn: String?, line: String? }
    @State private var review: ReviewRoute?
    @State private var agents: AgentSheet?
    @State private var table: OpenTable?
    @State private var photo: OpenPhoto?
    private var photos: PhotoContext {
        PhotoContext(model: model, key: key, entries: rows.field("entryImages") == "true", files: rows.field("fileImages") == "true", open: { photo = $0 })
    }
    @State private var loaded = false
    @State private var metrics: TailMetrics?
    @State private var manualLayout: TailContent?
    @GestureState private var dragging = false
    private var running: Bool { rows.field("status") == "\"running\"" }
    var body: some View {
        let groups = rows.groups, questions = groups["question"] ?? [], stream = groups["stream"] ?? []
        let entries = groups["entry"] ?? [], tools = groups["tool"] ?? [], edge = model.edge
        let timing = groups["thinkingMs"] ?? []
        let pc = edge?.pcPrompt != nil
        let text = stream.first { $0.id == "text" }?.string ?? "", thinking = stream.first { $0.id == "thinking" }?.string ?? ""
        let content = TailContent(turns: turns, stream: stream, questions: questions, pc: rows.field("pcPrompt"), receipt: model.receipt?.id, move: model.move?.id, agentActive: rows.field("agentActive") == "true")
        let didToggle = { manualLayout = content }
        VStack(spacing: 0) {
            HStack(spacing: 6) {
                Circle().fill(questions.isEmpty && !pc ? PiTheme.accent : PiTheme.waiting).frame(width: 6, height: 6)
                Text(pc && questions.isEmpty ? "PC で操作待ち" : !questions.isEmpty ? "入力待ち" : running ? "作業中" : "待機").modifier(PiFont(.caption))
            }.foregroundStyle(PiTheme.secondary).padding(.bottom, 6).accessibilityElement(children: .combine)
            ScrollViewReader { reader in
                GeometryReader { outer in
                    ScrollView {
                        LazyVStack(alignment: .leading, spacing: 22) {
                            if loaded && turns.isEmpty && stream.isEmpty && questions.isEmpty && !pc { Text("まだメッセージはありません").foregroundStyle(PiTheme.secondary) }
                            ForEach(turns) { turn in TurnView(turn: turn, active: turn.id == content.activeTurnID, runForCall: rows.originalRun(toolCallId:), openRun: { agents = AgentSheet(run: $0) }, didToggle: didToggle, openReview: { review = ReviewRoute(turn: turn.id, line: $0) }) }
                            if !thinking.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
                                let thought = WorkLine.streamThought(stream, hasAnswer: !text.isEmpty)
                                ActiveDisclosure(automatic: thought.automatic, didToggle: didToggle) { Text(verbatim: thinking) } label: { Text(thought.title) }
                            }
                            if !text.isEmpty {
                                // A new streamed answer must not render with the previous answer's parsed blocks.
                                let answer = "stream:" + (entries.last?.id ?? "begin")
                                MarkdownBody(source: text, answer: answer, copyAnswer: true).id(answer)
                            }
                            if let move = model.move { MoveNote(move: move) }
                            if let result = model.receipt { ReceiptNote(receipt: result) { model.dismissReceipt(host: key.host, result) } }
                            if let prompt = edge?.pcPrompt, questions.isEmpty { TerminalCard(title: prompt.str("title"), command: edge?.attachCommand) }
                            ForEach(questions, id: \.id) { q in QuestionCard(model: model, key: QuestionKey(editor: key, requestId: q.id), item: q).id(q.id) }
                            Color.clear.frame(height: 1).id("tail")
                        }.padding(.horizontal, 20).padding(.vertical, 12)
                        .onGeometryChange(for: TailMetrics.self) { geometry in
                            let top = geometry.frame(in: .named("transcript")).minY
                            return TailMetrics(height: geometry.size.height, viewport: CGRect(x: 0, y: -top, width: outer.size.width, height: outer.size.height))
                        } action: { next in
                            let follow = next.shouldFollow(after: metrics, dragging: dragging, content: content, manualLayout: manualLayout)
                            if next.height != metrics?.height || manualLayout != content { manualLayout = nil }
                            metrics = next
                            if follow { reader.scrollTo("tail", anchor: .bottom) }
                        }
                    }.coordinateSpace(name: "transcript").accessibilityIdentifier("transcript").modifier(PhotoViewport()).scrollDismissesKeyboard(.interactively)
                        .onPreferenceChange(HorizontalRegions.self) { horizontalRegions = $0 }
                        .simultaneousGesture(DragGesture(coordinateSpace: .named("transcript")).updating($dragging) { _, state, _ in state = true }.onEnded { drag in
                            if widthClass == .compact, drag.translation.width > 80, drag.translation.width > abs(drag.translation.height) * 2,
                               !horizontalRegions.contains(where: { $0.contains(drag.startLocation) }) { showList() }
                        })
                        .overlay(alignment: .bottomTrailing) {
                            if metrics?.nearTail == false {
                                Button { withAnimation { reader.scrollTo("tail", anchor: .bottom) } } label: {
                                    Label("最新へ", systemImage: "arrow.down").padding(10).background(.regularMaterial, in: Capsule())
                                }.padding(8)
                            }
                        }
                        .onChange(of: questions.map(\.id)) { _, ids in if !ids.isEmpty { UIAccessibility.post(notification: .announcement, argument: "Pi からの質問が届きました") } }
                }
            }
        }.background(PiTheme.background).foregroundStyle(PiTheme.text)
            .safeAreaInset(edge: .bottom, spacing: 8) {
                VStack(spacing: 0) {
                    AgentSummary(counts: rows.subagentCounts) { agents = AgentSheet(run: nil) }
                    Composer(model: model, key: key, rows: rows, running: running, blocked: !questions.isEmpty || pc)
                }.background(PiTheme.background)
            }
            .sheet(item: $agents) { AgentsView(model: model, key: key, sheet: $0) }
            // A streaming block can disappear at message_end; the presented table must outlive it.
            .toolbar { ToolbarItem(placement: .topBarTrailing) { Button("変更をレビュー", systemImage: "doc.text.magnifyingglass") { review = ReviewRoute() } } }
            .sheet(item: $review) { route in ReviewView(model: model, key: key, turns: turns.filter { $0.id != "begin" }, turn: route.turn, line: route.line) }
            .modifier(PiSheetMenu(model: model, key: key))
            .environment(\.photoContext, photos)
            .fullScreenCover(item: $photo) { shown in
                PhotoViewer(gallery: shown, context: photos)
            }
            .environment(\.openPiTable, { table = $0 })
            .fullScreenCover(item: $table) { TableViewer(shown: $0, context: photos) }
            .task(id: [entries, tools, timing]) {
                let task = Task.detached(priority: .userInitiated) { ChatTurn.read(entries, live: tools, timing: timing) }
                let result = await withTaskCancellationHandler { await task.value } onCancel: { task.cancel() }
                if !Task.isCancelled { turns = result; loaded = true }
            }
    }
}
struct TurnView: View {
    let turn: ChatTurn, active: Bool
    let runForCall: (String) -> String?, openRun: (String) -> Void
    var didToggle: () -> Void = {}
    var openReview: ((String?) -> Void)?
    var caption: String? = nil
    @State private var expanded = false
    var body: some View {
        let folds = !active && turn.work.contains { !$0.isThought }
        let firstWork = turn.work.first?.id
        VStack(alignment: .leading, spacing: 12) {
            if !turn.prompt.isEmpty || turn.imageCount > 0 {
                VStack(alignment: .leading, spacing: 4) {
                    if !turn.prompt.isEmpty { Text(verbatim: turn.prompt).textSelection(.enabled) }
                    if let images = turn.images { HistoryPhotos(images: images) }
                    if let caption { Text(caption).modifier(PiFont(.caption)).foregroundStyle(.secondary) }
                }.padding(.horizontal, 14).padding(.vertical, 10).background(PiTheme.surface, in: RoundedRectangle(cornerRadius: 16)).padding(.leading, 30).frame(maxWidth: .infinity, alignment: .trailing)
            }
            ForEach(turn.lines) { line in
                switch line {
                case .reply(let reply):
                    MarkdownBody(source: reply.text, answer: reply.id, copyAnswer: true)
                case .work(let w):
                    if folds && w.id == firstWork {
                        Button { didToggle(); expanded.toggle() } label: {
                            HStack { Text(turn.summary); Spacer(); SwiftUI.Image(systemName: expanded ? "chevron.down" : "chevron.right") }.frame(minHeight: 44)
                        }.modifier(PiFont(.lead)).foregroundStyle(PiTheme.secondary).accessibilityValue(expanded ? "展開中" : "折りたたみ")
                        if let openReview, turn.work.contains(where: { $0.hasChange }) { Button("このターンの変更") { openReview(nil) } }
                    }
                    let run = w.callID.flatMap(runForCall)
                    if !folds || expanded || (run != nil && w.state == "失敗") {
                        if let openReview, w.hasChange { Button { openReview(w.id) } label: { Label(w.name + " " + w.subject, systemImage: "doc.text.magnifyingglass") }.accessibilityHint("差分を開く") }
                        else { work(w) }
                    }
                    if let run {
                        Button { openRun(run) } label: { Label("サブエージェントの実行", systemImage: "person.2").frame(minHeight: 44) }
                    }
                    if let images = w.images { HistoryPhotos(images: images) }
                }
            }
        }
    }
    private func work(_ line: WorkLine) -> some View {
            ActiveDisclosure(automatic: line.state == "実行中", didToggle: didToggle) {
                Text(verbatim: line.output.isEmpty ? "出力はまだありません" : line.output).modifier(PiFont(.caption)).fontDesign(.monospaced).textSelection(.enabled).padding(10).frame(maxWidth: .infinity, alignment: .leading).background(PiTheme.surface, in: RoundedRectangle(cornerRadius: 10))
            } label: {
                HStack(spacing: 8) { SwiftUI.Image(systemName: line.state == "失敗" ? "exclamationmark.circle" : line.state == "実行中" ? "clock" : line.isThought ? "text.bubble" : "checkmark.circle"); Text(line.name).fontWeight(.semibold); Text(line.subject).lineLimit(2); Spacer(); Text(line.state).modifier(PiFont(.meta)) }.modifier(PiFont(.caption)).fontDesign(.monospaced).frame(minHeight: 44)
            }.foregroundStyle(line.state == "失敗" ? PiTheme.failure : PiTheme.secondary)
    }
}

private struct ActiveDisclosure<Content: View, Label: View>: View {
    let automatic: Bool, didToggle: () -> Void
    @ViewBuilder let content: () -> Content
    @ViewBuilder let label: () -> Label
    @State private var manual: Bool?
    var body: some View {
        DisclosureGroup(isExpanded: Binding(get: { manual ?? automatic }, set: { value in
            // A manual resize is a single layout change rather than a sequence of animated heights.
            var transaction = Transaction(); transaction.disablesAnimations = true
            withTransaction(transaction) { didToggle(); manual = value }
        }), content: content, label: label)
    }
}
