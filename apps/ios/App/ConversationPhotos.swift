import SwiftUI

struct OpenPhoto: Identifiable {
    let id = UUID()
    let start: Int, count: Int
    let label: (Int) -> String, source: (Int, Bool) -> ChatImageSource?
}
struct PhotoContext {
    let model: SessionModel, key: EditorKey, entries: Bool, files: Bool
    let open: (OpenPhoto) -> Void
}
private struct PhotoContextKey: EnvironmentKey { static let defaultValue: PhotoContext? = nil }
extension EnvironmentValues {
    var photoContext: PhotoContext? { get { self[PhotoContextKey.self] } set { self[PhotoContextKey.self] = newValue } }
}

private struct PhotoViewportKey: EnvironmentKey { static let defaultValue = CGRect.zero }
private extension EnvironmentValues {
    var photoViewport: CGRect { get { self[PhotoViewportKey.self] } set { self[PhotoViewportKey.self] = newValue } }
}
struct PhotoViewport: ViewModifier {
    @State private var viewport = CGRect.zero
    func body(content: Content) -> some View {
        content.environment(\.photoViewport, viewport).onGeometryChange(for: CGRect.self) { proxy in
            let inset = proxy.safeAreaInsets
            return proxy.frame(in: .global).inset(by: UIEdgeInsets(top: inset.top, left: inset.leading, bottom: inset.bottom, right: inset.trailing))
        } action: { viewport = $0 }
    }
}
struct HistoryPhotos: View {
    let images: HistoryImages
    @Environment(\.photoContext) private var context
    var body: some View {
        if let context, images.count > 0 {
            LazyVGrid(columns: Array(repeating: GridItem(.flexible(), spacing: 6), count: min(2, images.count)), spacing: 6) {
                ForEach(0..<min(4, images.count), id: \.self) { i in
                    let gallery = OpenPhoto(start: i, count: images.count, label: { "画像 \($0 + 1) / \(images.count)" }, source: { index, thumbnail in
                        images.source(at: index, thumbnail: thumbnail, canFetch: context.entries)
                    })
                    PhotoTile(context: context, gallery: gallery).frame(height: 88)
                        .overlay { if i == 3 && images.count > 4 { Text("+\(images.count - 3)").bold().foregroundStyle(.white).padding(6).background(.black.opacity(0.75), in: Capsule()).allowsHitTesting(false).accessibilityHidden(true) } }
                }
            }.frame(maxWidth: 256)
            if !context.entries { Text("画像 \(images.count) 枚 · プレビューのみ").font(.caption).foregroundStyle(PiTheme.secondary) }
        }
    }
}
struct PhotoTile: View {
    let context: PhotoContext, gallery: OpenPhoto
    @Environment(\.photoViewport) private var viewport
    @State private var visible = false
    var body: some View {
        Button { context.open(gallery) } label: {
            Group {
                if visible { LoadedPhoto(context: context, label: gallery.label(gallery.start), thumbnail: true, source: { gallery.source(gallery.start, true) }) }
                else { Color.clear }
            }.frame(maxWidth: .infinity, maxHeight: .infinity)
                .onGeometryChange(for: Bool.self) { proxy in
                    !viewport.isEmpty && viewport.intersects(proxy.frame(in: .global))
                } action: { visible = $0 }
                .background(PiTheme.surface, in: RoundedRectangle(cornerRadius: 10))
        }.buttonStyle(.plain).accessibilityLabel(gallery.label(gallery.start)).accessibilityHint("全画面で開く")
    }
}
struct PhotoViewer: View {
    let gallery: OpenPhoto, context: PhotoContext
    @Environment(\.dismiss) private var dismiss
    @State private var index: Int
    init(gallery: OpenPhoto, context: PhotoContext) {
        self.gallery = gallery; self.context = context; _index = State(initialValue: gallery.start)
    }
    var body: some View {
        NavigationStack {
            LoadedPhoto(context: context, label: gallery.label(index), thumbnail: false, source: { gallery.source(index, false) })
                .id(index).frame(maxWidth: .infinity, maxHeight: .infinity).contentShape(Rectangle())
                .gesture(DragGesture(minimumDistance: 50).onEnded { value in
                    let dx = value.translation.width
                    guard abs(dx) > abs(value.translation.height) else { return }
                    index = min(gallery.count - 1, max(0, index + (dx < 0 ? 1 : -1)))
                }).navigationTitle(gallery.label(index)).navigationBarTitleDisplayMode(.inline)
                .toolbar {
                    ToolbarItem(placement: .cancellationAction) { Button("閉じる") { dismiss() } }
                    if gallery.count > 1 {
                        ToolbarItemGroup(placement: .bottomBar) {
                            Button("前の画像", systemImage: "chevron.left") { index -= 1 }.disabled(index == 0)
                            Spacer(); Text("\(index + 1) / \(gallery.count)").monospacedDigit(); Spacer()
                            Button("次の画像", systemImage: "chevron.right") { index += 1 }.disabled(index == gallery.count - 1)
                        }
                    }
                }
        }
    }
}
struct LoadedPhoto: View {
    let context: PhotoContext, label: String, thumbnail: Bool, source: () -> ChatImageSource?
    @State private var image: UIImage?
    @State private var failed = false
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
                try Task.checkCancellation()
                let decode = Task.detached(priority: .userInitiated) { Task.isCancelled ? nil : scaledPhoto(data, pixels: pixels) }
                let result = await withTaskCancellationHandler { await decode.value } onCancel: { decode.cancel() }
                try Task.checkCancellation(); guard let result else { throw CoreError.BadRequest }; image = result
            } catch { if !Task.isCancelled { failed = true } }
        }
    }
}
