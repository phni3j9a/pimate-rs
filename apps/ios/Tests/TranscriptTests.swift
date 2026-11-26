import XCTest
@testable import PiMate

final class TranscriptTests: XCTestCase {
    private func item(_ id: String, _ o: Object) throws -> Item {
            Item(collection: "entry", id: id, position: 0, json: String(decoding: try JSONSerialization.data(withJSONObject: o), as: UTF8.self))
        }
    func testFirstAnswerDeltaClosesStreamingThoughtWithoutWaitingForMessageEnd() {
        let since = Item(collection: "stream", id: "thinkingSince", position: 0, json: "1000")
        let until = Item(collection: "stream", id: "thinkingUntil", position: 1, json: "13000")
        let live = WorkLine.streamThought([since], hasAnswer: false)
        XCTAssertTrue(live.automatic); XCTAssertEqual(live.title, "Thinking…")
        for hasAnswer in [true, false] { // An empty first delta also stops the core clock.
            let stopped = WorkLine.streamThought([since, until], hasAnswer: hasAnswer)
            XCTAssertFalse(stopped.automatic); XCTAssertEqual(stopped.title, "Thought for 12s")
        }
        let snapshot = WorkLine.streamThought([], hasAnswer: true)
        XCTAssertFalse(snapshot.automatic); XCTAssertEqual(snapshot.title, "Thought")
    }
    func testMeasuredThinkingLabels() {
        XCTAssertEqual(WorkLine.thoughtTitle(nil), "Thought"); XCTAssertEqual(WorkLine.thoughtTitle(999), "Thought")
        XCTAssertEqual(WorkLine.thoughtTitle(11900), "Thought for 12s"); XCTAssertEqual(WorkLine.thoughtTitle(61500), "Thought for 1m 2s")
    }
    func testCopySourceExcludesAbortStatus() throws {
        let md = "# 一部の回答\n", turn = try ChatTurn.read([item("r", ["kind": "assistant", "text": md, "errorMessage": "Request was aborted"])], live: [])
        guard case .reply(let reply) = turn.first?.lines.first else { return XCTFail("回答がありません") }
        XCTAssertEqual(reply.text, md); XCTAssertEqual(reply.notice, "中断しました")
    }
    func testAbortRequiresNormalizedReasonOrKnownWholeMessage() throws {
        let timeout = "Request was aborted because the connection timed out"
        for (stop, error, expected) in [("error", timeout, timeout), ("error", "AbortError: The user aborted a request.", "中断しました"), ("aborted", "", "中断しました")] {
            let turns = try ChatTurn.read([item("r", ["kind":"assistant", "stopReason":stop, "errorMessage":error])], live: [])
            guard case .reply(let reply) = turns.first?.lines.first else { return XCTFail("通知がありません") }
            XCTAssertEqual(reply.notice, expected)
        }
    }
    func testImageOnlyTurnSurvivesBeforeReply() throws {
        for (o, count): (Object, Int) in [(["imageCount": 4], 4), (["images": [["mimeType": "image/png"]]], 1)] {
            let result = try ChatTurn.read([item("user", o.merging(["kind": "user", "text": ""]) { a, _ in a })], live: [])
            XCTAssertEqual(result.map(\.imageCount), [count])
        }
    }
    func testRepliesAndWorkAreInterleavedInBothActiveAndCompletedHistory() throws {
        let history = try [item("a", ["kind": "assistant", "text": "先に確認", "toolCalls": [["name": "read", "id": "x"]]]), item("b", ["kind": "tool_result", "toolCallId": "x", "text": "ok"]), item("c", ["kind": "assistant", "text": "結果"])]
        XCTAssertEqual(ChatTurn.read(history, live: []).first?.lines.map(\.id), ["reply:a", "work:a:0", "reply:c"])
    }
    func testReusedToolIDAndLiveOutput() throws {
        let call: Object = ["kind": "assistant", "toolCalls": [["id": "x", "name": "bash", "args": ["command": "pwd"]]]]
        let history = try [item("1", ["kind": "user", "text": "一", "timestamp": 1000]), item("2", call), item("3", ["kind": "tool_result", "toolCallId": "x", "text": "前の結果", "timestamp": 4000]), item("4", ["kind": "user", "text": "二"]), item("5", call)]
        let live = try item("x", ["status": "running", "partial": ["content": [["text": "途中の出力"]]]])
        let turns = ChatTurn.read(history, live: [live])
        XCTAssertEqual(turns.map(\.prompt), ["一", "二"]); XCTAssertEqual(turns.first?.summary, "3秒 作業 · ツール 1 回")
        XCTAssertEqual(turns.first?.work.first?.output, "前の結果"); XCTAssertEqual(turns.last?.work.first?.output, "途中の出力"); XCTAssertEqual(turns.last?.work.first?.state, "実行中")
    }
}
