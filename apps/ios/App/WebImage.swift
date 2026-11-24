import Foundation

// A session is used for exactly one image, including all its redirects. Its delegate queue is serial.
final class WebImage: NSObject, URLSessionTaskDelegate, @unchecked Sendable {
    private var remaining = 5
    static func valid(_ url: URL?, scheme: String) -> Bool {
        guard let url else { return false }
        return url.scheme?.lowercased() == scheme && !(url.host ?? "").isEmpty && url.user == nil && url.password == nil
    }
    func urlSession(_ session: URLSession, task: URLSessionTask, willPerformHTTPRedirection response: HTTPURLResponse,
                    newRequest request: URLRequest, completionHandler: @escaping (URLRequest?) -> Void) {
        guard remaining > 0, Self.valid(request.url, scheme: "https") else { completionHandler(nil); return }
        remaining -= 1; completionHandler(request)
    }

    static func download(_ url: URL, configuration: URLSessionConfiguration = .ephemeral,
                         limit: Int = 8 * 1024 * 1024, timeout: Duration = .seconds(20)) async throws -> Data {
        guard valid(url, scheme: "https") else { throw CoreError.BadRequest }
        configuration.httpCookieStorage = nil; configuration.httpShouldSetCookies = false
        configuration.urlCredentialStorage = nil; configuration.urlCache = nil
        let deadline = ContinuousClock.now.advanced(by: timeout)
        let session = URLSession(configuration: configuration, delegate: WebImage(), delegateQueue: nil)
        return try await withThrowingTaskGroup(of: Data.self) { group in
            group.addTask {
                try await Task.sleep(until: deadline, clock: .continuous)
                throw URLError(.timedOut)
            }
            group.addTask {
                let (bytes, response) = try await session.bytes(from: url)
                guard let http = response as? HTTPURLResponse, (200..<300).contains(http.statusCode),
                      response.expectedContentLength <= Int64(limit) else { throw CoreError.BadRequest }
                var data = Data()
                for try await byte in bytes {
                    guard data.count < limit else { throw CoreError.BadRequest }; data.append(byte)
                }
                return data
            }
            defer { group.cancelAll(); session.invalidateAndCancel() }
            return try await group.next()!
        }
    }
}
