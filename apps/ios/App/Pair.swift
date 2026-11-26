import SwiftUI
import AVFoundation

struct PairScreen: View {
    let model: SessionModel
    let done: () -> Void
    @State private var link = ""
    @State private var error = ""
    @State private var entering = false
    @State private var scanning = false
    @State private var guide: SetupGuide.Step?
    @State private var scanAfterGuide = false
    @FocusState private var focused: Bool
    private struct Candidate { let link: String, preview: PairPreview }
    @State private var candidate: Candidate?
    private func preview(_ value: String) {
        focused = false; link = ""; error = ""
        guard let parsed = previewPair(link: value) else { error = "有効なペアリングリンクを指定してください。"; return }
        candidate = Candidate(link: value, preview: parsed)
    }
    private func pair(_ value: String) {
        candidate = nil
        focused = false; link = ""; error = ""
        Task { do { try await model.pair(value); done() } catch { self.error = "接続できません。ネットワークとリンクの有効期限を確認してください。" } }
    }
    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 24) {
                SwiftUI.Image("Mark").resizable().scaledToFit().frame(width: 34, height: 32).foregroundStyle(PiTheme.text).accessibilityHidden(true)
                VStack(alignment: .leading, spacing: 8) {
                    Text("PC の Pi とつなぐ").modifier(PiFont(.title)).bold().accessibilityAddTraits(.isHeader)
                    Text("PC で動いている Pi を、このスマホから確認・操作できます。").foregroundStyle(PiTheme.secondary)
                }
                VStack(alignment: .leading, spacing: 12) {
                    Label("pi", systemImage: "terminal").modifier(PiFont(.sheet)).fontDesign(.monospaced)
                    Divider(); Text("/pimate setup").modifier(PiFont(.body)).fontDesign(.monospaced); Divider()
                    Text("PC の Pi でこう打つと、QR コードが表示されます").modifier(PiFont(.caption)).foregroundStyle(PiTheme.secondary)
                }.padding(18).background(PiTheme.surface, in: RoundedRectangle(cornerRadius: 18))
                VStack(alignment: .leading, spacing: 20) {
                    Button { guide = .tailscale } label: {
                        step("1", "スマホと PC を Tailscale でつなぐ", "両方で同じアカウントにログイン")
                    }.foregroundStyle(PiTheme.text)
                    Button { guide = .pc } label: {
                        step("2", "PC に PiMate を入れる", "Pi で /pimate setup を実行")
                    }.foregroundStyle(PiTheme.text)
                    step("3", "QR コードを読み取る", "コードは 10 分間、1 回だけ使えます")
                }
                if !error.isEmpty || !model.message.isEmpty { Text(error.isEmpty ? model.message : error).foregroundStyle(PiTheme.failure) }
            }.padding(20).frame(maxWidth: 540, alignment: .leading).frame(maxWidth: .infinity)
        }.safeAreaInset(edge: .bottom) {
            VStack(spacing: 8) {
                if let candidate {
                    Text("接続先を確認").modifier(PiFont(.sheet)).fontWeight(.semibold).accessibilityAddTraits(.isHeader)
                    Text(verbatim: candidate.preview.name).accessibilityIdentifier("pair-name")
                    Text(verbatim: candidate.preview.address).modifier(PiFont(.lead)).fontDesign(.monospaced).accessibilityIdentifier("pair-address")
                    Button("このマシンに接続") { pair(candidate.link) }.buttonStyle(PiPrimary()).disabled(model.busy)
                    Button("キャンセル") { self.candidate = nil }.frame(minHeight: 44)
                } else if entering {
                    HStack {
                        SecureField("ペアリングリンク", text: $link).focused($focused).textInputAutocapitalization(.never).autocorrectionDisabled().accessibilityIdentifier("link")
                        PasteButton(payloadType: String.self) { if let s = $0.first { link = s } }.labelStyle(.iconOnly).accessibilityIdentifier("paste")
                    }.padding(.horizontal, 16).frame(minHeight: 48).background(PiTheme.surface, in: Capsule())
                    Button("接続先を確認") { preview(link) }.buttonStyle(PiPrimary()).disabled(link.isEmpty || model.busy)
                } else {
                    Button { scanning = true } label: { Label("QRコードを読み取る", systemImage: "qrcode.viewfinder") }.buttonStyle(PiPrimary())
                    Button("リンクを貼り付ける") { entering = true }.frame(minHeight: 44)
                }
                if model.busy { ProgressView("接続しています…") }
            }.padding(.horizontal, 20).padding(.vertical, 6).frame(maxWidth: 540).background(PiTheme.background)
        }.background(PiTheme.background).foregroundStyle(PiTheme.text)
            .sheet(item: $guide, onDismiss: {
                if scanAfterGuide { scanAfterGuide = false; scanning = true }
            }) { step in
                NavigationStack {
                    SetupGuide(step: step) { scanAfterGuide = true; guide = nil }
                        .toolbar { ToolbarItem(placement: .cancellationAction) { Button("閉じる") { guide = nil } } }
                }
            }.sheet(isPresented: $scanning) { ScannerSheet { value in scanning = false; preview(value) } }
    }
    private func step(_ n: String, _ title: String, _ detail: String) -> some View {
        HStack(alignment: .top, spacing: 14) {
            Text(n).modifier(PiFont(.body)).fontDesign(.monospaced).bold().frame(width: 30, height: 30).background(PiTheme.surface, in: RoundedRectangle(cornerRadius: 7))
            VStack(alignment: .leading, spacing: 3) { Text(title).fontWeight(.semibold); Text(detail).modifier(PiFont(.caption)).foregroundStyle(PiTheme.secondary) }
        }.accessibilityElement(children: .combine)
    }
}

