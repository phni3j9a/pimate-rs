import XCTest

// The expected port comes from the fixture launcher, not the clipboard or pairing link.
func confirmFixtureAddress(_ app: XCUIApplication) throws {
    let port = try XCTUnwrap(ProcessInfo.processInfo.environment["PIMATE_FIXTURE_PORT"].flatMap(UInt16.init), "Missing disposable fixture port")
    guard port > 0, port != 47800, app.staticTexts["pair-address"].label == "127.0.0.1:\(port)" else {
        throw NSError(domain: "PiMateUITests", code: 1, userInfo: [NSLocalizedDescriptionKey: "Refusing to pair: preview does not match the disposable loopback Host."])
    }
}

// A sheet can expose navigation bars from both the sheet and the chat underneath it.
// Tap the native back button in the named route, then verify that it actually popped.
func popSheetRoute(_ app: XCUIApplication, from title: String, to parent: String, file: StaticString = #filePath, line: UInt = #line) throws {
    let bar = app.navigationBars[title], back = bar.buttons.element(boundBy: 0)
    let hittable = XCTNSPredicateExpectation(predicate: NSPredicate(format: "exists == true AND hittable == true"), object: back)
    guard XCTWaiter.wait(for: [hittable], timeout: 12) == .completed else {
        XCTFail("No hittable back button in \(title)", file: file, line: line)
        throw NSError(domain: "PiMateUITests.Navigation", code: 1)
    }
    back.tap()
    XCTAssertTrue(app.navigationBars[parent].waitForExistence(timeout: 12), "Expected \(parent) after leaving \(title)", file: file, line: line)
    XCTAssertFalse(bar.exists, "Still on \(title)", file: file, line: line)
}

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
        try confirmFixtureAddress(app)
        app.buttons["キャンセル"].tap(); XCTAssertTrue(insert.exists); XCTAssertFalse(app.buttons["pi-waiting"].exists)
        insert.tap(); preview.tap(); try confirmFixtureAddress(app); app.buttons["このマシンに接続"].tap()
        XCTAssertTrue(app.buttons["一覧を開く"].waitForExistence(timeout: 15)); app.buttons["一覧を開く"].tap()
        let waiting = app.buttons["pi-waiting"], main = app.buttons["pi-main"]
        XCTAssertTrue(waiting.waitForExistence(timeout: 15)); XCTAssertTrue(waiting.label.contains("デモ PC")); shot("Drawer")
        main.tap(); XCTAssertTrue(app.textViews["message"].waitForExistence(timeout: 12)); XCTAssertTrue(app.staticTexts["bash"].waitForExistence(timeout: 8)); shot("Main"); XCTAssertTrue(app.buttons["画像を添付"].exists)
        if variant == "light" {
            let read = app.staticTexts["read"].firstMatch
            XCTAssertTrue(read.isHittable); read.tap()
            XCTAssertTrue(app.buttons["最新へ"].waitForExistence(timeout: 5)); XCTAssertTrue(read.isHittable)
            read.tap(); if app.buttons["最新へ"].isHittable { app.buttons["最新へ"].tap() }
        }
        func openSheet(_ name: String) {
            app.buttons["chat-options"].tap(); app.buttons[name].tap()
            XCTAssertTrue(app.buttons["sheet-close"].waitForExistence(timeout: 8))
        }
        openSheet("モデル・Thinking")
        XCTAssertTrue(app.buttons.matching(NSPredicate(format: "label CONTAINS %@", "をお気に入り")).firstMatch.waitForExistence(timeout: 12)); shot("Sheet-Model")
        app.buttons["sheet-close"].tap()
        openSheet("使用状況")
        XCTAssertTrue(app.buttons["コンテキストを圧縮"].waitForExistence(timeout: 8)); shot("Sheet-Usage")
        app.buttons["sheet-close"].tap()
        openSheet("セッション")
        XCTAssertTrue(app.buttons["新しいセッション"].waitForExistence(timeout: 8)); shot("Sheet-Session")
        let tree = app.buttons["session-tree"]
        for _ in 0..<4 where !tree.isHittable { app.swipeUp() }
        XCTAssertTrue(tree.isHittable); tree.tap()
        let branch = app.buttons["branch-side-user"]
        XCTAssertTrue(branch.waitForExistence(timeout: 12)); shot("Sheet-Tree"); branch.tap()
        XCTAssertTrue(app.textFields["ラベル"].waitForExistence(timeout: 8)); shot("Sheet-Tree-Node")
        for _ in 0..<4 where !app.buttons["ここへ移動"].isHittable { app.swipeUp() }
        XCTAssertTrue(app.buttons["ここへ移動"].waitForExistence(timeout: 8)); XCTAssertFalse(app.buttons["ここへ移動"].isEnabled)
        app.navigationBars["移動先を確認"].buttons.element(boundBy: 0).tap()
        app.navigationBars["セッションツリー"].buttons.element(boundBy: 0).tap()
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
        // Image fetch + decode on shared CI runners has exceeded 10 s; give image waits a longer budget.
        XCTAssertTrue(app.images["画像 5 / 5"].waitForExistence(timeout: 30)); shot("Viewer")
        app.buttons["画像を閉じる"].tap()
        let file = app.buttons["修正後の画面"]
        for _ in 0..<6 where !file.isHittable { scroll.swipeDown() }
        XCTAssertTrue(file.isHittable); file.tap()
        // Distinguish presenting the viewer from receiving and decoding the full PC file.
        XCTAssertTrue(app.navigationBars["修正後の画面"].waitForExistence(timeout: 12))
        XCTAssertTrue(app.images["修正後の画面"].waitForExistence(timeout: 30)); app.buttons["画像を閉じる"].tap()

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
        XCTAssertTrue(app.images["確認結果"].waitForExistence(timeout: 30)); XCTAssertTrue(app.staticTexts["3 / 4"].exists)
        app.buttons["次の画像"].tap()
        XCTAssertTrue(app.staticTexts["画像を表示できません"].waitForExistence(timeout: 30))
        app.swipeRight(); XCTAssertTrue(app.images["確認結果"].waitForExistence(timeout: 30))
        app.buttons["前の画像"].tap(); XCTAssertTrue(app.images["修正後の画面"].waitForExistence(timeout: 30))
        app.buttons["前の画像"].tap(); XCTAssertTrue(app.images["修正前の画面"].waitForExistence(timeout: 30))
        app.buttons["画像を閉じる"].tap(); XCTAssertTrue(app.buttons["表をコピー"].waitForExistence(timeout: 8)); shot("Table")
        app.buttons["表を閉じる"].tap()
        field.tap(); field.typeText("/fix")
        let fix = app.buttons["command-fix-tests"]
        XCTAssertTrue(fix.waitForExistence(timeout: 8)); shot("Commands"); fix.tap()
        XCTAssertEqual(field.value as? String, "/fix-tests ")
        if variant == "light" {
            openSheet("使用状況"); app.buttons["コンテキストを圧縮"].tap()
            XCTAssertTrue(app.staticTexts["操作が完了しました"].waitForExistence(timeout: 12))
            app.buttons["sheet-close"].tap()
        }
        openPi("edge")
        XCTAssertTrue(field.waitForExistence(timeout: 12)); field.tap(); field.typeText("draft")
        let confirm = app.buttons["新しい送信先を確認"]
        XCTAssertTrue(confirm.waitForExistence(timeout: 18)); shot("Chat-Edge"); field.tap(); field.typeText("next")
        XCTAssertFalse(app.buttons["送信"].isEnabled); confirm.tap(); XCTAssertTrue(app.buttons["送信"].isEnabled)
        openPi("demo")
        XCTAssertTrue(app.buttons.matching(NSPredicate(format: "label == %@", "表を開く")).firstMatch.waitForExistence(timeout: 20))
        XCTAssertTrue(app.staticTexts["メッセージを送信"].waitForExistence(timeout: 10))
        XCTAssertFalse(app.staticTexts.containing(NSPredicate(format: "label CONTAINS %@", "210 out")).firstMatch.exists)
        openVisibleTable()
        let copy = app.buttons["表をコピー"]
        guard copy.waitForExistence(timeout: 8) else { XCTFail("Table viewer did not open"); return }; copy.tap()
        app.buttons["表を閉じる"].tap()
        do {
            field.tap(); field.typeText("test")
            let jump = app.buttons["最新へ"], transcript = app.scrollViews["transcript"]
            let start = transcript.coordinate(withNormalizedOffset: CGVector(dx: 0.15, dy: 0.12))
            for _ in 0..<4 where !jump.isHittable { start.press(forDuration: 0.1, thenDragTo: start.withOffset(CGVector(dx: 0, dy: 165))) }
            XCTAssertTrue(jump.isHittable); app.buttons["送信"].tap()
            XCTAssertTrue(app.staticTexts["上の質問や PC の操作を完了すると入力できます"].waitForExistence(timeout: 30))
            XCTAssertTrue(jump.isHittable); shot("ReadingHistory"); jump.tap()
            XCTAssertTrue(app.buttons["はい"].waitForExistence(timeout: 8))
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
        app.buttons["添付画像 2 を開く"].tap()
        let draftPhoto = app.images["添付画像 2 / 4"]
        XCTAssertTrue(draftPhoto.waitForExistence(timeout: 12)); draftPhoto.doubleTap(); shot("DraftZoom")
        draftPhoto.swipeLeft(velocity: .slow); XCTAssertTrue(draftPhoto.isHittable)
        draftPhoto.doubleTap(); draftPhoto.swipeDown()
        XCTAssertTrue(fourth.exists)
        app.buttons["画像 2 を取り消す"].tap(); XCTAssertFalse(fourth.exists); XCTAssertTrue(app.buttons["画像 3 を取り消す"].exists)
        XCTAssertTrue(app.buttons["画像を添付"].isEnabled)
    }
}

