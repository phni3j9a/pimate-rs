import SwiftUI

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
    @StateObject private var input = MessageInput()
    var body: some View {
        let draft = model.body(key)
        VStack(alignment: .leading, spacing: 4) {
            DraftList(rows: rows, instance: key.target.instance, model: model, key: key)
            if !model.message.isEmpty { Text(model.message).font(.caption).foregroundStyle(PiTheme.failure) }
            if blocked { Label("上の質問や PC の操作を完了すると入力できます", systemImage: "pause.circle").modifier(PiFont(.caption)).foregroundStyle(PiTheme.secondary).padding(10) }
            else {
                VStack(alignment: .leading, spacing: 0) {
                    ZStack(alignment: .topLeading) {
                        if draft.text.isEmpty { Text("メッセージ").foregroundStyle(PiTheme.secondary).padding(.top, 9).accessibilityHidden(true) }
                        MessageField(text: draft.text, input: input) { model.text($0, for: key) }
                    }
                    HStack(spacing: 8) {
                        if !draft.images.isEmpty { Text("画像 \(draft.images.count)枚").font(.caption) }
                        Text(running ? "割り込みで送信" : "メッセージを送信").modifier(PiFont(.caption)).foregroundStyle(PiTheme.secondary); Spacer()
                        if running { Button("停止", systemImage: "stop.fill") { Task { try? await model.abort(key) } }.labelStyle(.iconOnly).frame(minWidth: 44, minHeight: 44).disabled(model.busy) }
                        Button { guard let sent = input.submission(draft) else { return }; Task { try? await model.send(key, sent) } } label: {
                            SwiftUI.Image(systemName: "arrow.up.circle.fill").font(.system(size: 36)).frame(minWidth: 44, minHeight: 44)
                        }.accessibilityLabel("送信").disabled(input.marked || model.busy || (draft.text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty && draft.images.isEmpty))
                    }
                }.padding(.horizontal, 12).padding(.vertical, 4).background(PiTheme.surface, in: RoundedRectangle(cornerRadius: 20))
            }
        }.padding(.horizontal, 12).padding(.vertical, 4).background(PiTheme.background)
    }
}

struct DraftList: View {
    let rows: Rows, instance: String, model: SessionModel, key: EditorKey?
    @State private var error = ""
    struct Notice: Identifiable {
        let id: String, status: String, text: String, session: String?
        @MainActor func canRestore(_ key: EditorKey?, model: SessionModel) -> Bool {
            guard let key, model.editor == key, !model.busy, session == key.target.session, status == "unconfirmed" else { return false }
            let draft = model.body(key); return draft.text.isEmpty && draft.images.isEmpty
        }
    }
    static func notices(_ rows: Rows, instance: String, ready: Bool) -> [Notice] {
        (rows.groups["draft"] ?? []).compactMap { item in
            let o = item.object, status = o.str("status")
            guard o.str("instanceId") == instance, (ready ? ["queued", "sending", "unconfirmed"] : ["unconfirmed"]).contains(status) else { return nil }
            return Notice(id: item.id, status: status, text: o.obj("params").str("text"), session: o["sessionId"] as? String)
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
                Text(verbatim: n.text).lineLimit(2)
                if n.status == "unconfirmed" {
                    Button("下書きへ戻す") {
                        guard let key else { return }
                        do { try model.restore(key, id: n.id); error = "" } catch { self.error = "下書きを復元できません" }
                    }.disabled(!n.canRestore(key, model: model))
                }
            }
            if !error.isEmpty { Text(error).foregroundStyle(PiTheme.failure) }
        }.frame(maxWidth: .infinity, alignment: .leading)
    }
}
