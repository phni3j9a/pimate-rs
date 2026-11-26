import XCTest
@testable import PiMate

final class TranscriptTests: XCTestCase {
    private func item(_ id: String, _ o: Object) throws -> Item {
            Item(collection: "entry", id: id, position: 0, json: String(decoding: try JSONSerialization.data(withJSONObject: o), as: UTF8.self))
        }
    func testMeasuredThinkingLabels() {
        XCTAssertEqual(WorkLine.thoughtTitle(nil), "Thought"); XCTAssertEqual(WorkLine.thoughtTitle(999), "Thought")
        XCTAssertEqual(WorkLine.thoughtTitle(11900), "Thought for 12s"); XCTAssertEqual(WorkLine.thoughtTitle(61500), "Thought for 1m 2s")
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
