import XCTest

// Supply a fresh loopback android-demo.py fixture on this disposable simulator's pasteboard.
final class AgentReviewJourney: XCTestCase {
    func testChatPages() throws {
        let env = ProcessInfo.processInfo.environment
        let real = env["PIMATE_CHAT_FIXTURE"] == "real"
        guard real || env["PIMATE_CHAT_FIXTURE"] == "1" else { throw XCTSkip("Requires a dedicated chat fixture") }
        let app = XCUIApplication(); app.launchEnvironment["PIMATE_TEST_STORE"] = "1"
        app.launchArguments = ["-AppleLanguages", "(ja)", "-AppleLocale", "ja_JP"]; app.launch()
        func tap(_ el: XCUIElement, file: StaticString = #filePath, line: UInt = #line) throws {
            guard el.waitForExistence(timeout: 15) else { XCTFail("Expected control was not found", file: file, line: line); throw NSError(domain: "Journey", code: 1) }
            for _ in 0..<4 where !el.isHittable { app.swipeUp() }; XCTAssertTrue(el.isHittable, file: file, line: line); el.tap()
        }
        func selectPi(_ id: String) throws {
            let row = app.buttons["pi-" + id]
            XCTAssertTrue(app.navigationBars["Pi 一覧"].waitForExistence(timeout: 15))
            for _ in 0..<6 {
                if row.exists, row.isHittable, row.frame.midY > 145, row.frame.midY < app.frame.maxY - 85 { break }
                app.swipeUp()
            }
            try tap(row)
        }
        func shot(_ name: String) {
            let a = XCTAttachment(screenshot: XCUIScreen.main.screenshot()); a.name = name + "-" + (env["PIMATE_VARIANT"] ?? "light"); a.lifetime = .keepAlways; add(a)
        }
        try tap(app.buttons["リンクを貼り付ける"]); try tap(app.descendants(matching: .any)["paste"].firstMatch)
        let preview = app.buttons["接続先を確認"]; XCTAssertEqual(XCTWaiter.wait(for: [XCTNSPredicateExpectation(predicate: NSPredicate(format: "enabled == true"), object: preview)], timeout: 10), .completed)
        try tap(preview); try tap(app.buttons["このマシンに接続"]); try selectPi(real ? "real-chat" : "main")
        try tap(app.buttons["変更をレビュー"]); XCTAssertTrue(app.buttons["この変更について依頼"].waitForExistence(timeout: 15)); if !real { shot("Review") }
        try tap(app.buttons["セッション全体"]); try tap(app.buttons["この変更について依頼"])
        XCTAssertTrue((app.textViews["message"].value as? String)?.contains("セッションの変更") == true)
        if real { XCTAssertTrue((app.textViews["message"].value as? String)?.contains("diff.txt") == true); return }
        app.navigationBars.buttons.element(boundBy: 0).tap(); try selectPi("api")
        let summary = app.buttons["agents-summary"]; XCTAssertTrue(summary.waitForExistence(timeout: 15)); shot("Chat-Subagents"); try tap(summary)
        let active = app.buttons["agent-run-0"]; XCTAssertTrue(active.waitForExistence(timeout: 12)); shot("Sheet-Subagents"); try tap(active)
        let instruction = app.textFields["サブエージェントへの指示"]
        XCTAssertTrue(instruction.waitForExistence(timeout: 12)); XCTAssertTrue(app.staticTexts["認証の期限切れ判定と、エラー時の処理を調べてください。"].waitForExistence(timeout: 12)); shot("Subagent-Detail")
        try tap(app.buttons["この子を停止…"])
        XCTAssertTrue(app.buttons["この子を停止"].waitForExistence(timeout: 10))
        if app.buttons["キャンセル"].isHittable { app.buttons["キャンセル"].tap() }
        else { app.coordinate(withNormalizedOffset: CGVector(dx: 0.2, dy: 0.17)).tap() } // iOS 26 popover dismisses on an outside tap.
        try tap(instruction); instruction.typeText("review tests"); try tap(app.buttons["割り込む"])
        XCTAssertTrue(app.staticTexts["あなた · 割り込む"].waitForExistence(timeout: 15))
        try tap(app.buttons["この子を停止…"]); try tap(app.buttons["この子を停止"])
        XCTAssertTrue(app.staticTexts.containing(NSPredicate(format: "label CONTAINS %@", "閲覧のみ")).firstMatch.waitForExistence(timeout: 12))
        app.navigationBars.buttons.element(boundBy: 0).tap(); try tap(app.buttons["agent-scout-0"])
        XCTAssertTrue(app.staticTexts.containing(NSPredicate(format: "label CONTAINS %@", "閲覧のみ")).firstMatch.waitForExistence(timeout: 12)); XCTAssertFalse(instruction.exists); shot("Subagent-Detail-Readonly")
    }
}
