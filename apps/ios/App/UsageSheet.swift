import SwiftUI

struct UsageRings: View {
    let model: SessionModel, key: EditorKey, rows: Rows
    @State private var failed = false
    var body: some View {
        TimelineView(.periodic(from: .now, by: 30)) { tick in
            let allowances = model.allowances(key, nowMs: UInt64(tick.date.timeIntervalSince1970 * 1000))
            let context = rows.groups["view"]?.first { $0.id == "context" }?.object["percent"] as? Double
            Button {
                do { _ = try model.openSheet(key, page: .usage) } catch { failed = true }
            } label: {
                HStack(spacing: 8) {
                    ring("コンテキスト", "text.alignleft", context)
                    ring("Claude", "sun.max", allowances.first { $0.source == "claude" }?.gauge)
                    ring("Codex", "terminal", allowances.first { $0.source == "codex" }?.gauge)
                }.frame(minHeight: 44).contentShape(Rectangle())
            }.accessibilityElement(children: .combine).accessibilityHint("使用状況を開く")
        }.alert("会話が変わりました。操作を選び直してください。", isPresented: $failed) { Button("閉じる") {} }
    }
    private func ring(_ name: String, _ icon: String, _ percent: Double?) -> some View {
        ZStack {
            Circle().stroke(PiTheme.secondary.opacity(0.35), lineWidth: 2)
            if let percent { Circle().trim(from: 0, to: min(1, max(0, percent / 100))).stroke(PiTheme.accent, lineWidth: 2).rotationEffect(.degrees(-90)) }
            SwiftUI.Image(systemName: icon).font(.system(size: 11))
        }.frame(width: 23, height: 23).accessibilityElement(children: .ignore).accessibilityLabel(name).accessibilityValue(percent.map { "\($0.formatted(.number.precision(.fractionLength(0))))%" } ?? "不明")
    }
}
struct UsageSheet: View {
    static func timeLeft(_ ms: UInt64) -> String {
        let minutes = ms / 60_000 + (ms.isMultiple(of: 60_000) ? 0 : 1), days = minutes / 1440, hours = minutes % 1440 / 60, rest = minutes % 60
        if days > 0 { return "あと\(days)日" + (hours > 0 ? "\(hours)時間" : "") }
        if hours > 0 { return "あと\(hours)時間" + (rest > 0 ? "\(rest)分" : "") }
        return "あと\(rest)分"
    }
    let panel: PiSheetPanel
    private func clock(_ ms: UInt64) -> String {
        Date(timeIntervalSince1970: Double(ms) / 1000).formatted(date: .abbreviated, time: .shortened)
    }
    private func windowName(_ minutes: UInt64) -> String {
        if minutes == 10_080 { return "週間" }
        if minutes > 0 && minutes.isMultiple(of: 60) { return "\(minutes / 60)時間" }
        return "\(minutes)分"
    }
    var body: some View {
        let context = panel.field("context")?.object ?? [:], stats = panel.model.stats(panel.opened.key)
        TimelineView(.periodic(from: .now, by: 30)) { tick in
            let now = UInt64(tick.date.timeIntervalSince1970 * 1000)
            List {
                Section("コンテキスト") {
                    let tokens = (context["tokens"] as? NSNumber)?.int64Value.formatted() ?? "不明"
                    let capacity = (context["contextWindow"] as? NSNumber)?.int64Value.formatted() ?? "不明"
                    UsageMeter(label: "\(tokens) / \(capacity) トークン", percent: context["percent"] as? Double)
                    if let stats, stats.cost > 0 { Text("このセッションの記録済みコスト · \(stats.cost.formatted(.currency(code: "USD")))").modifier(PiFont(.caption)).foregroundStyle(.secondary) }
                    Button("コンテキストを圧縮") { panel.act(.compact) }.disabled(!panel.ready)
                }
                ForEach(panel.model.allowances(panel.opened.key, nowMs: now), id: \.source) { allowance in
                    let name = allowance.source == "claude" ? "Claude" : "Codex"
                    Section {
                        if allowance.windows.isEmpty { Text("不明（この PC では利用枠をまだ確認できません）").foregroundStyle(.secondary) }
                        ForEach(allowance.windows.indices, id: \.self) { index in
                            let window = allowance.windows[index]
                            VStack(alignment: .leading, spacing: 6) {
                                UsageMeter(label: windowName(window.minutes), percent: window.usedPercent)
                                if let reset = window.resetsAt {
                                    Text(reset <= now ? "リセット済み · 次の応答で更新" : "\(clock(reset)) にリセット（\(Self.timeLeft(reset - now))）").modifier(PiFont(.caption)).foregroundStyle(.secondary)
                                }
                            }.padding(.vertical, 3).accessibilityElement(children: .combine)
                        }
                    } header: { Text(verbatim: name) } footer: {
                        if let observed = allowance.observedAt { Text("\(clock(observed)) 取得") }
                    }
                }
            }
        }.navigationTitle("使用状況").navigationBarTitleDisplayMode(.inline)
    }
}

private struct UsageMeter: View {
    let label: String, percent: Double?
    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            Text(verbatim: label).modifier(PiFont(.lead))
            if let percent, percent.isFinite {
                Text("\(percent.formatted(.number.precision(.fractionLength(0))))% 使用").modifier(PiFont(.lead)).fontWeight(.semibold).monospacedDigit()
                ProgressView(value: min(100, max(0, percent)), total: 100).accessibilityHidden(true)
            } else { Text("使用率は不明").modifier(PiFont(.lead)).foregroundStyle(.secondary) }
        }.accessibilityElement(children: .combine)
    }
}
