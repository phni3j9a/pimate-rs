import XCTest
@testable import PiMate

final class ChatStateTests: XCTestCase {
    func row(_ group: String, _ id: String, _ json: String) -> Item { Item(collection: group, id: id, position: 0, json: json) }
    @MainActor func testStopDependsOnParentActivityAndConnectionNotInputStatus() {
        for (active, phase, connection, expected) in [(true, "ready", "connected", true), (false, "ready", "connected", false), (true, "syncing", "connected", false), (true, "ready", "offline", false)] {
            let rows = Rows(Snapshot(revision: 1, items: [row("view", "status", #""waiting_input""#), row("view", "phase", "\"\(phase)\""), row("view", "agentActive", String(active)), row("host", "connection", "\"\(connection)\"")]))
            XCTAssertEqual(Composer.canStop(rows), expected)
        }
    }
    func testTailFollowsQuestionGrowthOnlyWhileReadingLatest() {
        func metrics(_ y: CGFloat, _ height: CGFloat) -> TailMetrics { TailMetrics(height: height, viewport: CGRect(x: 0, y: y, width: 300, height: 500)) }
        let after = metrics(500, 1300)
        let before = metrics(453, 1000)
        let entries = [row("entry", "prompt", #"{"kind":"user","text":"hello"}"#)]
        func stamp(_ tools: [Item] = [], questions: [Item] = []) -> TailContent {
            TailContent(turns: ChatTurn.read(entries, live: tools), stream: [], questions: questions, pc: nil, receipt: nil, move: nil, agentActive: true)
        }
        let manual = stamp(), question = stamp(questions: [row("question", "new", "{}")])
        let child = row("tool", "child", #"{"status":"running","parentToolCallId":"parent"}"#)
        let parent = row("tool", "parent", #"{"status":"running","toolName":"bash"}"#)
        XCTAssertFalse(after.shouldFollow(after: before, dragging: false, content: stamp([child]), manualLayout: manual))
        XCTAssertTrue(after.shouldFollow(after: before, dragging: false, content: stamp([parent]), manualLayout: manual))
        XCTAssertTrue(after.shouldFollow(after: before, dragging: false, content: question, manualLayout: manual))
        XCTAssertFalse(after.shouldFollow(after: metrics(452, 1000), dragging: false))
        XCTAssertFalse(after.shouldFollow(after: metrics(500, 1000), dragging: true))
    }
    func testQuestionDoesNotFoldActiveParentTurn() {
        let turns = [ChatTurn(id: "old"), ChatTurn(id: "current")]
        for active in [true, false] {
            let waiting = TailContent(turns: turns, stream: [], questions: [row("question", "q", "{}")], pc: nil, receipt: nil, move: nil, agentActive: active)
            XCTAssertEqual(waiting.activeTurnID, active ? "current" : nil)
        }
    }
    func testReceiptsSurviveSceneAndSnapshotButNeverChangeOwner() {
        let busy = row("busy", "operation", #"{"id":"r","method":"session_op","op":"fork","sessionId":"source"}"#)
        let done = row("operationResult", "latest", #"{"id":"r","sessionId":"dest","outcome":"done","forkText":"original"}"#)
        for snap in [false, true] {
            var rows = Rows(Snapshot(revision: 1, items: [row("view", "instanceId", #""A""#)]))
            let b = row("view", "instanceId", #""B""#)
            rows.apply(2, [.upsert(busy)], snapshot: { fatalError() })
            rows.apply(3, snap ? [.resyncRequired] : [.upsert(b), .upsert(done), .remove(collection: "busy", id: "operation")], snapshot: { Snapshot(revision: 3, items: [b, done]) })
            XCTAssertEqual(rows.notices.receipt?.instance, "A"); XCTAssertEqual(rows.notices.receipt?.outcome, .done)
            rows.notices.receipt?.dismissed = true; rows.replace(Snapshot(revision: 4, items: [done, b]))
            XCTAssertEqual(rows.notices.receipt?.dismissed, true)
            var otherHost = Rows(); otherHost.replace(Snapshot(revision: 9, items: [b, done])); XCTAssertNil(otherHost.notices.receipt)
        }
        for value: Object in [["outcome": "unknown", "event": ["type": "compact_done"]], ["event": [:]]] {
            XCTAssertEqual(ChatReceipt(instance: "A", id: "r", value: value).outcome, .unknown)
        }
    }
    func testMoveMustMatchOperationAndEachVisitGetsItsOwnConfirmation() {
        var n = ChatNotices()
        let change = row("notice", "sessionChanged", #"{"reason":"resume","from":{"id":"a"},"to":{"id":"b"}}"#)
        for op in ["compact", "switch"] {
            n.update("busy", "operation", row("busy", "operation", "{\"id\":\"r\",\"method\":\"session_op\",\"op\":\"\(op)\",\"sessionId\":\"a\",\"arrival\":\"b\"}"), instance: "pi")
            n.update("notice", "sessionChanged", change, instance: "pi"); XCTAssertEqual(n.move?.local, op == "switch")
            let id = n.move?.id; n.move?.confirmed = true; n.update("notice", "sessionChanged", change, instance: "pi", snapshot: true)
            XCTAssertNotEqual(n.move?.id, id); XCTAssertEqual(n.move?.confirmed, false); XCTAssertEqual(n.move?.local, false)
        }
    }
    func testSparseHistoryAndSafeTerminalCommand() {
        let slots = HistoryImages(row("entry", "e", #"{"imageCount":100000000,"imageIndices":[8],"images":[{"data":"YQ=="}]}"#))
        guard case .inline("YQ==") = slots.source(at: 8, thumbnail: true, canFetch: true), case .entry("e", 99999999) = slots.source(at: 99999999, thumbnail: true, canFetch: true) else { return XCTFail() }
        XCTAssertNil(slots.source(at: 1, thumbnail: true, canFetch: false))
        for (name, expected) in [("pimate-123", "pimate attach pimate-123" as String?), ("a;ls", nil), ("-x", nil), ("aa\\n", nil)] {
            XCTAssertEqual(Rows(Snapshot(revision: 1, items: [row("view", "abducoSession", "\"\(name)\"")])).attachCommand, expected)
        }
    }
}

extension ChatStateTests {
    func testMoveFromResyncSnapshotWaitsForReadyDestination() {
        var rows = Rows()
        let notice = row("notice", "sessionChanged", #"{"reason":"resume","from":{"id":"A"},"to":{"id":"B"}}"#)
        rows.replace(Snapshot(revision: 5, items: [notice])); XCTAssertNil(rows.notices.move)
        rows.apply(6, [.upsert(row("view", "instanceId", #""pi""#)), .upsert(row("view", "phase", #""ready""#)), .upsert(row("view", "session", #"{"id":"C"}"#))], snapshot: { fatalError() })
        XCTAssertNil(rows.notices.move)
        rows.apply(7, [.upsert(row("view", "session", #"{"id":"B"}"#))], snapshot: { fatalError() })
        XCTAssertEqual(rows.notices.move?.instance, "pi"); XCTAssertEqual(rows.notices.move?.local, false)
    }
}
