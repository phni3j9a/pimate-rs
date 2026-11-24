import XCTest
@testable import PiMate

private func item(_ collection: String, _ id: String, _ json: String) -> Item { Item(collection: collection, id: id, position: 0, json: json) }
private final class MachineConnection: Connection, @unchecked Sendable {
    var items = [item("view", "phase", "\"ready\""), item("view", "instanceId", "\"pi\""), item("host", "connection", "\"connected\"")]
    var calls: [PiMate.Operation] = [], holds: [String] = [], releases: [String] = []
    var reply: (PiMate.Operation) throws -> String = { _ in "{}" }
    override func snapshot() -> Snapshot { Snapshot(revision: 1, items: items) }
    override func subscribe(instance: String?) throws {}
    override func perform(operation: PiMate.Operation) async throws -> String { calls.append(operation); return try reply(operation) }
    override func openLogin(holdId: String) async throws -> Target { holds.append(holdId); return Target(instance: "auth", session: nil) }
    override func closeLogin(holdId: String) { releases.append(holdId) }
    override func followLoginOnce(target: Target) async throws -> Bool { false }
}
private final class MachineCore: Core, @unchecked Sendable {
    let a = MachineConnection(noHandle: .init()), b = MachineConnection(noHandle: .init())
    var listeners: [String: Listener] = [:], removed: String?
    override func hosts() throws -> [Host] { ["a", "b"].map { .init(id: $0, name: $0, address: "unused", deviceId: "d", needsPairing: false) } }
    override func connect(id: String, listener: Listener) throws -> Connection { listeners[id] = listener; return id == "a" ? a : b }
    override func forget(id: String) throws { removed = id }
}

