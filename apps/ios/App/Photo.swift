import Foundation
import ImageIO
import UniformTypeIdentifiers

func encodePhoto(_ url: URL, budget: Int) throws -> Image {
    guard url.isFileURL, budget > 0, let src = CGImageSourceCreateWithURL(url as CFURL, [kCGImageSourceShouldCache: false] as CFDictionary) else { throw CoreError.BadRequest }
    for (edge, quality) in [(1600, 0.85), (1200, 0.7), (800, 0.55), (512, 0.4), (320, 0.3)] {
        let options: [CFString: Any] = [kCGImageSourceCreateThumbnailFromImageAlways: true,
            kCGImageSourceCreateThumbnailWithTransform: true, kCGImageSourceThumbnailMaxPixelSize: edge]
        guard let image = CGImageSourceCreateThumbnailAtIndex(src, 0, options as CFDictionary) else { break }
        let bytes = NSMutableData()
        guard let dest = CGImageDestinationCreateWithData(bytes, UTType.jpeg.identifier as CFString, 1, nil) else { break }
        CGImageDestinationAddImage(dest, image, [kCGImageDestinationLossyCompressionQuality: quality] as CFDictionary)
        if CGImageDestinationFinalize(dest), (1...budget).contains(bytes.length) {
            return Image(mimeType: "image/jpeg", data: (bytes as Data).base64EncodedString())
        }
    }
    throw CoreError.BadRequest
}
