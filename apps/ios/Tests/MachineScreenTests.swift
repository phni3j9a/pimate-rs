import SwiftUI
import XCTest
@testable import PiMate

final class MachineScreenTests: XCTestCase {
    func testGitHubFailureGuidanceDistinguishesRepositoryFromMissingTarget() {
        XCTAssertTrue(GithubText.guidance("not_github").contains("GitHub の remote があるフォルダ"))
        XCTAssertTrue(GithubText.guidance("not_found").contains("アクセス権も確認"))
    }
    func testGitHubUsesHostReviewAndCheckCounts() {
        XCTAssertEqual(GithubText.review(["reviewDecision": "CHANGES_REQUESTED"]), "変更の依頼あり")
        XCTAssertEqual(GithubText.review(["isDraft": true, "reviewDecision": "APPROVED"]), "ドラフト")
        XCTAssertEqual(GithubText.review(["reviewDecision": "APPROVED"]), "承認済み")
        XCTAssertEqual(GithubText.ci(["ci": ["passed": 1, "failed": 2, "pending": 3]]), "CI 成功 1 · 失敗 2 · 待ち 3")
        XCTAssertEqual(GithubText.ci(["ci": ["passed": 0, "failed": 0, "pending": 0]]), "CI チェックなし")
    }
    func testSpawnCatalogBelongsOnlyToTheConnectedListedHostGeneration() {
        func hostRow(_ id: String, _ json: String) -> Item { Item(collection: "host", id: id, position: 0, json: json) }
        var rows = Rows(); rows.groups["host"] = [hostRow("connection", "\"connected\"")]
        XCTAssertNil(rows.catalogGeneration)
        rows.groups["host"]?.append(hostRow("listKnown", "true"))
        XCTAssertNil(rows.catalogGeneration) // A known empty list has no Pi to answer .hostModels.
        rows.groups["instance"] = [Item(collection: "instance", id: "first", position: 0, json: "{}")]
        let old = SpawnCatalog(generation: rows.connectionGeneration, payload: ["default": "old"])
        XCTAssertEqual(old.value(for: rows.catalogGeneration)?.str("default"), "old")
        rows.apply(1, [.upsert(hostRow("connection", "\"offline\""))]) { fatalError() }
        XCTAssertNil(old.value(for: rows.catalogGeneration))
        rows.apply(2, [.upsert(hostRow("connection", "\"connected\""))]) { fatalError() }
        XCTAssertNotNil(rows.catalogGeneration); XCTAssertNil(old.value(for: rows.catalogGeneration))
        XCTAssertNil(SpawnCatalog(generation: rows.connectionGeneration, payload: nil).value(for: rows.catalogGeneration))
    }
    func testCompletionPreservesTrustOnlyForTheSameLinuxFolder() {
        XCTAssertTrue(SpawnForm.sameFolder("~/code/", known: "/home/user/code", home: "/home/user"))
        XCTAssertFalse(SpawnForm.sameFolder("/code/../project", known: "/project", home: ""))
        XCTAssertFalse(SpawnForm.sameFolder("/か\u{3099}/", known: "/が", home: ""))
    }
    func testPackageUpdateRequiresANewerRelease() {
        XCTAssertTrue(AppUpdate.newerPackage("0.1.10", than: "0.1.9"))
        XCTAssertFalse(AppUpdate.newerPackage("0.1.9", than: "0.1.10"))
        XCTAssertFalse(AppUpdate.newerPackage("0.1.01", than: "0.1.1"))
        XCTAssertFalse(AppUpdate.newerPackage("0.2.0", than: "dev"))
    }
    func testSameNamedDestinationsIncludeTheExactAddress() {
        let a = Host(id: "a", name: "Mac", address: "127.0.0.1:1234", deviceId: "1", needsPairing: false)
        let b = Host(id: "b", name: "Mac", address: "127.0.0.1:2345", deviceId: "2", needsPairing: false)
        XCTAssertEqual(a.displayName(in: [a]), "Mac")
        XCTAssertEqual(a.displayName(in: [a, b]), "Mac · 127.0.0.1:1234")
        XCTAssertEqual(b.displayName(in: [a, b]), "Mac · 127.0.0.1:2345")
    }

    @MainActor func testSetupGuideHonorsBothTextSizeSettings() throws {
        let suite = "PiMate.TypeTest." + UUID().uuidString
        let store = try XCTUnwrap(UserDefaults(suiteName: suite)); defer { store.removePersistentDomain(forName: suite) }
        func height(_ size: TypeSize, _ dynamic: DynamicTypeSize) -> CGFloat {
            store.set(size.rawValue, forKey: "typeSize")
            let guide = DeviceBridge().defaultAppStorage(store).dynamicTypeSize(dynamic)
            return UIHostingController(rootView: guide).sizeThatFits(in: CGSize(width: 400, height: 600)).height
        }
        let standard = height(.standard, .large)
        XCTAssertGreaterThan(height(.large, .large), standard)
        XCTAssertGreaterThan(height(.standard, .accessibility2), standard)
    }
}
