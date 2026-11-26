import SwiftUI
import PhotosUI

// UIKit owns the marked range. Return only confirms a candidate or inserts a newline.
@MainActor final class MessageInput: NSObject, ObservableObject, UITextViewDelegate {
    @Published private(set) var marked = false
    private weak var field: UITextView?
    private var changed: (String) -> Void = { _ in }
    func bind(_ view: UITextView, changed: @escaping (String) -> Void) {
        field = view; self.changed = changed; view.delegate = self
    }
    func textViewDidChange(_ view: UITextView) {
        let composing = view.markedTextRange != nil
        if marked != composing { marked = composing }
        changed(view.text)
    }
    func textViewDidChangeSelection(_ view: UITextView) { if view.isFirstResponder { textViewDidChange(view) } }
    func submission(_ shown: RestoredDraft) -> RestoredDraft? {
        // Recheck UIKit synchronously at the tap, not just the last SwiftUI button state.
        guard let field, !marked, field.markedTextRange == nil, field.text == shown.text,
              !shown.text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty || !shown.images.isEmpty else { return nil }
        return shown
    }
}
private struct MessageField: UIViewRepresentable {
    let text: String, input: MessageInput, changed: (String) -> Void
    @ScaledMetric(relativeTo: .body) var points: CGFloat = 16
    @AppStorage("typeSize") var size = TypeSize.standard
    func makeUIView(context: Context) -> UITextView {
        let v = UITextView(); v.backgroundColor = .clear; v.accessibilityLabel = "メッセージ"; v.accessibilityIdentifier = "message"; v.textContainer.lineFragmentPadding = 0; return v
    }
    func updateUIView(_ v: UITextView, context: Context) {
        input.bind(v, changed: changed); v.font = .systemFont(ofSize: points * size.factor); v.textColor = UIColor(PiTheme.text)
        if v.markedTextRange == nil && v.text != text { v.text = text }
    }
    func sizeThatFits(_ p: ProposedViewSize, uiView: UITextView, context: Context) -> CGSize? {
        guard let w = p.width else { return nil }; return CGSize(width: w, height: min(136, max(44, uiView.sizeThatFits(CGSize(width: w, height: .greatestFiniteMagnitude)).height)))
    }
}
struct Composer: View {
    let model: SessionModel, key: EditorKey, rows: Rows, running: Bool, blocked: Bool
    @State private var sendError = ""
    @State private var selection: [PhotosPickerItem] = []
    @StateObject private var input = MessageInput()
    static func submit(_ model: SessionModel, key: EditorKey, sent: RestoredDraft) async throws -> String {
        // A slash after whitespace is prompt text. Only whitespace after the name is optional.
        if sent.text.hasPrefix("/") {
            let whitespace: (Unicode.Scalar) -> Bool = { $0 == "\u{feff}" || ($0.properties.isWhitespace && $0 != "\u{85}") }
            let tail = sent.text.unicodeScalars.dropFirst(), token = tail.prefix { !whitespace($0) }
            let name = String(token)
            if model.rows[key.host]?.groups["command"]?.contains(where: { $0.id == name && $0.object.str("source") == "builtin" }) == true {
                guard tail.dropFirst(token.count).allSatisfy(whitespace), sent.images.isEmpty else { return "この操作には引数や画像を付けずに送信してください。" }
                if let page = try await model.builtin(key, sent) {
                    switch page {
                    case "model": _ = try model.openSheet(key, page: .model)
                    case "resume": _ = try model.openSheet(key, page: .resume)
                    case "fork": _ = try model.openSheet(key, page: .fork)
                    case "tree": _ = try model.openSheet(key, page: .tree)
                    default: throw CoreError.Unsupported
                    }
                }; return ""
            }
        }
        try await model.send(key, sent); return ""
    }
    static func canStop(_ rows: Rows) -> Bool {
        rows.field("agentActive") == "true" && rows.field("phase") == "\"ready\"" && rows.machineOnline
    }
    @ViewBuilder private var stop: some View {
        if Self.canStop(rows) {
            Button { Task { try? await model.abort(key) } } label: {
                SwiftUI.Image(systemName: "stop.fill").frame(minWidth: 44, minHeight: 44)
            }.accessibilityLabel("停止").disabled(model.busy)
        }
    }
    var body: some View {
        let draft = model.body(key)
        VStack(alignment: .leading, spacing: 4) {
            DraftList(rows: rows, instance: key.target.instance, model: model, key: key)
            if model.needsMoveConfirmation, let move = model.move {
                VStack(alignment: .leading, spacing: 3) {
                    Label("送信先が変わりました", systemImage: "arrow.triangle.swap").font(.subheadline.bold())
                    let name = move.notice.obj("to").str("name")
                    Text(name.isEmpty ? "前の会話に下書きが残っています。" : "前の下書きは移動しません。これからの送信先は「\(name)」です。").font(.caption)
                    Button("新しい送信先を確認") { model.confirmMove(key, id: move.id) }.frame(minHeight: 44)
                }.frame(maxWidth: .infinity, alignment: .leading).padding(10).background(PiTheme.waiting.opacity(0.12), in: RoundedRectangle(cornerRadius: 12))
            }
            if !model.message.isEmpty || !sendError.isEmpty { Text(model.message.isEmpty ? sendError : model.message).font(.caption).foregroundStyle(PiTheme.failure) }
            UsageRings(model: model, key: key, rows: rows)
            if blocked { HStack { Label("上の質問や PC の操作を完了すると入力できます", systemImage: "pause.circle").modifier(PiFont(.caption)).foregroundStyle(PiTheme.secondary).padding(10); Spacer(); stop } }
            else {
                if rows.field("slashCommands") == "true", !input.marked, draft.text.hasPrefix("/"), !draft.text.contains(where: { $0.isWhitespace }) {
                    CommandPalette(model: model, key: key, text: draft.text, commands: rows.groups["command"] ?? [])
                }
                VStack(alignment: .leading, spacing: 0) {
                    if !draft.images.isEmpty {
                        DraftPhotos(images: draft.images) { i in
                            do { try model.removeAttachment(key, shown: draft.images, at: i) } catch { sendError = "画像を取り消せません。添付内容を確認してください。" }
                        }.disabled(!selection.isEmpty || model.submissionBlocked)
                    }
                    ZStack(alignment: .topLeading) {
                        if draft.text.isEmpty { Text("メッセージ").foregroundStyle(PiTheme.secondary).padding(.top, 9).accessibilityHidden(true) }
                        MessageField(text: draft.text, input: input) { model.text($0, for: key) }
                    }
                    HStack(spacing: 8) {
                        if rows.field("promptImages") == "true" {
                            let remaining = min(4, Int(rows.field("promptImageLimit") ?? "1") ?? 1) - draft.images.count
                            PhotosPicker(selection: $selection, maxSelectionCount: max(1, remaining), selectionBehavior: .ordered, matching: .images) {
                                SwiftUI.Image(systemName: "photo.on.rectangle").frame(minWidth: 44, minHeight: 44)
                            }.accessibilityLabel("画像を添付").disabled(remaining <= 0 || !selection.isEmpty || model.submissionBlocked)
                        }
                        if !selection.isEmpty { ProgressView().accessibilityLabel("画像を準備中") }
                        Text(running ? "割り込みで送信" : "メッセージを送信").modifier(PiFont(.caption)).foregroundStyle(PiTheme.secondary); Spacer()
                        stop
                        Button { guard let sent = input.submission(draft) else { return }; Task {
                            sendError = ""
                            do {
                                sendError = try await Self.submit(model, key: key, sent: sent)
                            } catch { if model.editor == key && model.message.isEmpty { sendError = "送信できません。現在の送信先と入力内容を確認してください。" } }
                        } } label: {
                            SwiftUI.Image(systemName: "arrow.up.circle.fill").font(.system(size: 36)).frame(minWidth: 44, minHeight: 44)
                        }.accessibilityLabel("送信").disabled(!selection.isEmpty || input.marked || model.submissionBlocked || (draft.text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty && draft.images.isEmpty))
                    }
                }.padding(.horizontal, 12).padding(.vertical, 4).background(PiTheme.surface, in: RoundedRectangle(cornerRadius: 20))
            }
        }.padding(.horizontal, 12).padding(.vertical, 4).background(PiTheme.background)
        .task(id: selection) {
            guard !selection.isEmpty else { return }
            var files: [URL] = []
            defer { for url in files { try? FileManager.default.removeItem(at: url) } }
            do {
                for item in selection {
                    try Task.checkCancellation()
                    guard let photo = try await item.loadTransferable(type: PickedPhoto.self) else { throw CoreError.BadRequest }
                    files.append(photo.url)
                }
                try Task.checkCancellation(); try await model.attach(key, files: files); sendError = ""
            } catch { if !Task.isCancelled { sendError = "画像を追加できませんでした。写真を選び直してください。" } }
            if !Task.isCancelled { selection = [] }
        }
    }
}