@MainActor final class RemainingStateTests: XCTestCase {
    func testHostRoutingAndTrustScope() async throws {
        let core = MachineCore(noHandle: .init()); core.b.items += [item("instance", "pi", "{\"updatedAt\":1}"), item("instance", "busy", "{\"updatedAt\":9}")]
        let model = SessionModel(core: core); try model.select(host: "a", instance: "pi"); let key = try XCTUnwrap(model.editor)
        var spawn = SpawnForm(host: "b", askTool: true); spawn.folder("/r ", knownTrusted: true)
        core.b.reply = { op in
            switch op {
            case .hostModels(let pi): if pi == "busy" { throw CoreError.Blocked }; return "{\"models\":[]}"
            case .spawn(let raw): let body = try decodeObject(raw); XCTAssertEqual(body.str("cwd"), "/r "); XCTAssertEqual(body["trust"] as? Bool, true); return "{\"instanceId\":\"pi\"}"
            default: return "{}"
            }
        }
        let models = try await model.hostModels("b"); XCTAssertNotNil(models?["models"]); XCTAssertEqual(core.b.calls.count, 2)
        let ready = try await model.spawn(spawn); XCTAssertEqual(ready, .ready("pi")); spawn.folder("/r"); XCTAssertFalse(spawn.trust)
        core.b.reply = { _ in "{\"instanceId\":\"started\",\"messageError\":\"timeout\"}" }
        let unknown = try await model.spawn(spawn); XCTAssertEqual(unknown, .messageUnconfirmed("started", attach: nil))
        for op: PiMate.Operation in [.folders, .dirs(path: "/r"), .devices, .githubStatus, .updateCheck] { _ = try await model.machineRequest("b", op) }
        for action: MachineConfirmation.Action in [.update, .restart, .revoke("phone")] { _ = try await model.confirm(model.confirmation(host: "b", action)) }
        XCTAssertEqual(model.editor, key); XCTAssertTrue(core.a.calls.isEmpty)
        _ = try await model.github(key, query: ["kind": "pr", "number": 1]); XCTAssertEqual(core.a.calls.count, 1)
        let github = try XCTUnwrap(GithubItem(key, ["number": 1, "title": "検査", "url": "https://name:pass@example.org/"])); XCTAssertNil(github.url)
        model.text("本文", for: key); try model.askAbout(github); XCTAssertEqual(model.body(key).text, "本文\n#1「検査」")
        _ = try await model.confirm(model.confirmation(host: "b", .forget)); XCTAssertEqual(core.removed, "b"); XCTAssertEqual(model.editor, key)
        try model.select(host: "a", instance: nil); XCTAssertThrowsError(try model.askAbout(github))
    }
    func testLoginInvalidatedAcrossUndrawnReconnect() async throws {
        let core = MachineCore(noHandle: .init()), m = SessionModel(core: core)
        try await m.withLogin("b") { login in
            _ = try await login.request(.authList); let again = try await login.followOnce(); XCTAssertFalse(again)
            let before = m.rows["b"]?.connectionGeneration
            try core.listeners["b"]?.changed(revision: 2, changes: [.upsert(item("host", "connection", "\"offline\"")), .upsert(item("host", "connection", "\"connected\""))])
            for _ in 0..<20 where before == m.rows["b"]?.connectionGeneration { try await Task.sleep(for: .milliseconds(10)) }
            do { _ = try await login.request(.authRespond, ["flowId": "f", "promptId": "p", "value": "fixture"]); XCTFail() } catch CoreError.StaleTarget {}
        }
        try await m.withLogin("b") { login in _ = try await login.request(.authList) }
        XCTAssertEqual(core.b.holds, core.b.releases); XCTAssertEqual(Set(core.b.holds).count, 2); XCTAssertEqual(core.b.calls.count, 2); XCTAssertTrue(core.a.calls.isEmpty)
        XCTAssertEqual(CredentialOutcome(["status": "failed", "error": "storage_failed"]), .storageUnknown)
        XCTAssertEqual(CredentialOutcome(["status": "succeeded", "refreshRequired": true]), .savedNeedsReload)
    }
    func testProviderSheetOwnsHoldAndNeverReplaysInput() async throws {
        let core = MachineCore(noHandle: .init()), model = SessionModel(core: core), page = ProviderState(host: "b")
        let generation = try XCTUnwrap(model.rows["b"]?.connectionGeneration)
        let holding = Task { try await page.hold(model, generation: generation) }; defer { holding.cancel() }
        for _ in 0..<50 where page.login == nil { try await Task.sleep(for: .milliseconds(10)) }
        let login = try XCTUnwrap(page.login)
        _ = try await login.request(.authBegin, ["provider": "anthropic", "method": "api_key"])
        _ = try await login.request(.authRespond, ["flowId": "flow", "promptId": "prompt", "value": "synthetic"])
        _ = try await login.request(.authCancel, ["flowId": "flow"])
        _ = try await login.request(.authLogout, ["provider": "anthropic"])
        do { _ = try await login.request(.authWait); XCTFail() } catch CoreError.BadRequest {}
        XCTAssertTrue(model.rows["b"]?.groups.values.flatMap { $0 }.allSatisfy { !$0.json.contains("synthetic") } == true)
        holding.cancel(); try await holding.value; XCTAssertNil(page.login); XCTAssertEqual(core.b.holds, core.b.releases)
        do { _ = try await login.request(.authList); XCTFail() } catch CoreError.StaleTarget {}
        XCTAssertEqual(core.b.calls.count, 4); XCTAssertTrue(core.a.calls.isEmpty)
    }
    func testReviewJoinsLateResultAndPreservesDraft() async throws {
        let entries: [Object] = [["kind": "user", "entryId": "a"], ["kind": "assistant", "toolCalls": [["id": "e", "name": "edit", "args": ["path": "@a"]]]], ["kind": "user", "entryId": "b"], ["kind": "tool_result", "toolCallId": "e", "details": ["patch": "--- a\n+++ a\n@@ -1 +1 @@\n-old\n+new\n\\ No newline"]]]
        let c = MachineCore(noHandle: .init()); c.a.items += try entries.enumerated().map { offset, entry in
            Item(collection: "entry", id: String(offset), position: UInt64(offset), json: try jsonString(entry))
        }
        let m = SessionModel(core: c); try m.select(host: "a", instance: "pi"); let key = try XCTUnwrap(m.editor)
        for turn: String? in ["a", nil] {
            let review = try await m.review(key, turn: turn); XCTAssertEqual(review.latestChangedTurn, "a"); XCTAssertEqual(review.files.first?.path, "a")
            let diff = try XCTUnwrap(review.files.first?.pieces.first?.diff); XCTAssertEqual(diff.added, 1); XCTAssertEqual(diff.removed, 1); XCTAssertEqual(diff.hunks.first?.lines.last?.noNewline, true)
        }
        let empty = try await m.review(key, turn: "b"); XCTAssertTrue(empty.files.isEmpty)
        let review = try await m.review(key, turn: nil); m.text("本文", for: key); try m.askAbout(review); XCTAssertTrue(m.body(key).text.hasPrefix("本文\n")); XCTAssertTrue(c.a.calls.isEmpty)
        try m.select(host: "b", instance: "pi"); XCTAssertThrowsError(try m.askAbout(review))
    }
    func testRunSelectionStopAndRecord() async throws {
        let c = MachineCore(noHandle: .init()); let run: Object = ["id": "old", "toolCallId": "call", "active": true, "stopAction": "interrupt", "children": [["id": "x", "state": "running", "actions": ["stop_child", "steer"]]]]
        c.a.items += [item("runWatch", "old", "true"), item("run", "old", try jsonString(run)), Item(collection: "run", id: "next", position: 1, json: #"{"id":"next","toolCallId":"call","children":[{"id":"y","index":4,"state":"paused"}]}"#)]
        let m = SessionModel(core: c); try m.select(host: "a", instance: "pi"); let key = try XCTUnwrap(m.editor), rows = try XCTUnwrap(m.rows["a"])
        XCTAssertEqual(rows.subagentCounts, ["running": 1, "paused": 1]); XCTAssertEqual(rows.originalRun(toolCallId: "call"), "old"); XCTAssertNil(rows.resumedChild(run: "pending", index: 4)); XCTAssertEqual(rows.resumedChild(run: "next", index: 8), .init(run: "next", child: "y"))
        let stop = try XCTUnwrap(rows.stopTarget(key, run: "old", viewedChild: "x")), child = AgentChild(run: "old", child: "x")
        _ = try await m.control(stop); _ = try await m.control(key, child, action: "steer", text: "続ける"); XCTAssertEqual(m.instructions.forChild(key, child, resumedFrom: nil).first?.text, "続ける")
        var changed = run; changed["children"] = [["id": "x", "state": "complete"], ["id": "y", "state": "running"]]
        let finished = item("run", "old", try jsonString(changed))
        c.a.items.removeAll { $0.collection == "run" && $0.id == "old" }; c.a.items.append(finished)
        do { _ = try await m.control(stop); XCTFail("undelivered child completion") } catch CoreError.StaleTarget {}
        try c.listeners["a"]?.changed(revision: 2, changes: [.upsert(finished)])
        for _ in 0..<20 where m.rows["a"]?.revision == 1 { try await Task.sleep(for: .milliseconds(10)) }
        do { _ = try await m.control(stop); XCTFail() } catch CoreError.StaleTarget {}; XCTAssertEqual(c.a.calls.count, 2)
        var pages = 0; c.a.reply = { op in guard case .rc(_, .subagentTranscript, let raw) = op else { throw CoreError.BadRequest }; XCTAssertEqual(try decodeObject(raw)["cursor"] as? String, pages == 0 ? nil : "p\(pages)"); pages += 1; return try jsonString(["cursor": pages < 9 ? "p\(pages)" : "", "items": [["entryId": "e", "text": "\(pages)"]]]) }
        let record = try m.workRecord(key, child); _ = try await record.load(); XCTAssertEqual(pages, 8); XCTAssertTrue(record.more); _ = try await record.load(); XCTAssertEqual(record.items.count, 1); XCTAssertEqual(record.items.first?.str("text"), "9")
    }
    func testFirstStepAndAppUpdate() {
        var r = Rows(); XCTAssertEqual(r.firstStep, .loading); r.groups["host"] = [item("host", "connection", "\"connected\"")]; XCTAssertEqual(r.firstStep, .loading)
        r.groups["host"]?.append(item("host", "listKnown", "true")); XCTAssertEqual(r.firstStep, .empty); r.groups["instance"] = [item("instance", "pi", "{}")]; XCTAssertEqual(r.firstStep, .list)
        r.groups["host"] = [item("host", "connection", "\"revoked\"")]; XCTAssertEqual(r.firstStep, .offline)
        let result: Object = ["results": [["kind": "software", "bundleId": "test", "trackId": 42, "version": "1.10"]]]
        XCTAssertEqual(AppUpdate.lookup(version: "1.9", bundle: "test", result: result), .available("1.10", URL(string: "https://apps.apple.com/app/id42")!)); XCTAssertEqual(AppUpdate.lookup(version: "1.10.0", bundle: "test", result: result), .latest); XCTAssertEqual(AppUpdate.lookup(version: "1.10", bundle: "other", result: result), .unknown)
    }
}
