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
    func testLinearTurnsStayAlignedAndSiblingsIndentTogether() {
        let nodes: [Object] = [["id": "a"], ["id": "b", "parentId": "a"], ["id": "c", "parentId": "b"], ["id": "d", "parentId": "c"], ["id": "e", "parentId": "b"]]
        let rows = BranchRow.read(nodes)
        XCTAssertEqual(rows.map(\.id), ["a", "b", "c", "d", "e"])
        XCTAssertEqual(rows.map(\.depth), [0, 0, 1, 1, 1])
    }
}
