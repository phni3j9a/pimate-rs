import SwiftUI
import CryptoKit

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
    func body(content: PhotoViewport.Content) -> some View {
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
    var answer = ""
    static func imageID(_ source: ChatImageSource?, answer: String) -> [String] {
        switch source {
        case .file(let id, let path): return ["file", id, path]
        case .entry(let id, let index): return ["entry", id, String(index)]
        case .web(let url): return ["web", answer, url.absoluteString]
        case .inline(let text): return ["inline", answer, Data(SHA256.hash(data: Data(text.utf8))).base64EncodedString()]
        case nil: return ["unavailable", answer]
        }
    }
    @Environment(\.photoViewport) private var viewport
    @State private var visible = false
    var body: some View {
        Button { context.open(gallery) } label: {
            Group {
                if visible {
                    let source = gallery.source(gallery.start, true)
                    LoadedPhoto(context: context, label: gallery.label(gallery.start), thumbnail: true, source: { source })
                        .id(Self.imageID(source, answer: answer))
                }
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
            LoadedPhoto(context: context, label: gallery.label(index), thumbnail: false, source: { gallery.source(index, false) }, swipe: { direction in
                if direction == .down { dismiss() }
                else { index = min(gallery.count - 1, max(0, index + (direction == .left ? 1 : -1))) }
            }).id(index).frame(maxWidth: .infinity, maxHeight: .infinity).navigationTitle(gallery.label(index)).navigationBarTitleDisplayMode(.inline)
                .toolbar {
                    ToolbarItem(placement: .cancellationAction) { Button("閉じる") { dismiss() }.accessibilityLabel("画像を閉じる") }
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
    var swipe: ((UISwipeGestureRecognizer.Direction) -> Void)? = nil
    @State private var image: UIImage?
    @State private var failed = false
    var body: some View {
        Group {
            if let swipe {
                ZoomPhoto(image: image, label: label, swipe: swipe).overlay {
                    if image == nil { placeholder.allowsHitTesting(false) }
                }
            }
            else if let image { SwiftUI.Image(uiImage: image).resizable().scaledToFit().accessibilityLabel(label) }
            else { placeholder }
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
    @ViewBuilder private var placeholder: some View {
        if failed { Label("画像を表示できません", systemImage: "photo.badge.exclamationmark").font(.caption).foregroundStyle(PiTheme.secondary) }
        else { ProgressView().accessibilityLabel("画像を読み込み中") }
    }
}

private struct ZoomPhoto: UIViewRepresentable {
    let image: UIImage?, label: String, swipe: (UISwipeGestureRecognizer.Direction) -> Void
    func makeUIView(context: Context) -> Canvas { Canvas() }
    func updateUIView(_ view: Canvas, context: Context) {
        if view.picture.image !== image { view.picture.image = image; view.setNeedsLayout() }
        view.picture.isAccessibilityElement = image != nil; view.picture.accessibilityLabel = label; view.swipe = swipe
    }
    final class Canvas: UIScrollView, UIScrollViewDelegate {
        let picture = UIImageView()
        var swipe: ((UISwipeGestureRecognizer.Direction) -> Void)?
        private var pageSwipes: [UISwipeGestureRecognizer] = []
        private var viewport = CGSize.zero
        override init(frame: CGRect) {
            super.init(frame: frame)
            delegate = self; maximumZoomScale = 4; bouncesZoom = false; contentInsetAdjustmentBehavior = .never
            showsHorizontalScrollIndicator = false; showsVerticalScrollIndicator = false
            addSubview(picture); picture.accessibilityTraits = .image
            let tap = UITapGestureRecognizer(target: self, action: #selector(toggleZoom(_:)))
            tap.numberOfTapsRequired = 2; addGestureRecognizer(tap)
            for direction: UISwipeGestureRecognizer.Direction in [.left, .right, .down] {
                let gesture = UISwipeGestureRecognizer(target: self, action: #selector(swiped(_:)))
                gesture.direction = direction; addGestureRecognizer(gesture); pageSwipes.append(gesture)
                panGestureRecognizer.require(toFail: gesture)
            }
        }
        required init?(coder: NSCoder) { fatalError() }
        override func layoutSubviews() {
            super.layoutSubviews()
            guard let size = picture.image?.size, bounds.width > 0, bounds.height > 0 else { return }
            if viewport != bounds.size {
                viewport = bounds.size; setZoomScale(1, animated: false)
                let fit = min(bounds.width / size.width, bounds.height / size.height)
                picture.frame = CGRect(origin: .zero, size: CGSize(width: size.width * fit, height: size.height * fit)); contentSize = picture.frame.size
            }
            picture.center = CGPoint(x: max(bounds.width, contentSize.width) / 2, y: max(bounds.height, contentSize.height) / 2)
        }
        func viewForZooming(in scrollView: UIScrollView) -> UIView? { picture }
        func scrollViewDidZoom(_ scrollView: UIScrollView) {
            for swipe in pageSwipes { swipe.isEnabled = zoomScale == minimumZoomScale }
        }
        @objc private func swiped(_ gesture: UISwipeGestureRecognizer) { swipe?(gesture.direction) }
        @objc private func toggleZoom(_ tap: UITapGestureRecognizer) {
            guard picture.image != nil else { return }
            if zoomScale > minimumZoomScale { setZoomScale(1, animated: true); return }
            let p = tap.location(in: picture), w = bounds.width / 2.5, h = bounds.height / 2.5
            zoom(to: CGRect(x: p.x - w / 2, y: p.y - h / 2, width: w, height: h), animated: true)
        }
    }
}
