import SwiftUI

struct OpenPhoto: Identifiable {
    let id = UUID(), label: String, source: ChatImageSource
}
struct PhotoContext {
    let model: SessionModel, key: EditorKey, entries: Bool, files: Bool
    let open: (OpenPhoto) -> Void
}
private struct PhotoContextKey: EnvironmentKey { static let defaultValue: PhotoContext? = nil }
extension EnvironmentValues {
    var photoContext: PhotoContext? { get { self[PhotoContextKey.self] } set { self[PhotoContextKey.self] = newValue } }
}

struct HistoryPhotos: View {
    let images: HistoryImages
    @Environment(\.photoContext) private var context
    var body: some View {
        if let context, images.count > 0 {
            ScrollView(.horizontal) {
                LazyHStack(spacing: 8) {
                    ForEach(0..<images.count, id: \.self) { i in
                        PhotoTile(context: context, label: "画像 \(i + 1) / \(images.count)", source: { thumbnail in
                            images.source(at: i, thumbnail: thumbnail, canFetch: context.entries)
                        }).frame(width: 108, height: 88)
                    }
                }
            }.frame(height: 88)
        }
    }
}
struct PhotoTile: View {
    let context: PhotoContext, label: String, source: (Bool) -> ChatImageSource?
    var body: some View {
        Button {
            if let full = source(false) { context.open(OpenPhoto(label: label, source: full)) }
        } label: {
            LoadedPhoto(context: context, label: label, thumbnail: true, source: { source(true) })
                .frame(maxWidth: .infinity, maxHeight: .infinity).background(PiTheme.surface, in: RoundedRectangle(cornerRadius: 10))
        }.buttonStyle(.plain).accessibilityLabel(label).accessibilityHint("全画面で開く")
    }
}
struct LoadedPhoto: View {
    let context: PhotoContext, label: String, thumbnail: Bool, source: () -> ChatImageSource?
    @State private var image: UIImage?, failed = false
    var body: some View {
        Group {
            if let image { SwiftUI.Image(uiImage: image).resizable().scaledToFit().accessibilityLabel(label) }
            else if failed { Label("画像を表示できません", systemImage: "photo.badge.exclamationmark").font(.caption).foregroundStyle(PiTheme.secondary) }
            else { ProgressView().accessibilityLabel("画像を読み込み中") }
        }.task {
            do {
                guard let source = source() else { throw CoreError.Unsupported }
                let data = try await context.model.image(context.key, source: source, thumbnail: thumbnail)
                let pixels = thumbnail ? 500 : 2400
                let result = await Task.detached(priority: .userInitiated) { scaledPhoto(data, pixels: pixels) }.value
                try Task.checkCancellation(); guard let result else { throw CoreError.BadRequest }; image = result
            } catch { if !Task.isCancelled { failed = true } }
        }
    }
}
