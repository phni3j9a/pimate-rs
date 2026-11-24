import SwiftUI

struct TerminalCard: View {
    let title: String, command: String?
    @State private var copied = false
    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            Label("PC のターミナルで操作が必要です", systemImage: "terminal").font(.headline).accessibilityAddTraits(.isHeader)
            Text(title.isEmpty ? "この画面はスマホでは操作できません。Pi を起動した PC で操作してください。" : "「\(title)」はスマホでは操作できません。Pi を起動した PC で操作してください。")
                .foregroundStyle(PiTheme.secondary)
            if let command {
                Button {
                    UIPasteboard.general.string = command; copied = true
                    UIAccessibility.post(notification: .announcement, argument: "コマンドをコピーしました")
                } label: {
                    HStack { Text(verbatim: command).font(.callout.monospaced()); Spacer(); SwiftUI.Image(systemName: copied ? "checkmark" : "doc.on.doc") }
                        .padding(.horizontal, 12).frame(minHeight: 44).background(PiTheme.background, in: RoundedRectangle(cornerRadius: 12))
                }.accessibilityLabel("コマンドをコピー").accessibilityValue(command)
            }
        }.padding(16).background(PiTheme.surface, in: RoundedRectangle(cornerRadius: 16))
    }
}

struct MoveNote: View {
    let move: ChatMove
    var body: some View {
        let from = move.notice.obj("from").str("name"), to = move.notice.obj("to").str("name")
        VStack(spacing: 4) {
            Label(move.local ? "この端末で会話を切り替えました" : "送信先の会話が切り替わりました", systemImage: "arrow.left.arrow.right")
            if !to.isEmpty { Text(from.isEmpty ? to : "\(from) → \(to)") }
        }.font(.caption).foregroundStyle(PiTheme.secondary).frame(maxWidth: .infinity).accessibilityElement(children: .combine)
    }
}
struct ReceiptNote: View {
    let receipt: ChatReceipt, close: () -> Void
    private var title: String {
        switch receipt.outcome {
        case .done: return "操作が完了しました"
        case .cancelled: return "操作は取り消されました"
        case .failed: return "操作に失敗しました"
        case .unknown: return "操作の結果を確認できません。履歴を確認してください。"
        }
    }
    var body: some View {
        HStack { Text(title).font(.callout); Spacer(); Button(action: close) { SwiftUI.Image(systemName: "xmark").frame(minWidth: 44, minHeight: 44) }.accessibilityLabel("操作結果を閉じる") }
    }
}