private struct ScannerSheet: View {
    @Environment(\.dismiss) private var dismiss
    let scanned: (String) -> Void
    @State private var granted = false
    @State private var message = ""
    var body: some View {
        NavigationStack {
            Group {
                if !message.isEmpty { ContentUnavailableView("読み取れません", systemImage: "camera", description: Text(message)) }
                else if granted { Scanner(scanned: scanned) { message = "カメラを使用できません。リンクを貼り付けてください。" }.accessibilityLabel("PC の QR コードにカメラを向けてください") }
                else { ProgressView() }
            }.navigationTitle("QR コードを読み取る").navigationBarTitleDisplayMode(.inline)
                .toolbar { ToolbarItem(placement: .cancellationAction) { Button("閉じる") { dismiss() } } }
        }.task {
            if await AVCaptureDevice.requestAccess(for: .video) { granted = true }
            else { message = "設定でカメラを許可するか、リンクを貼り付けてください。" }
        }
    }
}
private struct Scanner: UIViewRepresentable {
    let scanned: (String) -> Void, failed: () -> Void
    final class Preview: UIView {
        override class var layerClass: AnyClass { AVCaptureVideoPreviewLayer.self }
    }
    final class Coordinator: NSObject, AVCaptureMetadataOutputObjectsDelegate {
        let session = AVCaptureSession(), queue = DispatchQueue(label: "dev.pimate.qr")
        var receive: ((String) -> Void)?
        func metadataOutput(_ output: AVCaptureMetadataOutput, didOutput objects: [AVMetadataObject], from connection: AVCaptureConnection) {
            guard let value = (objects.first as? AVMetadataMachineReadableCodeObject)?.stringValue else { return }
            let callback = receive; receive = nil; callback?(value)
        }
    }
    func makeCoordinator() -> Coordinator { Coordinator() }
    func makeUIView(context: Context) -> Preview {
        let v = Preview(), c = context.coordinator, s = c.session
        let preview = v.layer as! AVCaptureVideoPreviewLayer; preview.session = s; preview.videoGravity = .resizeAspectFill
        c.receive = scanned
        c.queue.async {
            let output = AVCaptureMetadataOutput()
            guard let dev = AVCaptureDevice.default(for: .video), let input = try? AVCaptureDeviceInput(device: dev), s.canAddInput(input) else { DispatchQueue.main.async(execute: failed); return }
            s.addInput(input)
            guard s.canAddOutput(output) else { DispatchQueue.main.async(execute: failed); return }
            s.addOutput(output); output.setMetadataObjectsDelegate(c, queue: .main); output.metadataObjectTypes = [.qr]; s.startRunning()
        }; return v
    }
    func updateUIView(_ view: Preview, context: Context) {}
    static func dismantleUIView(_ view: Preview, coordinator: Coordinator) { coordinator.receive = nil; coordinator.queue.async { coordinator.session.stopRunning() } }
}
