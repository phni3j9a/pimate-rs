import CFNetwork
import Foundation

// Only an explicitly http: image uses this transport. HTTPS stays on URLSession with ATS.
@MainActor final class HTTPImage: NSObject, StreamDelegate {
    enum Reply { case bytes(Data), redirect(URL) }
    private var input: InputStream?, continuation: CheckedContinuation<Reply, Error>?, timeout: Task<Void, Never>?
    private var data = Data()
    private let url: URL, limit: Int
    init(url: URL, limit: Int) { self.url = url; self.limit = limit }

    func get() async throws -> Reply {
        try Task.checkCancellation()
        let request = CFHTTPMessageCreateRequest(nil, "GET" as CFString, url as CFURL, kCFHTTPVersion1_1).takeRetainedValue()
        let stream = CFReadStreamCreateForHTTPRequest(nil, request).takeRetainedValue() as InputStream
        return try await withTaskCancellationHandler {
            try await withCheckedThrowingContinuation { c in
                continuation = c; input = stream; stream.delegate = self
                timeout = Task { try? await Task.sleep(for: .seconds(20)); if !Task.isCancelled { finish(.failure(URLError(.timedOut))) } }
                stream.schedule(in: .main, forMode: .common); stream.open()
            }
        } onCancel: { Task { @MainActor in self.finish(.failure(CancellationError())) } }
    }
    nonisolated func stream(_ aStream: Stream, handle event: Stream.Event) {
        MainActor.assumeIsolated { receive(event) } // Scheduled only on the main run loop.
    }
    private func receive(_ event: Stream.Event) {
        guard let input else { return }
        switch event {
        case .hasBytesAvailable:
            var buffer = [UInt8](repeating: 0, count: 32768)
            let count = input.read(&buffer, maxLength: buffer.count)
            guard count >= 0, data.count + count <= limit else { finish(.failure(CoreError.BadRequest)); return }
            data.append(buffer, count: count)
        case .endEncountered:
            guard let header = input.property(forKey: Stream.PropertyKey(rawValue: kCFStreamPropertyHTTPResponseHeader as String)) else { finish(.failure(CoreError.BadRequest)); return }
            let response = header as! CFHTTPMessage, status = CFHTTPMessageGetResponseStatusCode(response)
            if (200..<300).contains(status) { finish(.success(.bytes(data))) }
            else if [301, 302, 303, 307, 308].contains(status), let location = CFHTTPMessageCopyHeaderFieldValue(response, "Location" as CFString)?.takeRetainedValue(), let next = URL(string: location as String, relativeTo: url)?.absoluteURL { finish(.success(.redirect(next))) }
            else { finish(.failure(CoreError.Rejected)) }
        case .errorOccurred: finish(.failure(input.streamError ?? CoreError.BadRequest))
        default: break
        }
    }
    private func finish(_ result: Result<Reply, Error>) {
        input?.delegate = nil; input?.remove(from: .main, forMode: .common); input?.close(); input = nil; timeout?.cancel(); timeout = nil
        continuation?.resume(with: result); continuation = nil
    }
}
