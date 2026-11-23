// UniFFI 0.32 does not propagate Swift Task.cancel() to the in-flight Rust future.
// Check between requests; register cleanup before the first await.
extension Connection {
    public func withLogin<Result>(holdId: String, _ body: (Target) async throws -> Result) async throws -> Result {
        try Task.checkCancellation()
        defer { closeLogin(holdId: holdId) }
        let target = try await openLogin(holdId: holdId)
        try Task.checkCancellation()
        return try await body(target)
    }

    public func followLogin(target: Target) async throws {
        repeat { try Task.checkCancellation() } while try await followLoginOnce(target: target)
        try Task.checkCancellation()
    }
}