struct DraftList: View {
    let rows: Rows, instance: String, model: SessionModel, key: EditorKey?
    @State private var error = ""
    struct Notice: Identifiable {
        let id: String, status: String, text: String, session: String?, imageCount: Int
        @MainActor func canRestore(_ key: EditorKey?, model: SessionModel) -> Bool {
            guard let key, model.editor == key, !model.busy, session == key.target.session, status == "unconfirmed" else { return false }
            let draft = model.body(key); return draft.text.isEmpty && draft.images.isEmpty
        }
    }
    static func notices(_ rows: Rows, instance: String, ready: Bool) -> [Notice] {
        (rows.groups["draft"] ?? []).compactMap { item in
            let o = item.object, status = o.str("status")
            guard o.str("instanceId") == instance, (ready ? ["queued", "sending", "unconfirmed"] : ["unconfirmed"]).contains(status) else { return nil }
            return Notice(id: item.id, status: status, text: o.obj("params").str("text"), session: o["sessionId"] as? String, imageCount: o.obj("params").list("images").count)
        }
    }
    var body: some View {
        let notices = Self.notices(rows, instance: instance, ready: key != nil)
        if !notices.isEmpty {
            ViewThatFits(in: .vertical) {
                content(notices).fixedSize(horizontal: false, vertical: true)
                ScrollView { content(notices) }.accessibilityLabel("送信状態の一覧")
            }.frame(maxHeight: 128).padding(.horizontal, 8)
        }
    }
    private func content(_ notices: [Notice]) -> some View {
        VStack(alignment: .leading, spacing: 6) {
            ForEach(notices) { n in
                Text(n.status == "queued" ? "割り込み待ち" : n.status == "sending" ? "送信中" : "届いたか未確認 · 履歴を確認してください").font(.caption.bold())
                if n.imageCount > 0 { Label("画像 \(n.imageCount) 枚", systemImage: "photo").font(.caption) }
                if !n.text.isEmpty { Text(verbatim: n.text).lineLimit(2) }
                if n.status == "unconfirmed" {
                    Text("自動で再送しません。").font(.caption).foregroundStyle(PiTheme.secondary)
                    Button("下書きへ戻す") {
                        guard let key else { return }
                        do { try model.restore(key, id: n.id); error = "" } catch { self.error = "下書きを復元できません" }
                    }.frame(minHeight: 44).disabled(!n.canRestore(key, model: model))
                }
            }
            if !error.isEmpty { Text(error).foregroundStyle(PiTheme.failure) }
        }.frame(maxWidth: .infinity, alignment: .leading)
    }
}
