// UniFFI 0.32 does not propagate Swift Task.cancel() to the in-flight Rust future.
// Check between requests; ARC releases the hold even on the cancellation path.
extension Connection {
    public func withLogin<Result>(holdId: String, _ body: (Target) async throws -> Result) async throws -> Result {
        try Task.checkCancellation()
        let session = try await openLogin(holdId: holdId)
        defer { withExtendedLifetime(session) {} }
        try Task.checkCancellation()
        return try await body(session.target())
    }

    public func followLogin(target: Target) async throws {
        repeat { try Task.checkCancellation() } while try await followLoginOnce(target: target)
        try Task.checkCancellation()
    }
}
