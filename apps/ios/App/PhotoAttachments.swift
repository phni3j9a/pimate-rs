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
    let images: [Image], remove: (Int) -> Void
    var body: some View {
        ScrollView(.horizontal) {
            HStack(spacing: 8) {
                ForEach(images.indices, id: \.self) { i in
                    Button { remove(i) } label: {
                        ZStack(alignment: .topTrailing) {
                            DraftThumbnail(image: images[i]).frame(width: 58, height: 58).clipped().clipShape(RoundedRectangle(cornerRadius: 8))
                            SwiftUI.Image(systemName: "xmark.circle.fill").symbolRenderingMode(.palette).foregroundStyle(.white, .black)
                        }
                    }.accessibilityLabel("画像 \(i + 1) を取り消す")
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
