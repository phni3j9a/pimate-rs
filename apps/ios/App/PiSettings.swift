import SwiftUI

// Same keys as Probe, Drawer, Markdown and PiFont; never store trust or authentication input here.
struct PiSettings: DynamicProperty {
    @AppStorage("appearance") var appearance = Appearance.system
    @AppStorage("typeSize") var typeSize = TypeSize.standard
    @AppStorage("askTool") var askTool = false
    static let issueURL = URL(string: "https://github.com/phni3j9a/pimate-rs/issues/new")!
    static var version: String? { Bundle.main.object(forInfoDictionaryKey: "CFBundleShortVersionString") as? String }
}

enum AppUpdate: Equatable {
    case unknown, latest, available(String, URL)
    static func newerPackage(_ latest: String, than current: String) -> Bool {
        func parts(_ version: String) -> [Substring]? {
            let parts = version.split(separator: ".", omittingEmptySubsequences: false)
            guard parts.count == 3, parts.allSatisfy({ !$0.isEmpty && $0.allSatisfy { $0.isASCII && $0.isNumber } }) else { return nil }
            return parts.map { $0.drop(while: { $0 == "0" }) }
        }
        guard let a = parts(latest), let b = parts(current) else { return false }
        for (x, y) in zip(a, b) where x != y { return x.count == y.count ? x > y : x.count > y.count }
        return false
    }
    static func lookup(version: String, bundle: String, result: Object) -> Self {
        func parts(_ text: String) -> [Int]? {
            let ns = text.split(separator: ".", omittingEmptySubsequences: false)
            guard (1...3).contains(ns.count), ns.allSatisfy({ !$0.isEmpty && $0.allSatisfy { $0.isASCII && $0.isNumber } && Int($0) != nil }) else { return nil }
            return ns.compactMap { Int($0) } + Array(repeating: 0, count: 3 - ns.count)
        }
        guard let item = result.list("results").first(where: { $0.str("bundleId") == bundle && $0.str("kind") == "software" }),
              let current = parts(version), let latest = parts(item.str("version")), let id = item["trackId"] as? Int, id > 0 else { return .unknown }
        return current.lexicographicallyPrecedes(latest) ? .available(item.str("version"), URL(string: "https://apps.apple.com/app/id\(id)")!) : .latest
    }
    static func check() async -> Self {
        guard let version = PiSettings.version, let bundle = Bundle.main.bundleIdentifier else { return .unknown }
        var url = URLComponents(string: "https://itunes.apple.com/lookup")!; url.queryItems = [URLQueryItem(name: "bundleId", value: bundle)]
        guard let endpoint = url.url, let data = try? await WebImage.download(endpoint, limit: 2_000_000, timeout: .seconds(10)),
              let json = (try? JSONSerialization.jsonObject(with: data)) as? Object else { return .unknown }
        return lookup(version: version, bundle: bundle, result: json)
    }
}
