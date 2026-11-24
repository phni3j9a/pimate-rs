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
        let preview = app.buttons["接続先を確認"]; XCTAssertEqual(XCTWaiter.wait(for: [XCTNSPredicateExpectation(predicate: NSPredicate(format: "enabled == true"), object: preview)], timeout: 8), .completed); preview.tap()
        XCTAssertEqual(app.staticTexts["pair-name"].label, "デモ PC")
        XCTAssertTrue(app.staticTexts["pair-address"].label.hasPrefix("127.0.0.1:"))
        app.buttons["キャンセル"].tap(); XCTAssertTrue(insert.exists); XCTAssertFalse(app.buttons["pi-waiting"].exists)
        insert.tap(); preview.tap(); app.buttons["このマシンに接続"].tap()
        let waiting = app.buttons["pi-waiting"], main = app.buttons["pi-main"]
        XCTAssertTrue(waiting.waitForExistence(timeout: 15)); XCTAssertTrue(waiting.label.contains("デモ PC")); shot("Drawer")
        main.tap(); XCTAssertTrue(app.textViews["message"].waitForExistence(timeout: 12)); XCTAssertTrue(app.staticTexts["bash"].waitForExistence(timeout: 8)); shot("Main"); XCTAssertTrue(app.buttons["画像を添付"].exists)
        func openSheet(_ name: String) {
            app.buttons["会話の操作"].tap(); app.buttons[name].tap()
            XCTAssertTrue(app.buttons["sheet-close"].waitForExistence(timeout: 8))
        }
        openSheet("モデル・Thinking")
        XCTAssertTrue(app.buttons.matching(NSPredicate(format: "label CONTAINS %@", "をお気に入り")).firstMatch.waitForExistence(timeout: 12)); shot("Sheet-Model")
        app.buttons["sheet-close"].tap()
        app.navigationBars.buttons.element(boundBy: 0).tap()
        XCTAssertTrue(waiting.waitForExistence(timeout: 5)); waiting.tap()
        let answer = app.buttons["回答する"]; XCTAssertTrue(answer.waitForExistence(timeout: 10)); shot("Chat-Waiting"); XCTAssertFalse(answer.isEnabled)
        app.buttons.containing(NSPredicate(format: "label CONTAINS %@", "既存の認証 API を拡張")).firstMatch.tap(); XCTAssertTrue(answer.isEnabled)
        // Under XXXL the footer can cover the center XCTest would tap. Scroll the whole card first.
        let scroll = app.scrollViews.firstMatch
        for _ in 0..<3 where !answer.isHittable || answer.frame.maxY > app.frame.maxY - 110 { scroll.swipeUp() }
        XCTAssertTrue(answer.isHittable); answer.tap()
        XCTAssertTrue(app.textViews["message"].waitForExistence(timeout: 8))
        func openPi(_ id: String) {
            app.navigationBars.buttons.element(boundBy: 0).tap()
            let row = app.buttons["pi-" + id]
            // A partially visible row is hittable even when its center lands under the tab bar.
            func visible() -> Bool { row.isHittable && row.frame.midY > 140 && row.frame.midY < app.frame.maxY - 50 }
            for _ in 0..<6 where !visible() { app.swipeDown() }
            for _ in 0..<8 where !visible() { app.swipeUp() }
            XCTAssertTrue(row.isHittable); row.tap()
            if row.waitForExistence(timeout: 1), row.isHittable { row.tap() }
        }
        openPi("attach")
        let terminal = app.buttons["コマンドをコピー"]
        XCTAssertTrue(terminal.waitForExistence(timeout: 10)); shot("Chat-Attach"); terminal.tap()
        XCTAssertFalse(app.textViews["message"].exists)

        openPi("images")
        let tile = app.buttons["画像 4 / 5"]
        XCTAssertTrue(tile.waitForExistence(timeout: 10))
        for _ in 0..<5 where !tile.isHittable { scroll.swipeUp() }
        shot("Images"); tile.tap()
        let next = app.buttons["次の画像"]
        XCTAssertTrue(next.waitForExistence(timeout: 8)); next.tap()
        XCTAssertTrue(app.images["画像 5 / 5"].waitForExistence(timeout: 8)); shot("Viewer")
        app.buttons["画像を閉じる"].tap()
        let file = app.buttons["修正後の画面"]
        for _ in 0..<6 where !file.isHittable { scroll.swipeDown() }
        XCTAssertTrue(file.isHittable); file.tap()
        XCTAssertTrue(app.images["修正後の画面"].waitForExistence(timeout: 25)); app.buttons["画像を閉じる"].tap()

        let field = app.textViews["message"]
        func openVisibleTable() {
            let tables = app.buttons.matching(NSPredicate(format: "label == %@", "表を開く"))
            // Hittable includes partially obscured buttons. Keep the entire label between the header and the composer.
            let top = app.navigationBars.firstMatch.frame.maxY + 44, bottom = field.frame.minY - 20
            func visibleTable() -> XCUIElement? {
                tables.allElementsBoundByIndex.first { $0.isHittable && $0.frame.minY > top && $0.frame.maxY < bottom }
            }
            for _ in 0..<20 {
                if visibleTable() != nil { break }
                let frames = tables.allElementsBoundByIndex.map(\.frame).filter { !$0.isEmpty }
                let dy: CGFloat = frames.first.map { $0.minY >= bottom - $0.height ? -180 : 180 } ?? 180
                let origin = app.coordinate(withNormalizedOffset: .zero)
                let start = origin.withOffset(CGVector(dx: 35, dy: (top + bottom - dy) / 2))
                start.press(forDuration: 0.15, thenDragTo: start.withOffset(CGVector(dx: 0, dy: dy)))
            }
            guard let table = visibleTable() else { XCTFail("No table button fully inside the transcript"); return }; table.tap()
        }
        openVisibleTable()
        let cell = app.buttons["確認結果"]
        XCTAssertTrue(cell.waitForExistence(timeout: 8)); cell.tap()
        XCTAssertTrue(app.images["確認結果"].waitForExistence(timeout: 10)); XCTAssertTrue(app.staticTexts["3 / 4"].exists)
        app.buttons["前の画像"].tap(); XCTAssertTrue(app.images["修正後の画面"].waitForExistence(timeout: 8))
        app.buttons["前の画像"].tap(); XCTAssertTrue(app.images["修正前の画面"].waitForExistence(timeout: 8))
        app.buttons["画像を閉じる"].tap(); XCTAssertTrue(app.buttons["表をコピー"].waitForExistence(timeout: 8)); shot("Table")
        app.buttons["表を閉じる"].tap()
        field.tap(); field.typeText("/fix")
        let fix = app.buttons["command-fix-tests"]
        XCTAssertTrue(fix.waitForExistence(timeout: 8)); shot("Commands"); fix.tap()
        XCTAssertEqual(field.value as? String, "/fix-tests ")
        openPi("edge")
        XCTAssertTrue(field.waitForExistence(timeout: 12)); field.tap(); field.typeText("draft")
        let confirm = app.buttons["新しい送信先を確認"]
        XCTAssertTrue(confirm.waitForExistence(timeout: 18)); shot("Chat-Edge"); field.tap(); field.typeText("next")
        XCTAssertFalse(app.buttons["送信"].isEnabled); confirm.tap(); XCTAssertTrue(app.buttons["送信"].isEnabled)
        openPi("demo")
        let receipt = app.staticTexts.containing(NSPredicate(format: "label CONTAINS %@", "210 out")).firstMatch
        XCTAssertTrue(receipt.waitForExistence(timeout: 20))
        openVisibleTable()
        let copy = app.buttons["表をコピー"]
        guard copy.waitForExistence(timeout: 8) else { XCTFail("Table viewer did not open"); return }; copy.tap()
        app.buttons["表を閉じる"].tap()
        if variant == "light" {
            field.tap(); field.typeText("test"); app.buttons["送信"].tap()
            XCTAssertTrue(app.buttons["はい"].waitForExistence(timeout: 30))
        }
        openPi("main")
        XCTAssertTrue(app.buttons["画像を添付"].waitForExistence(timeout: 12))
        app.buttons["画像を添付"].tap()
        let photos = app.images.matching(identifier: "PXGGridLayout-Info")
        // The out-of-process system grid may still be indexing the seeded library on CI.
        guard photos.element(boundBy: 3).waitForExistence(timeout: 45) else { XCTFail("Four seeded photos did not appear in the system picker"); return }
        for i in 0..<4 { photos.element(boundBy: i).coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: 0.5)).tap() }
        let fourth = app.buttons["画像 4 を取り消す"]
        // Ordered pickers can finish automatically at the selection limit.
        if !fourth.waitForExistence(timeout: 3) {
            let add = app.buttons.matching(NSPredicate(format: "identifier == %@ OR label CONTAINS[c] %@ OR label CONTAINS %@ OR label == %@ OR label == %@", "Add", "Add", "追加", "完了", "Done")).firstMatch
            if add.waitForExistence(timeout: 4) { add.tap() }
            else if photos.firstMatch.exists {
                // Native picker's confirmation is in the trailing edge of its top bar.
                let bar = app.navigationBars.allElementsBoundByIndex.last { $0.isHittable }
                bar?.coordinate(withNormalizedOffset: CGVector(dx: 0.92, dy: 0.5)).tap()
            }
        }
        XCTAssertTrue(fourth.waitForExistence(timeout: 10)); XCTAssertFalse(app.buttons["画像を添付"].isEnabled); shot("Attachments")
        app.buttons["画像 2 を取り消す"].tap(); XCTAssertFalse(fourth.exists); XCTAssertTrue(app.buttons["画像 3 を取り消す"].exists)
        XCTAssertTrue(app.buttons["画像を添付"].isEnabled)
    }
}
