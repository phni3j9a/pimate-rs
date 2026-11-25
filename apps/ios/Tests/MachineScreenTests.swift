import XCTest
@testable import PiMate

final class MachineScreenTests: XCTestCase {
    func testSameNamedDestinationsIncludeTheExactAddress() {
        let a = Host(id: "a", name: "Mac", address: "127.0.0.1:1234", deviceId: "1", needsPairing: false)
        let b = Host(id: "b", name: "Mac", address: "127.0.0.1:2345", deviceId: "2", needsPairing: false)
        XCTAssertEqual(a.displayName(in: [a]), "Mac")
        XCTAssertEqual(a.displayName(in: [a, b]), "Mac · 127.0.0.1:1234")
        XCTAssertEqual(b.displayName(in: [a, b]), "Mac · 127.0.0.1:2345")
    }
}
