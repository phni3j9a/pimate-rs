import CryptoKit
import Foundation

enum ChatImageSource {
    case entry(id: String, index: Int), file(answer: String, path: String), web(URL), inline(String)
    // Resolve Markdown's shared text only when a tile or viewer actually needs it.
    init(markdown: MarkdownImage, answer: String) throws {
        let src = markdown.src.value()
        switch markdown.kind {
        case .file: self = .file(answer: answer, path: src)
        case .data:
            guard let comma = src.firstIndex(of: ",") else { throw CoreError.BadRequest }
            self = .inline(String(src[src.index(after: comma)...]))
        case .remote:
            guard let url = URL(string: src) else { throw CoreError.BadRequest }; self = .web(url)
        }
    }
    fileprivate var identity: [String] {
        switch self {
        case .entry(let id, let index): return ["entry", id, String(index)]
        case .file(let answer, let path): return ["file", answer, path]
        case .web(let url): return ["url", url.absoluteString]
        case .inline(let base64): return ["data", Data(SHA256.hash(data: Data(base64.utf8))).base64EncodedString()]
        }
    }
    fileprivate func bytes(thumbnail: Bool, rc: (RcMethod, Object) async throws -> String) async throws -> Data {
        let raw: String
        switch self {
        case .entry(let id, let index): raw = try await rc(.entryImage, ["entryId": id, "index": index, "thumbnail": thumbnail])
        case .file(_, let path): raw = try await rc(.fileImage, ["path": path, "thumbnail": thumbnail])
        case .inline(let text): return try Self.decode(text)
        case .web(let url): return try await Self.download(url)
        }
        struct Payload: Decodable { struct Body: Decodable { let data: String }; let image: Body }
        return try Self.decode(JSONDecoder().decode(Payload.self, from: Data(raw.utf8)).image.data)
    }
    private static func decode(_ text: String) throws -> Data {
        guard !text.isEmpty, text.utf8.count <= 12 * 1024 * 1024, let bytes = Data(base64Encoded: text) else { throw CoreError.BadRequest }; return bytes
    }
    private static func download(_ url: URL) async throws -> Data {
        guard ["http", "https"].contains(url.scheme?.lowercased() ?? ""), url.host != nil, url.user == nil, url.password == nil else { throw CoreError.BadRequest }
        let config = URLSessionConfiguration.ephemeral; config.httpShouldSetCookies = false; config.urlCredentialStorage = nil; config.timeoutIntervalForResource = 20
        let session = URLSession(configuration: config); defer { session.invalidateAndCancel() }
        let (bytes, response) = try await session.bytes(from: url), limit = 8 * 1024 * 1024
        guard let http = response as? HTTPURLResponse, (200..<300).contains(http.statusCode), response.expectedContentLength <= Int64(limit) else { throw CoreError.BadRequest }
        var data = Data(); for try await byte in bytes { guard data.count < limit else { throw CoreError.BadRequest }; data.append(byte) }; return data
    }
}

// One current conversation. Cancelling a tile releases its lease, not the viewer's request.
@MainActor final class ChatImages {
    private struct Load { let id = UUID(), task: Task<Data, Error>; var users: Set<UUID> }
    private var loads: [[String]: Load] = [:], cached: [[String]: Data] = [:], order: [[String]] = []
    func clear() { for load in loads.values { load.task.cancel() }; loads.removeAll(); cached.removeAll(); order.removeAll() }
    func load(_ source: ChatImageSource, thumbnail: Bool, rc: @escaping (RcMethod, Object) async throws -> String) async throws -> Data {
        try Task.checkCancellation(); let key = source.identity + [String(thumbnail)], user = UUID()
        if let hit = cached[key] { return hit }
        if loads[key] == nil { loads[key] = Load(task: Task { try await source.bytes(thumbnail: thumbnail, rc: rc) }, users: []) }
        loads[key]?.users.insert(user); let flight = loads[key]!
        return try await withTaskCancellationHandler {
            defer { release(key, user) }
            let bytes = try await flight.task.value; try Task.checkCancellation()
            guard loads[key]?.id == flight.id else { throw CancellationError() }
            if cached[key] == nil {
                cached[key] = bytes; order.append(key)
                while order.count > 64 || cached.values.reduce(0, { $0 + $1.count }) > 12 * 1024 * 1024 { cached.removeValue(forKey: order.removeFirst()) }
            }
            return bytes
        } onCancel: { Task { @MainActor in self.release(key, user) } }
    }
    private func release(_ key: [String], _ user: UUID) {
        guard loads[key]?.users.remove(user) != nil, loads[key]?.users.isEmpty == true else { return }
        loads.removeValue(forKey: key)?.task.cancel()
    }
}

// Keep only supplied previews; imageCount can be much larger than the history page.
struct HistoryImages {
    let entry: String, count: Int
    private let previews: [Int: String]
    init(_ row: Item) {
        let o = row.object, images = o.list("images"), indices = o["imageIndices"] as? [Int]
        entry = row.id; count = max(images.count, o["imageCount"] as? Int ?? 0)
        var slots: [Int: String] = [:]
        for (i, image) in images.enumerated() {
            let slot = indices.map { $0.indices.contains(i) ? $0[i] : -1 } ?? i
            if (0..<count).contains(slot) { slots[slot] = image["data"] as? String }
        }
        previews = slots
    }
    func source(at index: Int, thumbnail: Bool, canFetch: Bool) -> ChatImageSource? {
        guard (0..<count).contains(index) else { return nil }
        if let text = previews[index], thumbnail || !canFetch { return .inline(text) }
        return canFetch ? .entry(id: entry, index: index) : nil
    }
}
