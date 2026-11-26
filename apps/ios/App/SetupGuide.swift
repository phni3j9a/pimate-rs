import SwiftUI

struct SetupGuide: View {
    enum Step: Int, Identifiable { case tailscale = 1, pc; var id: Int { rawValue } }
    let step: Step, scan: () -> Void
    private let install = "pi install npm:@phni3j9a/pimate"
    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 18) {
                Text("ペアリングの準備　\(step.rawValue) / 3").modifier(PiFont(.caption)).foregroundStyle(.secondary)
                if step == .tailscale {
                    DeviceBridge()
                    Text("このスマホ").modifier(PiFont(.sheet)).fontWeight(.semibold)
                    Text("Tailscale のアプリにログインし、VPN の接続を許可してください。")
                    Link("App Store で Tailscale を開く", destination: URL(string: "https://apps.apple.com/app/id1470499037")!)
                    Divider(); Text("PC").modifier(PiFont(.sheet)).fontWeight(.semibold)
                    Text("PC にも Tailscale を入れ、スマホと同じアカウントでログインします。")
                    Link("PC へのインストール方法", destination: URL(string: "https://tailscale.com/download")!)
                    Text("スマホの Tailscale に PC が表示されれば準備完了です。").foregroundStyle(.secondary)
                } else {
                    Text("1　ターミナルで実行").modifier(PiFont(.sheet)).fontWeight(.semibold)
                    Text("PC に PiMate をインストールし、Pi を起動します。")
                    command(install); command("pi")
                    Divider(); Text("2　Pi の入力欄で実行").modifier(PiFont(.sheet)).fontWeight(.semibold)
                    command("/pimate setup")
                    Text("QR コードが表示されます。再ペアリングや 2 台目のスマホには /pimate pair で出し直せます。")
                    ShareLink(item: "ターミナルで：\n\(install)\npi\nPi の入力欄で：\n/pimate setup") { Label("この手順を PC に送る", systemImage: "square.and.arrow.up") }
                }
            }.padding(20).frame(maxWidth: 560, alignment: .leading).frame(maxWidth: .infinity)
        }.background(PiTheme.background).navigationTitle(step == .pc ? "PC に PiMate を入れる" : "Tailscale でつなぐ").navigationBarTitleDisplayMode(.inline)
        .safeAreaInset(edge: .bottom) {
            Group {
                if step == .pc { Button("QRコードを読み取る", action: scan) }
                else { NavigationLink("次へ：PC に PiMate を入れる") { SetupGuide(step: .pc, scan: scan) } }
            }.buttonStyle(PiPrimary()).padding(.horizontal, 20).padding(.vertical, 8).background(PiTheme.background)
        }
    }
    private func command(_ text: String) -> some View {
        Text(verbatim: text).modifier(PiFont(.lead)).fontDesign(.monospaced).textSelection(.enabled).padding(14)
            .background(PiTheme.surface, in: Capsule())
    }
}

struct DeviceBridge: View {
    var body: some View {
        HStack(spacing: 18) { SwiftUI.Image(systemName: "iphone"); Text("···"); SwiftUI.Image(systemName: "network"); Text("···"); SwiftUI.Image(systemName: "desktopcomputer") }
            .modifier(PiFont(.title)).foregroundStyle(PiTheme.accent).padding(.vertical).frame(maxWidth: .infinity).accessibilityHidden(true)
    }
}
