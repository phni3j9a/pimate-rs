import XCTest

final class ProbeTests: XCTestCase {
    func testJourney() throws {
        let app = XCUIApplication(), variant = ProcessInfo.processInfo.environment["PIMATE_VARIANT"] ?? "light"
        app.launchEnvironment["PIMATE_TEST_STORE"] = "1"
        app.launchArguments = ["-AppleLanguages", "(ja)", "-AppleLocale", "ja_JP"]
        app.launch()
        func shot(_ name: String) { let a = XCTAttachment(screenshot: XCUIScreen.main.screenshot()); a.name = "\(name)-\(variant)"; a.lifetime = .keepAlways; add(a) }
        let paste = app.buttons["リンクを貼り付ける"]
        XCTAssertTrue(paste.waitForExistence(timeout: 10)); shot("Pair"); paste.tap()
        guard ProcessInfo.processInfo.environment["PIMATE_REQUIRE_DEMO"] == "1" else { throw XCTSkip("Run apps/ios/test-disposable.sh to include the loopback Host") }
        let insert = app.descendants(matching: .any)["paste"].firstMatch
        XCTAssertTrue(insert.waitForExistence(timeout: 5)); insert.tap()
        let connect = app.buttons["接続"]; XCTAssertTrue(connect.isEnabled); connect.tap()
        let waiting = app.buttons["pi-waiting"], main = app.buttons["pi-main"]
        XCTAssertTrue(waiting.waitForExistence(timeout: 15)); shot("Drawer")
        main.tap(); XCTAssertTrue(app.textViews["message"].waitForExistence(timeout: 12)); XCTAssertTrue(app.staticTexts["bash"].waitForExistence(timeout: 8)); shot("Main")
        app.navigationBars.buttons.element(boundBy: 0).tap()
        XCTAssertTrue(waiting.waitForExistence(timeout: 5)); waiting.tap()
        let answer = app.buttons["回答する"]; XCTAssertTrue(answer.waitForExistence(timeout: 10)); shot("Chat-Waiting"); XCTAssertFalse(answer.isEnabled)
        app.buttons.containing(NSPredicate(format: "label CONTAINS %@", "既存の認証 API を拡張")).firstMatch.tap(); XCTAssertTrue(answer.isEnabled); answer.tap()
        XCTAssertTrue(app.textViews["message"].waitForExistence(timeout: 8))
        if variant == "light" {
            app.navigationBars.buttons.element(boundBy: 0).tap(); app.buttons["pi-demo"].tap()
            let table = app.buttons["表を開く"].firstMatch
            XCTAssertTrue(table.waitForExistence(timeout: 15)); table.tap()
            let copy = app.buttons["表をコピー"]; XCTAssertTrue(copy.waitForExistence(timeout: 5)); copy.tap()
            app.buttons["閉じる"].tap()
            let field = app.textViews["message"]; field.tap(); field.typeText("test")
            app.buttons["送信"].tap()
            XCTAssertTrue(app.buttons["はい"].waitForExistence(timeout: 30))
        }
    }
}
