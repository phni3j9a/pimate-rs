import SwiftUI

struct CommandPalette: View {
    let model: SessionModel, key: EditorKey, text: String, commands: [Item]
    @State private var attempt = 0, loading = true, failed = false
    var body: some View {
        let names = model.suggestions(key, text: text)
        VStack(alignment: .leading, spacing: 4) {
            Text("コマンド候補").font(.caption.bold()).foregroundStyle(PiTheme.secondary).accessibilityAddTraits(.isHeader)
            if loading && names.isEmpty { ProgressView("一覧を取得中") }
            else if failed { Button("一覧を再取得") { attempt += 1 } }
            else if names.isEmpty { Text("一致するコマンドはありません").font(.caption) }
            ScrollView(.horizontal) {
                HStack(spacing: 8) { ForEach(names, id: \.self) { name in
                    let detail = commands.first { $0.id == name }?.object.str("description") ?? ""
                    Button {
                        model.text("/" + name + " ", for: key)
                    } label: {
                        VStack(alignment: .leading, spacing: 2) {
                            Text(verbatim: "/" + name).font(.callout.monospaced())
                            if !detail.isEmpty { Text(detail).font(.caption).lineLimit(2).foregroundStyle(PiTheme.secondary) }
                        }.padding(.horizontal, 12).frame(minHeight: 44).background(PiTheme.surface, in: RoundedRectangle(cornerRadius: 10))
                    }.buttonStyle(.plain).accessibilityIdentifier("command-" + name).accessibilityHint("入力欄に入れます。送信はしません")
                } }
            }.fixedSize(horizontal: false, vertical: true)
        }.task(id: attempt) {
            loading = true; defer { loading = false }
            do {
                let opened = try model.openSheet(key, page: .commands); defer { model.closeSheet(opened) }
                try await model.run(.commands, in: opened); failed = false
            } catch { if !Task.isCancelled { failed = true } }
        }
    }
}
