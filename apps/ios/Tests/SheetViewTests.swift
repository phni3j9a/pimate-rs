import XCTest
@testable import PiMate

final class SheetViewTests: XCTestCase {
    @MainActor func testShutdownWarnsWhenSessionFileIsNotKnown() {
        for session: Object in [[:], ["file": NSNull()], ["file": "  "]] {
            let warning = SessionSheetView.shutdownMessage(name: "試験", session: session)
            XCTAssertTrue(warning.contains("履歴が失われる可能性")); XCTAssertFalse(warning.contains("開き直せます"))
        }
        XCTAssertTrue(SessionSheetView.shutdownMessage(name: "試験", session: ["file": "/saved.jsonl"]).contains("保存済み"))
    }
    @MainActor func testTableKeepsAnIndependentTouchTargetWhenAllCellsAreInteractive() {
        for (header, expected) in [("[見出し](https://example.org)", true), ("見出し", false)] {
            let document = parse(source: "| " + header + " |\n| --- |\n| ![画像](photo.png) |", fileImages: true)
            guard case .table(let table) = document.blocks().first?.content else { return XCTFail("表がありません") }
            XCTAssertEqual(MarkdownTable.needsOpenButton(table), expected)
        }
    }
    func testFilteredTreeProjectsCurrentOntoNearestUserOnly() {
        let nodes: [Object] = [["id":"a", "kind":"user"], ["id":"b", "parentId":"a", "kind":"user"], ["id":"c", "parentId":"b", "kind":"assistant"], ["id":"d", "parentId":"c", "kind":"tool_result", "isCurrent":true], ["id":"e", "parentId":"a", "kind":"user"]]
        let all = BranchRow.read(nodes), users = BranchRow.userRows(all)
        XCTAssertEqual(users.filter { $0.detail.contains("現在位置") }.map(\.id), ["b"])
        XCTAssertEqual(all.filter { $0.detail.contains("現在位置") }.map(\.id), ["d"])
    }
    func testLinearTurnsStayAlignedAndSiblingsIndentTogether() {
        let nodes: [Object] = [["id": "a"], ["id": "b", "parentId": "a"], ["id": "c", "parentId": "b"], ["id": "d", "parentId": "c"], ["id": "e", "parentId": "b"]]
        let rows = BranchRow.read(nodes)
        XCTAssertEqual(rows.map(\.id), ["a", "b", "c", "d", "e"])
        XCTAssertEqual(rows.map(\.depth), [0, 0, 1, 1, 1])
    }
}
