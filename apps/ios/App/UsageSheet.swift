import SwiftUI

struct UsageSheet: View {
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
                    if let stats, stats.cost > 0 { Text("このセッションの記録済みコスト · \(stats.cost.formatted(.currency(code: "USD")))").font(.footnote).foregroundStyle(.secondary) }
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
                                    Text(reset <= now ? "リセット済み · 次の応答で更新" : "\(clock(reset)) にリセット").font(.caption).foregroundStyle(.secondary)
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
            Text(verbatim: label).font(.subheadline)
            if let percent, percent.isFinite {
                Text("\(percent.formatted(.number.precision(.fractionLength(0))))% 使用").font(.callout.bold()).monospacedDigit()
                ProgressView(value: min(100, max(0, percent)), total: 100).accessibilityHidden(true)
            } else { Text("使用率は不明").font(.callout).foregroundStyle(.secondary) }
        }.accessibilityElement(children: .combine)
    }
}
