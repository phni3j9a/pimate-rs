import SwiftUI

// UIKit owns the marked range. Return is always a newline/IME confirmation, never a send shortcut.
private struct MessageField: UIViewRepresentable {
    let text: String, changed: (String, Bool) -> Void
    @ScaledMetric(relativeTo: .body) var points: CGFloat = 16
    @AppStorage("typeSize") var size = TypeSize.standard
    final class Coordinator: NSObject, UITextViewDelegate {
        var changed: (String, Bool) -> Void
        init(_ changed: @escaping (String, Bool) -> Void) { self.changed = changed }
        func textViewDidChange(_ v: UITextView) { changed(v.text, v.markedTextRange != nil) }
        func textViewDidChangeSelection(_ v: UITextView) { if v.isFirstResponder { changed(v.text, v.markedTextRange != nil) } }
    }
    func makeCoordinator() -> Coordinator { Coordinator(changed) }
    func makeUIView(context: Context) -> UITextView {
        let v = UITextView(); v.delegate = context.coordinator; v.backgroundColor = .clear; v.accessibilityLabel = "メッセージ"; v.accessibilityIdentifier = "message"; v.textContainer.lineFragmentPadding = 0; return v
    }
    func updateUIView(_ v: UITextView, context: Context) {
        context.coordinator.changed = changed; v.font = .systemFont(ofSize: points * size.factor); v.textColor = UIColor(PiTheme.text)
        if v.markedTextRange == nil && v.text != text { v.text = text }
    }
    func sizeThatFits(_ p: ProposedViewSize, uiView: UITextView, context: Context) -> CGSize? {
        guard let w = p.width else { return nil }; return CGSize(width: w, height: min(136, max(44, uiView.sizeThatFits(CGSize(width: w, height: .greatestFiniteMagnitude)).height)))
    }
}
struct Composer: View {
    let model: SessionModel, key: EditorKey, rows: Rows, running: Bool, blocked: Bool
    @State private var marked = false
    @State private var error = ""
    var body: some View {
        let draft = model.body(key)
        VStack(alignment: .leading, spacing: 4) {
            ForEach(rows.groups["draft"] ?? [], id: \.id) { item in
                let o = item.object, status = o.str("status")
                if o.str("instanceId") == key.target.instance && ["queued","unconfirmed","sending"].contains(status) {
                    Text(status == "queued" ? "割り込み待ち" : status == "sending" ? "送信中" : "届いたか未確認 · 履歴を確認してください").font(.caption.bold())
                    Text(o.obj("params").str("text")).lineLimit(2)
                    if status == "unconfirmed" { Button("下書きへ戻す") { do { try model.restore(key, id: item.id) } catch { self.error = "下書きを復元できません" } }.disabled(model.busy || !draft.text.isEmpty || !draft.images.isEmpty || key.target.session != o["sessionId"] as? String) }
                }
            }
            if !model.message.isEmpty || !error.isEmpty { Text(error.isEmpty ? model.message : error).font(.caption).foregroundStyle(PiTheme.failure) }
            if blocked { Label("上の質問や PC の操作を完了すると入力できます", systemImage: "pause.circle").modifier(PiFont(.caption)).foregroundStyle(PiTheme.secondary).padding(10) }
            else {
                VStack(alignment: .leading, spacing: 0) {
                    ZStack(alignment: .topLeading) {
                        if draft.text.isEmpty { Text("メッセージ").foregroundStyle(PiTheme.secondary).padding(.top, 9).accessibilityHidden(true) }
                        MessageField(text: draft.text) { value, composing in model.text(value, for: key); marked = composing }
                    }
                    HStack(spacing: 8) {
                        if !draft.images.isEmpty { Text("画像 \(draft.images.count)枚").font(.caption) }
                        Text(running ? "割り込みで送信" : "メッセージを送信").modifier(PiFont(.caption)).foregroundStyle(PiTheme.secondary); Spacer()
                        if running { Button("停止", systemImage: "stop.fill") { Task { try? await model.abort(key) } }.labelStyle(.iconOnly).frame(minWidth: 44, minHeight: 44).disabled(model.busy) }
                        Button { guard !marked else { return }; error = ""; Task { try? await model.send(key, draft) } } label: {
                            SwiftUI.Image(systemName: "arrow.up.circle.fill").font(.system(size: 36)).frame(minWidth: 44, minHeight: 44)
                        }.accessibilityLabel("送信").disabled(marked || model.busy || (draft.text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty && draft.images.isEmpty))
                    }
                }.padding(.horizontal, 12).padding(.vertical, 4).background(PiTheme.surface, in: RoundedRectangle(cornerRadius: 20))
            }
        }.padding(.horizontal, 12).padding(.vertical, 4).background(PiTheme.background)
    }
}
