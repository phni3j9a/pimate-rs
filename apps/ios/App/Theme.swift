import SwiftUI

// docs/design/html/Tokens.html; app multiplier is applied *in addition* to Dynamic Type.
enum Appearance: String, CaseIterable { case system = "端末に合わせる", light = "ライト", dark = "ダーク"
    var scheme: ColorScheme? { switch self { case .system: nil; case .light: .light; case .dark: .dark } }
}
enum TypeSize: String, CaseIterable { case small = "小", standard = "標準", large = "大"
    var factor: CGFloat { switch self { case .small: 0.875; case .standard: 1; case .large: 1.15 } }
}
enum TypeRole: CGFloat { case title = 22, sheet = 17, lead = 15, body = 14, caption = 12, meta = 11 }
struct PiFont: ViewModifier {
    @AppStorage("typeSize") private var size = TypeSize.standard
    @ScaledMetric private var points: CGFloat
    init(_ role: TypeRole) { _points = ScaledMetric(wrappedValue: role.rawValue, relativeTo: .body) }
    func body(content: PiFont.Content) -> some View { content.font(.system(size: points * size.factor)) }
}
enum PiTheme {
    static let gutter: CGFloat = 20, turnGap: CGFloat = 32
    private static func color(_ light: Int, _ dark: Int) -> Color {
        Color(uiColor: UIColor { traits in
            let n = traits.userInterfaceStyle == .dark ? dark : light
            return UIColor(red: CGFloat(n >> 16) / 255, green: CGFloat((n >> 8) & 255) / 255, blue: CGFloat(n & 255) / 255, alpha: 1)
        })
    }
    static let background = color(0xffffff, 0x131618), surface = color(0xf1f3f4, 0x1e2326)
    static let text = color(0x1f2326, 0xe4e8ea), secondary = color(0x5e686d, 0x9aa5aa)
    static let accent = color(0x155a7a, 0x7cbede), failure = color(0xa84d45, 0xf09c91)
}
