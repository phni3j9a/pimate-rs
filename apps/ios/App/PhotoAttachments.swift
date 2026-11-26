import UniformTypeIdentifiers
import CoreTransferable
import PhotosUI
import SwiftUI
import ImageIO

// The provider owns its URL only for the duration of the transfer callback.
struct PickedPhoto: Transferable {
    let url: URL
    static var transferRepresentation: some TransferRepresentation {
        FileRepresentation(importedContentType: .image) { file in
            let copy = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
            try FileManager.default.copyItem(at: file.file, to: copy)
            return Self(url: copy)
        }
    }
}

func scaledPhoto(_ data: Data, pixels: Int) -> UIImage? {
    guard let source = CGImageSourceCreateWithData(data as CFData, [kCGImageSourceShouldCache: false] as CFDictionary),
          let bitmap = CGImageSourceCreateThumbnailAtIndex(source, 0, [kCGImageSourceCreateThumbnailFromImageAlways: true,
            kCGImageSourceCreateThumbnailWithTransform: true, kCGImageSourceThumbnailMaxPixelSize: pixels] as CFDictionary) else { return nil }
    return UIImage(cgImage: bitmap)
}

struct DraftPhotos: View {
    @Environment(\.photoContext) private var context
    let images: [Image], remove: (Int) -> Void
    var body: some View {
        ScrollView(.horizontal) {
            HStack(spacing: 8) {
                ForEach(images.indices, id: \.self) { i in
                    VStack(spacing: 0) {
                        Button {
                            context?.open(OpenPhoto(start: i, count: images.count, label: { "添付画像 \($0 + 1) / \(images.count)" }, source: { index, _ in .inline(images[index].data) }))
                        } label: {
                            DraftThumbnail(image: images[i]).frame(width: 58, height: 58).clipped().clipShape(RoundedRectangle(cornerRadius: 8))
                        }.accessibilityLabel("添付画像 \(i + 1) を開く")
                        Button { remove(i) } label: {
                            SwiftUI.Image(systemName: "xmark.circle").frame(width: 58, height: 44)
                        }.accessibilityLabel("画像 \(i + 1) を取り消す")
                    }
                }
            }
        }
    }
}
private struct DraftThumbnail: View {
    let image: Image
    @State private var thumbnail: UIImage?
    var body: some View {
        Group { if let thumbnail { SwiftUI.Image(uiImage: thumbnail).resizable().scaledToFill() } else { SwiftUI.Image(systemName: "photo") } }
            .task(id: image.data) { thumbnail = Data(base64Encoded: image.data).flatMap { scaledPhoto($0, pixels: 160) } }
    }
}
