import SwiftUI

struct ProviderView: View {
    let model: SessionModel, host: Host
    @State private var retry = 0
    var body: some View {
        Group {
            if let rows = model.rows[host.id], rows.machineOnline {
                ProviderConnection(model: model, host: host, generation: rows.connectionGeneration, retry: { retry += 1 })
                    .id([rows.connectionGeneration.uuidString, String(retry)])
            } else { ContentUnavailableView("PC に接続していません", systemImage: "wifi.slash", description: Text("再接続後に専用の Pi とログイン状態を取り直します。")) }
        }.navigationTitle("プロバイダーのログイン").navigationBarTitleDisplayMode(.inline)
    }
}

private struct ProviderConnection: View {
    let model: SessionModel, host: Host, generation: UUID, retry: () -> Void
    @State private var page: ProviderState
    @State private var error = ""
    @State private var note = ""
    @State private var logout: String?
    @State private var pending: Task<Void, Never>?
    @Environment(\.scenePhase) private var phase
    @State private var captured = UIScreen.main.isCaptured
    init(model: SessionModel, host: Host, generation: UUID, retry: @escaping () -> Void) {
        self.model = model; self.host = host; self.generation = generation; self.retry = retry
        _page = State(initialValue: ProviderState(host: host.id))
    }
    private func row(_ group: String) -> Item? { model.rows[host.id]?.groups[group]?.first { $0.id == page.login?.target.instance } }
    private var providers: [Object] { row("provider").flatMap { try? JSONSerialization.jsonObject(with: Data($0.json.utf8)) as? [Object] } ?? [] }
    private var flow: Object { row("auth")?.object ?? [:] }
    private var active: Bool { CredentialOutcome(flow) == .active }
    var body: some View {
        Form {
            if page.login == nil && error.isEmpty { ProgressView("専用の Pi を開いています…") }
            if let login = page.login, active {
                Section(flow.str("provider")) {
                    Text(flow.str("status") == "input" ? "入力を待っています" : flow.str("status") == "saving" ? "保存しています…" : "ログイン中…")
                    let auth = flow.obj("authorization"), prompt = flow.obj("prompt")
                    if phase == .active && !captured {
                        if let url = URL(string: auth.str("url")), WebImage.valid(url, scheme: "https") {
                            Link("ブラウザで認証", destination: url)
                            if !auth.str("userCode").isEmpty { Text(verbatim: auth.str("userCode")).font(.body.monospaced()).privacySensitive() }
                        }
                        if !prompt.str("id").isEmpty {
                            let id = flow.str("id"), pid = prompt.str("id")
                            ProviderPrompt(apiKey: prompt.str("kind") == "api_key") { value in send(login, .authRespond, ["flowId": id, "promptId": pid, "value": value]) }.id([id, pid])
                        }
                    } else { Text("認証情報は非表示です") }
                    Button("ログインをキャンセル") { send(login, .authCancel, ["flowId": flow.str("id")]) }
                }
            } else if let login = page.login {
                if !flow.isEmpty { Text(resultText) }
                if providers.isEmpty { Text("プロバイダーの一覧がありません。") }
                ForEach(providers.indices, id: \.self) { i in
                    let p = providers[i], id = p.str("id")
                    Section(id) {
                        Text(p["configured"] as? Bool == true ? "設定済み" : "未設定").foregroundStyle(.secondary)
                        ForEach(p["methods"] as? [String] ?? [], id: \.self) { method in
                            Button(method == "oauth" ? "ブラウザでログイン" : "API キーを設定") { send(login, .authBegin, ["provider": id, "method": method]) }
                        }
                        if !p.str("stored").isEmpty { Button("ログアウト…", role: .destructive) { logout = id } }
                    }
                }
            }
            if pending != nil { ProgressView("応答を待っています…") }
            if !note.isEmpty { Text(note) }; if !error.isEmpty { Text(error).foregroundStyle(PiTheme.failure) }
            Button("ログイン状態を再確認", action: retry)
        }.disabled(pending != nil).task(id: generation) {
            do { try await page.hold(model, generation: generation) } catch { if !Task.isCancelled { self.error = "専用の Pi に接続できません。" } }
        }.task(id: active ? flow.str("id") : nil) {
            guard let login = page.login, active else { return }
            do { while try await login.followOnce() { try Task.checkCancellation() }; _ = try await login.request(.authList) }
            catch { if !Task.isCancelled { self.error = "認証の進行状況を確認できません。入力し直す前に状態を再確認してください。" } }
        }.alert("ログアウトしますか？", isPresented: Binding(get: { logout != nil }, set: { if !$0 { logout = nil } }), presenting: logout) { provider in
            Button("キャンセル", role: .cancel) {}; Button("ログアウト", role: .destructive) {
                if let login = page.login, !active, providers.contains(where: { $0.str("id") == provider && !$0.str("stored").isEmpty }) { send(login, .authLogout, ["provider": provider]) }
            }
        } message: { Text("\(host.name) に保存された \($0) の資格情報を削除します。") }
        .onReceive(NotificationCenter.default.publisher(for: UIScreen.capturedDidChangeNotification)) { _ in captured = UIScreen.main.isCaptured }
        .onDisappear { pending?.cancel() }
    }
    private var resultText: String {
        switch CredentialOutcome(flow) {
        case .saved: "ログインしました。"
        case .savedNeedsReload: "認証情報は保存済みです。PC の Pi を /reload または再起動してください。再入力は不要です。"
        case .storageUnknown: "資格情報の保存結果は不明です。再入力する前にログイン状態を再確認してください。"
        case .failed: "ログインできませんでした。"
        case .expired: "ログインの期限が切れました。"
        default: "ログイン操作は終了しました。"
        }
    }
    private func send(_ login: LoginSession, _ method: RcMethod, _ fields: Object) {
        guard pending == nil else { return }; error = ""; note = ""
        pending = Task {
            defer { pending = nil }
            do {
                let result = try await login.request(method, fields)
                if method == .authLogout { note = result["refreshRequired"] as? Bool == true ? "削除済みです。PC の Pi を /reload してください。" : "ログアウトしました。" }
                _ = try await login.request(.authList)
            } catch { if !Task.isCancelled { self.error = "資格情報の状態は未確認です。再入力する前にログイン状態を再確認してください。" } }
        }
    }
}

private struct ProviderPrompt: View {
    let apiKey: Bool, submit: (String) -> Void
    @State private var value = ""
    var body: some View {
        SecureField(apiKey ? "API キー" : "認証コード", text: $value).textInputAutocapitalization(.never).autocorrectionDisabled().privacySensitive()
        Button("送信") { let input = value; value = ""; submit(input) }.disabled(value.isEmpty)
    }
}
