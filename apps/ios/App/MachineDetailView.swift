import SwiftUI

struct MachineDetailView: View {
    let model: SessionModel, host: Host
    @Environment(\.dismiss) private var dismiss
    @State private var devices: [Object]?, check: Object?, github = GithubStatus.unknown
    @State private var confirmation: MachineConfirmation?, title = "", targetName = ""
    @State private var note = "", error = "", refresh = 0, loading = false
    @State private var action: Task<Void, Never>?
    private var rows: Rows? { model.rows[host.id] }
    private var caps: Object { rows?.machineInfo ?? [:] }
    private var online: Bool { rows?.machineOnline == true }
    private func supports(_ key: String) -> Bool { online && caps[key] as? Bool == true }
    var body: some View {
        Form {
            Section("状態") {
                Text(MachineDetailView.summary(host, rows)); Text(verbatim: host.address).font(.callout.monospaced()).textSelection(.enabled)
                if let current = devices?.first(where: { $0["current"] as? Bool == true }), let paired = date(current["createdAt"]) { Text("\(paired) にペアリング").foregroundStyle(.secondary) }
            }
            Section("PC の PiMate") {
                LabeledContent("現在の版", value: check?["current"] as? String ?? caps["version"] as? String ?? "未確認")
                if check?.str("reason") == "not_package" { Text("開発版は PC で更新してください。") }
                else if let latest = check?["latest"] as? String, !latest.isEmpty {
                    if latest == check?.str("current") { Text("最新です") }
                    else {
                        LabeledContent("照会時の最新版", value: latest)
                        if supports("packageUpdate") && check?["canUpdate"] as? Bool == true { confirmButton("実行時の最新版に更新…", .update) }
                        else { Text("PC で実行：pi update npm:@phni3j9a/pimate").font(.callout.monospaced()).textSelection(.enabled) }
                    }
                } else { Text("最新版は未確認です").foregroundStyle(.secondary) }
                if supports("hostRestart") { confirmButton("Host を再起動…", .restart) }
            }
            Section("プロバイダー") {
                NavigationLink("プロバイダーのログイン") { ProviderView(model: model, host: host) }.disabled(!supports("authSession"))
            }
            Section("GitHub") {
                switch github {
                case .unknown: Text("ログイン状態は未確認です")
                case .notInstalled: Text("PC に GitHub CLI（gh）がありません")
                case .loggedOut: Text("PC で gh auth login を実行してください")
                case .loggedIn(let login): Text("\(login) でログイン中")
                }
            }
            Section("ペアリング済みの端末") {
                if let devices {
                    if devices.isEmpty { Text("登録された端末はありません") }
                    ForEach(devices.indices, id: \.self) { i in
                        let d = devices[i], current = d["current"] as? Bool == true
                        VStack(alignment: .leading, spacing: 4) {
                            Text(verbatim: d.str("name")); if current { Text("この端末").font(.caption) }
                            if let seen = date(d["lastSeenAt"]) { Text("最終接続 \(seen)").font(.caption).foregroundStyle(.secondary) }
                        }
                        if !current && supports("pairedDevices"), let id = d["deviceId"] as? String { confirmButton("取り消し…", .revoke(id), d.str("name")) }
                    }
                } else { Text("端末一覧は未確認です").foregroundStyle(.secondary) }
            }
            Section {
                if loading || action != nil { ProgressView("PC の応答を待っています…") }
                if !error.isEmpty { Text(error).foregroundStyle(PiTheme.failure) }; if !note.isEmpty { Text(note) }
                Button("状態を再読み込み") { refresh += 1 }.disabled(!online || loading)
                confirmButton("このマシンを削除…", .forget)
            }
        }.disabled(action != nil).navigationTitle(host.name).navigationBarTitleDisplayMode(.inline)
        .task(id: [rows?.connectionGeneration.uuidString ?? "", rows?.groups["host"]?.first { $0.id == "info" }?.json ?? "", String(refresh)]) { await load() }
        .alert(title, isPresented: Binding(get: { confirmation != nil }, set: { if !$0 { confirmation = nil } }), presenting: confirmation) { fixed in
            Button("キャンセル", role: .cancel) {}; Button("実行", role: .destructive) { perform(fixed) }
        } message: { fixed in Text(targetName + "\n" + fixed.warning + (fixed.action == .forget ? " 保存された下書きも削除されます。" : "")) }
        .onDisappear { action?.cancel() }
    }
    private func date(_ value: Any?) -> String? {
        guard let ms = value as? Double, ms > 0 else { return nil }; return Date(timeIntervalSince1970: ms / 1000).formatted(date: .abbreviated, time: .omitted)
    }
    static func summary(_ host: Host, _ rows: Rows?) -> String {
        if host.needsPairing { return "再ペアリングが必要" }
        guard rows?.machineOnline == true else { return rows?.firstStep == .offline ? "接続できません" : "接続しています…" }
        var parts = ["接続中"]; if let platform = rows?.machineInfo["platform"] as? String { parts.append(platform) }
        if rows?.firstStep != .loading { parts.append("Pi \(rows?.groups["instance"]?.count ?? 0) 件") }; return parts.joined(separator: " · ")
    }
    private func confirmButton(_ label: String, _ op: MachineConfirmation.Action, _ target: String = "") -> some View {
        Button(label, role: .destructive) {
            do { let fixed = try model.confirmation(host: host.id, op); title = label; targetName = [host.name, target].filter { !$0.isEmpty }.joined(separator: " · "); confirmation = fixed }
            catch { self.error = "対象の登録が変わりました。状態を確認してください。" }
        }
    }
    private func perform(_ fixed: MachineConfirmation) {
        guard action == nil else { return }; note = ""; error = ""
        action = Task {
            defer { action = nil }
            do {
                try Task.checkCancellation(); let reply = try await model.confirm(fixed); try Task.checkCancellation()
                switch fixed.action {
                case .forget: dismiss()
                case .restart: note = "再起動を受け付けました。"
                case .update: note = reply.str("to").isEmpty ? "更新後の版は未確認です。" : "\(reply.str("to")) に更新しました。"
                case .revoke: note = "端末の登録を取り消しました。"
                }; refresh += 1
            } catch { if !Task.isCancelled { self.error = "操作結果を確認できません。再実行の前に状態を確認してください。" } }
        }
    }
    private func load() async {
        devices = nil; check = nil; github = .unknown; error = ""; confirmation = nil
        guard online else { loading = false; return }; loading = true
        defer { if !Task.isCancelled { loading = false } }
        for (cap, op) in [("pairedDevices", Operation.devices), ("githubStatus", .githubStatus), ("packageUpdate", .updateCheck)] where supports(cap) {
            do {
                try Task.checkCancellation(); let r = try await model.machineRequest(host.id, op); try Task.checkCancellation()
                switch op { case .devices: devices = r["devices"] as? [Object]; case .githubStatus: github = GithubStatus(r["status"] as? Object); default: check = r }
            } catch { if Task.isCancelled { return }; self.error = "PC の状態を一部取得できませんでした。" }
        }
    }
}
