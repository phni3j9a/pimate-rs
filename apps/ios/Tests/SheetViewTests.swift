import XCTest
@testable import PiMate

final class SheetViewTests: XCTestCase {
    func testLinearTurnsStayAlignedAndSiblingsIndentTogether() {
        let nodes: [Object] = [["id": "a"], ["id": "b", "parentId": "a"], ["id": "c", "parentId": "b"], ["id": "d", "parentId": "c"], ["id": "e", "parentId": "b"]]
        let rows = BranchRow.read(nodes)
        XCTAssertEqual(rows.map(\.id), ["a", "b", "c", "d", "e"])
        XCTAssertEqual(rows.map(\.depth), [0, 0, 1, 1, 1])
    }
}