extension ProbeTests {
    func testSheetOperations() throws {
        guard ProcessInfo.processInfo.environment["PIMATE_REQUIRE_DEMO"] == "1" else { throw XCTSkip("Requires the sheet-review fixture") }
        continueAfterFailure = false
        let app = XCUIApplication()
        app.launchEnvironment["PIMATE_TEST_STORE"] = "1"; app.launchArguments = ["-AppleLanguages", "(ja)", "-AppleLocale", "ja_JP"]; app.launch()
        app.buttons["リンクを貼り付ける"].tap(); app.descendants(matching: .any)["paste"].firstMatch.tap()
        let preview = app.buttons["接続先を確認"]
        XCTAssertEqual(XCTWaiter.wait(for: [XCTNSPredicateExpectation(predicate: NSPredicate(format: "enabled == true"), object: preview)], timeout: 8), .completed)
        preview.tap(); try confirmFixtureAddress(app); app.buttons["このマシンに接続"].tap()
        XCTAssertTrue(app.buttons["一覧を開く"].waitForExistence(timeout: 15)); app.buttons["一覧を開く"].tap()
        let main = app.buttons["pi-main"]
        XCTAssertTrue(main.waitForExistence(timeout: 15)); main.tap()
        func open(_ title: String) { app.buttons["chat-options"].tap(); app.buttons[title].tap() }
        open("モデル・Thinking")
        app.buttons["すべて"].tap()
        let search = app.textFields["モデルを検索"]
        search.tap(); search.typeText("absent\n"); app.buttons["お気に入り"].tap()
        XCTAssertTrue(search.exists); XCTAssertEqual(search.value as? String, "absent")
        search.tap(); search.typeText(String(repeating: XCUIKeyboardKey.delete.rawValue, count: 6))
        XCTAssertTrue(app.buttons.matching(NSPredicate(format: "label CONTAINS %@", "をお気に入りから外す")).firstMatch.waitForExistence(timeout: 8))
        app.buttons["sheet-close"].tap(); open("セッション")
        XCTAssertFalse(app.buttons["新しいセッション"].isEnabled)
        app.buttons["過去のセッションを開く"].tap()
        let resume = app.buttons["resume-main-saved-0"]
        XCTAssertTrue(resume.waitForExistence(timeout: 8)); XCTAssertFalse(resume.isEnabled)
        app.navigationBars["過去のセッション"].buttons.element(boundBy: 0).tap()
        app.buttons["ここから分岐"].tap()
        let fork = app.buttons.matching(NSPredicate(format: "identifier BEGINSWITH %@", "fork-")).firstMatch
        XCTAssertTrue(fork.waitForExistence(timeout: 8)); XCTAssertFalse(fork.isEnabled)
        app.navigationBars["分岐元を選択"].buttons.element(boundBy: 0).tap()
        app.buttons["コンテキストを圧縮"].tap()
        XCTAssertTrue(app.staticTexts["操作が完了しました"].waitForExistence(timeout: 12)); app.buttons["session-tree"].tap()
        XCTAssertTrue(app.staticTexts["sheet-error"].waitForExistence(timeout: 8)); app.buttons["更新"].tap()
        let node = app.buttons["branch-side-user"]
        XCTAssertTrue(node.waitForExistence(timeout: 8)); node.tap()
        let field = app.textFields["ラベル"]
        field.tap(); field.typeText("review41\n"); app.buttons["ラベルを保存"].tap()
        func waitForLabel(_ present: Bool) {
            let predicate = NSPredicate(format: present ? "exists == true AND label CONTAINS %@" : "exists == true AND NOT (label CONTAINS %@)", "review41")
            XCTAssertEqual(XCTWaiter.wait(for: [XCTNSPredicateExpectation(predicate: predicate, object: node)], timeout: 12), .completed)
        }
        waitForLabel(true); node.tap(); XCTAssertEqual(field.value as? String, "review41"); app.buttons["ラベルを外す"].tap(); waitForLabel(false)
    }
}

