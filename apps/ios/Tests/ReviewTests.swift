import XCTest
import SwiftUI
import UIKit
@testable import PiMate

// UniFFI's no-handle fakes: no network or Keychain, and the test controls exactly when ACK arrives.
private final class TestConnection: Connection, @unchecked Sendable {
    var items: [Item] = []
    var revision: UInt64 = 1
    var checkError: CoreError?
    var receipt: @Sendable () async throws -> Void = {}
    var restored = RestoredDraft(text: "restored", images: [Image(mimeType: "image/jpeg", data: "YQ==")])
    override func takeUnconfirmed(target: Target, id: String) throws -> RestoredDraft { restored }
    override func subscribe(instance: String?) throws {}
    override func snapshot() -> Snapshot {
        Snapshot(revision: revision, items: items + [field("phase", "\"ready\""), field("instanceId", "\"pi\""), field("promptImages", "true"), field("promptImageLimit", "4")])
    }
    private func field(_ id: String, _ json: String) -> Item { Item(collection: "view", id: id, position: 0, json: json) }
    override func check(operation: PiMate.Operation) throws { if let checkError { throw checkError } }
    override func perform(operation: PiMate.Operation) async throws -> String { try await receipt(); return "{}" }
}
private final class TestCore: Core, @unchecked Sendable {
    var listeners: [String: Listener] = [:]
    var locked = false, blockedHost: String?, attempts: [String] = []
    var pairCalls = 0, lockAfterPair = false
    private var registrations = ["a", "b"].map { Host(id: $0, name: $0, address: "unused", deviceId: "d", needsPairing: false) }
    override func pair(link: String, name: String) async throws -> Host {
        pairCalls += 1
        let h = Host(id: "new", name: name, address: "unused", deviceId: "new-device", needsPairing: false)
        registrations.append(h); locked = lockAfterPair
        return h
    }
    let c = TestConnection(noHandle: .init())
    override func hosts() throws -> [Host] {
        if locked { throw CoreError.Storage }
        return registrations
    }
    override func connect(id: String, listener: Listener) throws -> Connection {
        listeners[id] = listener; attempts.append(id); if id == blockedHost { throw CoreError.Storage }; return c
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
    @MainActor func testWorkerAndHostRefusalsAreNotUnconfirmed() async throws {
        let core = TestCore(noHandle: .init()), model = SessionModel(core: core)
        try model.select(host: "a", instance: "pi"); let key = try XCTUnwrap(model.editor)
        model.text("未送信", for: key)
        for code: CoreError in [.Blocked, .BadRequest, .Unsupported, .Rejected, .BuiltinCommand] {
            core.c.receipt = { throw code }
            do { try await model.send(key, model.body(key)); XCTFail() } catch { XCTAssertEqual(error as? CoreError, code) }
            XCTAssertEqual(model.message, "操作は拒否されました"); XCTAssertEqual(model.body(key).text, "未送信")
        }
    }
    @MainActor func testUnlockAfterPairRetriesOnlyRegistrationReads() async throws {
        for listFailed in [true, false] {
            let core = TestCore(noHandle: .init()), model = SessionModel(core: core)
            core.lockAfterPair = listFailed; core.blockedHost = "new"
            try await model.pair("fixture")
            XCTAssertFalse(model.message.isEmpty); XCTAssertNil(model.rows["new"])
            core.locked = false; core.blockedHost = nil; model.retryRegistrations(); model.retryRegistrations()
            XCTAssertEqual(model.hosts.map(\.id), ["a", "b", "new"])
            XCTAssertEqual(core.attempts, listFailed ? ["a", "b", "new"] : ["a", "b", "new", "new"])
            XCTAssertEqual(core.pairCalls, 1); XCTAssertTrue(model.message.isEmpty)
            try model.select(host: "new", instance: "pi"); XCTAssertEqual(model.editor?.host, "new")
        }
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
    @MainActor func testRemoveRestoredPhotoAndPreserveEditingDuringBuiltin() async throws {
        let core = TestCore(noHandle: .init()), m = SessionModel(core: core)
        core.c.items = [Item(collection: "command", id: "compact", position: 0, json: #"{"source":"builtin"}"#)]
        try m.select(host: "a", instance: "pi"); let key = try XCTUnwrap(m.editor)
        try m.restore(key, id: "draft"); XCTAssertEqual(m.body(key), core.c.restored)
        try m.removeAttachment(key, shown: core.c.restored.images, at: 0)
        XCTAssertThrowsError(try m.removeAttachment(key, shown: core.c.restored.images, at: 0)); XCTAssertEqual(m.body(key).text, "restored")
        m.text("/compact", for: key); let sent = m.body(key)
        let gate = AsyncStream<Void>.makeStream(), wait = expectation(description: "awaiting builtin")
        core.c.receipt = { wait.fulfill(); for await _ in gate.stream { break } }
        let task = Task { try await m.builtin(key, sent) }; await fulfillment(of: [wait], timeout: 3)
        m.text("次の本文", for: key); gate.continuation.finish(); let sheet = try await task.value
        XCTAssertNil(sheet); XCTAssertEqual(m.body(key).text, "次の本文")
    }
    func testTestLaunchCannotReusePriorRegistration() throws {
        let env = ["PIMATE_TEST_STORE": "1"], first = KeychainStore.forLaunch(environment: env)
        let h = Host(id: "fixture", name: "must not appear", address: "unused", deviceId: "d", needsPairing: true)
        defer { try? first.forget(id: h.id) }; try first.save(host: h, token: "")
        XCTAssertTrue(try KeychainStore.forLaunch(environment: env).hosts().isEmpty)
    }
}

extension ReviewTests {
    @MainActor func testEmptyListRequiresFirstListAfterConnectOrReconnect() {
        var rows = Rows()
        XCTAssertEqual(Drawer.emptyMessage(connection: "接続中", rows: rows), "Pi 一覧を取得しています…")
        for (flag, expected) in [("true", "起動中の Pi はありません"), ("false", "Pi 一覧を取得しています…")] {
            rows.groups["host"] = [Item(collection: "host", id: "listKnown", position: 0, json: flag)]
            XCTAssertEqual(Drawer.emptyMessage(connection: "接続中", rows: rows), expected)
        }
        XCTAssertEqual(Drawer.emptyMessage(connection: "オフライン", rows: rows), "オフライン")
    }
    @MainActor func testSubscribeRejectionIsNotASpinner() {
        for code in ["unreachable", "not_found"] {
            let rows = Rows(Snapshot(revision: 1, items: [
                Item(collection: "view", id: "phase", position: 0, json: "\"subscribe_error\""),
                Item(collection: "view", id: "error", position: 1, json: "\"\(code)\""),
                Item(collection: "host", id: "connection", position: 0, json: "\"connected\"")]))
            XCTAssertTrue(ChatScreen.connectionProblem(rows)?.contains("一覧") == true)
        }
        XCTAssertNil(ChatScreen.connectionProblem(Rows()))
    }
    @MainActor func testOfflineUnconfirmedShownOnlyForSelectedPiAndCannotRestore() {
        var r = Rows(); r.groups["draft"] = [draftItem(1), draftItem(2, status: "queued"), draftItem(3, instance: "other")]
        let notices = DraftList.notices(r, instance: "pi", ready: false)
        XCTAssertEqual(notices.map(\.id), ["1"]); XCTAssertEqual(notices.first?.text, "例1")
        XCTAssertEqual(notices.first?.canRestore(nil, model: SessionModel(core: TestCore(noHandle: .init()))), false)
    }
    private func draftItem(_ n: Int, instance: String = "pi", status: String = "unconfirmed") -> Item {
        Item(collection: "draft", id: String(n), position: UInt64(n), json: "{\"instanceId\":\"\(instance)\",\"status\":\"\(status)\",\"params\":{\"text\":\"例\(n)\"}}")
    }
    @MainActor func testQueueHeightBoundAtLargeType() {
        var r = Rows(); r.groups["draft"] = (0..<30).map { draftItem($0, status: "queued") }
        let model = SessionModel(core: TestCore(noHandle: .init()))
        let view = DraftList(rows: r, instance: "pi", model: model, key: EditorKey(host: "a", target: Target(instance: "pi", session: nil)))
        let size = UIHostingController(rootView: view.dynamicTypeSize(.accessibility3)).sizeThatFits(in: CGSize(width: 320, height: 600))
        XCTAssertGreaterThan(size.height, 0); XCTAssertLessThanOrEqual(size.height, 128)
        XCTAssertEqual(DraftList.notices(r, instance: "pi", ready: true).count, 30)
    }
}

extension ReviewTests {
    @MainActor func testImageRequestsShareStreamingPathButNotAnswer() async throws {
        let images = ChatImages(), entered = expectation(description: "file_image"), gate = AsyncStream<Void>.makeStream()
        var calls = 0
        func fetch(_ answer: String) async throws -> Data {
            try await images.load(.file(answer: answer, path: "pic.png"), thumbnail: true) { method, fields in
                XCTAssertEqual(method, .fileImage); XCTAssertEqual(fields["path"] as? String, "pic.png"); calls += 1
                if calls == 1 { entered.fulfill(); for await _ in gate.stream { break } }; return #"{"image":{"data":"YQ=="}}"#
            }
        }
        let a = Task { try await fetch("A") }; await fulfillment(of: [entered], timeout: 3)
        let b = Task { try await fetch("A") }; await Task.yield(); gate.continuation.finish()
        let first = try await a.value, second = try await b.value; XCTAssertEqual(first, second)
        _ = try await fetch("A"); XCTAssertEqual(calls, 1); _ = try await fetch("B"); XCTAssertEqual(calls, 2)
    }
    @MainActor func testSheetDoesNotDeliverToReopenedSheet() async throws {
        let core = TestCore(noHandle: .init()), m = SessionModel(core: core)
        try m.select(host: "a", instance: "pi"); let key = try XCTUnwrap(m.editor), first = try m.openSheet(key, page: .resume)
        let wait = expectation(description: "pending"), gate = AsyncStream<Void>.makeStream()
        core.c.receipt = { wait.fulfill(); for await _ in gate.stream { break } }
        let task = Task { try await m.run(.sessions, in: first) }; await fulfillment(of: [wait], timeout: 3)
        let reopened = try m.openSheet(key, page: .resume); gate.continuation.finish()
        do { try await task.value; XCTFail() } catch CoreError.StaleTarget {}
        XCTAssertEqual(m.sheet?.id, reopened.id); XCTAssertNil(m.sheet?.reply); XCTAssertNil(m.sheet?.error)
    }
}


extension ReviewTests {
    @MainActor func testMoveConfirmationBlocksBuiltinBeforeRouting() async throws {
        let core = TestCore(noHandle: .init()), model = SessionModel(core: core)
        func session(_ id: String) -> Item { Item(collection: "view", id: "session", position: 0, json: "{\"id\":\"\(id)\"}") }
        core.c.items = [session("A")]; try model.select(host: "a", instance: "pi")
        model.text("old", for: try XCTUnwrap(model.editor))
        core.c.items = [session("B"), Item(collection: "notice", id: "sessionChanged", position: 0, json: #"{"reason":"resume","from":{"id":"A"},"to":{"id":"B"}}"#), Item(collection: "command", id: "compact", position: 0, json: #"{"source":"builtin"}"#)]
        core.c.revision += 1; try model.select(host: "a", instance: "pi"); let key = try XCTUnwrap(model.editor)
        model.text("/compact", for: key); core.c.receipt = { XCTFail("must not submit") }
        do { _ = try await model.builtin(key, model.body(key)); XCTFail() } catch CoreError.Blocked {}
        model.confirmMove(key, id: try XCTUnwrap(model.move?.id)); core.c.receipt = {}
        _ = try await model.builtin(key, model.body(key)); XCTAssertEqual(model.body(key).text, "")
    }
}

extension ReviewTests {
    @MainActor func testRestartWithinDeliveryBatchInvalidatesDestructiveSheet() async throws {
        for restart in [true, false] {
            let core = TestCore(noHandle: .init()), model = SessionModel(core: core)
            try model.select(host: "a", instance: "pi"); let key = try XCTUnwrap(model.editor), sheet = try model.openSheet(key, page: .session)
            let listener = try XCTUnwrap(core.listeners["a"])
            if restart { listener.changed(revision: 2, changes: [.upsert(Item(collection: "view", id: "startedAt", position: 0, json: "2"))]) }
            else {
                listener.changed(revision: 2, changes: [.remove(collection: "view", id: "phase")]); listener.changed(revision: 3, changes: [.upsert(Item(collection: "view", id: "phase", position: 0, json: #""ready""#))])
            }
            await withCheckedContinuation { c in DispatchQueue.main.async { c.resume() } }
            XCTAssertEqual(model.editor, key); XCTAssertNil(model.sheet); core.c.receipt = { XCTFail("old confirmation reached a new Pi") }
            do { try await model.run(.shutdown, in: sheet); XCTFail() } catch CoreError.StaleTarget {}
        }
    }
}

extension ReviewTests {
    @MainActor func testFileImageCacheEvictsLeastRecentlyUsed() async throws {
        let images = ChatImages(); var requests = [Int: Int]()
        func show(_ i: Int) async throws {
            _ = try await images.load(.file(answer: "A", path: "\(i).png"), thumbnail: true) { _, _ in
                requests[i, default: 0] += 1; return #"{"image":{"data":"YQ=="}}"#
            }
        }
        for i in 0..<64 { try await show(i) }
        try await show(0); try await show(64); try await show(0); try await show(1)
        XCTAssertEqual(requests[0], 1); XCTAssertEqual(requests[1], 2)
    }
}
