import SwiftUI
import XCTest
@testable import PiMate

final class MachineScreenTests: XCTestCase {
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