extension ProbeTests {
    // Run against a fresh loopback android-screen-fixture (never a user's registration).
    func testMachineScreens() throws {
        let env = ProcessInfo.processInfo.environment
        guard let mode = env["PIMATE_SCREEN_FIXTURE"], ["list", "empty", "faux"].contains(mode) else { throw XCTSkip("Requires a disposable management Host") }
        continueAfterFailure = false
        let app = XCUIApplication(), variant = env["PIMATE_VARIANT"] ?? "light"
        app.launchEnvironment["PIMATE_TEST_STORE"] = "1"
        app.launchArguments = ["-AppleLanguages", "(ja)", "-AppleLocale", "ja_JP"]; app.launch()
        func shot(_ name: String) {
            let image = XCTAttachment(screenshot: XCUIScreen.main.screenshot()); image.name = "\(name)-\(variant)"; image.lifetime = .keepAlways; add(image)
        }
        func button(_ label: String) -> XCUIElement { app.buttons.matching(NSPredicate(format: "label CONTAINS %@", label)).firstMatch }
        func tap(_ element: XCUIElement) {
            for _ in 0..<6 where !element.isHittable { app.swipeUp() }
            XCTAssertTrue(element.isHittable); element.tap()
        }
        if mode == "list" {
            tap(button("スマホと PC を Tailscale")); XCTAssertTrue(app.buttons["次へ：PC に PiMate を入れる"].waitForExistence(timeout: 6)); shot("Setup-Tailscale")
            app.buttons["次へ：PC に PiMate を入れる"].tap(); XCTAssertTrue(app.buttons["この手順を PC に送る"].waitForExistence(timeout: 8)); shot("Setup-PC")
            try popSheetRoute(app, from: "PC に PiMate を入れる", to: "Tailscale でつなぐ"); app.buttons["閉じる"].tap()
        }
        tap(app.buttons["リンクを貼り付ける"]); app.descendants(matching: .any)["paste"].firstMatch.tap(); app.buttons["接続先を確認"].tap()
        XCTAssertEqual(app.staticTexts["pair-name"].label, mode == "faux" ? "検証用 PC" : "デモ PC")
        try confirmFixtureAddress(app); app.buttons["このマシンに接続"].tap()
        if mode == "empty" {
            XCTAssertTrue(app.staticTexts.containing(NSPredicate(format: "label BEGINSWITH %@", "まだ起動中")).firstMatch.waitForExistence(timeout: 20)); shot("First-Step-Empty")
            app.buttons["新しい Pi を起動"].tap(); XCTAssertTrue(app.textFields["フォルダのパス"].waitForExistence(timeout: 8)); return
        }
        XCTAssertTrue(app.staticTexts["選ぶとチャットを開きます。"].waitForExistence(timeout: 20))
        if mode == "faux" {
            let work = button("work"); tap(work)
            let field = app.textViews["message"]; XCTAssertTrue(field.waitForExistence(timeout: 15)); field.tap(); field.typeText("say hello"); app.buttons["送信"].tap()
            XCTAssertTrue(app.descendants(matching: .any).containing(NSPredicate(format: "label CONTAINS %@ OR value CONTAINS %@", "Hello from the PiMate faux model.", "Hello from the PiMate faux model.")).firstMatch.waitForExistence(timeout: 45))
            app.navigationBars.buttons.element(boundBy: 0).tap(); app.buttons["新しい Pi"].tap(); tap(app.buttons["起動"])
            XCTAssertTrue(field.waitForExistence(timeout: 30)); return
        }
        shot("First-Step"); app.buttons["一覧を開く"].tap(); XCTAssertTrue(app.buttons["新しい Pi"].waitForExistence(timeout: 8)); shot("Drawer"); app.buttons["設定"].tap()
        let machine = app.buttons.matching(NSPredicate(format: "identifier BEGINSWITH 'machine-'")).firstMatch
        XCTAssertTrue(machine.waitForExistence(timeout: 8)); shot("Settings"); machine.tap()
        let update = app.buttons["実行時の最新版に更新…"]
        XCTAssertTrue(update.waitForExistence(timeout: 12)); shot("Machine-Detail"); tap(update)
        XCTAssertTrue(app.alerts.staticTexts.containing(NSPredicate(format: "label CONTAINS %@", "実行時の最新版に更新します")).firstMatch.exists); app.buttons["キャンセル"].tap()
        tap(app.buttons["このマシンを削除…"])
        XCTAssertTrue(app.alerts.staticTexts.containing(NSPredicate(format: "label CONTAINS %@", "PC 側の登録も取り消します")).firstMatch.exists); shot("Machine-Remove"); app.buttons["キャンセル"].tap()
        for _ in 0..<4 where !app.buttons["プロバイダーのログイン"].isHittable { app.swipeDown() }
        tap(app.buttons["プロバイダーのログイン"]); XCTAssertTrue(app.buttons["API キーを設定"].waitForExistence(timeout: 15)); shot("Provider")
        // Synthetic input goes only to the loopback fixture. Never attach a prompt, code, link or QR.
        if variant == "light" {
            app.buttons["API キーを設定"].tap(); let key = app.secureTextFields["API キー"]
            XCTAssertTrue(key.waitForExistence(timeout: 12)); key.tap(); key.typeText("fixture-only"); tap(app.buttons["送信"])
            XCTAssertTrue(app.staticTexts["ログインしました。起動中の Pi には /reload または再起動後に反映されます。"].waitForExistence(timeout: 12)); tap(app.buttons["ログアウト…"])
            app.alerts.buttons["キャンセル"].tap(); tap(app.buttons["ログアウト…"]); app.alerts.buttons["ログアウト"].tap()
            XCTAssertTrue(app.staticTexts["未設定"].waitForExistence(timeout: 12))
        }
        try popSheetRoute(app, from: "プロバイダーのログイン", to: "デモ PC"); try popSheetRoute(app, from: "デモ PC", to: "設定"); app.buttons["完了"].tap()
        app.buttons["新しい Pi"].tap(); let recent = app.buttons["/home/demo/code/demo"]; XCTAssertTrue(recent.waitForExistence(timeout: 12)); shot("New-Pi"); tap(recent)
        let path = app.textFields["フォルダのパス"]
        XCTAssertEqual(path.value as? String, "/home/demo/code/demo")
        path.tap(); path.typeText("/s"); let suggestion = app.buttons["src/"]
        XCTAssertTrue(suggestion.waitForExistence(timeout: 8)); tap(suggestion)
        XCTAssertEqual(path.value as? String, "/home/demo/code/demo/src/")
        tap(app.buttons["spawn-model"])
        XCTAssertTrue(button("Claude Opus 5.5 · medium").waitForExistence(timeout: 10)); shot("Spawn-Models")
        app.buttons["すべて"].tap(); let search = app.textFields["モデルを検索"]; search.tap(); search.typeText("gemini")
        XCTAssertTrue(button("google/gemini-3-pro").exists); XCTAssertFalse(button("anthropic/claude-opus").exists)
        try popSheetRoute(app, from: "モデル", to: "新しい Pi"); app.buttons["閉じる"].tap(); tap(app.buttons["pi-main"])
        XCTAssertTrue(app.buttons["GitHub"].waitForExistence(timeout: 12)); app.buttons["GitHub"].tap()
        let issue = button("Android の残りの画面"); XCTAssertTrue(issue.waitForExistence(timeout: 12)); shot("GitHub"); issue.tap()
        XCTAssertTrue(app.buttons["Pi に依頼"].waitForExistence(timeout: 12)); shot("GitHub-Issue"); try popSheetRoute(app, from: "Issue #20", to: "GitHub"); app.buttons["PR"].tap()
        if env["PIMATE_DEMO_GITHUB_TABLES"] == "1" {
            for (title, note, route) in [("チェック未提供", "CI チェックの内訳は未提供です", "PR #56"), ("空のチェック一覧", "チェックはありません", "PR #57")] {
                tap(button(title)); XCTAssertTrue(app.staticTexts[note].waitForExistence(timeout: 10))
                tap(app.buttons["表を開く"]); XCTAssertTrue(app.buttons["表をコピー"].waitForExistence(timeout: 8))
                // Untrusted GitHub images stay text, not tiles that fetch from the web or the PC.
                XCTAssertTrue(app.staticTexts["外部の画像"].exists); XCTAssertFalse(app.buttons["外部の画像"].exists); XCTAssertFalse(app.buttons["PC の画像"].exists)
                app.buttons["表を閉じる"].tap(); try popSheetRoute(app, from: route, to: "GitHub")
            }
        }
        tap(button("画面の実装"))
        XCTAssertTrue(app.staticTexts.containing(NSPredicate(format: "label CONTAINS %@", "CI チェックの一部を省略")).firstMatch.waitForExistence(timeout: 12)); shot("GitHub-PR"); tap(app.buttons["Pi に依頼"])
        XCTAssertTrue((app.textViews["message"].value as? String)?.contains("#20「画面の実装」") == true)
    }
}
