import XCTest
import UIKit
@testable import PiMate

// UniFFI's no-handle fakes: no network or Keychain, and the test controls exactly when ACK arrives.
private final class TestConnection: Connection, @unchecked Sendable {
    var items: [Item] = []
    var checkError: CoreError?
    var receipt: @Sendable () async throws -> Void = {}
    override func subscribe(instance: String?) throws {}
    override func snapshot() -> Snapshot {
        Snapshot(revision: 1, items: items + [field("phase", "\"ready\""), field("instanceId", "\"pi\""), field("promptImages", "true"), field("promptImageLimit", "4")])
    }
    private func field(_ id: String, _ json: String) -> Item { Item(collection: "view", id: id, position: 0, json: json) }
    override func check(operation: PiMate.Operation) throws { if let checkError { throw checkError } }
    override func perform(operation: PiMate.Operation) async throws -> String { try await receipt(); return "{}" }
}
private final class TestCore: Core, @unchecked Sendable {
    var locked = false, blockedHost: String?, attempts: [String] = []
    let c = TestConnection(noHandle: .init())
    override func hosts() throws -> [Host] {
        if locked { throw CoreError.Storage }
        return ["a", "b"].map { Host(id: $0, name: $0, address: "unused", deviceId: "d", needsPairing: false) }
    }
    override func connect(id: String, listener: Listener) throws -> Connection {
        attempts.append(id); if id == blockedHost { throw CoreError.Storage }; return c
    }
}

final class ReviewTests: XCTestCase {
    @MainActor func testLateAckCleansOnlyOriginalUnchangedDraft() async throws {
        for edited in [false, true] {
            let core = TestCore(noHandle: .init()), model = SessionModel(core: core)
            try model.select(host: "a", instance: "pi"); let a = try XCTUnwrap(model.editor)
            model.text("送信", for: a)
            let waiting = expectation(description: "sent, before ACK"), gate = AsyncStream<Void>.makeStream()
            core.c.receipt = { waiting.fulfill(); for await _ in gate.stream { break } }
            let sent = model.body(a), task = Task { try await model.send(a, sent) }
            await fulfillment(of: [waiting], timeout: 3)
            if edited { model.text("次の本文", for: a) }
            try model.select(host: "b", instance: "pi"); let b = try XCTUnwrap(model.editor)
            model.text("別の依頼", for: b); gate.continuation.finish()
            do { try await task.value; XCTFail("late success leaked to current screen") } catch CoreError.StaleTarget {}
            XCTAssertEqual(model.body(a).text, edited ? "次の本文" : ""); XCTAssertEqual(model.body(b).text, "別の依頼"); XCTAssertEqual(model.message, "")
        }
    }
    @MainActor func testUnsentAndUnknownAreDistinctAndSuccessClearsWarning() async throws {
        let core = TestCore(noHandle: .init()), model = SessionModel(core: core)
        try model.select(host: "a", instance: "pi"); let key = try XCTUnwrap(model.editor)
        model.text("送信", for: key); core.c.checkError = .BadRequest
        core.c.receipt = { XCTFail("preflight should not send") }
        do { try await model.send(key, model.body(key)); XCTFail() } catch CoreError.BadRequest {}
        XCTAssertEqual(model.message, "操作は送信されませんでした"); core.c.checkError = nil
        core.c.receipt = { throw CoreError.OutcomeUnknown }
        do { try await model.send(key, model.body(key)); XCTFail() } catch CoreError.OutcomeUnknown {}
        XCTAssertTrue(model.message.contains("履歴")); XCTAssertEqual(model.body(key).text, "送信")
        core.c.receipt = {}; try await model.send(key, model.body(key)); XCTAssertEqual(model.message, "")
    }
    @MainActor func testUnlockRetriesFailedReadsWithoutReplacingHealthyConnections() throws {
        let core = TestCore(noHandle: .init()); core.locked = true
        let model = SessionModel(core: core); XCTAssertTrue(model.hosts.isEmpty)
        core.locked = false; core.blockedHost = "b"; model.retryRegistrations()
        XCTAssertEqual(core.attempts, ["a", "b"]); XCTAssertThrowsError(try model.select(host: "b", instance: "pi"))
        core.blockedHost = nil; model.retryRegistrations(); model.retryRegistrations()
        XCTAssertEqual(core.attempts, ["a", "b", "b"]); try model.select(host: "b", instance: "pi"); XCTAssertEqual(model.message, "")
    }
    @MainActor func testQuestionMovesThenIsReplacedAndRemoved() async throws {
        let core = TestCore(noHandle: .init()), model = SessionModel(core: core)
        var question = Item(collection: "question", id: "B", position: 1, json: "old")
        func refresh() throws { core.c.items = [question]; try model.select(host: "a", instance: "pi") }
        try refresh(); let q = QuestionKey(editor: try XCTUnwrap(model.editor), requestId: "B")
        model.editAnswer(q, shown: "old", text: "下書き"); question.position = 0; try refresh()
        XCTAssertEqual(model.answerText(q), "下書き"); question.json = "new"; try refresh(); XCTAssertEqual(model.answerText(q), "")
        do { try await model.answer(q, shown: "old", fields: [:]); XCTFail() } catch CoreError.StaleTarget {}
        model.editAnswer(q, shown: "new", text: "次"); core.c.items = []; try model.select(host: "a", instance: "pi"); XCTAssertEqual(model.answerText(q), "")
    }
    @MainActor func testAttachmentsUseLocalFilesAndCommitOnlyCompleteBatch() async throws {
        let core = TestCore(noHandle: .init()), model = SessionModel(core: core)
        try model.select(host: "a", instance: "pi"); let key = try XCTUnwrap(model.editor)
        let url = URL.temporaryDirectory.appending(path: UUID().uuidString)
        let png = UIGraphicsImageRenderer(size: CGSize(width: 64, height: 64)).pngData { $0.fill(CGRect(x: 0, y: 0, width: 64, height: 64)) }
        try png.write(to: url); defer { try? FileManager.default.removeItem(at: url) }
        do { try await model.attach(key, files: [url, URL(string: "https://invalid.example/test")!]); XCTFail() } catch CoreError.BadRequest {}
        XCTAssertTrue(model.body(key).images.isEmpty)
        try await model.attach(key, files: Array(repeating: url, count: 4)); XCTAssertEqual(model.body(key).images.map(\.mimeType), Array(repeating: "image/jpeg", count: 4))
    }
    func testTestLaunchCannotReusePriorRegistration() throws {
        let env = ["PIMATE_TEST_STORE": "1"], first = KeychainStore.forLaunch(environment: env)
        let h = Host(id: "fixture", name: "must not appear", address: "unused", deviceId: "d", needsPairing: true)
        defer { try? first.forget(id: h.id) }; try first.save(host: h, token: "")
        XCTAssertTrue(try KeychainStore.forLaunch(environment: env).hosts().isEmpty)
    }
}
