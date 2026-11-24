import XCTest
final class ProbeTests: XCTestCase {
    func testEmptyProbe() {
        let app = XCUIApplication(); app.launchEnvironment["PIMATE_TEST_STORE"] = "1"; app.launch()
        XCTAssertTrue(app.staticTexts["emptyHosts"].waitForExistence(timeout: 8))
        XCTAssertTrue(app.secureTextFields["リンクを貼り付ける"].waitForExistence(timeout: 8))
        let shot = XCTAttachment(screenshot: app.screenshot()); shot.name = "接続の確認"; shot.lifetime = .keepAlways; add(shot)
    }
}
