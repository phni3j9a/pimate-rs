import UIKit
import XCTest
@testable import PiMate

// No DNS or network: stall after the headers, including on oversize to prove we don't wait for EOF.
private final class ImageProtocol: URLProtocol {
    override class func canInit(with request: URLRequest) -> Bool { true }
    override class func canonicalRequest(for request: URLRequest) -> URLRequest { request }
    override func startLoading() {
        let url = request.url!
        client?.urlProtocol(self, didReceive: HTTPURLResponse(url: url, statusCode: 200, httpVersion: "HTTP/1.1", headerFields: [:])!, cacheStoragePolicy: .notAllowed)
        switch url.path {
        case "/oversize": client?.urlProtocol(self, didLoad: Data(repeating: 1, count: 65536))
        case "/stall": break
        default: client?.urlProtocol(self, didLoad: Data([1, 2])); client?.urlProtocolDidFinishLoading(self)
        }
    }
    override func stopLoading() {}
}

final class WebImageTests: XCTestCase {
    @MainActor func testTileIdentitySurvivesReparseButChangesWithAnswerOrInlineBytes() throws {
        func key(_ src: String, _ kind: ImageKind, _ answer: String) throws -> [String] {
            let image = MarkdownImage(src: SharedText(value: src), alt: "図", kind: kind, index: 0)
            return PhotoTile.imageID(try ChatImageSource(markdown: image, answer: answer), answer: answer)
        }
        XCTAssertEqual(try key("shot.png", .file, "A"), try key("shot.png", .file, "A"))
        XCTAssertNotEqual(try key("shot.png", .file, "A"), try key("shot.png", .file, "B"))
        let prefix = "data:image/png;base64,"
        XCTAssertEqual(try key(prefix + "YQ==", .data, "A"), try key(prefix + "YQ==", .data, "A"))
        XCTAssertNotEqual(try key(prefix + "YQ==", .data, "A"), try key(prefix + "Yg==", .data, "A"))
    }
    @MainActor func testHTTPMarkdownIsALinkNotAFetch() async throws {
        let url = URL(string: "http://example.invalid/pic")!
        let image = MarkdownImage(src: SharedText(value: url.absoluteString), alt: "図", kind: .remote, index: 0)
        let label = Selectable.imageLabel(image, links: true, size: 15)
        XCTAssertEqual(label.string, "図"); XCTAssertEqual(label.attribute(.link, at: 0, effectiveRange: nil) as? URL, url)
        XCTAssertThrowsError(try ChatImageSource(markdown: image, answer: "a"))
        do { _ = try await WebImage.download(url); XCTFail() } catch CoreError.BadRequest {}
    }

    func testRedirectDelegateChecksEveryHop() {
        let url = URL(string: "https://example.invalid/a")!, session = URLSession(configuration: .ephemeral)
        let task = session.dataTask(with: url), redirect = HTTPURLResponse(url: url, statusCode: 302, httpVersion: nil, headerFields: nil)!
        defer { task.cancel(); session.invalidateAndCancel() }
        func check(_ delegate: WebImage, _ destination: String, _ allowed: Bool) {
            let request = URLRequest(url: URL(string: destination)!)
            var called = false
            delegate.urlSession(session, task: task, willPerformHTTPRedirection: redirect, newRequest: request) { next in
                called = true; XCTAssertEqual(next?.url, allowed ? request.url : nil)
            }; XCTAssertTrue(called)
        }
        for unsafe in ["http://example.invalid/a", "https://user:pw@example.invalid/a", "file:///tmp/a"] {
            let delegate = WebImage(); check(delegate, url.absoluteString, true); check(delegate, unsafe, false)
        }
        let delegate = WebImage(); for _ in 0..<5 { check(delegate, url.absoluteString, true) }; check(delegate, url.absoluteString, false)
    }

    func testOneDeadlineAndStreamingLimit() async throws {
        func load(_ path: String, timeout: Duration) async throws -> Data {
            let config = URLSessionConfiguration.ephemeral; config.protocolClasses = [ImageProtocol.self]
            return try await WebImage.download(URL(string: "https://example.invalid/" + path)!, configuration: config, limit: 2, timeout: timeout)
        }
        let bytes = try await load("exact", timeout: .seconds(3)); XCTAssertEqual(bytes, Data([1, 2]))
        do { _ = try await load("oversize", timeout: .seconds(3)); XCTFail("oversize response accepted") } catch CoreError.BadRequest {}
        do { _ = try await load("stall", timeout: .milliseconds(150)); XCTFail("stalled transfer completed") }
        catch let error as URLError { XCTAssertEqual(error.code, .timedOut) }
    }
}
