import XCTest
@testable import PiMate

final class FoundationTests: XCTestCase {
    func testAtomicStoreAndConditionalRevocation() throws {
        let service = "dev.pimate.tests.\(UUID())", store = KeychainStore(service: service)
        var h = Host(id: "h", name: "first", address: "a", deviceId: "old", needsPairing: false)
        defer { try? store.forget(id: h.id) }
        try store.save(host: h, token: "old value")
        h.deviceId = "new"; h.name = "second"; try store.save(host: h, token: "fresh value")
        try store.invalidate(id: "other", deviceId: h.deviceId); try store.invalidate(id: h.id, deviceId: "old")
        let reopened = KeychainStore(service: service)
        XCTAssertEqual(try reopened.hosts(), [h]); XCTAssertTrue(try reopened.load(id: h.id).token == "fresh value")
        try reopened.invalidate(id: h.id, deviceId: h.deviceId)
        XCTAssertTrue(try store.load(id: h.id).host.needsPairing); XCTAssertTrue(try store.load(id: h.id).token.isEmpty)
        try store.forget(id: h.id); XCTAssertTrue(try store.hosts().isEmpty)
    }
    func testRowsIgnoreOldRevisionsAndReplaceOnResync() {
        func row(_ id: String, _ json: String) -> Item { Item(collection: "view", id: id, position: 0, json: json) }
        let snapshot = Snapshot(revision: 5, items: [row("phase", "\"ready\""), row("instanceId", "\"a\"")])
        var r = Rows(); r.apply(1, [.resyncRequired], snapshot: { snapshot }); r.apply(4, [.remove(collection: "view", id: "phase")], snapshot: { snapshot })
        XCTAssertNil(r.ready("b")); XCTAssertEqual(r.ready("a"), Target(instance: "a", session: nil))
        r.apply(6, [.upsert(row("session", "{\"id\":\"s\"}"))], snapshot: { snapshot })
        XCTAssertEqual(r.ready("a")?.session, "s"); r.apply(7, [.remove(collection: "view", id: "phase")], snapshot: { snapshot }); XCTAssertNil(r.ready("a"))
    }
    @MainActor func testListenerOverloadOnMain() async {
        let done = expectation(description: "one drain")
        let listener = Updates { batches in
            XCTAssertTrue(Thread.isMainThread); XCTAssertEqual(batches.first?.1, [.resyncRequired]); XCTAssertEqual(batches.last?.0, 10); done.fulfill()
        }
        // Keep the main queue occupied while the Rust callback thread enqueues changes.
        DispatchQueue.global().sync { for rev in 1...10 { listener.changed(revision: UInt64(rev), changes: []) } }
        await fulfillment(of: [done], timeout: 3)
    }
    @MainActor func testDemo() async throws {
        let url = URL.cachesDirectory.appending(path: "pimate-test-link-1")
        guard FileManager.default.fileExists(atPath: url.path) else {
            if ProcessInfo.processInfo.environment["PIMATE_REQUIRE_DEMO"] == "1" { XCTFail("missing disposable fixture"); return }
            throw XCTSkip("Use apps/ios/test.sh for the loopback fixture")
        }
        func consume(_ url: URL) throws -> String {
            let link = try String(contentsOf: url, encoding: .utf8); try FileManager.default.removeItem(at: url); return link
        }
        let store = KeychainStore(service: "dev.pimate.tests.\(UUID())"), model = SessionModel(store: store)
        defer { for h in model.hosts { try? store.forget(id: h.id) } }
        try await model.pair(consume(url)); let host = try XCTUnwrap(model.hosts.first?.id)
        func wait(_ predicate: () -> Bool) async throws {
            for _ in 0..<200 { if predicate() { return }; try await Task.sleep(for: .milliseconds(100)) }; throw CoreError.Timeout
        }
        try await wait { model.rows[host]?.groups["instance"]?.contains { $0.id == "demo" } == true }
        try model.select(host: host, instance: "demo"); try await wait { model.editor != nil }
        let key = try XCTUnwrap(model.editor), text = "確認用の送信 \(UUID())"
        model.text(text, for: key)
        try await model.pair(consume(URL.cachesDirectory.appending(path: "pimate-test-link-2")))
        let other = try XCTUnwrap(model.hosts.first { $0.id != host }?.id)
        try await wait { model.rows[other]?.groups["instance"]?.contains { $0.id == "demo" } == true }
        try model.select(host: other, instance: "demo"); try await wait { model.editor?.host == other }
        let otherKey = try XCTUnwrap(model.editor); XCTAssertTrue(model.body(otherKey).text.isEmpty)
        model.text("stale", for: key)
        do { try await model.send(key, model.body(key)); XCTFail("old screen sent a prompt") } catch CoreError.StaleTarget {}
        XCTAssertTrue(model.body(otherKey).text.isEmpty)
        try model.select(host: host, instance: "demo"); try await wait { model.editor == key }; XCTAssertEqual(model.body(key).text, text)
        try await model.send(key, model.body(key)); try await wait { model.rows[host]?.groups["entry"]?.contains { $0.json.contains(text) } == true }
        XCTAssertEqual(model.body(key).text, "")
    }
}
