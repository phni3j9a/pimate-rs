import XCTest
import UIKit
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
        let store = KeychainStore(service: "dev.pimate.tests.\(UUID())"), model = SessionModel(core: Core(store: store))
        defer { for h in model.hosts { try? store.forget(id: h.id) } }
        try await model.pair(consume(url)); let host = try XCTUnwrap(model.hosts.first?.id)
        XCTAssertEqual(model.pairedHostId, host); model.consumePairedHost(); XCTAssertNil(model.pairedHostId)
        func wait(_ predicate: () -> Bool) async throws {
            for _ in 0..<200 { if predicate() { return }; try await Task.sleep(for: .milliseconds(100)) }; throw CoreError.Timeout
        }
        try await wait { model.rows[host]?.groups["instance"]?.contains { $0.id == "demo" } == true }
        try model.select(host: host, instance: "demo"); try await wait { model.editor != nil }
        let key = try XCTUnwrap(model.editor), text = "確認用の送信 \(UUID())"
        model.text(text, for: key)
        try await model.pair(consume(URL.cachesDirectory.appending(path: "pimate-test-link-2")))
        XCTAssertNil(model.pairedHostId)
        let other = try XCTUnwrap(model.hosts.first { $0.id != host }?.id)
        try await wait { model.rows[other]?.groups["instance"]?.contains { $0.id == "demo" } == true }
        try model.select(host: other, instance: "demo"); try await wait { model.editor?.host == other }
        let otherKey = try XCTUnwrap(model.editor); XCTAssertTrue(model.body(otherKey).text.isEmpty)
        model.text("stale", for: key)
        do { try await model.send(key, model.body(key)); XCTFail("old screen sent a prompt") } catch CoreError.StaleTarget {}
        XCTAssertTrue(model.body(otherKey).text.isEmpty)
        try model.select(host: host, instance: "demo"); try await wait { model.editor == key }; XCTAssertEqual(model.body(key).text, text)
        // Exercise the same UIKit delegate and submission gate used by Composer. A real
        // loopback Host supplies the history; the fixture's generic follow-up is not a receipt.
        let window = try XCTUnwrap(UIApplication.shared.connectedScenes.compactMap { $0 as? UIWindowScene }.flatMap(\.windows).first(where: \.isKeyWindow))
        let field = UITextView(frame: CGRect(x: 0, y: 0, width: 260, height: 80)), input = MessageInput()
        window.addSubview(field); defer { field.resignFirstResponder(); field.removeFromSuperview() }
        input.bind(field) { model.text($0, for: key) }; XCTAssertTrue(field.becomeFirstResponder())
        func sendIfConfirmed() async throws -> Bool {
            guard let draft = input.submission(model.body(key)) else { return false }
            try await model.send(key, draft); return true
        }
        func users() -> [String] {
            (model.rows[host]?.groups["entry"] ?? []).map(\.object).filter { $0.str("kind") == "user" }.map { $0.str("text") }
        }
        let before = users()
        field.setMarkedText("にほんご", selectedRange: NSRange(location: 4, length: 0)); XCTAssertNotNil(field.markedTextRange)
        try await wait { input.marked && model.body(key).text == "にほんご" }
        let premature = try await sendIfConfirmed(); XCTAssertFalse(premature)
        field.setMarkedText("日本語", selectedRange: NSRange(location: 3, length: 0)); field.unmarkText()
        field.insertText("\n"); field.insertText("確定")
        let expected = "日本語\n確定"
        try await wait { !input.marked && model.body(key).text == expected }
        XCTAssertNil(field.markedTextRange); XCTAssertEqual(users(), before) // Return did not send.
        let sent = try await sendIfConfirmed(); XCTAssertTrue(sent)
        try await wait { users().count > before.count }; XCTAssertEqual(users(), before + [expected])
        XCTAssertEqual(model.body(key).text, "")
        try await exerciseChatState(model, host: other)
    }

    @MainActor private func exerciseChatState(_ model: SessionModel, host: String) async throws {
        func wait(_ description: String, _ predicate: () -> Bool) async throws {
            for _ in 0..<200 { if predicate() { return }; try await Task.sleep(for: .milliseconds(100)) }
            XCTFail(description); throw CoreError.Timeout
        }
        let models = try await model.hostModels(host); XCTAssertFalse(models?.list("models").isEmpty ?? true)
        let devices = try await model.machineRequest(host, .devices); XCTAssertEqual(devices.list("devices").first?["current"] as? Bool, true)
        XCTAssertEqual(model.rows[host]?.firstStep, .list)
        try model.select(host: host, instance: "main"); try await wait("main ready") { model.editor?.target.instance == "main" }
        let reviewing = try XCTUnwrap(model.editor), review = try await model.review(reviewing, turn: nil)
        XCTAssertEqual(review.files.count, 2); XCTAssertNotNil(review.latestChangedTurn)
        try model.askAbout(review); XCTAssertEqual(model.body(reviewing).text, review.reference)
        try model.select(host: host, instance: "api"); try await wait("agent ready") { model.editor?.target.instance == "api" }
        let parent = try XCTUnwrap(model.editor), child = AgentChild(run: "run", child: "0"), record = try model.workRecord(parent, child)
        _ = try await record.load(); XCTAssertFalse(record.items.isEmpty)
        _ = try await model.control(parent, child, action: "steer", text: "試験指示")
        _ = try await record.load(); XCTAssertEqual(record.items.last?.str("text"), "試験指示")
        try model.select(host: host, instance: "images")
        try await wait("images ready") { model.editor?.target.instance == "images" }
        let key = try XCTUnwrap(model.editor), opened = try model.openSheet(key, page: .model)
        try await model.run(.commands, in: opened); XCTAssertTrue(model.suggestions(key, text: "/fix").contains("fix-tests"))
        try await model.run(.models, in: opened)
        let m = try XCTUnwrap(model.sheet?.reply?.list("models").first), ref = ModelRef(provider: m.str("provider"), id: m.str("id"))
        for action: SheetAction in [.select(ref), .thinking("low"), .favorites([ref]), .saveDefault(ref, thinking: "low")] { try await model.run(action, in: opened) }
        try await wait("model settings") {
            let rows = model.rows[host]; return rows?.field("thinking") == "\"low\"" && rows?.field("defaultModel")?.contains("low") == true && rows?.groups["model"]?.filter { $0.object["favorite"] as? Bool == true }.count == 1
        }
        XCTAssertNotNil(model.stats(key)); XCTAssertEqual(model.allowances(key, nowMs: 0).map(\.gauge), [nil, nil]); XCTAssertNotNil(model.rows[host]?.field("context"))
        let entry = try XCTUnwrap(model.rows[host]?.groups["entry"]?.first { $0.object["imageCount"] as? Int == 5 })
        let history = try XCTUnwrap(HistoryImages(entry).source(at: 4, thumbnail: true, canFetch: true))
        for source: ChatImageSource in [history, .file(answer: "A", path: "out/before.png")] {
            let bytes = try await model.image(key, source: source, thumbnail: true); XCTAssertNotNil(UIImage(data: bytes))
        }
        try await model.run(.tree, in: opened); let node = try XCTUnwrap(model.sheet?.reply?.list("nodes").first?.str("id"))
        try await model.run(.label(entry: node, value: "試験"), in: opened)
        try await model.run(.tree, in: opened); XCTAssertEqual(model.sheet?.reply?.list("nodes").first?.str("label"), "試験")
        try await model.run(.sessions, in: opened); let path = try XCTUnwrap(model.sheet?.reply?.list("sessions").first?.str("path"))
        func complete(_ action: SheetAction) async throws {
            let previous = model.receipt?.id, source = try XCTUnwrap(model.editor)
            do { try await model.run(action, in: model.openSheet(source, page: .session)) } catch CoreError.StaleTarget { /* the session changed before receipt */ }
            try await wait("correlated completion for \(action)") { model.receipt != nil && model.receipt?.id != previous && model.editor != nil }
            XCTAssertEqual(model.receipt?.outcome, .done)
        }
        try await complete(.compact); try await complete(.fork(entry: entry.id))
        try await wait("fork text belongs to the destination") { model.editor.map { model.body($0).text == entry.object.str("text") } == true }
        try await complete(.new); try await complete(.resume(path: path)); XCTAssertEqual(model.editor, key)
        try await complete(.move(entry: node, summarize: false))
        do { try await model.run(.shutdown, in: model.openSheet(key, page: .session)) } catch CoreError.StaleTarget {}
        try await wait("shutdown removes only the fixture Pi") { model.rows[host]?.groups["instance"]?.contains { $0.id == "images" } == false }

    }
}
